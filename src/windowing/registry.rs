use crate::windowing::backends::kwin;
use crate::windowing::types::WindowInfo;
use anyhow::{anyhow, Result};

pub use kwin::KWIN_BACKEND;

pub const KWIN_WINDOW_PERMISSION_HINT: &str =
    "KWin scripting is unavailable. Ensure org.kde.KWin scripting is exposed on the session bus.";

#[derive(Debug, Clone, Copy)]
pub struct BackendDescriptor {
    pub id: &'static str,
    pub failure_label: &'static str,
    pub list_note: &'static str,
    pub missing_hint: &'static str,
    pub can_exact_focus: bool,
}

#[derive(Debug, Clone)]
pub struct BackendProbe {
    pub id: &'static str,
    pub ok: bool,
    pub can_list_windows: bool,
    pub can_focus_apps: bool,
    pub can_focus_windows: bool,
    pub detail: String,
}

const KWIN_DESCRIPTOR: BackendDescriptor = BackendDescriptor {
    id: KWIN_BACKEND,
    failure_label: "KWin",
    list_note: "Window list came from KWin/Plasma DBus scripting. Terminal windows may include best-effort PTY and active-process context when the process tree is readable.",
    missing_hint: "On KDE/Plasma, ensure KWin exposes org.kde.KWin scripting on the session bus.",
    can_exact_focus: true,
};

const DESCRIPTORS: &[BackendDescriptor] = &[KWIN_DESCRIPTOR];

pub fn descriptors() -> &'static [BackendDescriptor] {
    DESCRIPTORS
}

pub fn descriptor(id: &str) -> Option<&'static BackendDescriptor> {
    DESCRIPTORS.iter().find(|descriptor| descriptor.id == id)
}

pub fn list_note(id: &str) -> &'static str {
    descriptor(id).unwrap_or(&KWIN_DESCRIPTOR).list_note
}

pub fn backend_can_exact_focus(id: &str) -> bool {
    descriptor(id).is_some_and(|descriptor| descriptor.can_exact_focus)
}

pub async fn list_windows() -> Result<Vec<WindowInfo>> {
    kwin::list_windows()
        .await
        .map_err(|error| anyhow!("{} failed: {error:#}", KWIN_DESCRIPTOR.failure_label))
}

pub async fn activate_window(window: &WindowInfo) -> Result<()> {
    match window.backend.as_str() {
        KWIN_BACKEND => kwin::activate_window(window.window_id).await,
        backend => Err(anyhow!(
            "Unsupported window backend for activation: {backend}"
        )),
    }
}

pub async fn focused_window_for_backend(backend: &str) -> Result<Option<WindowInfo>> {
    if backend != KWIN_BACKEND {
        return Err(anyhow!(
            "Unsupported window backend for focus query: {backend}"
        ));
    }
    Ok(kwin::list_windows()
        .await?
        .into_iter()
        .find(|window| window.focused))
}

pub async fn move_window(window: &WindowInfo, x: i32, y: i32) -> Result<String> {
    match window.backend.as_str() {
        KWIN_BACKEND => kwin::move_window(window.window_id, x, y).await,
        backend => Err(anyhow!("Unsupported window backend for move: {backend}")),
    }
}

pub async fn resize_window(window: &WindowInfo, width: i32, height: i32) -> Result<String> {
    match window.backend.as_str() {
        KWIN_BACKEND => kwin::resize_window(window.window_id, width, height).await,
        backend => Err(anyhow!("Unsupported window backend for resize: {backend}")),
    }
}

