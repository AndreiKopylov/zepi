use std::{io::Write, process::Stdio};

use editor::Editor;
use gpui::{Context, Entity, IntoElement, Render, Window};
use serde_json::{Value, json};
use ui::{ContextMenu, DropdownMenu, DropdownStyle, prelude::*};

pub(crate) struct PiBrowserSettingsView {
    data: Option<Value>,
    scroll_handle: gpui::ScrollHandle,
    executable: Entity<Editor>,
    keep_open: bool,
    window_mode: String,
    candidates: Vec<(String, String)>,
    busy: bool,
    notice: String,
    failed: bool,
}

fn input(placeholder: &str, masked: bool, window: &mut Window, cx: &mut Context<PiBrowserSettingsView>) -> Entity<Editor> {
    cx.new(|cx| {
        let mut editor = Editor::single_line(window, cx);
        editor.set_placeholder_text(placeholder, window, cx);
        editor.set_masked(masked, cx);
        editor
    })
}

fn field(label: &str, element: impl IntoElement) -> gpui::AnyElement {
    v_flex().gap_2().w_full().child(Label::new(label.to_owned()).size(LabelSize::Small)).child(element).into_any_element()
}

fn text_field(editor: Entity<Editor>, cx: &App) -> gpui::AnyElement {
    div().w_full().px_2().py_2().rounded_md().border_1().border_color(cx.theme().colors().border)
        .bg(cx.theme().colors().editor_background).child(editor).into_any_element()
}

