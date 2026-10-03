//! Minimal PipeWire client that keeps the newest frame of each screen-cast
//! node, so a screenshot is a memory copy instead of a portal round trip.
//!
//! libpipewire is loaded with `dlopen` rather than linked. The binary still
//! starts, and falls back to the Screenshot portal, where PipeWire is missing,
//! and builds need neither PipeWire headers nor libclang. Only the handful of
//! stable `pw_*` entry points and the SPA layouts below are used; their values
//! come from the PipeWire 0.3 / 1.x headers.

use anyhow::{anyhow, bail, Result};
use image::RgbImage;
use std::{
    ffi::{c_char, c_int, c_void, CStr},
    mem::size_of,
    os::fd::{IntoRawFd, OwnedFd},
    ptr,
    sync::OnceLock,
};

const LIBPIPEWIRE: &CStr = c"libpipewire-0.3.so.0";

// spa/utils/type.h
const SPA_TYPE_ID: u32 = 3;
const SPA_TYPE_RECTANGLE: u32 = 10;
const SPA_TYPE_FRACTION: u32 = 11;
const SPA_TYPE_OBJECT: u32 = 15;
const SPA_TYPE_CHOICE: u32 = 19;
const SPA_TYPE_OBJECT_FORMAT: u32 = 0x40003;
// spa/param/param.h
const SPA_PARAM_ENUM_FORMAT: u32 = 3;
const SPA_PARAM_FORMAT: u32 = 4;
// spa/param/format.h
const SPA_FORMAT_MEDIA_TYPE: u32 = 1;
const SPA_FORMAT_MEDIA_SUBTYPE: u32 = 2;
const SPA_FORMAT_VIDEO_FORMAT: u32 = 0x20001;
const SPA_FORMAT_VIDEO_SIZE: u32 = 0x20003;
const SPA_FORMAT_VIDEO_FRAMERATE: u32 = 0x20004;
const SPA_MEDIA_TYPE_VIDEO: u32 = 2;
const SPA_MEDIA_SUBTYPE_RAW: u32 = 1;
// spa/pod/pod.h
const SPA_CHOICE_RANGE: u32 = 1;
const SPA_CHOICE_ENUM: u32 = 3;
// spa/param/video/raw.h
const SPA_VIDEO_FORMAT_RGBX: u32 = 7;
const SPA_VIDEO_FORMAT_BGRX: u32 = 8;
const SPA_VIDEO_FORMAT_RGBA: u32 = 11;
const SPA_VIDEO_FORMAT_BGRA: u32 = 12;
// spa/buffer/buffer.h
const SPA_CHUNK_FLAG_CORRUPTED: i32 = 1;
// pipewire/stream.h
const PW_DIRECTION_INPUT: c_int = 0;
const PW_STREAM_FLAG_AUTOCONNECT: c_int = 1 << 0;
const PW_STREAM_FLAG_MAP_BUFFERS: c_int = 1 << 2;
const PW_STREAM_FLAG_DONT_RECONNECT: c_int = 1 << 7;
const PW_STREAM_STATE_ERROR: c_int = -1;
const PW_STREAM_STATE_UNCONNECTED: c_int = 0;
const PW_STREAM_STATE_PAUSED: c_int = 2;

const BYTES_PER_PIXEL: usize = 4;

/// `struct spa_pod` header.
#[repr(C)]
struct SpaPod {
    size: u32,
    kind: u32,
}

/// `struct spa_hook`: list link, callbacks, `removed`, `priv`. PipeWire owns
/// the contents; we only provide zeroed storage that outlives the stream.
#[repr(C)]
struct SpaHook {
    link: [*mut c_void; 2],
    callbacks: [*mut c_void; 2],
    removed: *mut c_void,
    private: *mut c_void,
}

/// Version 0 of `struct pw_stream_events`. PipeWire checks `version` before
/// touching the later `command` and `trigger_done` members.
#[repr(C)]
struct PwStreamEvents {
    version: u32,
    destroy: Option<unsafe extern "C" fn(*mut c_void)>,
    state_changed: Option<unsafe extern "C" fn(*mut c_void, c_int, c_int, *const c_char)>,
    control_info: Option<unsafe extern "C" fn(*mut c_void, u32, *const c_void)>,
    io_changed: Option<unsafe extern "C" fn(*mut c_void, u32, *mut c_void, u32)>,
    param_changed: Option<unsafe extern "C" fn(*mut c_void, u32, *const SpaPod)>,
    add_buffer: Option<unsafe extern "C" fn(*mut c_void, *mut PwBuffer)>,
    remove_buffer: Option<unsafe extern "C" fn(*mut c_void, *mut PwBuffer)>,
    process: Option<unsafe extern "C" fn(*mut c_void)>,
    drained: Option<unsafe extern "C" fn(*mut c_void)>,
}

