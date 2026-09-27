use crate::diagnostics::hydrate_session_bus_env;
use crate::terminal::enrich_terminal_windows;
use crate::windowing::registry::BackendProbe;
use crate::windowing::types::{WindowBounds, WindowInfo};
use anyhow::{bail, Context, Result};
use serde::Deserialize;
use std::{
    fs::{self, OpenOptions},
    io::Write,
    sync::atomic::{AtomicBool, AtomicU64, Ordering},
    sync::mpsc,
    time::Duration,
};
use tokio::time::{sleep, timeout};
use zbus::{
    message::Header,
    names::{BusName, OwnedUniqueName},
    Proxy,
};

pub const KWIN_BACKEND: &str = "kwin";
const KWIN_SCRIPT_TIMEOUT: Duration = Duration::from_secs(2);
const KWIN_SCRIPTING_SERVICE: &str = "org.kde.KWin";
const KWIN_SCRIPTING_OBJECT_PATH: &str = "/Scripting";
const KWIN_SCRIPTING_INTERFACE: &str = "org.kde.kwin.Scripting";
const KWIN_CALLBACK_OBJECT_PATH_PREFIX: &str = "/dev/avifenesh/ComputerUseLinux/KWinWindowQuery";
const KWIN_CALLBACK_INTERFACE: &str = "dev.avifenesh.ComputerUseLinux.KWinWindowQuery";
static KWIN_PLUGIN_SEQUENCE: AtomicU64 = AtomicU64::new(0);

pub fn probe() -> BackendProbe {
    let check = gdbus_introspect_contains(
        "org.kde.KWin",
        "/Scripting",
        "org.kde.kwin.Scripting",
        "loadScript",
    );
    BackendProbe {
        id: KWIN_BACKEND,
        ok: check.ok,
        can_list_windows: check.ok,
        can_focus_apps: check.ok,
        can_focus_windows: check.ok,
        detail: if check.ok {
            "KWin scripting is available on the session bus".to_string()
        } else {
            format!("KWin scripting unavailable: {}", check.detail)
        },
    }
}

pub async fn list_windows() -> Result<Vec<WindowInfo>> {
    let json = call_kwin_window_script().await?;
    let mut windows = parse_kwin_windows(&json)?;
    enrich_terminal_windows(&mut windows);
    Ok(windows)
}

pub(crate) async fn logical_desktop_rect() -> Result<(i32, i32, i32, i32)> {
    let json = call_kwin_window_script().await?;
    parse_kwin_logical_desktop_rect(&json)
}

pub async fn activate_window(window_id: u64) -> Result<()> {
    let uuid = kwin_uuid_for_window_id(window_id).await?.with_context(|| {
        format!("No KWin window matched window_id {window_id} during activation")
    })?;
    call_kwin_activate_script(&uuid).await
}

pub async fn move_window(window_id: u64, x: i32, y: i32) -> Result<String> {
    let (uuid, frame) = kwin_frame_for_window_id(window_id)
        .await?
        .with_context(|| format!("No KWin window matched window_id {window_id} during move"))?;
    call_kwin_move_script(&uuid, x, y, frame.width, frame.height).await
}

pub async fn resize_window(window_id: u64, width: i32, height: i32) -> Result<String> {
    if width <= 0 || height <= 0 {
        bail!("KWin resize dimensions must be positive (received {width}x{height})");
    }
    let (uuid, frame) = kwin_frame_for_window_id(window_id)
        .await?
        .with_context(|| format!("No KWin window matched window_id {window_id} during resize"))?;
    call_kwin_resize_script(&uuid, width, height, frame.x, frame.y).await
}

async fn kwin_uuid_for_window_id(window_id: u64) -> Result<Option<String>> {
    let json = call_kwin_window_script().await?;
    let snapshot = parse_kwin_snapshot(&json)?;
    Ok(snapshot.windows.into_iter().find_map(|window| {
        let uuid = window.kwin_uuid()?;
        (kwin_window_id_from_uuid(&uuid) == window_id).then_some(uuid)
    }))
}

async fn kwin_frame_for_window_id(window_id: u64) -> Result<Option<(String, KwinFrameGeometry)>> {
    let json = call_kwin_window_script().await?;
    let snapshot = parse_kwin_snapshot(&json)?;
    let Some(window) = snapshot.windows.into_iter().find(|window| {
        window
            .kwin_uuid()
            .is_some_and(|uuid| kwin_window_id_from_uuid(&uuid) == window_id)
    }) else {
        return Ok(None);
    };
    let uuid = window
        .kwin_uuid()
        .context("KWin target window did not include a usable uuid")?;
    let frame = window.frame_geometry()?;
    Ok(Some((uuid, frame)))
}

#[derive(Debug, Deserialize)]
struct KwinScriptResult {
    #[serde(default)]
    ok: bool,
    error: Option<String>,
}

async fn call_kwin_activate_script(uuid: &str) -> Result<()> {
    let uuid = uuid.to_string();
    let json = call_kwin_script(
        KwinCallbackKind::Result,
        move |service_name, callback_object_path, plugin_name| {
            write_kwin_activate_script(service_name, callback_object_path, plugin_name, &uuid)
        },
    )
    .await?;
    let result: KwinScriptResult =
        serde_json::from_str(&json).context("failed to parse KWin activation script output")?;

    if result.ok {
        Ok(())
    } else {
        bail!(
            "KWin activation script refused activation: {}",
            result.error.unwrap_or_else(|| "unknown error".to_string())
        );
    }
}

async fn call_kwin_move_script(
    uuid: &str,
    x: i32,
    y: i32,
    width: i32,
    height: i32,
) -> Result<String> {
    let uuid = uuid.to_string();
    call_kwin_geometry_script(
        "move",
        move |service_name, callback_object_path, plugin_name| {
            write_kwin_move_script(
                service_name,
                callback_object_path,
                plugin_name,
                &uuid,
                x,
                y,
                width,
                height,
            )
        },
    )
    .await
}

async fn call_kwin_resize_script(
    uuid: &str,
    width: i32,
    height: i32,
    x: i32,
    y: i32,
) -> Result<String> {
    let uuid = uuid.to_string();
    call_kwin_geometry_script(
        "resize",
        move |service_name, callback_object_path, plugin_name| {
            write_kwin_resize_script(
                service_name,
                callback_object_path,
                plugin_name,
                &uuid,
                width,
                height,
                x,
                y,
            )
        },
    )
    .await
}

async fn call_kwin_geometry_script<F>(operation: &str, write_script: F) -> Result<String>
where
    F: FnOnce(&str, &str, &str) -> Result<std::path::PathBuf>,
{
    let json = call_kwin_script(KwinCallbackKind::Result, write_script).await?;
    let result: KwinScriptResult =
        serde_json::from_str(&json).context("failed to parse KWin geometry script output")?;
    if result.ok {
        Ok(json)
    } else {
        bail!(
            "KWin {operation} script refused the operation: {}",
            result.error.unwrap_or_else(|| "unknown error".to_string())
        );
    }
}

async fn call_kwin_window_script() -> Result<String> {
    call_kwin_script(KwinCallbackKind::Windows, write_kwin_window_script).await
}

async fn call_kwin_script<F>(expected_kind: KwinCallbackKind, write_script: F) -> Result<String>
where
    F: FnOnce(&str, &str, &str) -> Result<std::path::PathBuf>,
{
    hydrate_session_bus_env();

    let connection = timeout(KWIN_SCRIPT_TIMEOUT, zbus::Connection::session())
        .await
        .context("timed out while connecting to the session bus for KWin scripting")?
        .context("failed to connect to session bus")?;

    call_kwin_script_on_connection(connection, expected_kind, write_script, KWIN_SCRIPT_TIMEOUT)
        .await
}

async fn call_kwin_script_on_connection<F>(
    connection: zbus::Connection,
    expected_kind: KwinCallbackKind,
    write_script: F,
    transaction_timeout: Duration,
) -> Result<String>
where
    F: FnOnce(&str, &str, &str) -> Result<std::path::PathBuf>,
{
    call_kwin_script_on_connection_with_plugin_name(
        connection,
        expected_kind,
        write_script,
        transaction_timeout,
        temporary_kwin_plugin_name()?,
    )
    .await
}

