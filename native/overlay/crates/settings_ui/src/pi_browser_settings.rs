use std::{io::Write, process::Stdio};

use gpui::{ClipboardItem, Context, IntoElement, Render, Window};
use serde_json::{Value, json};
use ui::{CommonAnimationExt, ContextMenu, DropdownMenu, DropdownStyle, prelude::*};

pub(crate) struct PiBrowserSettingsView {
    data: Option<Value>,
    browse: Option<Value>,
    bsettings: Option<Value>,
    update_info: Option<Value>,
    auto_tried: bool,
    scroll_handle: gpui::ScrollHandle,
    folder: String,
    code_mode: String,
    pairing_code: String,
    busy: bool,
    busy_op: String,
    notice: String,
    failed: bool,
}

fn field(label: &str, element: impl IntoElement) -> gpui::AnyElement {
    v_flex().gap_2().w_full().child(Label::new(label.to_owned()).size(LabelSize::Small)).child(element).into_any_element()
}

fn muted(text: impl Into<String>) -> gpui::AnyElement {
    Label::new(text.into()).size(LabelSize::Small).color(Color::Muted).into_any_element()
}

fn default_bsettings() -> Value {
    json!({"settings": {
        "humanInput": {"value": "off", "allowed": ["on", "off"]},
        "grouping": {"value": "shared", "allowed": ["shared", "session", "none"]},
        "window": {"value": "current", "allowed": ["current", "separate"]},
        "idleMinutes": {"value": 10, "allowed": [10, 30, 60, "never"]},
        "handoffNotifications": {"value": "recording", "allowed": ["recording", "always"]}
    }})
}