/// Leading member of `struct pw_buffer`; the rest is never touched.
#[repr(C)]
struct PwBuffer {
    buffer: *mut SpaBuffer,
}

#[repr(C)]
struct SpaBuffer {
    n_metas: u32,
    n_datas: u32,
    metas: *mut c_void,
    datas: *mut SpaData,
}

#[repr(C)]
struct SpaData {
    kind: u32,
    flags: u32,
    fd: i64,
    mapoffset: u32,
    maxsize: u32,
    data: *mut c_void,
    chunk: *mut SpaChunk,
}

#[repr(C)]
struct SpaChunk {
    offset: u32,
    size: u32,
    stride: i32,
    flags: i32,
}

struct Api {
    thread_loop_new: unsafe extern "C" fn(*const c_char, *const c_void) -> *mut c_void,
    thread_loop_get_loop: unsafe extern "C" fn(*mut c_void) -> *mut c_void,
    thread_loop_start: unsafe extern "C" fn(*mut c_void) -> c_int,
    thread_loop_stop: unsafe extern "C" fn(*mut c_void),
    thread_loop_destroy: unsafe extern "C" fn(*mut c_void),
    thread_loop_lock: unsafe extern "C" fn(*mut c_void),
    thread_loop_unlock: unsafe extern "C" fn(*mut c_void),
    context_new: unsafe extern "C" fn(*mut c_void, *mut c_void, usize) -> *mut c_void,
    context_destroy: unsafe extern "C" fn(*mut c_void),
    context_connect_fd: unsafe extern "C" fn(*mut c_void, c_int, *mut c_void, usize) -> *mut c_void,
    core_disconnect: unsafe extern "C" fn(*mut c_void) -> c_int,
    properties_new_string: unsafe extern "C" fn(*const c_char) -> *mut c_void,
    stream_new: unsafe extern "C" fn(*mut c_void, *const c_char, *mut c_void) -> *mut c_void,
    stream_add_listener:
        unsafe extern "C" fn(*mut c_void, *mut SpaHook, *const PwStreamEvents, *mut c_void),
    stream_connect:
        unsafe extern "C" fn(*mut c_void, c_int, u32, c_int, *mut *const SpaPod, u32) -> c_int,
    stream_dequeue_buffer: unsafe extern "C" fn(*mut c_void) -> *mut PwBuffer,
    stream_queue_buffer: unsafe extern "C" fn(*mut c_void, *mut PwBuffer) -> c_int,
    stream_destroy: unsafe extern "C" fn(*mut c_void),
}

/// Load libpipewire once per process, reporting why it is unusable.
pub(crate) fn ensure_available() -> Result<()> {
    api().map(|_| ())
}

fn api() -> Result<&'static Api> {
    static API: OnceLock<std::result::Result<Api, String>> = OnceLock::new();
    API.get_or_init(|| unsafe { load_api() })
        .as_ref()
        .map_err(|error| anyhow!("{error}"))
}

unsafe fn load_api() -> std::result::Result<Api, String> {
    let handle = libc::dlopen(LIBPIPEWIRE.as_ptr(), libc::RTLD_NOW | libc::RTLD_LOCAL);
    if handle.is_null() {
        return Err(format!(
            "could not load {}: {}",
            LIBPIPEWIRE.to_string_lossy(),
            dlerror_message()
        ));
    }
    let init: unsafe extern "C" fn(*mut c_int, *mut *mut *mut c_char) = symbol(handle, c"pw_init")?;
    init(ptr::null_mut(), ptr::null_mut());
    Ok(Api {
        thread_loop_new: symbol(handle, c"pw_thread_loop_new")?,
        thread_loop_get_loop: symbol(handle, c"pw_thread_loop_get_loop")?,
        thread_loop_start: symbol(handle, c"pw_thread_loop_start")?,
        thread_loop_stop: symbol(handle, c"pw_thread_loop_stop")?,
        thread_loop_destroy: symbol(handle, c"pw_thread_loop_destroy")?,
        thread_loop_lock: symbol(handle, c"pw_thread_loop_lock")?,
        thread_loop_unlock: symbol(handle, c"pw_thread_loop_unlock")?,
        context_new: symbol(handle, c"pw_context_new")?,
        context_destroy: symbol(handle, c"pw_context_destroy")?,
        context_connect_fd: symbol(handle, c"pw_context_connect_fd")?,
        core_disconnect: symbol(handle, c"pw_core_disconnect")?,
        properties_new_string: symbol(handle, c"pw_properties_new_string")?,
        stream_new: symbol(handle, c"pw_stream_new")?,
        stream_add_listener: symbol(handle, c"pw_stream_add_listener")?,
        stream_connect: symbol(handle, c"pw_stream_connect")?,
        stream_dequeue_buffer: symbol(handle, c"pw_stream_dequeue_buffer")?,
        stream_queue_buffer: symbol(handle, c"pw_stream_queue_buffer")?,
        stream_destroy: symbol(handle, c"pw_stream_destroy")?,
    })
}