async fn call_kwin_script_on_connection_with_plugin_name<F>(
    connection: zbus::Connection,
    expected_kind: KwinCallbackKind,
    write_script: F,
    transaction_timeout: Duration,
    plugin_name: String,
) -> Result<String>
where
    F: FnOnce(&str, &str, &str) -> Result<std::path::PathBuf>,
{
    let unique_name = connection
        .unique_name()
        .context("session bus did not assign a unique name")?
        .to_string();
    let callback_object_path = format!("{KWIN_CALLBACK_OBJECT_PATH_PREFIX}/{plugin_name}");
    let mut cleanup = KwinScriptCleanup::new(
        connection.clone(),
        plugin_name.clone(),
        callback_object_path.clone(),
    );

    let transaction = async {
        let dbus_proxy = zbus::fdo::DBusProxy::new(&connection)
            .await
            .context("failed to create session-bus identity proxy")?;
        let expected_sender = dbus_proxy
            .get_name_owner(BusName::try_from(KWIN_SCRIPTING_SERVICE)?)
            .await
            .context("failed to resolve the KWin session-bus owner")?;
        let (sender, receiver) = mpsc::channel();
        let callback_registered = connection
            .object_server()
            .at(
                callback_object_path.as_str(),
                KwinWindowCallback {
                    sender,
                    expected_sender,
                    expected_kind,
                    plugin_name: plugin_name.clone(),
                    delivered: AtomicBool::new(false),
                },
            )
            .await
            .context("failed to register temporary KWin callback object")?;
        if !callback_registered {
            bail!("temporary KWin callback object path was already registered");
        }
        cleanup.owns_callback = true;

        let path = write_script(&unique_name, &callback_object_path, &plugin_name)?;
        cleanup.script_path = Some(path.clone());
        let scripting_proxy = Proxy::new(
            &connection,
            KWIN_SCRIPTING_SERVICE,
            KWIN_SCRIPTING_OBJECT_PATH,
            KWIN_SCRIPTING_INTERFACE,
        )
        .await
        .context("failed to create KWin scripting proxy")?;

        // Plasma 6 can return 0 here even when isScriptLoaded reports success;
        // the callback below is the authoritative completion signal.
        let _script_id: i32 = scripting_proxy
            .call(
                "loadScript",
                &(path.to_string_lossy().as_ref(), plugin_name.as_str()),
            )
            .await
            .context("KWin loadScript failed")?;

        let _: () = scripting_proxy
            .call("start", &())
            .await
            .context("KWin start failed after loading the temporary script")?;

        loop {
            match receiver.try_recv() {
                Ok(json) => return Ok(json),
                Err(mpsc::TryRecvError::Disconnected) => {
                    bail!("KWin temporary script callback disconnected before returning data");
                }
                Err(mpsc::TryRecvError::Empty) => sleep(Duration::from_millis(20)).await,
            }
        }
    };
    let result = match timeout(transaction_timeout, transaction).await {
        Ok(result) => result,
        Err(_) => Err(anyhow::anyhow!(
            "KWin temporary script transaction timed out"
        )),
    };
    cleanup.run().await;
    result
}

struct KwinScriptCleanup {
    connection: zbus::Connection,
    plugin_name: String,
    callback_object_path: String,
    script_path: Option<std::path::PathBuf>,
    owns_callback: bool,
    armed: bool,
}

impl KwinScriptCleanup {
    fn new(
        connection: zbus::Connection,
        plugin_name: String,
        callback_object_path: String,
    ) -> Self {
        Self {
            connection,
            plugin_name,
            callback_object_path,
            script_path: None,
            owns_callback: false,
            armed: true,
        }
    }

    async fn run(&mut self) {
        cleanup_kwin_script(
            self.connection.clone(),
            self.plugin_name.clone(),
            self.callback_object_path.clone(),
            self.script_path.clone(),
            self.owns_callback,
        )
        .await;
        self.script_path = None;
        self.owns_callback = false;
        self.armed = false;
    }
}

impl Drop for KwinScriptCleanup {
    fn drop(&mut self) {
        if !self.armed {
            return;
        }
        let connection = self.connection.clone();
        let plugin_name = self.plugin_name.clone();
        let callback_object_path = self.callback_object_path.clone();
        let script_path = self.script_path.take();
        let owns_callback = self.owns_callback;
        if let Ok(runtime) = tokio::runtime::Handle::try_current() {
            runtime.spawn(cleanup_kwin_script(
                connection,
                plugin_name,
                callback_object_path,
                script_path,
                owns_callback,
            ));
        }
    }
}

async fn cleanup_kwin_script(
    connection: zbus::Connection,
    plugin_name: String,
    callback_object_path: String,
    script_path: Option<std::path::PathBuf>,
    owns_callback: bool,
) {
    if owns_callback {
        let _ = timeout(Duration::from_secs(1), async {
            if let Ok(scripting_proxy) = Proxy::new(
                &connection,
                KWIN_SCRIPTING_SERVICE,
                KWIN_SCRIPTING_OBJECT_PATH,
                KWIN_SCRIPTING_INTERFACE,
            )
            .await
            {
                let _: Result<bool, _> = scripting_proxy
                    .call("unloadScript", &(plugin_name.as_str()))
                    .await;
            }
        })
        .await;
        let _: Result<bool, _> = connection
            .object_server()
            .remove::<KwinWindowCallback, _>(callback_object_path.as_str())
            .await;
    }
    if let Some(script_path) = script_path {
        let _ = fs::remove_file(script_path);
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum KwinCallbackKind {
    Windows,
    Result,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct KwinCallbackEnvelope {
    backend: String,
    plugin_name: String,
}

struct KwinWindowCallback {
    sender: mpsc::Sender<String>,
    expected_sender: OwnedUniqueName,
    expected_kind: KwinCallbackKind,
    plugin_name: String,
    delivered: AtomicBool,
}

impl KwinWindowCallback {
    fn accept(
        &self,
        actual_sender: Option<&str>,
        kind: KwinCallbackKind,
        json: &str,
    ) -> zbus::fdo::Result<()> {
        if actual_sender != Some(self.expected_sender.as_str()) {
            return Err(zbus::fdo::Error::AccessDenied(
                "KWin callback sender did not own org.kde.KWin".to_string(),
            ));
        }
        if kind != self.expected_kind {
            return Err(zbus::fdo::Error::AccessDenied(
                "KWin callback method did not match the requested operation".to_string(),
            ));
        }
        let envelope: KwinCallbackEnvelope = serde_json::from_str(json).map_err(|error| {
            zbus::fdo::Error::InvalidArgs(format!("invalid KWin callback payload: {error}"))
        })?;
        if envelope.backend != KWIN_BACKEND || envelope.plugin_name != self.plugin_name {
            return Err(zbus::fdo::Error::AccessDenied(
                "KWin callback payload did not match the active script".to_string(),
            ));
        }
        if self
            .delivered
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            return Err(zbus::fdo::Error::AccessDenied(
                "KWin callback response was already delivered".to_string(),
            ));
        }
        self.sender
            .send(json.to_string())
            .map_err(|error| zbus::fdo::Error::Failed(error.to_string()))
    }
}

#[zbus::interface(name = "dev.avifenesh.ComputerUseLinux.KWinWindowQuery")]
impl KwinWindowCallback {
    fn receive_windows(
        &self,
        #[zbus(header)] header: Header<'_>,
        json: &str,
    ) -> zbus::fdo::Result<()> {
        self.accept(
            header.sender().map(|sender| sender.as_str()),
            KwinCallbackKind::Windows,
            json,
        )
    }

    fn receive_result(
        &self,
        #[zbus(header)] header: Header<'_>,
        json: &str,
    ) -> zbus::fdo::Result<()> {
        self.accept(
            header.sender().map(|sender| sender.as_str()),
            KwinCallbackKind::Result,
            json,
        )
    }
}

fn temporary_kwin_plugin_name() -> Result<String> {
    let pid = std::process::id();
    let sequence = KWIN_PLUGIN_SEQUENCE
        .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |value| {
            value.checked_add(1)
        })
        .map_err(|_| anyhow::anyhow!("temporary KWin plugin sequence exhausted"))?;
    let mut nonce = [0_u8; 16];
    getrandom::fill(&mut nonce).map_err(|error| {
        anyhow::anyhow!("failed to generate temporary KWin plugin nonce: {error}")
    })?;
    let nonce = nonce
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    Ok(format!(
        "computer_use_linux_kwin_window_query_{pid}_{sequence}_{nonce}"
    ))
}

fn write_kwin_window_script(
    service_name: &str,
    callback_object_path: &str,
    plugin_name: &str,
) -> Result<std::path::PathBuf> {
    let script = kwin_window_script_source(service_name, callback_object_path, plugin_name)?;
    write_kwin_script_file(plugin_name, &script)
}

fn kwin_window_script_source(
    service_name: &str,
    callback_object_path: &str,
    plugin_name: &str,
) -> Result<String> {
    let service_name = serde_json::to_string(service_name)?;
    let object_path = serde_json::to_string(callback_object_path)?;
    let interface = serde_json::to_string(KWIN_CALLBACK_INTERFACE)?;
    let plugin_name_json = serde_json::to_string(plugin_name)?;
    Ok(format!(
        r#"(function() {{
    var serviceName = {service_name};
    var objectPath = {object_path};
    var iface = {interface};
    var pluginName = {plugin_name_json};

    function read(obj, key) {{
        try {{
            if (obj === null || obj === undefined) {{
                return null;
            }}
            var value = obj[key];
            if (typeof value === "function") {{
                return null;
            }}
            return serialize(value);
        }} catch (error) {{
            return null;
        }}
    }}

    function serialize(value) {{
        if (value === null || value === undefined) {{
            return null;
        }}
        if (typeof value === "string" || typeof value === "number" || typeof value === "boolean") {{
            return value;
        }}
        if (Array.isArray(value)) {{
            return value.map(serialize);
        }}
        try {{
            if (typeof value.toString === "function") {{
                return value.toString();
            }}
        }} catch (error) {{}}
        return null;
    }}

    function geometry(window) {{
        var frame = null;
        try {{
            frame = window.frameGeometry;
        }} catch (error) {{}}
        var x = read(window, "x");
        var y = read(window, "y");
        var width = read(window, "width");
        var height = read(window, "height");
        return {{
            x: x !== null ? x : read(frame, "x"),
            y: y !== null ? y : read(frame, "y"),
            width: width !== null ? width : read(frame, "width"),
            height: height !== null ? height : read(frame, "height")
        }};
    }}

    function workspaceGeometry() {{
        var rect = null;
        try {{
            rect = workspace.virtualScreenGeometry;
        }} catch (error) {{}}
        if (rect === null || rect === undefined) {{
            return null;
        }}
        return {{
            x: read(rect, "x"),
            y: read(rect, "y"),
            width: read(rect, "width"),
            height: read(rect, "height")
        }};
    }}

    function firstDesktop(window) {{
        var desktops = read(window, "desktops");
        if (!Array.isArray(desktops) || desktops.length === 0) {{
            return null;
        }}
        var first = desktops[0];
        var parsed = parseInt(first, 10);
        return isFinite(parsed) ? parsed : null;
    }}

    function clientType(window) {{
        if (read(window, "waylandClient")) {{
            return "wayland";
        }}
        if (read(window, "x11Client")) {{
            return "x11";
        }}
        var objectDescription = serialize(window);
        if (typeof objectDescription === "string") {{
            var separator = objectDescription.indexOf("(");
            var objectClass = (separator >= 0
                ? objectDescription.slice(0, separator)
                : objectDescription).trim();
            if (objectClass === "KWin::XdgToplevelWindow") {{
                return "wayland";
            }}
            if (objectClass === "KWin::X11Window") {{
                return "x11";
            }}
        }}
        return null;
    }}

    function listWindows() {{
        try {{
            if (typeof workspace.windowList === "function") {{
                return workspace.windowList();
            }}
        }} catch (error) {{}}
        try {{
            if (typeof workspace.clientList === "function") {{
                return workspace.clientList();
            }}
        }} catch (error) {{}}
        try {{
            if (workspace.stackingOrder && typeof workspace.stackingOrder.length === "number") {{
                return workspace.stackingOrder;
            }}
        }} catch (error) {{}}
        return [];
    }}

    var activeWindow = null;
    try {{
        activeWindow = "activeWindow" in workspace ? workspace.activeWindow : workspace.activeClient;
    }} catch (error) {{}}
    var windows = listWindows().map(function(window) {{
        var geo = geometry(window);
        return {{
            uuid: read(window, "uuid"),
            internalId: read(window, "internalId"),
            caption: read(window, "caption"),
            desktopFile: read(window, "desktopFile"),
            resourceClass: read(window, "resourceClass"),
            resourceName: read(window, "resourceName"),
            windowClass: read(window, "windowClass"),
            pid: read(window, "pid"),
            x: geo.x,
            y: geo.y,
            width: geo.width,
            height: geo.height,
            workspace: firstDesktop(window),
            minimized: read(window, "minimized"),
            active: read(window, "active") || window === activeWindow,
            clientType: clientType(window),
            normalWindow: read(window, "normalWindow"),
            desktopWindow: read(window, "desktopWindow"),
            skipTaskbar: read(window, "skipTaskbar"),
            dock: read(window, "dock")
        }};
    }});

    callDBus(serviceName, objectPath, iface, "ReceiveWindows", JSON.stringify({{
        backend: "kwin",
        pluginName: pluginName,
        desktopGeometry: workspaceGeometry(),
        windows: windows
    }}));
}})();
"#
    ))
}

