//! Screenshot source backed by an XDG desktop portal ScreenCast session.
//!
//! The first capture asks the user to share their monitors, then keeps the
//! PipeWire streams open so later captures copy the newest frame instead of
//! waiting on a Screenshot portal round trip. The session closes after it has
//! gone unused for the idle timeout, and the next capture asks again. If the
//! user declines, or PipeWire or the portal is unusable, this process falls
//! back to the Screenshot portal and stops asking.

use crate::{
    pipewire::{self, Frame, PipeWireCapture},
    remote_desktop::{
        await_portal_response, close_portal_session, insert_screencast_source_options,
        last_path_component, parse_streams, portal_request_stream, request_token, screencast_proxy,
        PortalSessionCleanup, PortalStream, PORTAL_CALL_TIMEOUT,
    },
    screenshot_impl::RawScreenshotCapture,
    windowing::backends::kwin,
};
use anyhow::{anyhow, bail, Context, Result};
use image::{
    codecs::png::{CompressionType, FilterType as PngFilter, PngEncoder},
    imageops::{self, FilterType},
    ExtendedColorType, ImageEncoder, RgbImage,
};
use std::{
    collections::HashMap,
    os::fd::OwnedFd,
    sync::{
        atomic::{AtomicU64, Ordering},
        OnceLock,
    },
    time::{Duration, Instant},
};
use tokio::sync::Mutex as AsyncMutex;
use zbus::{
    zvariant::{OwnedObjectPath, OwnedValue, Value},
    Connection,
};

const SCREENCAST_ENV: &str = "COMPUTER_USE_KWIN_SCREENCAST";
const SCREENCAST_IDLE_ENV: &str = "COMPUTER_USE_KWIN_SCREENCAST_IDLE_SECS";
const DEFAULT_IDLE_TIMEOUT: Duration = Duration::from_secs(5 * 60);
/// How long a freshly started stream may take to deliver its first frame.
const FIRST_FRAME_TIMEOUT: Duration = Duration::from_secs(5);
/// How long a running stream may go without a usable frame, for example
/// while the compositor renegotiates buffers after a mode change.
const FRAME_TIMEOUT: Duration = Duration::from_secs(1);
const FRAME_POLL: Duration = Duration::from_millis(10);
const CLOSE_TIMEOUT: Duration = Duration::from_secs(1);
const MAX_CANVAS_DIMENSION: u32 = 16384;
const SOURCE: &str = "xdg-desktop-portal-screencast";

/// Logical `(x, y, width, height)`, the coordinate space of KWin's
/// `virtualScreenGeometry` and the portal's stream `position`/`size`.
type LogicalRect = (i32, i32, i32, i32);

enum State {
    Idle,
    Active(Box<Session>),
    /// The user declined, or screen-cast capture cannot work here.
    Disabled,
}

struct Session {
    id: u64,
    connection: Connection,
    session_handle: OwnedObjectPath,
    capture: PipeWireCapture,
    /// Where each stream sits on the desktop, in stream order.
    rects: Vec<Option<LogicalRect>>,
    workspace: Option<LogicalRect>,
    frame_sizes: Vec<(u32, u32)>,
    last_used: Instant,
}

enum CaptureError {
    /// The streams are gone (the user stopped sharing, or PipeWire failed).
    Ended(anyhow::Error),
    /// This capture failed but the session may still work.
    Failed(anyhow::Error),
}

fn state() -> &'static AsyncMutex<State> {
    static STATE: OnceLock<AsyncMutex<State>> = OnceLock::new();
    STATE.get_or_init(|| AsyncMutex::new(State::Idle))
}

/// Capture the desktop from the shared screen-cast session, starting one
/// (and prompting the user) when none is open.
///
/// `None` means the caller should use the Screenshot portal instead: capture
/// is disabled, the user declined, or this particular frame failed.
pub(crate) async fn capture_raw() -> Option<RawScreenshotCapture> {
    if std::env::var(SCREENCAST_ENV).ok().as_deref() == Some("0") {
        return None;
    }
    let mut state = state().lock().await;
    match &mut *state {
        State::Disabled => return None,
        State::Idle => {}
        State::Active(session) => match session.capture().await {
            Ok(raw) => return Some(raw),
            Err(CaptureError::Failed(error)) => {
                eprintln!(
                    "[computer-use-kwin] screen-cast capture failed ({error:#}); using the screenshot portal for this capture"
                );
                return None;
            }
            Err(CaptureError::Ended(error)) => {
                eprintln!(
                    "[computer-use-kwin] screen-cast session ended ({error:#}); asking to share the screen again"
                );
                if let State::Active(session) = std::mem::replace(&mut *state, State::Idle) {
                    session.close().await;
                }
            }
        },
    }

    let mut session = match Session::start().await {
        Ok(session) => session,
        Err(error) => {
            eprintln!(
                "[computer-use-kwin] screen-cast capture disabled for this process ({error:#}); screenshots use the screenshot portal"
            );
            *state = State::Disabled;
            return None;
        }
    };
    let capture = session.capture().await;
    let id = session.id;
    *state = State::Active(Box::new(session));
    tokio::spawn(close_when_idle(id, idle_timeout()));
    match capture {
        Ok(raw) => Some(raw),
        Err(CaptureError::Failed(error) | CaptureError::Ended(error)) => {
            eprintln!(
                "[computer-use-kwin] screen-cast capture failed ({error:#}); using the screenshot portal for this capture"
            );
            None
        }
    }
}