/// Resolve `name` as a function pointer of type `T`.
unsafe fn symbol<T: Copy>(handle: *mut c_void, name: &CStr) -> std::result::Result<T, String> {
    debug_assert_eq!(size_of::<T>(), size_of::<*mut c_void>());
    let pointer = libc::dlsym(handle, name.as_ptr());
    if pointer.is_null() {
        return Err(format!(
            "{} does not export {}",
            LIBPIPEWIRE.to_string_lossy(),
            name.to_string_lossy()
        ));
    }
    Ok(std::mem::transmute_copy::<*mut c_void, T>(&pointer))
}

fn dlerror_message() -> String {
    // SAFETY: dlerror returns null or a NUL-terminated thread-local string.
    let message = unsafe { libc::dlerror() };
    if message.is_null() {
        "unknown dlopen error".to_string()
    } else {
        unsafe { CStr::from_ptr(message) }
            .to_string_lossy()
            .into_owned()
    }
}

/// Byte order of a negotiated 32-bit RGB frame. The alpha/padding byte is
/// dropped either way.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PixelLayout {
    Bgrx,
    Rgbx,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct VideoFormat {
    layout: PixelLayout,
    width: u32,
    height: u32,
}

/// One copied frame, rows packed without stride padding.
pub(crate) struct Frame {
    width: u32,
    height: u32,
    layout: PixelLayout,
    pixels: Vec<u8>,
}

impl Frame {
    #[cfg(test)]
    pub(crate) fn from_bgrx(width: u32, height: u32, pixels: Vec<u8>) -> Self {
        assert_eq!(
            pixels.len(),
            width as usize * height as usize * BYTES_PER_PIXEL
        );
        Self {
            width,
            height,
            layout: PixelLayout::Bgrx,
            pixels,
        }
    }

    pub(crate) fn width(&self) -> u32 {
        self.width
    }

    pub(crate) fn height(&self) -> u32 {
        self.height
    }

    pub(crate) fn into_rgb_image(self) -> RgbImage {
        let mut rgb = vec![0; self.pixels.len() / BYTES_PER_PIXEL * 3];
        let source = self.pixels.chunks_exact(BYTES_PER_PIXEL);
        let target = rgb.chunks_exact_mut(3);
        match self.layout {
            PixelLayout::Bgrx => source.zip(target).for_each(|(pixel, out)| {
                out.copy_from_slice(&[pixel[2], pixel[1], pixel[0]]);
            }),
            PixelLayout::Rgbx => source.zip(target).for_each(|(pixel, out)| {
                out.copy_from_slice(&pixel[..3]);
            }),
        }
        RgbImage::from_raw(self.width, self.height, rgb)
            .expect("frame buffer holds width * height pixels")
    }
}

/// Per-stream state shared with the PipeWire thread. Only touched from
/// stream callbacks (which run with the thread-loop lock held) or by
/// [`PipeWireCapture`] while it holds that lock.
struct StreamState {
    api: &'static Api,
    stream: *mut c_void,
    hook: SpaHook,
    format: Option<VideoFormat>,
    /// Newest complete frame, dequeued and kept out of the producer's pool
    /// until a newer one replaces it.
    held: *mut PwBuffer,
    state: c_int,
    connected: bool,
    error: Option<String>,
}

static STREAM_EVENTS: PwStreamEvents = PwStreamEvents {
    version: 0,
    destroy: None,
    state_changed: Some(on_state_changed),
    control_info: None,
    io_changed: None,
    param_changed: Some(on_param_changed),
    add_buffer: None,
    remove_buffer: Some(on_remove_buffer),
    process: Some(on_process),
    drained: None,
};

unsafe extern "C" fn on_state_changed(
    data: *mut c_void,
    _old: c_int,
    state: c_int,
    error: *const c_char,
) {
    let stream = &mut *data.cast::<StreamState>();
    stream.state = state;
    if state >= PW_STREAM_STATE_PAUSED {
        stream.connected = true;
    }
    if !error.is_null() {
        stream.error = Some(CStr::from_ptr(error).to_string_lossy().into_owned());
    }
}

unsafe extern "C" fn on_param_changed(data: *mut c_void, id: u32, param: *const SpaPod) {
    if id != SPA_PARAM_FORMAT {
        return;
    }
    let stream = &mut *data.cast::<StreamState>();
    stream.format = if param.is_null() {
        None
    } else {
        let len = size_of::<SpaPod>() + (*param).size as usize;
        parse_video_format(std::slice::from_raw_parts(param.cast::<u8>(), len))
    };
}