fn write_kwin_activate_script(
    service_name: &str,
    callback_object_path: &str,
    plugin_name: &str,
    uuid: &str,
) -> Result<std::path::PathBuf> {
    let script =
        kwin_activate_script_source(service_name, callback_object_path, plugin_name, uuid)?;
    write_kwin_script_file(plugin_name, &script)
}

fn kwin_activate_script_source(
    service_name: &str,
    callback_object_path: &str,
    plugin_name: &str,
    uuid: &str,
) -> Result<String> {
    let target_uuid = normalize_kwin_uuid(uuid).context("KWin activation requires a uuid")?;
    let service_name = serde_json::to_string(service_name)?;
    let object_path = serde_json::to_string(callback_object_path)?;
    let interface = serde_json::to_string(KWIN_CALLBACK_INTERFACE)?;
    let plugin_name_json = serde_json::to_string(plugin_name)?;
    let target_uuid = serde_json::to_string(&target_uuid)?;

    Ok(format!(
        r#"(function() {{
    var serviceName = {service_name};
    var objectPath = {object_path};
    var iface = {interface};
    var pluginName = {plugin_name_json};
    var targetUuid = {target_uuid};

    function send(payload) {{
        payload.backend = "kwin";
        payload.pluginName = pluginName;
        callDBus(serviceName, objectPath, iface, "ReceiveResult", JSON.stringify(payload));
    }}

    function serialize(value) {{
        if (value === null || value === undefined) {{
            return null;
        }}
        if (typeof value === "string" || typeof value === "number" || typeof value === "boolean") {{
            return value;
        }}
        try {{
            if (typeof value.toString === "function") {{
                return value.toString();
            }}
        }} catch (error) {{}}
        return null;
    }}

    function read(obj, key) {{
        try {{
            if (obj === null || obj === undefined) {{
                return null;
            }}
            var value = obj[key];
            if (typeof value === "function") {{
                return null;
            }}
            return serialize(value);
        }} catch (error) {{
            return null;
        }}
    }}

    function normalizeUuid(value) {{
        var text = serialize(value);
        if (text === null || text === undefined) {{
            return null;
        }}
        text = String(text).trim().toLowerCase();
        if (text.charAt(0) === "{{" && text.charAt(text.length - 1) === "}}") {{
            text = text.substring(1, text.length - 1);
        }}
        return text.length > 0 ? text : null;
    }}

    function windowUuid(window) {{
        return normalizeUuid(read(window, "uuid")) || normalizeUuid(read(window, "internalId"));
    }}

    function listWindows() {{
        try {{
            if (typeof workspace.windowList === "function") {{
                return workspace.windowList();
            }}
        }} catch (error) {{}}
        try {{
            if (typeof workspace.clientList === "function") {{
                return workspace.clientList();
            }}
        }} catch (error) {{}}
        try {{
            if (workspace.stackingOrder && typeof workspace.stackingOrder.length === "number") {{
                return workspace.stackingOrder;
            }}
        }} catch (error) {{}}
        return [];
    }}

    function activateDesktop(window) {{
        var desktops = null;
        try {{
            desktops = window.desktops;
        }} catch (error) {{}}
        if (desktops && desktops.length > 0) {{
            try {{
                workspace.currentDesktop = desktops[0];
            }} catch (error) {{}}
        }}
    }}

    try {{
        var targetWindow = null;
        var windows = listWindows();
        for (var i = 0; i < windows.length; i++) {{
            if (windowUuid(windows[i]) === targetUuid) {{
                targetWindow = windows[i];
                break;
            }}
        }}

        if (!targetWindow) {{
            throw new Error("window not found: " + targetUuid);
        }}

        try {{
            targetWindow.minimized = false;
        }} catch (error) {{}}
        activateDesktop(targetWindow);

        var activated = false;
        var activationError = null;
        if ("activeWindow" in workspace) {{
            try {{
                workspace.activeWindow = targetWindow;
                activated = true;
            }} catch (error) {{
                activationError = error;
            }}
        }} else {{
            try {{
                workspace.activeClient = targetWindow;
                activated = true;
            }} catch (error) {{
                activationError = error;
            }}
        }}
        if (!activated) {{
            try {{
                if (typeof targetWindow.activate === "function") {{
                    targetWindow.activate();
                    activated = true;
                }}
            }} catch (error) {{
                activationError = error;
            }}
        }}
        if (!activated) {{
            throw activationError || new Error("workspace refused activeWindow assignment");
        }}

        try {{
            if (typeof workspace.raiseWindow === "function") {{
                workspace.raiseWindow(targetWindow);
            }}
        }} catch (error) {{}}

        send({{
            ok: true,
            uuid: windowUuid(targetWindow)
        }});
    }} catch (error) {{
        send({{
            ok: false,
            error: String(error && error.message ? error.message : error)
        }});
    }}
}})();
"#
    ))
}