impl PiBrowserSettingsView {
    pub(crate) fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let mut view = Self {
            data: None,
            browse: None,
            bsettings: None,
            update_info: None,
            auto_tried: false,
            scroll_handle: gpui::ScrollHandle::new(),
            folder: String::new(),
            code_mode: "plain".into(),
            pairing_code: String::new(),
            busy: false,
            busy_op: String::new(),
            notice: String::new(),
            failed: false,
        };
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
        self.busy_op = operation.clone();
        let root = std::env::var_os("ZEPI_ROOT").map(std::path::PathBuf::from);
        let task = cx.background_spawn(async move {
            let root = root.ok_or_else(|| "Zepi runtime directory is unavailable.".to_string())?;
            let agent = std::env::var_os("ZEPI_AGENT").map(std::path::PathBuf::from).filter(|p| p.is_file())
                .or_else(|| { let b = root.join("agent.exe"); b.is_file().then_some(b) })
                .ok_or_else(|| "Zepi agent is unavailable.".to_string())?;
            let mut child = util::command::new_std_command(agent)
                .arg("pi-settings")
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
            if response["ok"] == true { Ok(response["data"].clone()) }
            else { Err(response["error"].as_str().unwrap_or("Unable to update pi settings.").to_owned()) }
        });
        cx.spawn_in(window, async move |view, cx| {
            let result = task.await;
            let _ = view.update_in(cx, |view, window, cx| {
                view.busy = false;
                view.busy_op = String::new();
                match result {
                    Ok(data) => {
                        if let Some(browser) = data.get("browser") {
                            view.folder = browser["browseDir"].as_str().unwrap_or_default().to_owned();
                            view.code_mode = browser["browseTools"].as_str().unwrap_or("plain").to_owned();
                        }
                        let first = view.data.is_none();
                        view.data = Some(data.clone());
                        match operation.as_str() {
                            "browse-status" | "browse-install" => {
                                view.browse = Some(data);
                                if operation != "browse-status" { view.notice = "browse installed.".into(); view.update_info = None; }
                            }
                            "browse-check-update" => {
                                let available = data["updateAvailable"] == true;
                                view.notice = if available { format!("Update available: v{}.", data["latest"].as_str().unwrap_or("?")) } else { "Already up to date.".into() };
                                view.update_info = Some(data);
                            }
                            "browse-update" => { view.update_info = None; view.browse = Some(data); view.notice = "browse updated.".into(); }
                            "browse-settings" | "browse-set" => view.bsettings = Some(data),
                            "browse-pair" => view.pairing_code = data["code"].as_str().unwrap_or_default().to_owned(),
                            "browse-mcp" => view.notice = "Browser tools mode saved.".into(),
                            _ => {}
                        }
                        if first {
                            view.request(json!({"operation": "browse-status"}), window, cx);
                        } else if operation == "browse-status" && view.connected() {
                            view.request(json!({"operation": "browse-settings"}), window, cx);
                        }
                        // One small auto-connect attempt on first open; afterwards use the Connect button.
                        if operation == "browse-status" && !view.connected() && !view.auto_tried {
                            view.auto_tried = true;
                            cx.spawn_in(window, async move |view, cx| {
                                cx.background_executor().timer(std::time::Duration::from_millis(1500)).await;
                                let _ = view.update_in(cx, |view, window, cx| {
                                    if !view.busy && !view.connected() {
                                        view.request(json!({"operation": "browse-status"}), window, cx);
                                    }
                                });
                            }).detach();
                        }
                        view.failed = false;
                    }
                    Err(message) => { view.notice = message; view.failed = true; }
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn browse(&self) -> Value {
        self.browse.clone().unwrap_or(Value::Null)
    }

    fn installed(&self) -> bool {
        self.browse()["installed"] == true
    }

    fn connected(&self) -> bool {
        self.browse()["live"]["extension"]["connected"] == true
    }

    fn choose_folder(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let receiver = cx.prompt_for_paths(gpui::PathPromptOptions {
            files: false, directories: true, multiple: false,
            prompt: Some("Choose a folder for browse (relay + CLI + bun)".into()),
        });
        let view = cx.entity();
        window.spawn(cx, async move |cx| {
            let path = match receiver.await { Ok(Ok(Some(mut p))) => p.pop(), _ => None };
            let Some(path) = path else { return };
            let text = path.to_string_lossy().to_string();
            view.update_in(cx, |view, window, cx| {
                view.folder = text.clone();
                view.request(json!({"operation": "browse-install", "dir": text}), window, cx);
            }).ok();
        }).detach();
    }

    fn open_extension(&self) {
        let url = self.browse()["extensionUrl"].as_str().unwrap_or("https://chromewebstore.google.com/detail/mfnicocicmmlkpjnaffgihfjhdgjkdjg").to_owned();
        let _ = util::command::new_std_command("explorer.exe").arg(url).spawn();
    }

    fn code_menu(&self, window: &mut Window, cx: &mut Context<Self>) -> gpui::AnyElement {
        let weak = cx.entity().downgrade();
        let label = format!("Tools: {}", if self.code_mode == "execute" { "code (execute)" } else { "plain" });
        let menu = ContextMenu::build(window, cx, move |mut menu, _, _| {
            for (value, name) in [("plain", "plain - direct tools"), ("execute", "code - JavaScript execute")] {
                let weak = weak.clone();
                let value = value.to_owned();
                menu = menu.entry(name, None, move |window, cx| {
                    let _ = weak.update(cx, |view, cx| {
                        view.code_mode = value.clone();
                        view.request(json!({"operation": "browse-mcp", "tools": value}), window, cx);
                        cx.notify();
                    });
                });
            }
            menu
        });
        DropdownMenu::new("pib-code", label, menu).style(DropdownStyle::Outlined).into_any_element()
    }

    fn setting_menu(&self, key: &str, title: &str, window: &mut Window, cx: &mut Context<Self>) -> gpui::AnyElement {
        let bs = self.bsettings.clone().unwrap_or(Value::Null);
        let mut entry = bs["settings"][key].clone();
        if entry.is_null() { entry = default_bsettings()["settings"][key].clone(); }
        let current = entry["value"].clone();
        let allowed = entry["allowed"].as_array().cloned().unwrap_or_default();
        let weak = cx.entity().downgrade();
        let label = format!("{title}: {}", current.as_str().map(|s| s.to_owned()).unwrap_or_else(|| current.to_string()));
        let key = key.to_owned();
        let id = format!("pib-{key}");
        let menu = ContextMenu::build(window, cx, move |mut menu, _, _| {
            for value in allowed {
                let weak = weak.clone();
                let key = key.clone();
                let name = value.as_str().map(|s| s.to_owned()).unwrap_or_else(|| value.to_string());
                menu = menu.entry(name, None, move |window, cx| {
                    let _ = weak.update(cx, |view, cx| {
                        let mut changes = serde_json::Map::new();
                        changes.insert(key.clone(), value.clone());
                        view.request(json!({"operation": "browse-set", "changes": Value::Object(changes)}), window, cx);
                    });
                });
            }
            menu
        });
        DropdownMenu::new(id, label, menu).style(DropdownStyle::Outlined).into_any_element()
    }
}

impl Render for PiBrowserSettingsView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let browse = self.browse();
        let installed = self.installed();
        let connected = self.connected();
        let version = browse["versions"]["cli"].as_str().unwrap_or("?");
        v_flex().id("zepi-pi-browser-settings").size_full().overflow_y_scroll().track_scroll(&self.scroll_handle).px_8().pb_12().gap_6()
            .child(v_flex().gap_2().pt_4()
                .child(h_flex().w_full().justify_between().gap_2()
                    .child(Label::new("Browser"))
                    .when(!self.notice.is_empty(), |this| this.child(Label::new(self.notice.clone()).size(LabelSize::Small).color(if self.failed { Color::Error } else { Color::Success }))))
                .child(muted("browse drives your real browser (logins, SSO) via a Chromium extension.")))
            .child(v_flex().gap_3().pt_4().border_t_1().border_color(cx.theme().colors().border_variant)
                .child(h_flex().w_full().justify_between().items_center()
                    .child(Label::new("Setup"))
                    .when(self.busy, |this| this.child(h_flex().gap_1().items_center()
                        .child(Icon::new(IconName::LoadCircle).size(IconSize::Small).color(Color::Muted).with_rotate_animation(2))
                        .child(Label::new("Working...").size(LabelSize::Small).color(Color::Muted)))))
                .when(!installed, |this| this
                    .child(muted(format!("browse is not installed. Folder: {}", self.folder)))
                    .child(h_flex().gap_2()
                        .child(Button::new("pib-install", "Install browse").style(ButtonStyle::Filled).disabled(self.busy).loading(self.busy && self.busy_op == "browse-install")
                            .on_click(cx.listener(|this, _, window, cx| this.request(json!({"operation": "browse-install", "dir": this.folder.clone()}), window, cx))))
                        .child(Button::new("pib-folder", "Choose folder...").style(ButtonStyle::Outlined).disabled(self.busy)
                            .on_click(cx.listener(|this, _, window, cx| this.choose_folder(window, cx))))))
                .when(installed, |this| this
                    .child(muted(format!("Installed v{version} in {}.", self.folder)))
                    .child(h_flex().gap_2()
                        .child(Button::new("pib-check", "Check for update").style(ButtonStyle::Outlined).disabled(self.busy).loading(self.busy && self.busy_op == "browse-check-update")
                            .on_click(cx.listener(|this, _, window, cx| this.request(json!({"operation": "browse-check-update"}), window, cx))))
                        .child(Button::new("pib-folder2", "Change folder...").style(ButtonStyle::Outlined).disabled(self.busy)
                            .on_click(cx.listener(|this, _, window, cx| this.choose_folder(window, cx))))
                        .when(self.update_info.as_ref().map(|u| u["updateAvailable"] == true).unwrap_or(false), |this| this
                            .child(Button::new("pib-update", "Update browse").style(ButtonStyle::Filled).disabled(self.busy).loading(self.busy && self.busy_op == "browse-update")
                                .on_click(cx.listener(|this, _, window, cx| this.request(json!({"operation": "browse-update"}), window, cx))))))
                    .child(muted(if connected { "Extension connected." } else { "Extension not connected. Install it and pair below." }))
                    .child(h_flex().gap_2()
                        .child(Button::new("pib-ext", "Install extension (Web Store)").style(ButtonStyle::Outlined)
                            .on_click(cx.listener(|this, _, _, _| this.open_extension())))
                        .child(Button::new("pib-pair", "Generate pairing code").style(ButtonStyle::Outlined).disabled(self.busy)
                            .on_click(cx.listener(|this, _, window, cx| this.request(json!({"operation": "browse-pair"}), window, cx)))))
                    .when(!self.pairing_code.is_empty(), |this| this.child(field("Pairing code (enter in extension Options, valid 10 min)",
                        h_flex().gap_2().items_center()
                            .child(Label::new(self.pairing_code.clone()))
                            .child(Button::new("pib-copy-code", "Copy").start_icon(Icon::new(IconName::Copy)).style(ButtonStyle::Outlined)
                                .on_click(cx.listener(|this, _, _, cx| {
                                    cx.write_to_clipboard(ClipboardItem::new_string(this.pairing_code.clone()));
                                    this.notice = "Pairing code copied.".into();
                                    cx.notify();
                                }))))))
                    .child(muted("1. Install the extension."))
                    .child(muted("2. Generate a code and enter it in extension Options > Pair with a code > Connect."))
                    .child(muted("3. New tasks use your real browser."))))
            .when(installed && !connected, |this| this.child(Button::new("pib-connect", "Connect").style(ButtonStyle::Outlined).disabled(self.busy).loading(self.busy && self.busy_op == "browse-status")
                .on_click(cx.listener(|this, _, window, cx| this.request(json!({"operation": "browse-status"}), window, cx)))))
            .when(installed && connected, |this| this.child(v_flex().gap_3().pt_4().border_t_1().border_color(cx.theme().colors().border_variant)
                .child(Label::new("Agent tools"))
                .child(muted("plain: the agent calls browser tools directly. code: the agent writes JavaScript (execute)."))
                .child(self.code_menu(window, cx))))
            .when(installed && connected, |this| this.child(v_flex().gap_3().pt_4().border_t_1().border_color(cx.theme().colors().border_variant)
                .child(Label::new("browse behavior"))
                .child(self.setting_menu("humanInput", "Human-like input", window, cx))
                .child(self.setting_menu("grouping", "Tab grouping", window, cx))
                .child(self.setting_menu("window", "Agent tabs window", window, cx))
                .child(self.setting_menu("idleMinutes", "Idle close (min)", window, cx))
                .child(self.setting_menu("handoffNotifications", "Handoff notifications", window, cx))))
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