unsafe extern "C" fn on_remove_buffer(data: *mut c_void, buffer: *mut PwBuffer) {
    let stream = &mut *data.cast::<StreamState>();
    if stream.held == buffer {
        stream.held = ptr::null_mut();
    }
}

unsafe extern "C" fn on_process(data: *mut c_void) {
    let stream = &mut *data.cast::<StreamState>();
    let api = stream.api;
    // Drain the queue, keeping only the newest complete frame. Everything
    // else goes straight back to the producer.
    let mut newest: *mut PwBuffer = ptr::null_mut();
    loop {
        let buffer = (api.stream_dequeue_buffer)(stream.stream);
        if buffer.is_null() {
            break;
        }
        if buffer_has_frame(buffer) {
            if !newest.is_null() {
                (api.stream_queue_buffer)(stream.stream, newest);
            }
            newest = buffer;
        } else {
            // Cursor-only or damaged updates carry no usable pixels.
            (api.stream_queue_buffer)(stream.stream, buffer);
        }
    }
    if newest.is_null() {
        return;
    }
    if !stream.held.is_null() {
        (api.stream_queue_buffer)(stream.stream, stream.held);
    }
    stream.held = newest;
}

unsafe fn buffer_has_frame(buffer: *mut PwBuffer) -> bool {
    let Some(data) = first_data(buffer) else {
        return false;
    };
    let chunk = &*data.chunk;
    chunk.size > 0 && chunk.flags & SPA_CHUNK_FLAG_CORRUPTED == 0
}

unsafe fn first_data<'a>(buffer: *mut PwBuffer) -> Option<&'a SpaData> {
    let spa = (*buffer).buffer;
    if spa.is_null() || (*spa).n_datas == 0 || (*spa).datas.is_null() {
        return None;
    }
    let data = &*(*spa).datas;
    (!data.data.is_null() && !data.chunk.is_null()).then_some(data)
}

/// Copy the pixels of `buffer` out of the producer's shared memory.
unsafe fn copy_frame(buffer: *mut PwBuffer, format: VideoFormat) -> Option<Frame> {
    let data = first_data(buffer)?;
    let chunk = &*data.chunk;
    let width = format.width as usize;
    let height = format.height as usize;
    let row = width.checked_mul(BYTES_PER_PIXEL)?;
    if row == 0 || height == 0 {
        return None;
    }
    let stride = if chunk.stride > 0 {
        chunk.stride as usize
    } else {
        row
    };
    let maxsize = data.maxsize as usize;
    if stride < row || maxsize == 0 {
        return None;
    }
    let offset = chunk.offset as usize % maxsize;
    let end = stride
        .checked_mul(height - 1)?
        .checked_add(row)?
        .checked_add(offset)?;
    if end > maxsize {
        return None;
    }
    let base = data.data.cast::<u8>().add(offset);
    let mut pixels = Vec::with_capacity(row * height);
    for y in 0..height {
        pixels.extend_from_slice(std::slice::from_raw_parts(base.add(y * stride), row));
    }
    Some(Frame {
        width: format.width,
        height: format.height,
        layout: format.layout,
        pixels,
    })
}

/// Holds the PipeWire thread-loop lock for its lifetime.
struct LoopLock {
    api: &'static Api,
    thread_loop: *mut c_void,
}

impl LoopLock {
    unsafe fn new(api: &'static Api, thread_loop: *mut c_void) -> Self {
        (api.thread_loop_lock)(thread_loop);
        Self { api, thread_loop }
    }
}

impl Drop for LoopLock {
    fn drop(&mut self) {
        unsafe { (self.api.thread_loop_unlock)(self.thread_loop) };
    }
}

/// PipeWire connection with one input stream per screen-cast node.
pub(crate) struct PipeWireCapture {
    api: &'static Api,
    thread_loop: *mut c_void,
    context: *mut c_void,
    core: *mut c_void,
    streams: Vec<*mut StreamState>,
}

// SAFETY: the raw pointers are only dereferenced while holding the thread-loop
// lock (a mutex, so `&self` access from several threads is serialized), or in
// Drop after the loop thread has been stopped.
unsafe impl Send for PipeWireCapture {}
unsafe impl Sync for PipeWireCapture {}