fn write_kwin_move_script(
    service_name: &str,
    callback_object_path: &str,
    plugin_name: &str,
    uuid: &str,
    x: i32,
    y: i32,
    width: i32,
    height: i32,
) -> Result<std::path::PathBuf> {
    let script = kwin_move_script_source(
        service_name,
        callback_object_path,
        plugin_name,
        uuid,
        x,
        y,
        width,
        height,
    )?;
    write_kwin_script_file(plugin_name, &script)
}

fn write_kwin_resize_script(
    service_name: &str,
    callback_object_path: &str,
    plugin_name: &str,
    uuid: &str,
    width: i32,
    height: i32,
    x: i32,
    y: i32,
) -> Result<std::path::PathBuf> {
    let script = kwin_resize_script_source(
        service_name,
        callback_object_path,
        plugin_name,
        uuid,
        width,
        height,
        x,
        y,
    )?;
    write_kwin_script_file(plugin_name, &script)
}

fn kwin_move_script_source(
    service_name: &str,
    callback_object_path: &str,
    plugin_name: &str,
    uuid: &str,
    x: i32,
    y: i32,
    width: i32,
    height: i32,
) -> Result<String> {
    kwin_geometry_script_source(
        service_name,
        callback_object_path,
        plugin_name,
        uuid,
        &format!("Qt.rect({x}, {y}, {width}, {height})"),
        "move",
    )
}

fn kwin_resize_script_source(
    service_name: &str,
    callback_object_path: &str,
    plugin_name: &str,
    uuid: &str,
    width: i32,
    height: i32,
    x: i32,
    y: i32,
) -> Result<String> {
    if width <= 0 || height <= 0 {
        bail!("KWin resize dimensions must be positive (received {width}x{height})");
    }
    kwin_geometry_script_source(
        service_name,
        callback_object_path,
        plugin_name,
        uuid,
        &format!("Qt.rect({x}, {y}, {width}, {height})"),
        "resize",
    )
}

fn kwin_geometry_script_source(
    service_name: &str,
    callback_object_path: &str,
    plugin_name: &str,
    uuid: &str,
    geometry_expression: &str,
    operation: &str,
) -> Result<String> {
    let target_uuid =
        normalize_kwin_uuid(uuid).context("KWin geometry operation requires a uuid")?;
    let service_name = serde_json::to_string(service_name)?;
    let object_path = serde_json::to_string(callback_object_path)?;
    let interface = serde_json::to_string(KWIN_CALLBACK_INTERFACE)?;
    let plugin_name_json = serde_json::to_string(plugin_name)?;
    let target_uuid = serde_json::to_string(&target_uuid)?;
    let operation = serde_json::to_string(operation)?;

    Ok(format!(
        r#"(function() {{
    var serviceName = {service_name};
    var objectPath = {object_path};
    var iface = {interface};
    var pluginName = {plugin_name_json};
    var targetUuid = {target_uuid};

    function send(payload) {{
        payload.backend = "kwin";
        payload.pluginName = pluginName;
        callDBus(serviceName, objectPath, iface, "ReceiveResult", JSON.stringify(payload));
    }}

    function normalizeUuid(value) {{
        if (value === null || value === undefined) {{
            return null;
        }}
        var text = String(value).trim().toLowerCase();
        if (text.charAt(0) === "{{" && text.charAt(text.length - 1) === "}}") {{
            text = text.substring(1, text.length - 1);
        }}
        return text.length > 0 ? text : null;
    }}

    function windowUuid(window) {{
        var uuid = null;
        try {{
            uuid = normalizeUuid(window.uuid);
        }} catch (error) {{}}
        if (!uuid) {{
            try {{
                uuid = normalizeUuid(window.internalId);
            }} catch (error) {{}}
        }}
        return uuid;
    }}

    function listWindows() {{
        try {{
            if (typeof workspace.windowList === "function") {{
                return workspace.windowList();
            }}
        }} catch (error) {{}}
        try {{
            if (typeof workspace.clientList === "function") {{
                return workspace.clientList();
            }}
        }} catch (error) {{}}
        return [];
    }}

    try {{
        var windows = listWindows();
        var window = null;
        for (var i = 0; i < windows.length; i++) {{
            if (windowUuid(windows[i]) === targetUuid) {{
                window = windows[i];
                break;
            }}
        }}
        if (!window) {{
            throw new Error("window not found: " + targetUuid);
        }}

        var frame = window.frameGeometry;
        if (!frame || !isFinite(frame.x) || !isFinite(frame.y) ||
                !isFinite(frame.width) || !isFinite(frame.height) ||
                frame.width <= 0 || frame.height <= 0) {{
            throw new Error("invalid frame geometry");
        }}
        var expectedFrame = {geometry_expression};
        window.frameGeometry = {geometry_expression};
        var actualFrame = window.frameGeometry;
        if (!actualFrame || actualFrame.x !== expectedFrame.x ||
                actualFrame.y !== expectedFrame.y ||
                actualFrame.width !== expectedFrame.width ||
                actualFrame.height !== expectedFrame.height) {{
            throw new Error("KWin refused to " + {operation} + " the window");
        }}
        send({{
            ok: true,
            uuid: windowUuid(window),
            x: actualFrame.x,
            y: actualFrame.y,
            width: actualFrame.width,
            height: actualFrame.height
        }});
    }} catch (error) {{
        send({{
            ok: false,
            error: String(error && error.message ? error.message : error)
        }});
    }}
}})();
"#
    ))
}

fn write_kwin_script_file(plugin_name: &str, script: &str) -> Result<std::path::PathBuf> {
    for attempt in 0..4 {
        let filename = if attempt == 0 {
            format!("{plugin_name}.js")
        } else {
            format!("{plugin_name}-{attempt}.js")
        };
        let path = std::env::temp_dir().join(filename);
        match OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(mut file) => {
                if let Err(error) = file.write_all(script.as_bytes()) {
                    let _ = fs::remove_file(&path);
                    return Err(error).with_context(|| {
                        format!("failed to write temporary KWin script {}", path.display())
                    });
                }
                return Ok(path);
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => {
                return Err(error).with_context(|| {
                    format!("failed to create temporary KWin script {}", path.display())
                });
            }
        }
    }

    bail!("failed to create a unique temporary KWin script path for {plugin_name}")
}

fn parse_kwin_windows(json: &str) -> Result<Vec<WindowInfo>> {
    let snapshot = parse_kwin_snapshot(json)?;
    let mut windows = snapshot
        .windows
        .into_iter()
        .filter(|window| !json_value_as_bool(window.desktop_window.as_ref()).unwrap_or(false))
        .filter(|window| !json_value_as_bool(window.dock.as_ref()).unwrap_or(false))
        .filter(|window| !json_value_as_bool(window.skip_taskbar.as_ref()).unwrap_or(false))
        .filter(|window| json_value_as_bool(window.normal_window.as_ref()).unwrap_or(true))
        .map(WindowInfo::try_from)
        .collect::<Result<Vec<_>>>()?;
    windows.sort_by_key(|window| window.window_id);
    Ok(windows)
}

fn parse_kwin_logical_desktop_rect(json: &str) -> Result<(i32, i32, i32, i32)> {
    parse_kwin_snapshot(json)?.logical_desktop_rect()
}

fn parse_kwin_snapshot(json: &str) -> Result<KwinWindowSnapshot> {
    serde_json::from_str(json).context("failed to parse KWin temporary script output")
}

#[derive(Debug, Deserialize)]
struct KwinWindowSnapshot {
    #[serde(default, rename = "desktopGeometry")]
    desktop_geometry: Option<KwinRawGeometry>,
    windows: Vec<KwinRawWindow>,
}

impl KwinWindowSnapshot {
    fn logical_desktop_rect(&self) -> Result<(i32, i32, i32, i32)> {
        let geometry = self
            .desktop_geometry
            .as_ref()
            .context("KWin did not expose virtualScreenGeometry")?;
        let x = json_value_as_i32(geometry.x.as_ref())
            .context("KWin virtualScreenGeometry x is unavailable")?;
        let y = json_value_as_i32(geometry.y.as_ref())
            .context("KWin virtualScreenGeometry y is unavailable")?;
        let width = json_value_as_i32(geometry.width.as_ref())
            .filter(|width| *width > 0)
            .context("KWin virtualScreenGeometry width is unavailable")?;
        let height = json_value_as_i32(geometry.height.as_ref())
            .filter(|height| *height > 0)
            .context("KWin virtualScreenGeometry height is unavailable")?;
        Ok((x, y, width, height))
    }
}