/// Close session `id` once it has gone `idle` without a capture.
async fn close_when_idle(id: u64, idle: Duration) {
    loop {
        let deadline = match &*state().lock().await {
            State::Active(session) if session.id == id => session.last_used + idle,
            _ => return,
        };
        tokio::time::sleep_until(deadline.into()).await;

        let mut state = state().lock().await;
        match &*state {
            State::Active(session) if session.id == id => {
                if session.last_used + idle > Instant::now() {
                    continue;
                }
            }
            _ => return,
        }
        if let State::Active(session) = std::mem::replace(&mut *state, State::Idle) {
            session.close().await;
        }
        return;
    }
}

fn idle_timeout() -> Duration {
    let Ok(value) = std::env::var(SCREENCAST_IDLE_ENV) else {
        return DEFAULT_IDLE_TIMEOUT;
    };
    match value.trim().parse::<u64>() {
        Ok(seconds) if seconds > 0 => Duration::from_secs(seconds),
        _ => {
            static ONCE: OnceLock<()> = OnceLock::new();
            if ONCE.set(()).is_ok() {
                eprintln!(
                    "[computer-use-kwin] ignoring {SCREENCAST_IDLE_ENV}={value:?}; expected a whole number of seconds above 0"
                );
            }
            DEFAULT_IDLE_TIMEOUT
        }
    }
}

impl Session {
    async fn start() -> Result<Self> {
        static NEXT_ID: AtomicU64 = AtomicU64::new(1);
        pipewire::ensure_available()?;

        let connection = Connection::session()
            .await
            .context("failed to connect to session bus for the ScreenCast portal")?;
        let session_handle = create_session(&connection).await?;
        let mut cleanup = PortalSessionCleanup::new(connection.clone(), session_handle.clone());
        select_monitor_sources(&connection, &session_handle).await?;
        let streams = start_session(&connection, &session_handle).await?;
        if streams.is_empty() {
            bail!("the ScreenCast portal started without any monitor streams");
        }
        let remote = open_pipewire_remote(&connection, &session_handle).await?;
        let node_ids: Vec<u32> = streams.iter().map(|stream| stream.node_id).collect();
        let capture = PipeWireCapture::connect(remote, &node_ids)?;
        let frames = next_frames(&capture, FIRST_FRAME_TIMEOUT)
            .await
            .map_err(CaptureError::into_inner)?;
        let workspace = logical_workspace().await;
        cleanup.disarm();

        Ok(Self {
            id: NEXT_ID.fetch_add(1, Ordering::Relaxed),
            connection,
            session_handle,
            capture,
            rects: streams.iter().map(stream_rect).collect(),
            workspace,
            frame_sizes: frame_sizes(&frames),
            last_used: Instant::now(),
        })
    }

    async fn capture(&mut self) -> Result<RawScreenshotCapture, CaptureError> {
        self.last_used = Instant::now();
        let frames = next_frames(&self.capture, FRAME_TIMEOUT).await?;
        let sizes = frame_sizes(&frames);
        if sizes != self.frame_sizes {
            // An output changed mode or scale, so the workspace may have too.
            self.workspace = logical_workspace().await;
            self.frame_sizes = sizes;
        }
        let rects = self.rects.clone();
        let workspace = self.workspace;
        let raw = tokio::task::spawn_blocking(move || render(frames, rects, workspace))
            .await
            .map_err(|error| {
                CaptureError::Failed(anyhow!("screen-cast render task failed: {error}"))
            })?
            .map_err(CaptureError::Failed)?;
        self.last_used = Instant::now();
        Ok(raw)
    }

    async fn close(self) {
        let Self {
            connection,
            session_handle,
            capture,
            ..
        } = self;
        drop(capture);
        let _ = tokio::time::timeout(
            CLOSE_TIMEOUT,
            close_portal_session(&connection, &session_handle),
        )
        .await;
    }
}