impl PipeWireCapture {
    /// Connect to the PipeWire remote `remote` (from the portal's
    /// `OpenPipeWireRemote`) and start reading frames from `node_ids`.
    pub(crate) fn connect(remote: OwnedFd, node_ids: &[u32]) -> Result<Self> {
        let api = api()?;
        let thread_loop =
            unsafe { (api.thread_loop_new)(c"computer-use-kwin-screencast".as_ptr(), ptr::null()) };
        if thread_loop.is_null() {
            bail!("could not create a PipeWire thread loop");
        }
        // From here on, Drop tears down whatever has been created.
        let mut capture = Self {
            api,
            thread_loop,
            context: ptr::null_mut(),
            core: ptr::null_mut(),
            streams: Vec::with_capacity(node_ids.len()),
        };
        unsafe {
            if (api.thread_loop_start)(thread_loop) < 0 {
                bail!("could not start the PipeWire thread loop");
            }
            let _lock = LoopLock::new(api, thread_loop);
            capture.context =
                (api.context_new)((api.thread_loop_get_loop)(thread_loop), ptr::null_mut(), 0);
            if capture.context.is_null() {
                bail!("could not create a PipeWire context");
            }
            // pw_context_connect_fd owns the socket from here, even on failure.
            capture.core =
                (api.context_connect_fd)(capture.context, remote.into_raw_fd(), ptr::null_mut(), 0);
            if capture.core.is_null() {
                bail!("could not connect to the screen-cast PipeWire remote");
            }
            let params = enum_format_pod();
            for &node_id in node_ids {
                let properties = (api.properties_new_string)(
                    c"media.type=Video media.category=Capture media.role=Screen".as_ptr(),
                );
                let stream = (api.stream_new)(
                    capture.core,
                    c"computer-use-kwin-screenshot".as_ptr(),
                    properties,
                );
                if stream.is_null() {
                    bail!("could not create a PipeWire stream for node {node_id}");
                }
                let state = Box::into_raw(Box::new(StreamState {
                    api,
                    stream,
                    hook: SpaHook {
                        link: [ptr::null_mut(); 2],
                        callbacks: [ptr::null_mut(); 2],
                        removed: ptr::null_mut(),
                        private: ptr::null_mut(),
                    },
                    format: None,
                    held: ptr::null_mut(),
                    state: PW_STREAM_STATE_UNCONNECTED,
                    connected: false,
                    error: None,
                }));
                capture.streams.push(state);
                (api.stream_add_listener)(
                    stream,
                    ptr::addr_of_mut!((*state).hook),
                    &STREAM_EVENTS,
                    state.cast(),
                );
                let mut param_list = [params.as_ptr().cast::<SpaPod>()];
                let result = (api.stream_connect)(
                    stream,
                    PW_DIRECTION_INPUT,
                    node_id,
                    PW_STREAM_FLAG_AUTOCONNECT
                        | PW_STREAM_FLAG_MAP_BUFFERS
                        | PW_STREAM_FLAG_DONT_RECONNECT,
                    param_list.as_mut_ptr(),
                    param_list.len() as u32,
                );
                if result < 0 {
                    bail!("could not connect to screen-cast node {node_id} (error {result})");
                }
            }
        }
        Ok(capture)
    }

    /// Copy the newest frame of every stream, in `node_ids` order.
    ///
    /// `Ok(None)` means some stream has not delivered a frame yet. An error
    /// means a stream failed or its node went away, so the capture is dead.
    pub(crate) fn grab(&self) -> Result<Option<Vec<Frame>>> {
        let _lock = unsafe { LoopLock::new(self.api, self.thread_loop) };
        let mut frames = Vec::with_capacity(self.streams.len());
        for &state in &self.streams {
            let stream = unsafe { &*state };
            if stream.state == PW_STREAM_STATE_ERROR
                || (stream.connected && stream.state == PW_STREAM_STATE_UNCONNECTED)
            {
                bail!(
                    "the screen-cast stream stopped: {}",
                    stream.error.as_deref().unwrap_or("disconnected")
                );
            }
            let Some(format) = stream.format else {
                return Ok(None);
            };
            if stream.held.is_null() {
                return Ok(None);
            }
            match unsafe { copy_frame(stream.held, format) } {
                Some(frame) => frames.push(frame),
                None => return Ok(None),
            }
        }
        Ok(Some(frames))
    }
}

impl Drop for PipeWireCapture {
    fn drop(&mut self) {
        let api = self.api;
        unsafe {
            // Must run without the lock; joins the loop thread.
            (api.thread_loop_stop)(self.thread_loop);
            for &state in &self.streams {
                (api.stream_destroy)((*state).stream);
            }
            for state in self.streams.drain(..) {
                drop(Box::from_raw(state));
            }
            if !self.core.is_null() {
                (api.core_disconnect)(self.core);
            }
            if !self.context.is_null() {
                (api.context_destroy)(self.context);
            }
            (api.thread_loop_destroy)(self.thread_loop);
        }
    }
}

