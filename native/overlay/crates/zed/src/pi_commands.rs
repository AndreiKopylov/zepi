use std::path::PathBuf;

use gpui::{App, SharedString, actions};
use notifications::status_toast::StatusToast;
use ui::{Color, Icon, IconName, IconSize};
use workspace::{AppState, MultiWorkspace, OpenOptions};

actions!(
    pi,
    [
        OpenSettingsFile,
        OpenMcpFile,
        OpenModelsFile,
        OpenModelsStoreFile,
        UpdateModels,
    ]
);

fn pi_dir() -> Option<PathBuf> {
    if let Ok(d) = std::env::var("ZEPI_PI_CONFIG_DIR") {
        return Some(PathBuf::from(d));
    }
    std::env::var("LOCALAPPDATA")
        .ok()
        .map(|l| PathBuf::from(l).join("Zepi").join("pi"))
}

fn open_pi_file(name: &str, cx: &mut App) {
    let Some(dir) = pi_dir() else { return };
    let path = dir.join(name);
    if !path.exists() {
        let _ = std::fs::create_dir_all(&dir);
        let _ = std::fs::write(&path, "{}\n");
    }
    let Some(app_state) = AppState::try_global(cx) else { return };
    workspace::open_paths(&[path], app_state, OpenOptions::default(), cx).detach();
}

fn toast(cx: &mut App, message: impl Into<SharedString>) {
    let Some(mw) = cx.active_window().and_then(|w| w.downcast::<MultiWorkspace>()) else {
        return;
    };
    let Ok(mws) = mw.read(cx) else { return };
    let ws = mws.workspace().clone();
    ws.update(cx, |ws, cx| {
        let t = StatusToast::new(message, cx, |this, _| {
            this.icon(
                Icon::new(IconName::Sparkle)
                    .size(IconSize::Small)
                    .color(Color::Muted),
            )
            .dismiss_button(true)
        });
        ws.toggle_status_toast(t, cx);
    });
}

pub fn init(cx: &mut App) {
    cx.on_action(|_: &OpenSettingsFile, cx| open_pi_file("settings.json", cx));
    cx.on_action(|_: &OpenMcpFile, cx| open_pi_file("mcp.json", cx));
    cx.on_action(|_: &OpenModelsFile, cx| open_pi_file("models.json", cx));
    cx.on_action(|_: &OpenModelsStoreFile, cx| open_pi_file("models-store.json", cx));

    cx.on_action(|_: &UpdateModels, cx| {
        let Some(agent) = std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(|d| d.join("agent.exe")))
        else {
            return;
        };
        toast(cx, "Updating models…");
        cx.spawn(async move |cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    util::command::new_std_command(&agent)
                        .arg("pi-settings")
                        .stdin(std::process::Stdio::piped())
                        .stdout(std::process::Stdio::piped())
                        .stderr(std::process::Stdio::null())
                        .spawn()
                        .and_then(|mut child| {
                            if let Some(mut stdin) = child.stdin.take() {
                                use std::io::Write;
                                let _ = stdin.write_all(br#"{"operation":"status","refreshModels":true}"#);
                            }
                            child.wait_with_output()
                        })
                })
                .await;
            let message = match result {
                    Ok(out) => {
                        let text = String::from_utf8_lossy(&out.stdout);
                        if out.status.success() && text.contains("\"ok\":true") {
                            serde_json::from_str::<serde_json::Value>(&text)
                                .ok()
                                .and_then(|v| {
                                    v["data"]["modelCatalogNotice"]
                                        .as_str()
                                        .map(|s| s.to_string())
                                })
                                .filter(|s| !s.is_empty())
                                .unwrap_or_else(|| "Models updated".to_string())
                        } else {
                            format!("Model update failed: {text}")
                        }
                    }
                Err(e) => format!("Model update failed: {e}"),
            };
            cx.update(|cx| toast(cx, message));
        })
        .detach();
    });
}