impl CaptureError {
    fn into_inner(self) -> anyhow::Error {
        match self {
            Self::Ended(error) | Self::Failed(error) => error,
        }
    }
}

async fn next_frames(
    capture: &PipeWireCapture,
    timeout: Duration,
) -> Result<Vec<Frame>, CaptureError> {
    let deadline = Instant::now() + timeout;
    loop {
        match capture.grab() {
            Ok(Some(frames)) => return Ok(frames),
            Ok(None) if Instant::now() < deadline => tokio::time::sleep(FRAME_POLL).await,
            Ok(None) => {
                return Err(CaptureError::Failed(anyhow!(
                    "no screen-cast frame arrived within {timeout:?}"
                )))
            }
            Err(error) => return Err(CaptureError::Ended(error)),
        }
    }
}

fn frame_sizes(frames: &[Frame]) -> Vec<(u32, u32)> {
    frames
        .iter()
        .map(|frame| (frame.width(), frame.height()))
        .collect()
}

fn stream_rect(stream: &PortalStream) -> Option<LogicalRect> {
    let ((x, y), (width, height)) = stream.position.zip(stream.size)?;
    (width > 0 && height > 0).then_some((x, y, width, height))
}

async fn logical_workspace() -> Option<LogicalRect> {
    kwin::logical_desktop_rect().await.ok()
}

fn render(
    frames: Vec<Frame>,
    rects: Vec<Option<LogicalRect>>,
    workspace: Option<LogicalRect>,
) -> Result<RawScreenshotCapture> {
    let parts = frames
        .into_iter()
        .zip(rects)
        .map(|(frame, rect)| Part {
            rect,
            image: frame.into_rgb_image(),
        })
        .collect();
    let image = compose_desktop(parts, workspace)?;
    let mut bytes = Vec::new();
    // Fast compression: this PNG is usually decoded again for resizing, so
    // encode speed matters more than size here.
    PngEncoder::new_with_quality(&mut bytes, CompressionType::Fast, PngFilter::Adaptive)
        .write_image(
            image.as_raw(),
            image.width(),
            image.height(),
            ExtendedColorType::Rgb8,
        )
        .context("failed to encode screen-cast frame as PNG")?;
    Ok(RawScreenshotCapture {
        mime_type: "image/png".to_string(),
        bytes,
        source: SOURCE.to_string(),
        width: image.width(),
        height: image.height(),
    })
}

struct Part {
    rect: Option<LogicalRect>,
    image: RgbImage,
}

/// Lay the monitor frames out as one image of the whole workspace, so the
/// result has the same coordinate space as a Screenshot portal capture.
///
/// The image is rendered at the highest stream scale (frame pixels per
/// logical pixel). Monitors that were not shared stay black.
fn compose_desktop(mut parts: Vec<Part>, workspace: Option<LogicalRect>) -> Result<RgbImage> {
    if let [part] = parts.as_slice() {
        let covers_workspace = match (part.rect, workspace) {
            (Some(rect), Some(workspace)) => rect == workspace,
            _ => true,
        };
        if covers_workspace {
            return Ok(parts.remove(0).image);
        }
    }
    let rects: Vec<LogicalRect> = parts
        .iter()
        .map(|part| part.rect)
        .collect::<Option<_>>()
        .context(
            "a shared monitor has no position or size, so it cannot be placed on the desktop image",
        )?;
    let bounds = match workspace {
        Some(workspace) => workspace,
        None => union(&rects).context("no shared monitor has a usable size")?,
    };
    let scale = parts
        .iter()
        .zip(&rects)
        .map(|(part, rect)| f64::from(part.image.width()) / f64::from(rect.2))
        .fold(0.0, f64::max);
    if !scale.is_finite() || scale <= 0.0 {
        bail!("could not determine the screen-cast scale");
    }

    let mut canvas = RgbImage::new(scaled(bounds.2, scale)?, scaled(bounds.3, scale)?);
    for (part, rect) in parts.into_iter().zip(rects) {
        let (width, height) = (scaled(rect.2, scale)?, scaled(rect.3, scale)?);
        let image = if part.image.dimensions() == (width, height) {
            part.image
        } else {
            imageops::resize(&part.image, width, height, FilterType::Triangle)
        };
        let x = (f64::from(rect.0 - bounds.0) * scale).round() as i64;
        let y = (f64::from(rect.1 - bounds.1) * scale).round() as i64;
        imageops::replace(&mut canvas, &image, x, y);
    }
    Ok(canvas)
}