impl PiBrowserSettingsView {
    pub(crate) fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let mut view = Self {
            data: None,
            scroll_handle: gpui::ScrollHandle::new(),
            executable: input("Detected automatically", false, window, cx),
            keep_open: false,
            window_mode: "fullscreen".into(),
            candidates: Vec::new(),
            busy: false,
            notice: String::new(),
            failed: false,
        };
        view.executable.update(cx, |editor, cx| editor.set_read_only(true));
        view.request(json!({"operation": "status"}), window, cx);
        view
    }

    fn request(&mut self, request: Value, window: &mut Window, cx: &mut Context<Self>) {
        if self.busy {
            return;
        }
        self.busy = true;
        self.notice.clear();
        self.failed = false;
        let operation = request["operation"].as_str().unwrap_or("status").to_owned();
        let root = std::env::var_os("ZEPI_ROOT").map(std::path::PathBuf::from);
        let node = std::env::var_os("ZEPI_NODE");
        let task = cx.background_spawn(async move {
            let root = root.ok_or_else(|| "Zepi runtime directory is unavailable.".to_string())?;
            let agent = std::env::var_os("ZEPI_AGENT").map(std::path::PathBuf::from).filter(|p| p.is_file())
                .or_else(|| { let b = root.join("agent.exe"); b.is_file().then_some(b) });
            let mut child = match agent {
                Some(a) => { let mut c = util::command::new_std_command(a); c.arg("pi-settings"); c }
                None => { let mut c = util::command::new_std_command(node.ok_or_else(|| "Zepi Node.js runtime is unavailable.".to_string())?); c.arg(root.join("packages/runtime/bin/pi-settings.mjs")); c }
            }
            .current_dir(&root)
                .stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null())
                .spawn().map_err(|_| "Unable to start pi settings.".to_string())?;
            let body = serde_json::to_vec(&request).map_err(|_| "Unable to read settings.".to_string())?;
            if let Some(mut stdin) = child.stdin.take() {
                stdin.write_all(&body).map_err(|_| "Unable to send settings.".to_string())?;
            }
            let output = child.wait_with_output().map_err(|_| "The pi settings request did not complete.".to_string())?;
            let response: Value = serde_json::from_slice(&output.stdout)
                .map_err(|_| "The pi settings response was invalid.".to_string())?;
            if response["ok"] == true {
                Ok(response["data"].clone())
            } else {
                Err(response["error"].as_str().unwrap_or("Unable to update pi settings.").to_owned())
            }
        });
        cx.spawn_in(window, async move |view, cx| {
            let result = task.await;
            let _ = view.update_in(cx, |view, window, cx| {
                view.busy = false;
                match result {
                    Ok(data) => {
                        let first = view.data.is_none();
                        view.keep_open = data["browser"]["keepOpen"] == true;
                        view.window_mode = data["browser"]["window"].as_str().unwrap_or("fullscreen").to_owned();
                        view.candidates = data["browser"]["candidates"].as_array().into_iter().flatten()
                            .filter_map(|item| Some((item["path"].as_str()?.to_owned(), item["source"].as_str().unwrap_or_default().to_owned())))
                            .collect();
                        if first || matches!(operation.as_str(), "status" | "browser-set") {
                            let path = data["browser"]["path"].as_str().unwrap_or_default();
                            view.executable.update(cx, |editor, cx| editor.set_text(path, window, cx));
                        }
                        view.data = Some(data);
                        if operation == "browser-set" {
                            view.notice = "Browser settings saved.".into();
                        }
                        view.failed = false;
                    }
                    Err(message) => {
                        view.notice = message;
                        view.failed = true;
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn browse(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let paths_receiver = cx.prompt_for_paths(gpui::PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some("Select a Chromium-based browser executable".into()),
        });
        let view = cx.entity();
        window
            .spawn(cx, async move |cx| {
                let path = match paths_receiver.await {
                    Ok(Ok(Some(mut paths))) => paths.pop(),
                    Ok(_) => None,
                    Err(error) => {
                        view.update(cx, |view, cx| {
                            view.notice = format!("The file dialog failed: {error}");
                            view.failed = true;
                            cx.notify();
                        });
                        None
                    }
                };
                let Some(path) = path else { return };
                let text = path.to_string_lossy().to_string();
                view.update_in(cx, |view, window, cx| {
                    view.request(json!({"operation": "browser-set", "executablePath": text}), window, cx);
                })
                .ok();
            })
            .detach();
    }

    fn override_set(&self) -> bool {
        !self.browser()["executablePath"].as_str().unwrap_or_default().is_empty()
    }

    fn change_menu(&self, window: &mut Window, cx: &mut Context<Self>) -> gpui::AnyElement {
        let weak = cx.entity().downgrade();
        let entries = self.candidates.clone();
        let label = match (self.override_set(), self.browser()["path"].as_str()) {
            (true, Some(path)) => format!("Custom: {}", path.rsplit(['\\', '/']).next().unwrap_or(path)),
            _ => "Auto detection".into(),
        };
        let menu = ContextMenu::build(window, cx, move |mut menu, _, _| {
            for (path, source) in entries {
                let weak = weak.clone();
                let path = path.clone();
                let name = format!("{} ({})", path.rsplit(['\\', '/']).next().unwrap_or(path.as_str()), source);
                menu = menu.entry(name, None, move |window, cx| {
                    let _ = weak.update(cx, |view, cx| view.request(json!({"operation": "browser-set", "executablePath": path}), window, cx));
                });
            }
            let weak_browse = weak.clone();
            menu = menu.entry("Browse for an executable...".to_owned(), None, move |window, cx| {
                let _ = weak_browse.update(cx, |view, cx| view.browse(window, cx));
            });
            let weak_auto = weak.clone();
            menu = menu.separator();
            menu = menu.entry("Use auto detection".to_owned(), None, move |window, cx| {
                let _ = weak_auto.update(cx, |view, cx| view.request(json!({"operation": "browser-set", "executablePath": ""}), window, cx));
            });
            menu
        });
        DropdownMenu::new("pib-change", label, menu).style(DropdownStyle::Outlined).into_any_element()
    }

    fn browser(&self) -> Value {
        self.data.as_ref().map(|data| data["browser"].clone()).unwrap_or(Value::Null)
    }

    fn window_menu(&self, window: &mut Window, cx: &mut Context<Self>) -> gpui::AnyElement {
        let weak = cx.entity().downgrade();
        let label = format!("Window: {}", match self.window_mode.as_str() {
            "normal" => "Normal",
            "maximized" => "Maximized",
            _ => "Fullscreen",
        });
        let menu = ContextMenu::build(window, cx, move |mut menu, _, _| {
            for (value, name) in [("fullscreen", "Fullscreen"), ("maximized", "Maximized"), ("normal", "Normal")] {
                let weak = weak.clone();
                let value = value.to_owned();
                menu = menu.entry(name, None, move |window, cx| {
                    let _ = weak.update(cx, |view, cx| {
                        view.window_mode = value.clone();
                        view.request(json!({"operation": "browser-set", "window": value}), window, cx);
                        cx.notify();
                    });
                });
            }
            menu
        });
        DropdownMenu::new("pib-window", label, menu)
            .style(DropdownStyle::Outlined)
            .attach(gpui::Anchor::BottomLeft)
            .into_any_element()
    }
}

impl Render for PiBrowserSettingsView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let browser = self.browser();
        let source = browser["source"].as_str().unwrap_or("none");
        let path = browser["path"].as_str().unwrap_or_default();
        let found = !matches!(source, "none" | "missing");
        let source_label = match source {
            "setting" => "configured path",
            "zepi" => "Zepi browsers directory",
            "system" => "system browser",
            "missing" => "configured path not found",
            _ => "no Chromium-based browser found",
        };
        let file_name = path.rsplit(['\\', '/']).next().unwrap_or(path);
        v_flex().id("zepi-pi-browser-settings").size_full().overflow_y_scroll().track_scroll(&self.scroll_handle).px_8().pb_12().gap_6()
            .child(v_flex().gap_2().pt_4()
                .child(h_flex().w_full().justify_between().gap_2()
                    .child(Label::new("Browser"))
                    .when(!self.notice.is_empty(), |this| {
                        this.child(Label::new(self.notice.clone()).size(LabelSize::Small).color(if self.failed { Color::Error } else { Color::Success }))
                    }))
                .child(Label::new("Chromium-family browser driven by the native Rust/CDP engine.").size(LabelSize::Small).color(Color::Muted)))
            .child(v_flex().gap_3().pt_4().border_t_1().border_color(cx.theme().colors().border_variant)
                .child(Label::new("Browser use"))
                .when(found, |this| this
                    .child(Label::new(format!("Browser: {file_name} ({source_label})")).size(LabelSize::Small).color(Color::Muted)))
                .when(!found, |this| this
                    .child(Label::new("No Chromium-based browser found. Pick an existing Chrome or Edge executable.").size(LabelSize::Small).color(Color::Warning)))
                .child(field("Executable", text_field(self.executable.clone(), cx)))
                .child(Label::new("Change pins one executable. Auto detection checks the Zepi browsers directory, then system Chrome or Edge.").size(LabelSize::Small).color(Color::Muted))
                .child(self.change_menu(window, cx))
                .child(Label::new("Applies to tasks started after the change.").size(LabelSize::Small).color(Color::Muted))
                .child(Label::new(if self.keep_open {
                    "The task browser stays open between turns and closes with the task."
                } else {
                    "The task browser closes when each turn ends or is cancelled."
                }).size(LabelSize::Small).color(Color::Muted))
                .child(Button::new("pib-keep-open", if self.keep_open { "Keep Open: On" } else { "Keep Open: Off" }).style(ButtonStyle::Outlined).disabled(self.busy)
                    .on_click(cx.listener(|this, _, window, cx| {
                        let keep = !this.keep_open;
                        this.request(json!({"operation": "browser-set", "keepOpen": keep}), window, cx);
                    }))))
            .child(v_flex().gap_2()
                .child(Label::new(match self.window_mode.as_str() {
                    "normal" => "The window opens at its default size.",
                    "maximized" => "The window opens maximized and the page fills it. Applies to browsers opened after the change.",
                    _ => "The window opens in fullscreen and the page fills it. Applies to browsers opened after the change.",
                }).size(LabelSize::Small).color(Color::Muted))
                .child(h_flex().child(self.window_menu(window, cx))))
    }
}

pub(crate) fn render_pi_browser_page(
    settings: &crate::SettingsWindow,
    _: &gpui::ScrollHandle,
    _: &mut Window,
    _: &mut Context<crate::SettingsWindow>,
) -> gpui::AnyElement {
    div().size_full().children(settings.pi_browser_settings.clone()).into_any_element()
}