#[derive(Debug, Deserialize)]
struct KwinRawGeometry {
    x: Option<serde_json::Value>,
    y: Option<serde_json::Value>,
    width: Option<serde_json::Value>,
    height: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct KwinRawWindow {
    uuid: Option<String>,
    internal_id: Option<String>,
    caption: Option<String>,
    desktop_file: Option<String>,
    resource_class: Option<String>,
    resource_name: Option<String>,
    window_class: Option<String>,
    pid: Option<serde_json::Value>,
    x: Option<serde_json::Value>,
    y: Option<serde_json::Value>,
    width: Option<serde_json::Value>,
    height: Option<serde_json::Value>,
    workspace: Option<serde_json::Value>,
    minimized: Option<serde_json::Value>,
    active: Option<serde_json::Value>,
    client_type: Option<String>,
    normal_window: Option<serde_json::Value>,
    desktop_window: Option<serde_json::Value>,
    skip_taskbar: Option<serde_json::Value>,
    dock: Option<serde_json::Value>,
}

impl KwinRawWindow {
    fn kwin_uuid(&self) -> Option<String> {
        self.uuid
            .as_deref()
            .or(self.internal_id.as_deref())
            .and_then(normalize_kwin_uuid)
    }

    fn frame_geometry(&self) -> Result<KwinFrameGeometry> {
        Ok(KwinFrameGeometry {
            x: frame_geometry_integer(self.x.as_ref(), "x")?,
            y: frame_geometry_integer(self.y.as_ref(), "y")?,
            width: frame_geometry_dimension(self.width.as_ref(), "width")?,
            height: frame_geometry_dimension(self.height.as_ref(), "height")?,
        })
    }
}

#[derive(Debug, Clone, Copy)]
struct KwinFrameGeometry {
    x: i32,
    y: i32,
    width: i32,
    height: i32,
}

fn frame_geometry_integer(value: Option<&serde_json::Value>, field: &str) -> Result<i32> {
    let value = value
        .and_then(serde_json::Value::as_f64)
        .filter(|value| {
            value.is_finite()
                && value.fract() == 0.0
                && *value >= i32::MIN as f64
                && *value <= i32::MAX as f64
        })
        .ok_or_else(|| {
            anyhow::anyhow!("invalid KWin frame geometry: {field} must be a finite integer")
        })?;
    Ok(value as i32)
}

fn frame_geometry_dimension(value: Option<&serde_json::Value>, field: &str) -> Result<i32> {
    let value = frame_geometry_integer(value, field)?;
    if value <= 0 {
        bail!("invalid KWin frame geometry: {field} must be a positive integer");
    }
    Ok(value)
}

impl TryFrom<KwinRawWindow> for WindowInfo {
    type Error = anyhow::Error;

    fn try_from(window: KwinRawWindow) -> Result<Self> {
        let uuid = window
            .kwin_uuid()
            .context("KWin window did not include uuid or internalId")?;
        let width = json_value_as_u32(window.width.as_ref());
        let height = json_value_as_u32(window.height.as_ref());
        let bounds = width.zip(height).map(|(width, height)| WindowBounds {
            x: json_value_as_i32(window.x.as_ref()),
            y: json_value_as_i32(window.y.as_ref()),
            width,
            height,
        });
        let app_id = clean_string(window.desktop_file.as_deref())
            .or_else(|| clean_string(window.resource_class.as_deref()));
        let wm_class = clean_string(window.resource_class.as_deref())
            .or_else(|| clean_string(window.window_class.as_deref()))
            .or_else(|| clean_string(window.resource_name.as_deref()));
        let client_type = clean_string(window.client_type.as_deref());

        Ok(WindowInfo {
            window_id: kwin_window_id_from_uuid(&uuid),
            title: clean_string(window.caption.as_deref()),
            app_id,
            wm_class,
            pid: json_value_as_u32(window.pid.as_ref()),
            bounds,
            workspace: json_value_as_i32(window.workspace.as_ref()),
            focused: json_value_as_bool(window.active.as_ref()).unwrap_or(false),
            hidden: json_value_as_bool(window.minimized.as_ref()).unwrap_or(false),
            client_type,
            backend: KWIN_BACKEND.to_string(),
            terminal: None,
        })
    }
}

fn kwin_window_id_from_uuid(uuid: &str) -> u64 {
    let normalized = normalize_kwin_uuid(uuid).unwrap_or_else(|| uuid.trim().to_ascii_lowercase());
    let mut hash = 0xcbf29ce484222325_u64;
    for byte in normalized.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}

fn normalize_kwin_uuid(uuid: &str) -> Option<String> {
    let value = uuid
        .trim()
        .trim_start_matches('{')
        .trim_end_matches('}')
        .trim()
        .to_ascii_lowercase();
    (!value.is_empty()).then_some(value)
}

fn clean_string(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty() && *value != "null")
        .map(ToOwned::to_owned)
}

fn json_value_as_bool(value: Option<&serde_json::Value>) -> Option<bool> {
    match value? {
        serde_json::Value::Bool(value) => Some(*value),
        serde_json::Value::String(value) => match value.to_ascii_lowercase().as_str() {
            "true" => Some(true),
            "false" => Some(false),
            _ => None,
        },
        _ => None,
    }
}

fn json_value_as_u32(value: Option<&serde_json::Value>) -> Option<u32> {
    let value = json_value_as_f64(value)?;
    if !value.is_finite() || value < 0.0 || value > u32::MAX as f64 {
        return None;
    }
    Some(value.round() as u32)
}

fn json_value_as_i32(value: Option<&serde_json::Value>) -> Option<i32> {
    let value = json_value_as_f64(value)?;
    if !value.is_finite() || value < i32::MIN as f64 || value > i32::MAX as f64 {
        return None;
    }
    Some(value.round() as i32)
}

fn json_value_as_f64(value: Option<&serde_json::Value>) -> Option<f64> {
    match value? {
        serde_json::Value::Number(value) => value.as_f64(),
        serde_json::Value::String(value) => value.parse::<f64>().ok(),
        _ => None,
    }
}

struct ProbeCheck {
    ok: bool,
    detail: String,
}

fn gdbus_introspect_contains(
    destination: &str,
    object_path: &str,
    interface: &str,
    member: &str,
) -> ProbeCheck {
    match std::process::Command::new("gdbus")
        .args([
            "introspect",
            "--session",
            "--dest",
            destination,
            "--object-path",
            object_path,
        ])
        .output()
    {
        Ok(output) if output.status.success() => {
            let stdout = String::from_utf8_lossy(&output.stdout);
            let needle = format!("{interface}.{member}");
            let ok = stdout.contains(&needle) || stdout.contains(member);
            ProbeCheck {
                ok,
                detail: if ok {
                    format!("{interface}.{member} is present")
                } else {
                    format!("{interface}.{member} not found")
                },
            }
        }
        Ok(output) => {
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
            let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
            ProbeCheck {
                ok: false,
                detail: if stderr.is_empty() { stdout } else { stderr },
            }
        }
        Err(error) => ProbeCheck {
            ok: false,
            detail: error.to_string(),
        },
    }
}

#[cfg(test)]
mod adapter_tests {
    use super::*;

    #[test]
    fn parses_kwin_windows_as_window_info() {
        let uuid = "b4dfacf8-a559-43c9-8b1f-ecd5cfd78359";
        let windows_json = r#"{
          "backend": "kwin",
          "desktopGeometry": {"x": 100, "y": -50, "width": 3840, "height": "2160"},
          "windows": [
            {
              "uuid": "{b4dfacf8-a559-43c9-8b1f-ecd5cfd78359}",
              "caption": "Codex",
              "desktopFile": "codex-desktop",
              "resourceClass": "codex-desktop",
              "resourceName": "codex",
              "pid": 68986,
              "x": 10,
              "y": 48,
              "width": 1200,
              "height": 800,
              "workspace": 1,
              "minimized": false,
              "active": true,
              "clientType": "wayland",
              "normalWindow": true,
              "desktopWindow": false,
              "dock": false
            },
            {
              "uuid": "{11111111-2222-3333-4444-555555555555}",
              "caption": "Desktop",
              "desktopWindow": true
            }
          ]
        }"#;

        let windows = parse_kwin_windows(windows_json).unwrap();