fn union(rects: &[LogicalRect]) -> Option<LogicalRect> {
    let left = rects.iter().map(|rect| rect.0).min()?;
    let top = rects.iter().map(|rect| rect.1).min()?;
    let right = rects.iter().map(|rect| rect.0 + rect.2).max()?;
    let bottom = rects.iter().map(|rect| rect.1 + rect.3).max()?;
    Some((left, top, right - left, bottom - top))
}

fn scaled(length: i32, scale: f64) -> Result<u32> {
    let pixels = (f64::from(length) * scale).round();
    if !(1.0..=f64::from(MAX_CANVAS_DIMENSION)).contains(&pixels) {
        bail!("screen-cast desktop dimension {pixels} is out of range");
    }
    Ok(pixels as u32)
}

async fn create_session(connection: &Connection) -> Result<OwnedObjectPath> {
    let proxy = screencast_proxy(connection).await?;
    let (request_path, mut responses) = portal_request_stream(connection, "sc_create").await?;
    let session_token = request_token("sc_session");
    let mut options: HashMap<&str, Value<'_>> = HashMap::new();
    options.insert(
        "handle_token",
        Value::from(last_path_component(&request_path)),
    );
    options.insert("session_handle_token", Value::from(session_token.as_str()));

    let handle: OwnedObjectPath =
        tokio::time::timeout(PORTAL_CALL_TIMEOUT, proxy.call("CreateSession", &(options)))
            .await
            .context("ScreenCast CreateSession call timed out")?
            .context("ScreenCast CreateSession call failed")?;
    let results = granted(
        "CreateSession",
        await_portal_response(connection, handle, &request_path, &mut responses).await?,
    )?;
    let session_handle: String = results
        .get("session_handle")
        .context("ScreenCast CreateSession response did not include session_handle")?
        .try_clone()
        .context("failed to clone session_handle")?
        .try_into()
        .context("ScreenCast session_handle was not a string")?;
    OwnedObjectPath::try_from(session_handle)
        .context("ScreenCast session_handle was not a valid object path")
}

async fn select_monitor_sources(connection: &Connection, session: &OwnedObjectPath) -> Result<()> {
    let proxy = screencast_proxy(connection).await?;
    let (request_path, mut responses) = portal_request_stream(connection, "sc_sources").await?;
    // No persist_mode: every new session asks the user again.
    let mut options: HashMap<&str, Value<'_>> = HashMap::new();
    insert_screencast_source_options(&mut options, last_path_component(&request_path));

    let handle: OwnedObjectPath = tokio::time::timeout(
        PORTAL_CALL_TIMEOUT,
        proxy.call("SelectSources", &(session, options)),
    )
    .await
    .context("ScreenCast SelectSources call timed out")?
    .context("ScreenCast SelectSources call failed")?;
    granted(
        "SelectSources",
        await_portal_response(connection, handle, &request_path, &mut responses).await?,
    )?;
    Ok(())
}

async fn start_session(
    connection: &Connection,
    session: &OwnedObjectPath,
) -> Result<Vec<PortalStream>> {
    let proxy = screencast_proxy(connection).await?;
    let (request_path, mut responses) = portal_request_stream(connection, "sc_start").await?;
    let mut options: HashMap<&str, Value<'_>> = HashMap::new();
    options.insert(
        "handle_token",
        Value::from(last_path_component(&request_path)),
    );

    let handle: OwnedObjectPath = tokio::time::timeout(
        PORTAL_CALL_TIMEOUT,
        proxy.call("Start", &(session, "", options)),
    )
    .await
    .context("ScreenCast Start call timed out")?
    .context("ScreenCast Start call failed")?;
    let results = granted(
        "Start",
        await_portal_response(connection, handle, &request_path, &mut responses).await?,
    )?;
    Ok(results
        .get("streams")
        .map(parse_streams)
        .transpose()?
        .unwrap_or_default())
}

async fn open_pipewire_remote(
    connection: &Connection,
    session: &OwnedObjectPath,
) -> Result<OwnedFd> {
    let proxy = screencast_proxy(connection).await?;
    let options: HashMap<&str, Value<'_>> = HashMap::new();
    let remote: zbus::zvariant::OwnedFd = tokio::time::timeout(
        PORTAL_CALL_TIMEOUT,
        proxy.call("OpenPipeWireRemote", &(session, options)),
    )
    .await
    .context("ScreenCast OpenPipeWireRemote call timed out")?
    .context("ScreenCast OpenPipeWireRemote call failed")?;
    Ok(remote.into())
}