/// The EnumFormat param offered to the compositor: raw video in packed
/// 32-bit RGB layouts. It carries no modifier property, so the producer
/// shares frames through mappable memory instead of DMA-BUF.
fn enum_format_pod() -> Vec<u64> {
    let mut pod = PodWriter::default();
    pod.object(SPA_TYPE_OBJECT_FORMAT, SPA_PARAM_ENUM_FORMAT, |pod| {
        pod.property(SPA_FORMAT_MEDIA_TYPE);
        pod.id(SPA_MEDIA_TYPE_VIDEO);
        pod.property(SPA_FORMAT_MEDIA_SUBTYPE);
        pod.id(SPA_MEDIA_SUBTYPE_RAW);
        pod.property(SPA_FORMAT_VIDEO_FORMAT);
        pod.choice(
            SPA_CHOICE_ENUM,
            SPA_TYPE_ID,
            1,
            &[
                SPA_VIDEO_FORMAT_BGRX,
                SPA_VIDEO_FORMAT_BGRX,
                SPA_VIDEO_FORMAT_BGRA,
                SPA_VIDEO_FORMAT_RGBX,
                SPA_VIDEO_FORMAT_RGBA,
            ],
        );
        // Range choices are default, min, max.
        pod.property(SPA_FORMAT_VIDEO_SIZE);
        pod.choice(
            SPA_CHOICE_RANGE,
            SPA_TYPE_RECTANGLE,
            2,
            &[1920, 1080, 1, 1, 16384, 16384],
        );
        pod.property(SPA_FORMAT_VIDEO_FRAMERATE);
        pod.choice(
            SPA_CHOICE_RANGE,
            SPA_TYPE_FRACTION,
            2,
            &[30, 1, 0, 1, 1000, 1],
        );
    });
    pod.into_aligned()
}

/// Writes SPA pods in native byte order, padding each value to 8 bytes.
#[derive(Default)]
struct PodWriter {
    bytes: Vec<u8>,
}

impl PodWriter {
    fn u32(&mut self, value: u32) {
        self.bytes.extend_from_slice(&value.to_ne_bytes());
    }

    fn pad(&mut self) {
        self.bytes.resize(self.bytes.len().next_multiple_of(8), 0);
    }

    fn object(&mut self, object_type: u32, id: u32, body: impl FnOnce(&mut Self)) {
        let start = self.bytes.len();
        self.u32(0);
        self.u32(SPA_TYPE_OBJECT);
        self.u32(object_type);
        self.u32(id);
        body(self);
        let size = (self.bytes.len() - start - size_of::<SpaPod>()) as u32;
        self.bytes[start..start + 4].copy_from_slice(&size.to_ne_bytes());
    }

    fn property(&mut self, key: u32) {
        self.u32(key);
        self.u32(0);
    }

    fn id(&mut self, value: u32) {
        self.u32(4);
        self.u32(SPA_TYPE_ID);
        self.u32(value);
        self.pad();
    }

    /// A choice of `child_type` values, each `words_per_value` u32s long.
    fn choice(&mut self, choice_type: u32, child_type: u32, words_per_value: usize, words: &[u32]) {
        let child_size = (words_per_value * 4) as u32;
        self.u32(16 + (words.len() * 4) as u32);
        self.u32(SPA_TYPE_CHOICE);
        self.u32(choice_type);
        self.u32(0);
        self.u32(child_size);
        self.u32(child_type);
        words.iter().for_each(|word| self.u32(*word));
        self.pad();
    }

    fn into_aligned(self) -> Vec<u64> {
        self.bytes
            .chunks_exact(8)
            .map(|chunk| u64::from_ne_bytes(chunk.try_into().expect("8-byte chunk")))
            .collect()
    }
}

/// Read a negotiated raw-video Format object. Returns `None` for anything
/// other than raw video in one of the layouts offered by [`enum_format_pod`].
fn parse_video_format(pod: &[u8]) -> Option<VideoFormat> {
    if read_u32(pod, 4)? != SPA_TYPE_OBJECT {
        return None;
    }
    let end = size_of::<SpaPod>().checked_add(read_u32(pod, 0)? as usize)?;
    let pod = pod.get(..end)?;
    let mut media = (None, None);
    let mut layout = None;
    let mut size = None;
    // Skip the header, object type and object id.
    let mut offset = 16;
    while offset + 16 <= pod.len() {
        let key = read_u32(pod, offset)?;
        let value_size = read_u32(pod, offset + 8)? as usize;
        let value_type = read_u32(pod, offset + 12)?;
        let value = pod.get(offset + 16..(offset + 16).checked_add(value_size)?)?;
        let (value_type, value) = current_value(value_type, value)?;
        match (key, value_type) {
            (SPA_FORMAT_MEDIA_TYPE, SPA_TYPE_ID) => media.0 = read_u32(value, 0),
            (SPA_FORMAT_MEDIA_SUBTYPE, SPA_TYPE_ID) => media.1 = read_u32(value, 0),
            (SPA_FORMAT_VIDEO_FORMAT, SPA_TYPE_ID) => {
                layout = match read_u32(value, 0)? {
                    SPA_VIDEO_FORMAT_BGRX | SPA_VIDEO_FORMAT_BGRA => Some(PixelLayout::Bgrx),
                    SPA_VIDEO_FORMAT_RGBX | SPA_VIDEO_FORMAT_RGBA => Some(PixelLayout::Rgbx),
                    _ => None,
                }
            }
            (SPA_FORMAT_VIDEO_SIZE, SPA_TYPE_RECTANGLE) => {
                size = Some((read_u32(value, 0)?, read_u32(value, 4)?))
            }
            _ => {}
        }
        offset += 16 + value_size.next_multiple_of(8);
    }
    if media != (Some(SPA_MEDIA_TYPE_VIDEO), Some(SPA_MEDIA_SUBTYPE_RAW)) {
        return None;
    }
    let (width, height) = size.filter(|(width, height)| *width > 0 && *height > 0)?;
    Some(VideoFormat {
        layout: layout?,
        width,
        height,
    })
}