        assert_eq!(windows.len(), 1);
        assert_eq!(windows[0].window_id, kwin_window_id_from_uuid(uuid));
        assert_eq!(windows[0].title.as_deref(), Some("Codex"));
        assert_eq!(windows[0].app_id.as_deref(), Some("codex-desktop"));
        assert_eq!(windows[0].wm_class.as_deref(), Some("codex-desktop"));
        assert_eq!(windows[0].pid, Some(68986));
        assert_eq!(windows[0].bounds.as_ref().unwrap().x, Some(10));
        assert_eq!(windows[0].bounds.as_ref().unwrap().height, 800);
        assert_eq!(windows[0].workspace, Some(1));
        assert!(windows[0].focused);
        assert!(!windows[0].hidden);
        assert_eq!(windows[0].client_type.as_deref(), Some("wayland"));
        assert_eq!(windows[0].backend, KWIN_BACKEND);
        assert_eq!(
            parse_kwin_logical_desktop_rect(windows_json).unwrap(),
            (100, -50, 3840, 2160)
        );
    }

    #[test]
    fn kwin_window_ids_are_stable_across_uuid_formats() {
        let bare = "b4dfacf8-a559-43c9-8b1f-ecd5cfd78359";
        let braced_upper = "{B4DFACF8-A559-43C9-8B1F-ECD5CFD78359}";

        assert_eq!(
            kwin_window_id_from_uuid(bare),
            kwin_window_id_from_uuid(braced_upper)
        );
    }

    #[test]
    fn kwin_window_script_supports_plasma5_and_plasma6_window_apis() {
        let script = kwin_window_script_source(
            ":1.234",
            "/dev/avifenesh/ComputerUseLinux/KWinWindowQuery/test",
            "computer_use_linux_kwin_window_query_test",
        )
        .unwrap();

        assert!(script.contains(r#"typeof workspace.windowList === "function""#));
        assert!(script.contains("workspace.windowList()"));
        assert!(script.contains(r#"typeof workspace.clientList === "function""#));
        assert!(script.contains("workspace.clientList()"));
        assert!(script.contains(
            r#"activeWindow = "activeWindow" in workspace ? workspace.activeWindow : workspace.activeClient;"#
        ));
        assert!(script.contains("workspace.virtualScreenGeometry"));
        assert!(script.contains("desktopGeometry: workspaceGeometry()"));
        assert!(script.contains("var objectDescription = serialize(window)"));
        assert!(script.contains(r#"objectClass === "KWin::XdgToplevelWindow""#));
        assert!(script.contains(r#"objectClass === "KWin::X11Window""#));
        assert!(!script.contains("objectClass: objectClass(window)"));
    }

    #[test]
    fn kwin_activation_script_focuses_window_directly() {
        let script = kwin_activate_script_source(
            ":1.234",
            "/dev/avifenesh/ComputerUseLinux/KWinWindowQuery/test",
            "computer_use_linux_kwin_window_query_test",
            "{B4DFACF8-A559-43C9-8B1F-ECD5CFD78359}",
        )
        .unwrap();

        assert!(script.contains(r#"var targetUuid = "b4dfacf8-a559-43c9-8b1f-ecd5cfd78359";"#));
        assert!(script.contains("targetWindow.minimized = false;"));
        assert!(script.contains("workspace.activeWindow = targetWindow;"));
        assert!(script.contains(r#"typeof workspace.clientList === "function""#));
        assert!(script.contains("workspace.clientList()"));
        assert!(script.contains(r#""activeWindow" in workspace"#));
        assert!(script.contains("workspace.activeClient = targetWindow;"));
        assert!(script.contains(r#""ReceiveResult""#));
        assert!(!script.contains("WindowsRunner"));
    }

    #[test]
    fn kwin_move_and_resize_scripts_preserve_unspecified_frame_geometry() {
        let args = (
            ":1.234",
            "/dev/avifenesh/ComputerUseLinux/KWinWindowQuery/test",
            "computer_use_linux_kwin_window_query_test",
            "{B4DFACF8-A559-43C9-8B1F-ECD5CFD78359}",
        );
        let move_script =
            kwin_move_script_source(args.0, args.1, args.2, args.3, 120, 240, 800, 600).unwrap();
        let resize_script =
            kwin_resize_script_source(args.0, args.1, args.2, args.3, 640, 480, 10, 20).unwrap();

        for script in [&move_script, &resize_script] {
            assert!(script.contains(r#"var targetUuid = "b4dfacf8-a559-43c9-8b1f-ecd5cfd78359";"#));
            assert!(script.contains("windowUuid(windows[i]) === targetUuid"));
            assert!(script.contains(r#""ReceiveResult""#));
            assert!(script.contains("ok: true"));
            assert!(script.contains(r#"throw new Error("window not found: " + targetUuid)"#));
            assert!(script.contains("ok: false"));
        }

        assert!(move_script.contains("window.frameGeometry = Qt.rect(120, 240, 800, 600)"));
        assert!(resize_script.contains("window.frameGeometry = Qt.rect(10, 20, 640, 480)"));
        assert!(kwin_resize_script_source(args.0, args.1, args.2, args.3, 0, 480, 10, 20).is_err());
        assert!(
            kwin_resize_script_source(args.0, args.1, args.2, args.3, 640, -1, 10, 20).is_err()
        );
    }

    #[tokio::test]
    async fn kwin_resize_rejects_non_positive_dimensions_before_bus_access() {
        let error = resize_window(1, 0, 480).await.unwrap_err();

        assert!(error.to_string().contains("positive"));
    }
}

#[cfg(test)]
mod callback_tests {
    use super::*;

    fn callback() -> (KwinWindowCallback, mpsc::Receiver<String>) {
        let (sender, receiver) = mpsc::channel();
        (
            KwinWindowCallback {
                sender,
                expected_sender: OwnedUniqueName::try_from(":1.42").unwrap(),
                expected_kind: KwinCallbackKind::Windows,
                plugin_name: "computer_use_linux_kwin_window_query_test".to_string(),
                delivered: AtomicBool::new(false),
            },
            receiver,
        )
    }

    #[test]
    fn callback_accepts_only_the_kwin_owner_requested_method_nonce_and_first_response() {
        let (callback, receiver) = callback();
        let valid = r#"{"backend":"kwin","pluginName":"computer_use_linux_kwin_window_query_test","windows":[]}"#;

        assert!(callback
            .accept(Some(":1.99"), KwinCallbackKind::Windows, valid)
            .is_err());
        assert!(receiver.try_recv().is_err());

        assert!(callback
            .accept(Some(":1.42"), KwinCallbackKind::Result, valid)
            .is_err());
        assert!(receiver.try_recv().is_err());

        for payload in [
            "not-json",
            r#"{"backend":"other","pluginName":"computer_use_linux_kwin_window_query_test","windows":[]}"#,
            r#"{"backend":"kwin","pluginName":"wrong","windows":[]}"#,
        ] {
            assert!(callback
                .accept(Some(":1.42"), KwinCallbackKind::Windows, payload)
                .is_err());
            assert!(receiver.try_recv().is_err());
        }

        callback
            .accept(Some(":1.42"), KwinCallbackKind::Windows, valid)
            .unwrap();
        assert_eq!(receiver.try_recv().unwrap(), valid);

        assert!(callback
            .accept(Some(":1.42"), KwinCallbackKind::Windows, valid)
            .is_err());
        assert!(receiver.try_recv().is_err());
    }
}

#[cfg(test)]
pub(crate) mod transaction_tests {
    use super::*;
    use std::{
        collections::VecDeque,
        future::pending,
        io::{BufRead, BufReader},
        process::{Child, Command, Stdio},
        sync::{
            atomic::{AtomicUsize, Ordering},
            Arc, Mutex,
        },
        time::Instant,
    };

    #[derive(Clone, Copy)]
    enum FakeKwinBehavior {
        HangLoad,
        HangStart,
        NoCallback,
        Callback,
    }

    static SESSION_BUS_ENV_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

    struct FakeKwinScripting {
        behavior: FakeKwinBehavior,
        load_calls: Arc<AtomicUsize>,
        start_calls: Arc<AtomicUsize>,
        unload_calls: Arc<AtomicUsize>,
        responses: Arc<Mutex<VecDeque<String>>>,
        loaded_script: Arc<Mutex<Option<(String, String)>>>,
        service_connection: Arc<std::sync::OnceLock<zbus::Connection>>,
    }

    #[zbus::interface(name = "org.kde.kwin.Scripting")]
    impl FakeKwinScripting {
        #[zbus(name = "loadScript")]
        async fn load_script(&self, path: &str, plugin_name: &str) -> i32 {
            self.load_calls.fetch_add(1, Ordering::SeqCst);
            *self.loaded_script.lock().unwrap() = Some((path.to_string(), plugin_name.to_string()));
            if matches!(self.behavior, FakeKwinBehavior::HangLoad) {
                pending::<()>().await;
            }
            1
        }

        #[zbus(name = "start")]
        async fn start(&self) -> zbus::fdo::Result<()> {
            self.start_calls.fetch_add(1, Ordering::SeqCst);
            if matches!(self.behavior, FakeKwinBehavior::HangStart) {
                pending::<()>().await;
            }
            if matches!(self.behavior, FakeKwinBehavior::Callback) {
                let (script_path, plugin_name) =
                    self.loaded_script.lock().unwrap().clone().ok_or_else(|| {
                        zbus::fdo::Error::Failed("script was not loaded".to_string())
                    })?;
                let script = fs::read_to_string(script_path)
                    .map_err(|error| zbus::fdo::Error::Failed(error.to_string()))?;
                let destination = script_string(&script, "var serviceName = ")?;
                let object_path = script_string(&script, "var objectPath = ")?;
                let payload = self
                    .responses
                    .lock()
                    .unwrap()
                    .pop_front()
                    .ok_or_else(|| {
                        zbus::fdo::Error::Failed("no fake KWin response queued".to_string())
                    })?
                    .replace("placeholder", &plugin_name);

                let connection = self.service_connection.get().ok_or_else(|| {
                    zbus::fdo::Error::Failed("fake KWin connection was not initialized".to_string())
                })?;
                let proxy = Proxy::new(
                    connection,
                    destination.as_str(),
                    object_path.as_str(),
                    KWIN_CALLBACK_INTERFACE,
                )
                .await
                .map_err(|error| zbus::fdo::Error::Failed(error.to_string()))?;
                let method = if script.contains("\"ReceiveWindows\"") {
                    "ReceiveWindows"
                } else {
                    "ReceiveResult"
                };
                let _: () = proxy
                    .call(method, &(payload.as_str(),))
                    .await
                    .map_err(|error| zbus::fdo::Error::Failed(error.to_string()))?;
            }
            Ok(())
        }

        #[zbus(name = "unloadScript")]
        fn unload_script(&self, _plugin_name: &str) -> bool {
            self.unload_calls.fetch_add(1, Ordering::SeqCst);
            true
        }
    }

    fn script_string(script: &str, prefix: &str) -> zbus::fdo::Result<String> {
        let value = script
            .split_once(prefix)
            .and_then(|(_, rest)| rest.trim_start().strip_prefix('"'))
            .and_then(|rest| rest.split_once('"').map(|(value, _)| value))
            .ok_or_else(|| zbus::fdo::Error::Failed(format!("missing {prefix} in script")))?;
        Ok(value.to_string())
    }

    struct TestSessionBus {
        child: Child,
        address: String,
    }

    impl TestSessionBus {
        fn start() -> Self {
            let mut child = Command::new("dbus-daemon")
                .args(["--session", "--nofork", "--nopidfile", "--print-address=1"])
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .expect("start private dbus-daemon");
            let mut address = String::new();
            BufReader::new(child.stdout.take().expect("private bus stdout"))
                .read_line(&mut address)
                .expect("read private bus address");
            assert!(!address.trim().is_empty(), "private bus emitted no address");
            Self {
                child,
                address: address.trim().to_string(),
            }
        }
    }

    impl Drop for TestSessionBus {
        fn drop(&mut self) {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }

    struct SessionBusEnvGuard(Option<std::ffi::OsString>);

    impl SessionBusEnvGuard {
        fn set(address: &str) -> Self {
            let previous = std::env::var_os("DBUS_SESSION_BUS_ADDRESS");
            std::env::set_var("DBUS_SESSION_BUS_ADDRESS", address);
            Self(previous)
        }
    }

    impl Drop for SessionBusEnvGuard {
        fn drop(&mut self) {
            if let Some(previous) = self.0.take() {
                std::env::set_var("DBUS_SESSION_BUS_ADDRESS", previous);
            } else {
                std::env::remove_var("DBUS_SESSION_BUS_ADDRESS");
            }
        }
    }

    pub(crate) async fn with_fake_kwin_responses<F, Fut, T>(
        responses: Vec<String>,
        operation: F,
    ) -> T
    where
        F: FnOnce() -> Fut,
        Fut: std::future::Future<Output = T>,
    {
        let _env_lock = SESSION_BUS_ENV_LOCK.lock().await;
        let bus = TestSessionBus::start();
        let load_calls = Arc::new(AtomicUsize::new(0));
        let start_calls = Arc::new(AtomicUsize::new(0));
        let unload_calls = Arc::new(AtomicUsize::new(0));
        let queued_responses = Arc::new(Mutex::new(responses.into()));
        let server_connection = Arc::new(std::sync::OnceLock::new());
        let service_connection = zbus::connection::Builder::address(bus.address.as_str())
            .unwrap()
            .name(KWIN_SCRIPTING_SERVICE)
            .unwrap()
            .serve_at(
                KWIN_SCRIPTING_OBJECT_PATH,
                FakeKwinScripting {
                    behavior: FakeKwinBehavior::Callback,
                    load_calls,
                    start_calls,
                    unload_calls,
                    responses: queued_responses,
                    loaded_script: Arc::new(Mutex::new(None)),
                    service_connection: Arc::clone(&server_connection),
                },
            )
            .unwrap()
            .build()
            .await
            .unwrap();
        assert!(server_connection.set(service_connection.clone()).is_ok());
        let _env = SessionBusEnvGuard::set(bus.address.as_str());

        let result = operation().await;
        drop(_env);
        drop(service_connection);
        drop(bus);
        result
    }

    async fn assert_transaction_timeout_cleans_up(behavior: FakeKwinBehavior) {
        let bus = TestSessionBus::start();
        let load_calls = Arc::new(AtomicUsize::new(0));
        let start_calls = Arc::new(AtomicUsize::new(0));
        let unload_calls = Arc::new(AtomicUsize::new(0));
        let server_connection = Arc::new(std::sync::OnceLock::new());
        let service_connection = zbus::connection::Builder::address(bus.address.as_str())
            .unwrap()
            .name(KWIN_SCRIPTING_SERVICE)
            .unwrap()
            .serve_at(
                KWIN_SCRIPTING_OBJECT_PATH,
                FakeKwinScripting {
                    behavior,
                    load_calls: Arc::clone(&load_calls),
                    start_calls: Arc::clone(&start_calls),
                    unload_calls: Arc::clone(&unload_calls),
                    responses: Arc::new(Mutex::new(VecDeque::new())),
                    loaded_script: Arc::new(Mutex::new(None)),
                    service_connection: Arc::clone(&server_connection),
                },
            )
            .unwrap()
            .build()
            .await
            .unwrap();
        assert!(server_connection.set(service_connection.clone()).is_ok());
        let client_connection = zbus::connection::Builder::address(bus.address.as_str())
            .unwrap()
            .build()
            .await
            .unwrap();
        let callback_path = Arc::new(Mutex::new(None::<String>));
        let script_path = Arc::new(Mutex::new(None::<std::path::PathBuf>));
        let callback_path_for_writer = Arc::clone(&callback_path);
        let script_path_for_writer = Arc::clone(&script_path);
        let started = Instant::now();

        let error = call_kwin_script_on_connection(
            client_connection.clone(),
            KwinCallbackKind::Windows,
            move |service_name, object_path, plugin_name| {
                let path = write_kwin_window_script(service_name, object_path, plugin_name)?;
                *callback_path_for_writer.lock().unwrap() = Some(object_path.to_string());
                *script_path_for_writer.lock().unwrap() = Some(path.clone());
                Ok(path)
            },
            Duration::from_millis(100),
        )
        .await
        .unwrap_err();

        assert!(error.to_string().contains("timed out"));
        assert!(started.elapsed() < Duration::from_secs(2));
        assert!(unload_calls.load(Ordering::SeqCst) >= 1);

        let path = script_path.lock().unwrap().clone().unwrap();
        assert!(!path.exists(), "temporary KWin script was not removed");
        let object_path = callback_path.lock().unwrap().clone().unwrap();
        assert!(client_connection
            .object_server()
            .interface::<_, KwinWindowCallback>(object_path.as_str())
            .await
            .is_err());

        drop(service_connection);
        drop(client_connection);
        drop(bus);
    }

    #[tokio::test]
    async fn transaction_times_out_and_cleans_up_when_load_script_never_replies() {
        assert_transaction_timeout_cleans_up(FakeKwinBehavior::HangLoad).await;
    }

    #[tokio::test]
    async fn transaction_times_out_and_cleans_up_when_start_never_replies() {
        assert_transaction_timeout_cleans_up(FakeKwinBehavior::HangStart).await;
    }

    #[tokio::test]
    async fn transaction_times_out_and_cleans_up_when_callback_never_arrives() {
        assert_transaction_timeout_cleans_up(FakeKwinBehavior::NoCallback).await;
    }

    #[tokio::test]
    async fn move_result_from_registered_kwin_callback_is_returned_as_success() {
        let result_json = r#"{"backend":"kwin","pluginName":"placeholder","ok":true,"uuid":"b4dfacf8-a559-43c9-8b1f-ecd5cfd78359","x":120,"y":240,"width":800,"height":600}"#;
        let result = with_fake_kwin_responses(vec![result_json.to_string()], || async {
            call_kwin_move_script("b4dfacf8-a559-43c9-8b1f-ecd5cfd78359", 120, 240, 800, 600).await
        })
        .await;

        let result: serde_json::Value = serde_json::from_str(&result.unwrap()).unwrap();
        assert_eq!(result["ok"], true);
        assert_eq!(result["x"], 120);
        assert_eq!(result["y"], 240);
        assert_eq!(result["width"], 800);
        assert_eq!(result["height"], 600);
    }

    #[tokio::test]
    async fn resize_result_from_registered_kwin_callback_is_returned_as_success() {
        let result_json = r#"{"backend":"kwin","pluginName":"placeholder","ok":true,"uuid":"b4dfacf8-a559-43c9-8b1f-ecd5cfd78359","x":10,"y":20,"width":640,"height":480}"#;
        let result = with_fake_kwin_responses(vec![result_json.to_string()], || async {
            call_kwin_resize_script("b4dfacf8-a559-43c9-8b1f-ecd5cfd78359", 640, 480, 10, 20).await
        })
        .await;

        let result: serde_json::Value = serde_json::from_str(&result.unwrap()).unwrap();
        assert_eq!(result["ok"], true);
        assert_eq!(result["width"], 640);
        assert_eq!(result["height"], 480);
    }

    #[tokio::test]
    async fn move_returns_an_error_when_the_window_is_missing() {
        let result = with_fake_kwin_responses(
            vec![r#"{"backend":"kwin","pluginName":"placeholder","windows":[]}"#.to_string()],
            || async {
                move_window(
                    kwin_window_id_from_uuid("b4dfacf8-a559-43c9-8b1f-ecd5cfd78359"),
                    120,
                    240,
                )
                .await
            },
        )
        .await;

        assert!(result
            .unwrap_err()
            .to_string()
            .contains("No KWin window matched window_id"));
    }

    #[tokio::test]
    async fn resize_returns_an_error_when_the_window_is_missing() {
        let result = with_fake_kwin_responses(
            vec![
                r#"{"backend":"kwin","pluginName":"placeholder","windows":[{"uuid":"another-window","x":0,"y":0,"width":10,"height":10}]}"#.to_string(),
            ],
            || async {
                resize_window(
                    kwin_window_id_from_uuid("b4dfacf8-a559-43c9-8b1f-ecd5cfd78359"),
                    640,
                    480,
                )
                .await
            },
        )
        .await;

        assert!(result
            .unwrap_err()
            .to_string()
            .contains("No KWin window matched window_id"));
    }

    #[tokio::test]
    async fn move_rejects_missing_frame_geometry_before_loading_geometry_script() {
        let snapshot = r#"{"backend":"kwin","pluginName":"placeholder","windows":[{"uuid":"b4dfacf8-a559-43c9-8b1f-ecd5cfd78359","y":20,"width":800,"height":600}]}"#;
        let result = with_fake_kwin_responses(vec![snapshot.to_string()], || async {
            move_window(
                kwin_window_id_from_uuid("b4dfacf8-a559-43c9-8b1f-ecd5cfd78359"),
                120,
                240,
            )
            .await
        })
        .await;

        assert!(result
            .unwrap_err()
            .to_string()
            .contains("invalid KWin frame geometry: x must be a finite integer"));
    }

    #[tokio::test]
    async fn resize_rejects_wrong_type_frame_geometry_before_loading_geometry_script() {
        let snapshot = r#"{"backend":"kwin","pluginName":"placeholder","windows":[{"uuid":"b4dfacf8-a559-43c9-8b1f-ecd5cfd78359","x":10,"y":20,"width":"640","height":480}]}"#;
        let result = with_fake_kwin_responses(vec![snapshot.to_string()], || async {
            resize_window(
                kwin_window_id_from_uuid("b4dfacf8-a559-43c9-8b1f-ecd5cfd78359"),
                640,
                480,
            )
            .await
        })
        .await;

        assert!(result
            .unwrap_err()
            .to_string()
            .contains("invalid KWin frame geometry: width must be a finite integer"));
    }

    #[tokio::test]
    async fn resize_rejects_non_positive_snapshot_dimensions_before_loading_geometry_script() {
        let snapshot = r#"{"backend":"kwin","pluginName":"placeholder","windows":[{"uuid":"b4dfacf8-a559-43c9-8b1f-ecd5cfd78359","x":10,"y":20,"width":640,"height":0}]}"#;
        let result = with_fake_kwin_responses(vec![snapshot.to_string()], || async {
            resize_window(
                kwin_window_id_from_uuid("b4dfacf8-a559-43c9-8b1f-ecd5cfd78359"),
                640,
                480,
            )
            .await
        })
        .await;

        assert!(result
            .unwrap_err()
            .to_string()
            .contains("invalid KWin frame geometry: height must be a positive integer"));
    }

    #[tokio::test]
    async fn geometry_operation_callback_error_is_returned() {
        let result_json = r#"{"backend":"kwin","pluginName":"placeholder","ok":false,"error":"invalid frame geometry"}"#;
        let result = with_fake_kwin_responses(vec![result_json.to_string()], || async {
            call_kwin_move_script("b4dfacf8-a559-43c9-8b1f-ecd5cfd78359", 120, 240, 800, 600).await
        })
        .await;

        assert!(result
            .unwrap_err()
            .to_string()
            .contains("invalid frame geometry"));
    }

    #[tokio::test]
    async fn duplicate_callback_path_fails_without_disturbing_its_owner() {
        let bus = TestSessionBus::start();
        let load_calls = Arc::new(AtomicUsize::new(0));
        let start_calls = Arc::new(AtomicUsize::new(0));
        let unload_calls = Arc::new(AtomicUsize::new(0));
        let server_connection = Arc::new(std::sync::OnceLock::new());
        let service_connection = zbus::connection::Builder::address(bus.address.as_str())
            .unwrap()
            .name(KWIN_SCRIPTING_SERVICE)
            .unwrap()
            .serve_at(
                KWIN_SCRIPTING_OBJECT_PATH,
                FakeKwinScripting {
                    behavior: FakeKwinBehavior::NoCallback,
                    load_calls: Arc::clone(&load_calls),
                    start_calls: Arc::clone(&start_calls),
                    unload_calls: Arc::clone(&unload_calls),
                    responses: Arc::new(Mutex::new(VecDeque::new())),
                    loaded_script: Arc::new(Mutex::new(None)),
                    service_connection: Arc::clone(&server_connection),
                },
            )
            .unwrap()
            .build()
            .await
            .unwrap();
        assert!(server_connection.set(service_connection.clone()).is_ok());
        let client_connection = zbus::connection::Builder::address(bus.address.as_str())
            .unwrap()
            .build()
            .await
            .unwrap();
        let plugin_name = "computer_use_linux_kwin_window_query_duplicate";
        let callback_path = format!("{KWIN_CALLBACK_OBJECT_PATH_PREFIX}/{plugin_name}");
        let expected_sender = service_connection.unique_name().unwrap().to_owned();
        let expected_sender_name = expected_sender.to_string();
        let (sender, receiver) = mpsc::channel();
        assert!(client_connection
            .object_server()
            .at(
                callback_path.as_str(),
                KwinWindowCallback {
                    sender,
                    expected_sender,
                    expected_kind: KwinCallbackKind::Windows,
                    plugin_name: plugin_name.to_string(),
                    delivered: AtomicBool::new(false),
                },
            )
            .await
            .unwrap());

        let error = call_kwin_script_on_connection_with_plugin_name(
            client_connection.clone(),
            KwinCallbackKind::Windows,
            |_, _, _| bail!("script writer must not run after a callback collision"),
            Duration::from_millis(100),
            plugin_name.to_string(),
        )
        .await
        .unwrap_err();

        assert!(error.to_string().contains("already registered"));
        assert_eq!(load_calls.load(Ordering::SeqCst), 0);
        assert_eq!(start_calls.load(Ordering::SeqCst), 0);
        assert_eq!(unload_calls.load(Ordering::SeqCst), 0);

        let callback = client_connection
            .object_server()
            .interface::<_, KwinWindowCallback>(callback_path.as_str())
            .await
            .expect("the original callback must remain registered");
        let payload = format!(r#"{{"backend":"kwin","pluginName":"{plugin_name}","windows":[]}}"#);
        callback
            .get()
            .await
            .accept(
                Some(expected_sender_name.as_str()),
                KwinCallbackKind::Windows,
                &payload,
            )
            .unwrap();
        assert_eq!(receiver.try_recv().unwrap(), payload);

        drop(service_connection);
        drop(client_connection);
        drop(bus);
    }
}