pub fn probe_backends() -> Vec<BackendProbe> {
    vec![kwin::probe()]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::windowing::types::WindowBounds;

    fn window(backend: &str) -> WindowInfo {
        WindowInfo {
            window_id: 1,
            title: Some("Codex".to_string()),
            app_id: Some("codex-desktop".to_string()),
            wm_class: Some("codex-desktop".to_string()),
            pid: Some(1234),
            bounds: Some(WindowBounds {
                x: Some(0),
                y: Some(0),
                width: 800,
                height: 600,
            }),
            workspace: None,
            focused: true,
            hidden: false,
            client_type: Some("wayland".to_string()),
            backend: backend.to_string(),
            terminal: None,
        }
    }

    #[test]
    fn registry_contains_only_kwin() {
        assert_eq!(
            descriptors()
                .iter()
                .map(|descriptor| descriptor.id)
                .collect::<Vec<_>>(),
            [KWIN_BACKEND]
        );
        assert_eq!(descriptor(KWIN_BACKEND).unwrap().failure_label, "KWin");
        assert!(descriptor("gnome-shell-introspect").is_none());
        assert!(list_note("unknown-backend").contains("KWin/Plasma"));
    }

    #[tokio::test]
    async fn dispatches_kwin_list_focus_and_activation() {
        let uuid = "b4dfacf8-a559-43c9-8b1f-ecd5cfd78359";
        let windows_json = format!(
            r#"{{"backend":"kwin","pluginName":"placeholder","windows":[{{"uuid":"{uuid}","caption":"Codex","normalWindow":true,"active":true,"x":10,"y":20,"width":800,"height":600}}]}}"#
        );
        let activation_json =
            format!(r#"{{"backend":"kwin","pluginName":"placeholder","ok":true,"uuid":"{uuid}"}}"#);

        crate::windowing::backends::kwin::transaction_tests::with_fake_kwin_responses(
            vec![
                windows_json.clone(),
                windows_json.clone(),
                windows_json,
                activation_json,
            ],
            || async {
                let windows = list_windows().await.unwrap();
                assert_eq!(windows.len(), 1);
                assert_eq!(windows[0].backend, KWIN_BACKEND);
                assert_eq!(
                    focused_window_for_backend(KWIN_BACKEND)
                        .await
                        .unwrap()
                        .unwrap()
                        .backend,
                    KWIN_BACKEND
                );
                activate_window(&windows[0]).await.unwrap();
            },
        )
        .await;
    }

    #[tokio::test]
    async fn unsupported_backend_ids_fail_explicitly() {
        let unsupported = window("gnome-shell-extension");

        assert!(activate_window(&unsupported)
            .await
            .unwrap_err()
            .to_string()
            .contains("Unsupported window backend for activation"));
        assert!(focused_window_for_backend(&unsupported.backend)
            .await
            .unwrap_err()
            .to_string()
            .contains("Unsupported window backend for focus query"));
        assert!(move_window(&unsupported, 120, 240)
            .await
            .unwrap_err()
            .to_string()
            .contains("Unsupported window backend for move"));
        assert!(resize_window(&unsupported, 640, 480)
            .await
            .unwrap_err()
            .to_string()
            .contains("Unsupported window backend for resize"));
    }

    #[tokio::test]
    async fn kwin_geometry_rejects_invalid_dimensions() {
        let kwin_error = resize_window(&window(KWIN_BACKEND), 0, 480)
            .await
            .unwrap_err();
        assert!(kwin_error.to_string().contains("positive"));
    }

    #[tokio::test]
    async fn dispatches_successful_kwin_move_and_resize() {
        let uuid = "b4dfacf8-a559-43c9-8b1f-ecd5cfd78359";
        let mut target = window(KWIN_BACKEND);
        target.window_id = 15_605_548_758_018_230_245;
        let windows_json = format!(
            r#"{{"backend":"kwin","pluginName":"placeholder","windows":[{{"uuid":"{uuid}","caption":"Codex","normalWindow":true,"x":10,"y":20,"width":800,"height":600}}]}}"#
        );
        let move_result_json = format!(
            r#"{{"backend":"kwin","pluginName":"placeholder","ok":true,"uuid":"{uuid}","x":120,"y":240,"width":800,"height":600}}"#
        );
        let resize_result_json = format!(
            r#"{{"backend":"kwin","pluginName":"placeholder","ok":true,"uuid":"{uuid}","x":10,"y":20,"width":640,"height":480}}"#
        );

        let (moved, resized) =
            crate::windowing::backends::kwin::transaction_tests::with_fake_kwin_responses(
                vec![
                    windows_json.clone(),
                    move_result_json,
                    windows_json,
                    resize_result_json,
                ],
                || async {
                    let moved = move_window(&target, 120, 240).await;
                    let resized = resize_window(&target, 640, 480).await;
                    (moved, resized)
                },
            )
            .await;

        let moved: serde_json::Value = serde_json::from_str(&moved.unwrap()).unwrap();
        let resized: serde_json::Value = serde_json::from_str(&resized.unwrap()).unwrap();
        assert_eq!(moved["ok"], true);
        assert_eq!(moved["x"], 120);
        assert_eq!(moved["y"], 240);
        assert_eq!(resized["ok"], true);
        assert_eq!(resized["width"], 640);
        assert_eq!(resized["height"], 480);
    }
}