/// The value of a property: a plain pod, or the first (current/default)
/// value of a choice.
fn current_value(value_type: u32, value: &[u8]) -> Option<(u32, &[u8])> {
    if value_type != SPA_TYPE_CHOICE {
        return Some((value_type, value));
    }
    let child_size = read_u32(value, 8)? as usize;
    let child_type = read_u32(value, 12)?;
    Some((child_type, value.get(16..16usize.checked_add(child_size)?)?))
}

fn read_u32(bytes: &[u8], offset: usize) -> Option<u32> {
    let bytes = bytes.get(offset..offset.checked_add(4)?)?;
    Some(u32::from_ne_bytes(bytes.try_into().ok()?))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hex(text: &str) -> Vec<u8> {
        let digits: Vec<u8> = text.bytes().filter(u8::is_ascii_hexdigit).collect();
        digits
            .chunks_exact(2)
            .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
            .collect()
    }

    fn pod_bytes(words: &[u64]) -> Vec<u8> {
        words.iter().flat_map(|word| word.to_ne_bytes()).collect()
    }

    /// Bytes of a negotiated Format object as `spa_pod_builder_add_object`
    /// writes it: RGBA, 2560x1440, framerate 0/1.
    const FIXED_RGBA_FORMAT: &str = "
        800000000f000000030004000400000001000000000000000400000003000000
        0200000000000000020000000000000004000000030000000100000000000000
        010002000000000004000000030000000b000000000000000300020000000000
        080000000a000000000a0000a00500000400020000000000080000000b000000
        0000000001000000";

    #[test]
    #[cfg(target_endian = "little")]
    fn enum_format_matches_libspa_builder_output() {
        // spa_pod_builder_add_object with the same properties (PipeWire 1.0).
        let expected = hex("
            e00000000f000000030004000300000001000000000000000400000003000000
            0200000000000000020000000000000004000000030000000100000000000000
            0100020000000000240000001300000003000000000000000400000003000000
            08000000080000000c000000070000000b000000000000000300020000000000
            28000000130000000100000000000000080000000a0000008007000038040000
            0100000001000000004000000040000004000200000000002800000013000000
            0100000000000000080000000b0000001e000000010000000000000001000000
            e803000001000000");

        assert_eq!(pod_bytes(&enum_format_pod()), expected);
    }

    #[test]
    #[cfg(target_endian = "little")]
    fn parses_negotiated_format_from_libspa_bytes() {
        assert_eq!(
            parse_video_format(&hex(FIXED_RGBA_FORMAT)),
            Some(VideoFormat {
                layout: PixelLayout::Rgbx,
                width: 2560,
                height: 1440,
            })
        );
    }

    #[test]
    fn parses_choice_wrapped_values_and_rejects_other_media() {
        let mut pod = PodWriter::default();
        pod.object(SPA_TYPE_OBJECT_FORMAT, SPA_PARAM_FORMAT, |pod| {
            pod.property(SPA_FORMAT_MEDIA_TYPE);
            pod.id(SPA_MEDIA_TYPE_VIDEO);
            pod.property(SPA_FORMAT_MEDIA_SUBTYPE);
            pod.id(SPA_MEDIA_SUBTYPE_RAW);
            pod.property(SPA_FORMAT_VIDEO_FORMAT);
            pod.choice(0, SPA_TYPE_ID, 1, &[SPA_VIDEO_FORMAT_BGRA]);
            pod.property(SPA_FORMAT_VIDEO_SIZE);
            pod.choice(0, SPA_TYPE_RECTANGLE, 2, &[800, 600]);
        });
        assert_eq!(
            parse_video_format(&pod.bytes),
            Some(VideoFormat {
                layout: PixelLayout::Bgrx,
                width: 800,
                height: 600,
            })
        );

        let mut audio = PodWriter::default();
        audio.object(SPA_TYPE_OBJECT_FORMAT, SPA_PARAM_FORMAT, |pod| {
            pod.property(SPA_FORMAT_MEDIA_TYPE);
            pod.id(1);
            pod.property(SPA_FORMAT_MEDIA_SUBTYPE);
            pod.id(SPA_MEDIA_SUBTYPE_RAW);
        });
        assert_eq!(parse_video_format(&audio.bytes), None);
    }

    #[test]
    fn truncated_format_is_rejected_without_panicking() {
        let bytes = hex(FIXED_RGBA_FORMAT);
        for len in 0..bytes.len() {
            assert_eq!(parse_video_format(&bytes[..len]), None, "length {len}");
        }
    }

    #[test]
    fn frame_conversion_drops_padding_in_both_layouts() {
        let pixels = vec![1, 2, 3, 255, 4, 5, 6, 0];
        let bgrx = Frame {
            width: 2,
            height: 1,
            layout: PixelLayout::Bgrx,
            pixels: pixels.clone(),
        };
        assert_eq!(bgrx.into_rgb_image().into_raw(), vec![3, 2, 1, 6, 5, 4]);
        let rgbx = Frame {
            width: 1,
            height: 2,
            layout: PixelLayout::Rgbx,
            pixels,
        };
        assert_eq!(rgbx.into_rgb_image().into_raw(), vec![1, 2, 3, 4, 5, 6]);
    }

    #[test]
    fn copy_frame_honours_stride_offset_and_bounds() {
        // 2x2 BGRx frame with 12-byte stride, starting 4 bytes in.
        let mut memory = vec![0u8; 4 + 12 * 2];
        for (index, byte) in memory.iter_mut().enumerate() {
            *byte = index as u8;
        }
        let mut chunk = SpaChunk {
            offset: 4,
            size: 24,
            stride: 12,
            flags: 0,
        };
        let mut data = SpaData {
            kind: 1,
            flags: 0,
            fd: -1,
            mapoffset: 0,
            maxsize: memory.len() as u32,
            data: memory.as_mut_ptr().cast(),
            chunk: &mut chunk,
        };
        let mut spa = SpaBuffer {
            n_metas: 0,
            n_datas: 1,
            metas: ptr::null_mut(),
            datas: &mut data,
        };
        let mut buffer = PwBuffer { buffer: &mut spa };
        let format = VideoFormat {
            layout: PixelLayout::Bgrx,
            width: 2,
            height: 2,
        };

        let frame = unsafe { copy_frame(&mut buffer, format) }.unwrap();
        assert_eq!(
            frame.pixels,
            [4, 5, 6, 7, 8, 9, 10, 11, 16, 17, 18, 19, 20, 21, 22, 23]
        );

        // A frame that would read past maxsize is refused.
        let taller = VideoFormat {
            height: 3,
            ..format
        };
        assert!(unsafe { copy_frame(&mut buffer, taller) }.is_none());
    }

    /// Live check against a PipeWire daemon, bypassing the portal. Start a
    /// producer, for example
    /// `gst-launch-1.0 videotestsrc ! video/x-raw,format=BGRx ! pipewiresink mode=provide`,
    /// then run with `COMPUTER_USE_KWIN_TEST_PIPEWIRE_NODE=<node id>`.
    #[test]
    #[ignore = "needs a running PipeWire daemon and COMPUTER_USE_KWIN_TEST_PIPEWIRE_NODE"]
    fn captures_a_frame_from_a_local_pipewire_node() {
        use std::os::unix::net::UnixStream;
        use std::time::{Duration, Instant};

        let node: u32 = std::env::var("COMPUTER_USE_KWIN_TEST_PIPEWIRE_NODE")
            .expect("COMPUTER_USE_KWIN_TEST_PIPEWIRE_NODE")
            .parse()
            .expect("node id");
        let runtime_dir = std::env::var("XDG_RUNTIME_DIR").expect("XDG_RUNTIME_DIR");
        let socket = UnixStream::connect(std::path::Path::new(&runtime_dir).join("pipewire-0"))
            .expect("connect to the PipeWire socket");
        let capture = PipeWireCapture::connect(OwnedFd::from(socket), &[node]).unwrap();

        let deadline = Instant::now() + Duration::from_secs(5);
        let frames = loop {
            if let Some(frames) = capture.grab().unwrap() {
                break frames;
            }
            assert!(Instant::now() < deadline, "no frame within 5s");
            std::thread::sleep(Duration::from_millis(20));
        };
        assert_eq!(frames.len(), 1);
        let image = frames.into_iter().next().unwrap().into_rgb_image();
        assert!(image.width() > 0 && image.height() > 0);
        eprintln!("captured {}x{}", image.width(), image.height());
    }
}