fn granted(
    step: &str,
    (response_code, results): (u32, HashMap<String, OwnedValue>),
) -> Result<HashMap<String, OwnedValue>> {
    match response_code {
        0 => Ok(results),
        1 => bail!("the screen-share request was declined (ScreenCast {step})"),
        code => bail!("ScreenCast {step} failed with response code {code}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::Rgb;

    fn solid(width: u32, height: u32, value: u8) -> RgbImage {
        RgbImage::from_pixel(width, height, Rgb([value; 3]))
    }

    fn part(rect: Option<LogicalRect>, image: RgbImage) -> Part {
        Part { rect, image }
    }

    #[test]
    fn single_stream_covering_the_workspace_is_returned_unchanged() {
        let image = compose_desktop(
            vec![part(Some((0, 0, 1280, 720)), solid(2560, 1440, 7))],
            Some((0, 0, 1280, 720)),
        )
        .unwrap();
        assert_eq!(image.dimensions(), (2560, 1440));
    }

    #[test]
    fn single_stream_without_placement_is_returned_unchanged() {
        let image = compose_desktop(
            vec![part(None, solid(300, 200, 7))],
            Some((0, 0, 1000, 500)),
        )
        .unwrap();
        assert_eq!(image.dimensions(), (300, 200));
    }

    #[test]
    fn side_by_side_monitors_are_placed_by_logical_position() {
        let image = compose_desktop(
            vec![
                part(Some((100, 0, 100, 50)), solid(100, 50, 2)),
                part(Some((0, 0, 100, 50)), solid(100, 50, 1)),
            ],
            Some((0, 0, 200, 50)),
        )
        .unwrap();
        assert_eq!(image.dimensions(), (200, 50));
        assert_eq!(image.get_pixel(50, 25), &Rgb([1; 3]));
        assert_eq!(image.get_pixel(150, 25), &Rgb([2; 3]));
    }

    #[test]
    fn unshared_monitor_stays_black_and_keeps_workspace_size() {
        let image = compose_desktop(
            vec![part(Some((0, 0, 100, 50)), solid(100, 50, 9))],
            Some((0, 0, 200, 50)),
        )
        .unwrap();
        assert_eq!(image.dimensions(), (200, 50));
        assert_eq!(image.get_pixel(10, 10), &Rgb([9; 3]));
        assert_eq!(image.get_pixel(150, 10), &Rgb([0; 3]));
    }

    #[test]
    fn mixed_scale_monitors_render_at_the_highest_scale() {
        // Left monitor at scale 2, right monitor at scale 1, offset workspace.
        let image = compose_desktop(
            vec![
                part(Some((-100, 0, 100, 50)), solid(200, 100, 3)),
                part(Some((0, 0, 100, 50)), solid(100, 50, 4)),
            ],
            None,
        )
        .unwrap();
        assert_eq!(image.dimensions(), (400, 100));
        assert_eq!(image.get_pixel(100, 50), &Rgb([3; 3]));
        assert_eq!(image.get_pixel(300, 50), &Rgb([4; 3]));
    }

    #[test]
    fn multiple_streams_without_placement_are_refused() {
        let result = compose_desktop(
            vec![
                part(None, solid(10, 10, 1)),
                part(Some((10, 0, 10, 10)), solid(10, 10, 2)),
            ],
            None,
        );
        assert!(result.is_err());
    }

    #[test]
    fn render_encodes_the_frame_as_png() {
        // 2x1 BGRx: pure blue, then pure red.
        let frame = Frame::from_bgrx(2, 1, vec![255, 0, 0, 0, 0, 0, 255, 0]);
        let raw = render(vec![frame], vec![Some((0, 0, 2, 1))], Some((0, 0, 2, 1))).unwrap();

        assert_eq!(raw.mime_type, "image/png");
        assert_eq!(raw.source, SOURCE);
        assert_eq!((raw.width, raw.height), (2, 1));
        let decoded = image::load_from_memory_with_format(&raw.bytes, image::ImageFormat::Png)
            .unwrap()
            .to_rgb8();
        assert_eq!(decoded.into_raw(), vec![0, 0, 255, 255, 0, 0]);
    }

    #[test]
    fn render_without_streams_fails() {
        assert!(render(Vec::new(), Vec::new(), None).is_err());
    }

    #[test]
    fn declined_dialog_is_reported_as_declined() {
        let error = granted("Start", (1, HashMap::new())).unwrap_err();
        assert!(error.to_string().contains("declined"));
        assert!(granted("Start", (0, HashMap::new())).is_ok());
        assert!(granted("Start", (2, HashMap::new()))
            .unwrap_err()
            .to_string()
            .contains("response code 2"));
    }
}
