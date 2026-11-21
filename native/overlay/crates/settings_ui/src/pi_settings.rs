use std::{collections::{HashMap, HashSet}, io::{BufRead, BufReader, Write}, process::{ChildStdin, Stdio}, sync::mpsc, time::Duration};

use editor::Editor;
use gpui::{Context, Entity, IntoElement, Render, Window};
use serde_json::{Value, json};
use ui::{Checkbox, ContextMenu, DropdownMenu, DropdownStyle, Tooltip, prelude::*};

pub(crate) mod sections {
    pub(crate) const GENERAL: &str = "general";
    pub(crate) const MODELS: &str = "models";
    pub(crate) const TOOLS: &str = "tools";
    pub(crate) const MCP: &str = "mcp";
    pub(crate) const SKILLS: &str = "skills";
}

struct ModelSettingsDraft {
    expected: Value,
    thinking: Value,
    reserve: Entity<Editor>,
    recent: Entity<Editor>,
}

pub(crate) struct PiSettingsView {
    data: Option<Value>,
    section: &'static str,
    scroll_handle: gpui::ScrollHandle,
    provider: String,
    model: String,
    thinking: String,
    model_settings_open: bool,
    model_settings: HashMap<String, ModelSettingsDraft>,
    enabled_open: HashSet<String>,
    default_tools: Entity<Editor>,
    http_proxy: Entity<Editor>,
    http_proxy_expected: Value,
    key: Entity<Editor>,
    key_provider: Option<String>,
    custom_provider: Entity<Editor>,
    custom_url: Entity<Editor>,
    custom_model: Entity<Editor>,
    custom_api: String,
    custom_key: Entity<Editor>,
    custom_model_open: bool,
    custom_editing: Option<String>,
    custom_revision: Value,
    custom_candidates: Vec<Value>,
    custom_selected: HashSet<String>,
    custom_incomplete: bool,
    delete_confirm: Option<String>,
    import_open: bool,
    import_text: Entity<Editor>,
    extensions: Option<Value>,
    ext_busy: bool,
    mcp_text: Entity<Editor>,
    mcp_add_open: bool,
    auth_name: Option<String>,
    auth_input: Option<ChildStdin>,
    auth_task: Option<gpui::Task<()>>,
    busy: bool,
    notice: String,
    failed: bool,
}

fn input(placeholder: &str, masked: bool, window: &mut Window, cx: &mut Context<PiSettingsView>) -> Entity<Editor> {
    cx.new(|cx| {
        let mut editor = Editor::single_line(window, cx);
        editor.set_placeholder_text(placeholder, window, cx);
        editor.set_masked(masked, cx);
        editor
    })
}

impl PiSettingsView {
    pub(crate) fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let mut view = Self {
            data: None, section: sections::GENERAL, scroll_handle: gpui::ScrollHandle::new(), provider: String::new(), model: String::new(), thinking: "off".into(),
            model_settings_open: false, model_settings: HashMap::new(), enabled_open: HashSet::new(),
            default_tools: input("Leave blank to use Zepi defaults", false, window, cx),
            http_proxy: input("HTTP(S) proxy URL; leave blank to keep", true, window, cx), http_proxy_expected: Value::Null,
            key: input("API key or $ENV_VAR reference", true, window, cx), key_provider: None,
            custom_provider: input("Provider ID, e.g. my-provider", false, window, cx),
            custom_url: input("Base URL, e.g. https://api.example.com/v1", false, window, cx),
            custom_model: cx.new(|cx| { let mut editor = Editor::auto_height(3, 8, window, cx); editor.set_placeholder_text("Model IDs, separated by commas or new lines", window, cx); editor }),
            custom_api: "openai-completions".into(),
            custom_key: input("Leave blank to keep the stored key", true, window, cx),
            custom_model_open: false, custom_editing: None, custom_revision: Value::Null,
            custom_candidates: Vec::new(), custom_selected: HashSet::new(), custom_incomplete: false, delete_confirm: None,
            import_open: false,
            import_text: cx.new(|cx| { let mut editor = Editor::auto_height(6, 18, window, cx); editor.set_placeholder_text("Paste a provider JSON object, for example { \"my-gateway\": { \"baseUrl\": ..., \"api\": \"openai-completions\", \"models\": [...] } }", window, cx); editor }),
            extensions: None, ext_busy: false, mcp_add_open: false,
            mcp_text: cx.new(|cx| { let mut editor = Editor::auto_height(4, 12, window, cx); editor.set_placeholder_text("{ \"docs\": { \"url\": \"https://example.com/mcp\" } }", window, cx); editor }),
            auth_name: None, auth_input: None, auth_task: None,
            busy: false, notice: String::new(), failed: false,
        };
        view.request(json!({"operation":"status"}), window, cx);
        view.ext_request(json!({"operation":"status"}), window, cx);
        view
    }

    pub(crate) fn set_section(&mut self, section: &'static str) {
        if self.section != section {
            self.section = section;
            // A notice belongs to the page that produced it.
            self.notice.clear();
            self.failed = false;
        }
    }

    fn request(&mut self, request: Value, window: &mut Window, cx: &mut Context<Self>) {
        if self.busy { return; }
        self.busy = true;
        self.delete_confirm = None;
        self.notice.clear();
        self.failed = false;
        let operation = request["operation"].as_str().unwrap_or("status").to_owned();
        let saved_model = format!("{}/{}", request["provider"].as_str().unwrap_or_default(), request["model"].as_str().unwrap_or_default());
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
            if response["ok"] == true { Ok(response["data"].clone()) }
            else { Err(response["error"].as_str().unwrap_or("Unable to update pi settings.").to_owned()) }
        });
        cx.spawn_in(window, async move |view, cx| {
            let result = task.await;
            let _ = view.update_in(cx, |view, window, cx| {
                view.busy = false;
                match result {
                    Ok(data) => {
                        let model_catalog_notice = data["modelCatalogNotice"].as_str().unwrap_or_default().to_owned();
                        if operation == "list-connection-models" {
                            view.custom_candidates = data["candidates"].as_array().cloned().unwrap_or_default();
                            view.custom_selected.clear();
                            view.custom_incomplete = data["incomplete"].as_bool().unwrap_or(false);
                            cx.notify();
                            return;
                        }
                        if operation == "custom-connection" {
                            view.custom_key.update(cx, |editor, cx| editor.set_text("", window, cx));
                            view.custom_model_open = false;
                            view.custom_candidates.clear();
                            view.custom_selected.clear();
                        }
                        if operation == "import-connection" {
                            view.import_open = false;
                            view.import_text.update(cx, |editor, cx| editor.set_text("", window, cx));
                        }
                        if operation == "remove-connection" {
                            view.custom_editing = None;
                            view.custom_model_open = false;
                        }
                        if view.data.is_none() || matches!(operation.as_str(), "status" | "http-proxy" | "import") {
                            view.http_proxy_expected = data["httpProxy"]["revision"].clone();
                            view.http_proxy.update(cx, |editor, cx| editor.set_text("", window, cx));
                        }
                        if view.data.is_none() || matches!(operation.as_str(), "status" | "tool-defaults" | "import") {
                            let text = data["defaultTools"].as_array().map(|names| {
                                if names.is_empty() { "[]".to_owned() }
                                else { names.iter().filter_map(Value::as_str).collect::<Vec<_>>().join(", ") }
                            }).unwrap_or_default();
                            view.default_tools.update(cx, |editor, cx| editor.set_text(text, window, cx));
                        }
                        if view.data.is_none() || matches!(operation.as_str(), "status" | "import") {
                            view.provider = data["defaultProvider"].as_str().unwrap_or_default().into();
                            view.model = data["defaultModel"].as_str().unwrap_or_default().into();
                            view.thinking = data["defaultThinkingLevel"].as_str().unwrap_or("off").into();
                        }
                        view.data = Some(data);
                        if matches!(operation.as_str(), "status" | "import") { view.model_settings.clear(); }
                        if operation == "model-settings" { view.model_settings.remove(&saved_model); }
                        if view.provider.is_empty() {
                            let saved_provider = view.data.as_ref().and_then(|d| d["defaultProvider"].as_str()).unwrap_or_default().to_owned();
                            let saved_model = view.data.as_ref().and_then(|d| d["defaultModel"].as_str()).unwrap_or_default().to_owned();
                            let has_provider = view.providers().iter().any(|p| p["id"].as_str() == Some(saved_provider.as_str()));
                            view.provider = if has_provider { saved_provider } else {
                                view.providers().first().map(|p| p["id"].as_str().unwrap_or_default().into()).unwrap_or_default()
                            };
                            if view.models().iter().any(|m| m["id"].as_str() == Some(saved_model.as_str())) { view.model = saved_model; }
                            else { view.select_first_model(); }
                        }
                        view.ensure_model_settings(window, cx);
                        view.notice = match operation.as_str() {
                            "status" => model_catalog_notice,
                            "check-update" => String::new(),
                            "import" => "Local pi configuration imported. New tasks will use these settings.".into(),
                            "defaults" => "Defaults saved for new tasks. Existing tasks keep their models.".into(),
                            "model-settings" => "Model overrides saved. New tasks use them; /reload updates compaction in open tasks. Thinking applies when selecting this model.".into(),
                            "tool-defaults" => "Tool defaults saved. New tasks use this list; /reload adds newly selected tools to the current task.".into(),
                            "http-proxy" => "HTTP proxy saved. Restart Zepi to apply this change to all tasks.".into(),
                            "enabled-models" => "Enabled models saved. New tasks and the composer use this list.".into(),
                            "custom-connection" => "Connection saved. New tasks use it; reselect the model to refresh an open task. Defaults are unchanged.".into(),
                            "import-connection" => "Connection imported from JSON. Enable the models you want under Enabled models.".into(),
                            "remove-connection" => "Connection removed. Its stored key is gone too.".into(),
                            "custom-model" => "Model saved. Select it above to make it the default.".into(),
                            _ => "pi credentials updated.".into(),
                        };
                    }
                    Err(error) => { view.failed = true; view.notice = error; }
                }
                cx.notify();
            });
        }).detach();
        cx.notify();
    }

    fn ext_request(&mut self, request: Value, window: &mut Window, cx: &mut Context<Self>) {
        if self.ext_busy { return; }
        self.ext_busy = true;
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
                Some(a) => { let mut c = util::command::new_std_command(a); c.arg("pi-extensions"); c }
                None => { let mut c = util::command::new_std_command(node.ok_or_else(|| "Zepi Node.js runtime is unavailable.".to_string())?); c.arg(root.join("packages/runtime/bin/pi-extensions.mjs")); c }
            }
            .current_dir(&root)
                .stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null())
                .spawn().map_err(|_| "Unable to start pi extensions.".to_string())?;
            let body = serde_json::to_vec(&request).map_err(|_| "Unable to read extensions.".to_string())?;
            if let Some(mut stdin) = child.stdin.take() {
                stdin.write_all(&body).map_err(|_| "Unable to send extensions.".to_string())?;
            }
            let output = child.wait_with_output().map_err(|_| "The pi extensions request did not complete.".to_string())?;
            let response: Value = serde_json::from_slice(&output.stdout)
                .map_err(|_| "The pi extensions response was invalid.".to_string())?;
            if response["ok"] == true { Ok(response["data"].clone()) }
            else { Err(response["error"].as_str().unwrap_or("Unable to update pi extensions.").to_owned()) }
        });
        cx.spawn_in(window, async move |view, cx| {
            let result = task.await;
            let _ = view.update_in(cx, |view, window, cx| {
                view.ext_busy = false;
                match result {
                    Ok(data) => {
                        if operation == "mcp-add" {
                            view.mcp_add_open = false;
                            view.mcp_text.update(cx, |editor, cx| editor.set_text("", window, cx));
                        }
                        view.extensions = Some(data);
                        view.notice = match operation.as_str() {
                            "status" => String::new(),
                            "mcp-add" => "MCP servers added. New tasks connect to them.".into(),
                            "mcp-toggle" => "MCP server updated. New tasks use this setting.".into(),
                            "mcp-remove" => "MCP server removed.".into(),
                            "mcp-signout" => "Signed out of this MCP server.".into(),
                            "toggle-resource" => "Skill updated. New tasks use this setting.".into(),
                            _ => String::new(),
                        };
                    }
                    Err(error) => { view.failed = true; view.notice = error; }
                }
                cx.notify();
            });
        }).detach();
        cx.notify();
    }

    fn providers(&self) -> &[Value] {
        self.data.as_ref().and_then(|data| data["providers"].as_array()).map(Vec::as_slice).unwrap_or_default()
    }

    fn model_key(&self) -> String { format!("{}/{}", self.provider, self.model) }

    fn ensure_model_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.model_settings_open || self.model.is_empty() { return; }
        let key = self.model_key();
        if self.model_settings.contains_key(&key) { return; }
        let expected = self.models().iter().find(|model| model["id"].as_str() == Some(&self.model))
            .map(|model| model["settings"]["values"].clone()).unwrap_or_else(|| json!({}));
        let reserve = input("Use global default", false, window, cx);
        let recent = input("Use global default", false, window, cx);
        reserve.update(cx, |editor, cx| editor.set_text(runtime_text(&expected["reserveTokens"]), window, cx));
        recent.update(cx, |editor, cx| editor.set_text(runtime_text(&expected["keepRecentTokens"]), window, cx));
        self.model_settings.insert(key, ModelSettingsDraft {thinking: expected["thinkingLevel"].clone(), expected, reserve, recent});
    }

    fn save_model_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(draft) = self.model_settings.get(&self.model_key()) else { return; };
        let mut values = json!({"thinkingLevel": draft.thinking});
        for (key, editor) in [("reserveTokens", &draft.reserve), ("keepRecentTokens", &draft.recent)] {
            let text = editor.read(cx).text(cx);
            values[key] = if text.trim().is_empty() { Value::Null }
                else if let Ok(value) = text.trim().parse::<u64>() { json!(value) }
                else { self.failed = true; self.notice = "Enter a non-negative whole token count, or leave blank for the global default.".into(); cx.notify(); return; };
        }
        let changes: serde_json::Map<String, Value> = values.as_object().unwrap().iter()
            .filter(|(key, value)| *value != &draft.expected[*key]).map(|(key, value)| (key.clone(), value.clone())).collect();
        if changes.is_empty() { self.failed = false; self.notice = "No model overrides to save.".into(); cx.notify(); return; }
        self.request(json!({"operation":"model-settings", "provider": self.provider, "model": self.model,
            "changes": changes, "expected": draft.expected}), window, cx);
    }

    fn model_settings_section(&self, window: &mut Window, cx: &mut Context<Self>) -> gpui::AnyElement {
        let mut section = v_flex().gap_3().child(h_flex().child(Button::new("pi-model-overrides", "Model Overrides")
            .start_icon(Icon::new(if self.model_settings_open { IconName::ChevronDown } else { IconName::ChevronRight }))
            .on_click(cx.listener(|view, _, window, cx| {
                view.model_settings_open = !view.model_settings_open; view.ensure_model_settings(window, cx); cx.notify();
            }))));
        if !self.model_settings_open { return section.into_any_element(); }
        let key = self.model_key();
        let Some(draft) = self.model_settings.get(&key) else { return section.into_any_element(); };
        let selected = draft.thinking.clone();
        let levels = self.levels();
        let weak = cx.entity().downgrade();
        let menu = ContextMenu::build(window, cx, move |mut menu, _, _| {
            for value in std::iter::once(Value::Null).chain(levels.into_iter().map(Value::String)) {
                let weak = weak.clone(); let key = key.clone();
                let label = if value.is_null() { "Use global default".to_owned() } else { runtime_choice(&value) };
                menu = menu.entry(label, None, move |_, cx| { let _ = weak.update(cx, |view, cx| {
                    if view.busy { return; }
                    if let Some(draft) = view.model_settings.get_mut(&key) { draft.thinking = value.clone(); }
                    cx.notify();
                }); });
            }
            menu
        });
        let effective = self.models().iter().find(|model| model["id"].as_str() == Some(&self.model))
            .map(|model| model["settings"]["effective"].clone()).unwrap_or(Value::Null);
        section = section
            .child(Label::new(format!("Global overrides for {}", self.model_key())).size(LabelSize::Small).color(Color::Muted))
            .child(field("Thinking Level", DropdownMenu::new("pi-model-thinking", if selected.is_null() {"Use global default".to_owned()} else {runtime_choice(&selected)}, menu)
                .style(DropdownStyle::Outlined).full_width(true).into_any_element()))
            .child(field("Reserved Tokens", text_field(draft.reserve.clone(), cx)))
            .child(field("Recent Tokens to Keep", text_field(draft.recent.clone(), cx)))
            .child(Label::new(format!("Effective token limits: {} reserved · {} recent. Blank fields inherit global compaction settings.",
                runtime_text(&effective["reserveTokens"]), runtime_text(&effective["keepRecentTokens"]))).size(LabelSize::Small).color(Color::Muted))
            .child(h_flex().child(Button::new("pi-save-model-overrides", "Save Model Overrides").style(ButtonStyle::Outlined).disabled(self.busy)
                .on_click(cx.listener(|view, _, window, cx| view.save_model_settings(window, cx)))));
        section.into_any_element()
    }

    fn proxy_section(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let configured = self.data.as_ref().is_some_and(|data| data["httpProxy"]["configured"] == true);
        v_flex().gap_3().pt_4().border_t_1().border_color(cx.theme().colors().border_variant)
            .child(Label::new(if configured { "HTTP Proxy · Configured" } else { "HTTP Proxy · Environment defaults" }))
            .child(Label::new("Stored in global pi settings. Existing proxy environment variables take precedence. Restart Zepi after changes. URLs and credentials stay hidden.").size(LabelSize::Small).color(Color::Muted))
            .child(text_field(self.http_proxy.clone(), cx))
            .child(h_flex().gap_2()
                .child(Button::new("pi-save-http-proxy", "Save Proxy").style(ButtonStyle::Outlined).disabled(self.busy)
                    .on_click(cx.listener(|view, _, window, cx| {
                        let proxy = view.http_proxy.read(cx).text(cx);
                        if proxy.trim().is_empty() { view.failed = false; view.notice = "Proxy unchanged. Enter a URL to replace it, or choose Remove Proxy.".into(); cx.notify(); return; }
                        view.request(json!({"operation":"http-proxy", "proxy":proxy, "expected":view.http_proxy_expected}), window, cx);
                    })))
                .child(Button::new("pi-remove-http-proxy", "Remove Proxy").disabled(self.busy || !configured)
                    .on_click(cx.listener(|view, _, window, cx| view.request(json!({"operation":"http-proxy", "proxy":Value::Null, "expected":view.http_proxy_expected}), window, cx)))))
            .into_any_element()
    }

    fn selected_provider(&self) -> Option<&Value> {
        self.providers().iter().find(|p| p["id"].as_str() == Some(self.provider.as_str()))
    }
    fn models(&self) -> &[Value] {
        self.selected_provider().and_then(|p| p["models"].as_array()).map(Vec::as_slice).unwrap_or_default()
    }
    fn levels(&self) -> Vec<String> {
        self.models().iter().find(|m| m["id"].as_str() == Some(self.model.as_str()))
            .and_then(|m| m["thinkingLevels"].as_array())
            .map(|levels| levels.iter().filter_map(|v| v.as_str().map(str::to_owned)).collect()).unwrap_or_else(|| vec!["off".into()])
    }
    fn select_first_model(&mut self) {
        self.model = self.models().first().and_then(|m| m["id"].as_str()).unwrap_or_default().into();
        if !self.levels().contains(&self.thinking) { self.thinking = "off".into(); }
    }

    fn enabled_refs(&self) -> Vec<String> {
        let mut refs = Vec::new();
        for provider in self.providers() {
            let Some(id) = provider["id"].as_str() else { continue };
            for model in provider["models"].as_array().into_iter().flatten() {
                if model["enabled"] == true {
                    if let Some(model) = model["id"].as_str() { refs.push(format!("{id}/{model}")); }
                }
            }
        }
        refs
    }

    fn provider_refs(&self, provider: &str) -> Vec<String> {
        self.providers().iter().filter(|p| p["id"].as_str() == Some(provider))
            .flat_map(|p| p["models"].as_array().into_iter().flatten().filter_map(|m| m["id"].as_str().map(|id| format!("{provider}/{id}"))))
            .collect()
    }

    fn toggle_model(&mut self, reference: String, enabled: bool, window: &mut Window, cx: &mut Context<Self>) {
        let mut refs = self.enabled_refs();
        if enabled { if !refs.contains(&reference) { refs.push(reference); } }
        else { refs.retain(|value| value != &reference); }
        self.request(json!({"operation":"enabled-models","enabled":refs}), window, cx);
    }

    fn toggle_provider(&mut self, provider: String, enable: bool, window: &mut Window, cx: &mut Context<Self>) {
        let refs = self.provider_refs(&provider);
        let mut current = self.enabled_refs();
        if enable { for reference in refs { if !current.contains(&reference) { current.push(reference); } } }
        else { current.retain(|value| !refs.contains(value)); }
        self.request(json!({"operation":"enabled-models","enabled":current}), window, cx);
    }

    fn edit_connection(&mut self, connection: Option<Value>, window: &mut Window, cx: &mut Context<Self>) {
        if self.busy { return; }
        self.delete_confirm = None;
        let value = connection.unwrap_or(Value::Null);
        self.custom_editing = value["id"].as_str().map(str::to_owned);
        self.custom_revision = value["revision"].clone();
        self.custom_api = value["api"].as_str().unwrap_or("openai-completions").into();
        for (editor, text) in [(&self.custom_provider, value["id"].as_str().unwrap_or_default().to_owned()),
            (&self.custom_url, value["baseUrl"].as_str().unwrap_or_default().to_owned()),
            (&self.custom_model, value["models"].as_array().into_iter().flatten().filter_map(|m| m["id"].as_str()).collect::<Vec<_>>().join("\n")),
            (&self.custom_key, String::new())] {
            editor.update(cx, |editor, cx| editor.set_text(text, window, cx));
        }
        self.custom_candidates.clear(); self.custom_selected.clear(); self.custom_incomplete = false;
        self.custom_model_open = true;
        cx.notify();
    }

    fn connection_request(&self, operation: &str, cx: &App) -> Value {
        let models = self.custom_model.read(cx).text(cx).split([',', '\n', '\r']).map(str::trim)
            .filter(|s| !s.is_empty()).map(str::to_owned).collect::<Vec<_>>();
        json!({"operation":operation, "provider":self.custom_editing.clone().unwrap_or_else(|| self.custom_provider.read(cx).text(cx)),
            "baseUrl":self.custom_url.read(cx).text(cx), "api":self.custom_api, "models":models,
            "key":self.custom_key.read(cx).text(cx), "expectedRevision":self.custom_revision})
    }

    fn sign_in_mcp(&mut self, name: String, window: &mut Window, cx: &mut Context<Self>) {
        if self.ext_busy { return; }
        self.ext_busy = true;
        self.failed = false;
        self.notice = format!("Sign in to {name} in your browser.");
        self.auth_name = Some(name.clone());
        let (sender, receiver) = mpsc::channel();
        cx.background_spawn(async move { run_mcp_sign_in(name, sender); }).detach();
        self.auth_task = Some(cx.spawn_in(window, async move |this, cx| {
            loop {
                match receiver.try_recv() {
                    Ok(event) => {
                        let finished = matches!(&event, AuthEvent::Finished(_));
                        if this.update_in(cx, |this, window, cx| {
                            match event {
                                AuthEvent::Started(input) => {
                                    if this.auth_name.is_some() { this.auth_input = Some(input); }
                                }
                                AuthEvent::Url(url) => {
                                    if this.auth_name.is_some() { cx.open_url(&url); }
                                }
                                AuthEvent::Finished(result) => {
                                    this.auth_input = None;
                                    this.auth_name = None;
                                    this.ext_busy = false;
                                    match result {
                                        Ok(()) => {
                                            this.ext_request(json!({"operation":"status"}), window, cx);
                                            this.notice = "Signed in. New tasks use these credentials.".into();
                                        }
                                        Err(error) => { this.failed = error != "Sign-in cancelled."; this.notice = error; }
                                    }
                                }
                            }
                            cx.notify();
                        }).is_err() || finished { break; }
                    }
                    Err(mpsc::TryRecvError::Disconnected) => break,
                    Err(mpsc::TryRecvError::Empty) => cx.background_executor().timer(Duration::from_millis(50)).await,
                }
            }
        }));
        cx.notify();
    }

    fn selector(&self, id: &'static str, label: String, choices: Vec<(String, String)>, window: &mut Window, cx: &mut Context<Self>) -> gpui::AnyElement {
        let weak = cx.entity().downgrade();
        let busy = self.busy;
        let menu = ContextMenu::build(window, cx, move |mut menu, _, _| {
            for (value, name) in choices {
                let weak = weak.clone();
                menu = menu.entry(name, None, move |window, cx| {
                    let _ = weak.update(cx, |view, cx| {
                        if busy || view.busy { return; }
                        match id {
                            "pi-default-model" => {
                                if let Some((provider, model)) = value.split_once('/') {
                                    view.provider = provider.to_owned(); view.model = model.to_owned();
                                    if !view.levels().contains(&view.thinking) { view.thinking = "off".into(); }
                                }
                                view.key.update(cx, |editor, cx| editor.set_text("", window, cx));
                                view.key_provider = None;
                            }
                            "pi-thinking" => view.thinking = value.clone(),
                            "pi-custom-api" => view.custom_api = value.clone(),
                            _ => {}
                        }
                        view.ensure_model_settings(window, cx);
                        cx.notify();
                    });
                });
            }
            menu
        });
        DropdownMenu::new(id, label, menu).style(DropdownStyle::Outlined).full_width(true).into_any_element()
    }
}

impl Render for PiSettingsView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        v_flex().size_full()
            .child(div().px_8().pt_3().child(Label::new(self.header_text())))
            // Status strip: a hint by default, the last save or error after an action.
            .child(div().min_h(px(16.)).flex_none().px_8().pt_1().child(Label::new(if self.notice.is_empty() {
                self.default_hint().to_owned()
            } else { self.notice.clone() }).size(LabelSize::Small).color(if self.notice.is_empty() {
                Color::Muted
            } else if self.failed { Color::Error } else { Color::Success })))
            .child(v_flex().id("zepi-pi-settings").flex_1().min_h_0().overflow_y_scroll().track_scroll(&self.scroll_handle).px_8().pb_12().gap_4()
            .child(match self.section {
                sections::MODELS => self.models_page(window, cx),
                sections::TOOLS => self.tools_page(cx),
                sections::MCP => self.mcp_page(window, cx),
                sections::SKILLS => self.skills_page(window, cx),
                _ => self.general_page(window, cx),
            }))
    }
}

impl PiSettingsView {
    fn header_text(&self) -> String {
        match self.section {
            sections::MODELS => "Models and providers".to_owned(),
            sections::TOOLS => "Default tools".to_owned(),
            sections::MCP => "MCP servers".to_owned(),
            sections::SKILLS => "Skills".to_owned(),
            _ => {
                let version = self.data.as_ref().and_then(|data| data["version"].as_str()).unwrap_or("1.6.0");
                format!("pi {version} · Built into Zepi")
            }
        }
    }

    fn default_hint(&self) -> &'static str {
        match self.section {
            sections::MODELS => "Defaults apply to new tasks; an open task keeps its model until you reselect it.",
            sections::TOOLS => "Tool defaults apply to new tasks; /reload adds them to an open task.",
            sections::MCP => "mcp.json changes apply to new tasks; an open task picks them up after /reload.",
            sections::SKILLS => "Skill toggles apply to new tasks; /reload refreshes an open task.",
            _ => "Changes save straight to the pi configuration.",
        }
    }

    fn general_page(&self, _window: &mut Window, cx: &mut Context<Self>) -> gpui::AnyElement {
        let directory = self.data.as_ref().and_then(|d| d["directory"].as_str()).unwrap_or_default().to_owned();
        let update = self.data.as_ref().map(|d| &d["update"]);
        let update_label = match update.and_then(|u| u["status"].as_str()) {
            Some("up_to_date") => "You are using the latest stable pi release.".to_owned(),
            Some("update_available") => format!("pi {} is available. Your installed version has not changed.", update.and_then(|u| u["latest"].as_str()).unwrap_or_default()),
            Some("ahead") => "Your bundled version is newer than the stable release.".into(),
            Some("error") => "Unable to check for updates. Try again later.".into(),
            _ => "Updates have not been checked.".into(),
        };
        let checked_at = update.and_then(|u| u["checkedAt"].as_str()).map(|s| format!("Last checked: {s}"));
        v_flex().gap_4()
            .child(Label::new(if directory.is_empty() { "The configuration directory is unavailable.".to_owned() } else { directory })
                .size(LabelSize::XSmall).color(Color::Muted))
            .child(v_flex().gap_2().pt_4().border_t_1().border_color(cx.theme().colors().border_variant)
                .child(Label::new("pi Updates"))
                .child(Label::new(update_label).size(LabelSize::Small).color(Color::Muted))
                .when_some(checked_at, |this, text| this.child(Label::new(text).size(LabelSize::XSmall).color(Color::Muted)))
                .child(Button::new("pi-check-update", if self.busy { "Please Wait" } else { "Check for Updates" }).style(ButtonStyle::Outlined).disabled(self.busy)
                    .on_click(cx.listener(|this, _, window, cx| this.request(json!({"operation":"check-update"}), window, cx))))
                .child(Label::new("Checks the latest release without replacing the running agent.").size(LabelSize::Small).color(Color::Muted)))
            .child(v_flex().gap_2().pt_4().border_t_1().border_color(cx.theme().colors().border_variant)
                .child(Label::new("Import"))
                .child(Label::new("Copies models, credentials and defaults from your local pi configuration. Existing entries win.").size(LabelSize::Small).color(Color::Muted))
                .child(Button::new("pi-import", "Import Local pi Configuration").style(ButtonStyle::Outlined).disabled(self.busy)
                    .on_click(cx.listener(|this, _, window, cx| this.request(json!({"operation":"import"}), window, cx)))))
            .child(self.proxy_section(cx))
            .into_any_element()
    }

    fn models_page(&self, window: &mut Window, cx: &mut Context<Self>) -> gpui::AnyElement {
        let default_label = self.models().iter().find(|m| m["id"].as_str() == Some(self.model.as_str()))
            .map(|m| m["name"].as_str().unwrap_or_default().to_owned())
            .map(|name| format!("{} · {name}", self.selected_provider().and_then(|p| p["name"].as_str()).unwrap_or(&self.provider)))
            .unwrap_or_else(|| "Select a model".to_owned());
        let mut choices: Vec<(String, String)> = Vec::new();
        for provider in self.providers() {
            let Some(id) = provider["id"].as_str() else { continue };
            let name = provider["name"].as_str().unwrap_or(id);
            for model in provider["models"].as_array().into_iter().flatten() {
                let Some(model_id) = model["id"].as_str() else { continue };
                let enabled = model["enabled"] == true;
                let current = id == self.provider && model_id == self.model;
                if !enabled && !current { continue; }
                let label = format!("{name} · {}{}", model["name"].as_str().unwrap_or(model_id), if enabled { "" } else { " (disabled)" });
                choices.push((format!("{id}/{model_id}"), label));
            }
        }
        let levels = self.levels().into_iter().map(|level| (level.clone(), level)).collect();
        v_flex().gap_4()
            .child(field("Default Model", self.selector("pi-default-model", default_label, choices, window, cx)))
            .child(field("Thinking Level", self.selector("pi-thinking", self.thinking.clone(), levels, window, cx)))
            .child(h_flex().gap_2()
                .child(Button::new("pi-save-defaults", "Save Defaults").style(ButtonStyle::Filled).disabled(self.busy || self.model.is_empty())
                    .on_click(cx.listener(|this, _, window, cx| this.request(json!({"operation":"defaults", "provider":this.provider,"model":this.model,"thinking":this.thinking}), window, cx))))
                .child(Button::new("pi-refresh", "Reload & update models").disabled(self.busy)
                    .on_click(cx.listener(|this, _, window, cx| this.request(json!({"operation":"status", "refreshModels":true}), window, cx)))))
            .child(self.model_settings_section(window, cx))
            .child(self.providers_section(window, cx))
            .child(self.enabled_section(window, cx))
            .into_any_element()
    }

    fn providers_section(&self, window: &mut Window, cx: &mut Context<Self>) -> gpui::AnyElement {
        let connections = self.data.as_ref().and_then(|d| d["customConnections"].as_array()).cloned().unwrap_or_default();
        let mut rows: Vec<gpui::AnyElement> = Vec::new();
        for provider in visible_providers(self.providers()) {
            let id = provider["id"].as_str().unwrap_or_default().to_owned();
            let custom = provider["custom"] == true;
            let credential = provider["credential"].as_str().unwrap_or("none").to_owned();
            let source = provider["credentialSource"].as_str().unwrap_or_default().to_owned();
            let count = provider["models"].as_array().map(|models| models.len()).unwrap_or(0);
            let status = match source.as_str() {
                "api_key" => "api key stored".to_owned(),
                "oauth" => "oauth session".to_owned(),
                "environment" => "environment".to_owned(),
                "models.json" => "models.json".to_owned(),
                "" => match credential.as_str() {
                    "oauth" => "oauth session".to_owned(), "api_key" => "api key stored".to_owned(),
                    "none" => "no credentials".to_owned(), _ => "environment".to_owned(),
                },
                other => other.to_owned(),
            };
            let stored = matches!(source.as_str(), "api_key" | "oauth") || matches!(credential.as_str(), "api_key" | "oauth");
            let connection = connections.iter().find(|c| c["id"].as_str() == Some(id.as_str())).cloned();
            let key_id = id.clone();
            let remove_id = id.clone();
            // Build the icons first, then pad on the left so columns stay put without gaps.
            let mut actions: Vec<gpui::AnyElement> = Vec::new();
            actions.push(IconButton::new(SharedString::from(format!("pi-key-{id}")), IconName::Lock)
                .icon_size(IconSize::Small)
                .aria_label("Set API key")
                .tooltip(Tooltip::text("Set API key"))
                .disabled(self.busy)
                .on_click(cx.listener(move |this, _, _, cx| { this.key_provider = Some(key_id.clone()); cx.notify(); }))
                .into_any_element());
            if stored {
                let label = if credential == "oauth" { "Sign out" } else { "Remove key" };
                actions.push(IconButton::new(SharedString::from(format!("pi-remove-key-{id}")), IconName::Trash)
                    .icon_size(IconSize::Small)
                    .aria_label(label)
                    .tooltip(Tooltip::text(label))
                    .disabled(self.busy)
                    .on_click(cx.listener(move |this, _, window, cx| this.request(json!({"operation":"remove-key", "provider":remove_id.clone()}), window, cx)))
                    .into_any_element());
            }
            if let Some(connection) = connection {
                actions.push(IconButton::new(SharedString::from(format!("pi-connection-edit-{id}")), IconName::Pencil)
                    .icon_size(IconSize::Small)
                    .aria_label("Edit connection")
                    .tooltip(Tooltip::text("Edit connection"))
                    .disabled(self.busy)
                    .on_click(cx.listener(move |this, _, window, cx| this.edit_connection(Some(connection.clone()), window, cx)))
                    .into_any_element());
            }
            let mut action_group = h_flex().gap_0p5().flex_none().items_center();
            for _ in actions.len()..3 {
                action_group = action_group.child(div().w(px(24.)).flex_none());
            }
            action_group = action_group.children(actions);
            let mut row = h_flex().min_w_0().gap_2().items_center()
                .child(Label::new(provider["name"].as_str().unwrap_or(&id).to_owned()))
                .child(Label::new(if custom { "Custom" } else { "Built-in" }).size(LabelSize::XSmall).color(Color::Muted))
                .child(Label::new(format!("{count} models")).size(LabelSize::XSmall).color(Color::Muted))
                .child(div().min_w_0().flex_1().child(Label::new(status).size(LabelSize::XSmall)
                    .color(if credential == "none" { Color::Warning } else { Color::Muted }).truncate()))
                .child(action_group);
            rows.push(row.into_any_element());
            if self.key_provider.as_deref() == Some(id.as_str()) {
                rows.push(v_flex().gap_2()
                    .child(text_field(self.key.clone(), cx))
                    .child(h_flex().gap_2()
                        .child(Button::new("pi-save-key", "Save API Key").style(ButtonStyle::Outlined).disabled(self.busy)
                            .on_click(cx.listener(|this, _, window, cx| {
                                let key = this.key.read(cx).text(cx);
                                let provider = this.key_provider.clone().unwrap_or_default();
                                this.key.update(cx, |editor, cx| editor.set_text("", window, cx));
                                this.key_provider = None;
                                this.request(json!({"operation":"key", "provider":provider, "key":key}), window, cx);
                            })))
                        .child(Button::new("pi-cancel-key", "Cancel").disabled(self.busy)
                            .on_click(cx.listener(|this, _, window, cx| { this.key.update(cx, |editor, cx| editor.set_text("", window, cx)); this.key_provider = None; cx.notify(); }))))
                    .into_any_element());
            }
        }
        v_flex().gap_3().pt_4().border_t_1().border_color(cx.theme().colors().border_variant)
            .child(Label::new("Providers"))
            .child(Label::new("Unconfigured built-in providers stay hidden until they have credentials. The badge names where the credential comes from.").size(LabelSize::XSmall).color(Color::Muted))
            .children(rows)
            .when(self.custom_model_open, |section| section.child(self.connection_form(window, cx)))
            .child(h_flex().gap_2()
                .child(Button::new("pi-connection-add", "+ Add custom API")
                    .disabled(self.busy || self.custom_model_open)
                    .on_click(cx.listener(|this, _, window, cx| this.edit_connection(None, window, cx))))
                .child(Button::new("pi-connection-import", "Import from JSON")
                    .disabled(self.busy)
                    .on_click(cx.listener(|this, _, _, cx| { this.import_open = !this.import_open; cx.notify(); }))))
            .when(self.import_open, |section| section.child(v_flex().gap_2().p_4().rounded_md()
                .border_1().border_color(cx.theme().colors().border)
                .child(Label::new("Paste a provider object"))
                .child(Label::new("One provider per import: baseUrl, api, apiKey ($ENV_VAR is fine) and models with their pi options. Existing models of the same provider are replaced.").size(LabelSize::XSmall).color(Color::Muted))
                .child(text_field(self.import_text.clone(), cx))
                .child(h_flex().gap_2().justify_end()
                    .child(Button::new("pi-connection-import-cancel", "Cancel").disabled(self.busy)
                        .on_click(cx.listener(|this, _, _, cx| { this.import_open = false; cx.notify(); })))
                    .child(Button::new("pi-connection-import-save", "Import Connection").style(ButtonStyle::Filled).disabled(self.busy)
                        .on_click(cx.listener(|this, _, window, cx| {
                            let text = this.import_text.read(cx).text(cx);
                            this.request(json!({"operation":"import-connection", "text":text}), window, cx);
                        }))))))
            .into_any_element()
    }

    fn connection_form(&self, window: &mut Window, cx: &mut Context<Self>) -> gpui::AnyElement {
        let title = match &self.custom_editing {
            Some(id) => format!("Edit {id}"),
            None => "New connection".to_owned(),
        };
        let hint = |text: &'static str, cx: &App| Label::new(text).size(LabelSize::XSmall).color(Color::Muted).into_any_element();
        let mut card = v_flex().gap_4().p_4().rounded_md().border_1().border_color(cx.theme().colors().border)
            .child(v_flex().gap_1()
                .child(Label::new(title))
                .child(Label::new("OpenAI-compatible or Anthropic-compatible endpoint. Existing model IDs keep their pi options.").size(LabelSize::Small).color(Color::Muted)))
            .child(v_flex().gap_3()
                .child(v_flex().gap_1()
                    .child(field("Provider ID", if let Some(id) = &self.custom_editing { Label::new(id.clone()).into_any_element() } else { text_field(self.custom_provider.clone(), cx) }))
                    .child(hint("Letters, numbers, dots, hyphens and underscores. Models are grouped under this name.", cx)))
                .child(v_flex().gap_1()
                    .child(field("Base URL", text_field(self.custom_url.clone(), cx)))
                    .child(hint("Example: https://api.example.com/v1", cx)))
                .child(field("Protocol", self.selector("pi-custom-api", match self.custom_api.as_str() {
                    "openai-responses" => "OpenAI Responses", "anthropic-messages" => "Anthropic Messages", _ => "OpenAI Chat Completions",
                }.to_owned(), vec![("openai-completions".into(), "OpenAI Chat Completions".into()),
                    ("openai-responses".into(), "OpenAI Responses".into()), ("anthropic-messages".into(), "Anthropic Messages".into())], window, cx)))
                .child(v_flex().gap_1()
                    .child(field("Model IDs", text_field(self.custom_model.clone(), cx)))
                    .child(hint("One ID per line or separated by commas. Fetch the list from the endpoint to pick them.", cx)))
                .child(v_flex().gap_1()
                    .child(field("API Key", text_field(self.custom_key.clone(), cx)))
                    .child(hint("Leave blank to keep the stored key. $ENV_VAR references an environment variable.", cx))));
        if self.custom_incomplete {
            card = card.child(Label::new("The endpoint returned more models than Zepi lists. Enter the rest manually.").size(LabelSize::Small).color(Color::Warning));
        }
        if !self.custom_candidates.is_empty() {
            let mut grid = h_flex().gap_3().gap_y_2().flex_wrap();
            for item in &self.custom_candidates {
                let Some(id) = item["id"].as_str() else { continue };
                let reference = id.to_owned();
                let selected = self.custom_selected.contains(id);
                let weak = cx.weak_entity();
                let accent = Color::Accent.color(cx);
                let target = reference.clone();
                let mut checkbox = Checkbox::new(SharedString::from(format!("pi-model-candidate-{reference}")), selected.into())
                    .label(reference)
                    .label_color(if selected { Color::Default } else { Color::Muted })
                    .disabled(self.busy)
                    .on_click(move |_, _, cx| {
                        let _ = weak.update(cx, |this, cx| {
                            if !this.custom_selected.remove(&target) { this.custom_selected.insert(target.clone()); }
                            cx.notify();
                        });
                    });
                if selected { checkbox = checkbox.style(ui::ToggleStyle::Custom(accent)).fill(); }
                grid = grid.child(div().w(px(260.)).flex_none().child(checkbox));
            }
            card = card
                .child(Label::new("Models found on the endpoint").size(LabelSize::Small).color(Color::Muted))
                .child(div().w_full().child(grid));
        }
        card.child(h_flex().gap_2().items_center()
            .children(self.custom_editing.clone().map(|id| {
                let armed = self.delete_confirm.as_deref() == Some(id.as_str());
                let confirm_id = id.clone();
                let state_id = id.clone();
                h_flex().gap_2()
                    .child(Button::new("pi-delete-connection", if armed { "Confirm delete" } else { "Delete connection" })
                        .style(ButtonStyle::Tinted(ui::TintColor::Error)).disabled(self.busy)
                        .on_click(cx.listener(move |this, _, window, cx| {
                            if this.delete_confirm.as_deref() != Some(confirm_id.as_str()) {
                                this.delete_confirm = Some(confirm_id.clone());
                                cx.notify();
                                return;
                            }
                            let revision = this.data.as_ref().and_then(|d| d["customConnections"].as_array()).and_then(|list| list.iter()
                                .find(|c| c["id"].as_str() == Some(confirm_id.as_str())).map(|c| c["revision"].clone())).unwrap_or(Value::Null);
                            this.delete_confirm = None;
                            this.request(json!({"operation":"remove-connection", "provider":confirm_id.clone(), "expectedRevision":revision}), window, cx);
                        })))
                    .when(armed, |row| row.child(Button::new("pi-delete-connection-cancel", "Cancel").disabled(self.busy)
                        .on_click(cx.listener(move |this, _, _, cx| { let _ = &state_id; this.delete_confirm = None; cx.notify(); }))))
                    .into_any_element()
            }))
            .child(div().flex_1())
            .when(!self.custom_candidates.is_empty(), |row| row.child(
                Button::new("pi-add-model-candidates", "Add selected").disabled(self.busy)
                    .on_click(cx.listener(|this, _, window, cx| {
                        let current = this.custom_model.read(cx).text(cx);
                        let mut ids = current.split([',', '\n', '\r']).map(str::trim).filter(|s| !s.is_empty()).map(str::to_owned).collect::<Vec<_>>();
                        for item in &this.custom_candidates {
                            if let Some(id) = item["id"].as_str() {
                                if this.custom_selected.contains(id) && !ids.iter().any(|value| value == id) { ids.push(id.to_owned()); }
                            }
                        }
                        this.custom_model.update(cx, |editor, cx| editor.set_text(ids.join("\n"), window, cx));
                        this.custom_candidates.clear(); this.custom_selected.clear(); this.custom_incomplete = false;
                        cx.notify();
                    }))))
            .child(Button::new("pi-fetch-models", "Fetch models").disabled(self.busy)
                .on_click(cx.listener(|this, _, window, cx| { let request = this.connection_request("list-connection-models", cx); this.request(request, window, cx); })))
            .child(Button::new("pi-cancel-connection", "Cancel").disabled(self.busy)
                .on_click(cx.listener(|this, _, _, cx| { this.custom_model_open = false; this.custom_candidates.clear(); this.custom_selected.clear(); cx.notify(); })))
            .child(Button::new("pi-save-connection", "Save Connection").style(ButtonStyle::Filled).disabled(self.busy)
                .on_click(cx.listener(|this, _, window, cx| { let request = this.connection_request("custom-connection", cx); this.request(request, window, cx); }))))
            .into_any_element()
    }

    fn enabled_section(&self, window: &mut Window, cx: &mut Context<Self>) -> gpui::AnyElement {
        let total: usize = self.providers().iter().map(|p| p["models"].as_array().map(|m| m.len()).unwrap_or(0)).sum();
        let enabled = self.enabled_refs().len();
        let mut rows: Vec<gpui::AnyElement> = Vec::new();
        for provider in visible_providers(self.providers()) {
            let id = provider["id"].as_str().unwrap_or_default().to_owned();
            let name = provider["name"].as_str().unwrap_or(&id).to_owned();
            let models = provider["models"].as_array().cloned().unwrap_or_default();
            let on = models.iter().filter(|model| model["enabled"] == true).count();
            let all_enabled = !models.is_empty() && on == models.len();
            let open = self.enabled_open.contains(&id);
            let group_id = id.clone();
            let toggle_id = id.clone();
            let mut card = v_flex().gap_2()
                .child(h_flex().gap_2().items_center()
                    .child(Button::new(SharedString::from(format!("pi-enabled-group-{id}")), name)
                        .start_icon(Icon::new(if open { IconName::ChevronDown } else { IconName::ChevronRight }))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            if !this.enabled_open.remove(&group_id) { this.enabled_open.insert(group_id.clone()); }
                            cx.notify();
                        })))
                    .child(Label::new(format!("{on}/{}", models.len())).size(LabelSize::XSmall).color(Color::Muted))
                    .child(div().flex_1())
                    .child(Button::new(SharedString::from(format!("pi-enabled-provider-{id}")), if all_enabled { "Disable all" } else { "Enable all" })
                        .disabled(self.busy)
                        .on_click(cx.listener(move |this, _, window, cx| this.toggle_provider(toggle_id.clone(), !all_enabled, window, cx)))));
            if open {
                let mut grid = h_flex().gap_3().gap_y_2().flex_wrap();
                for model in models {
                    let Some(model_id) = model["id"].as_str() else { continue };
                    let reference = format!("{id}/{model_id}");
                    let selected = model["enabled"] == true;
                    let label = model["name"].as_str().unwrap_or(model_id).to_owned();
                    let weak = cx.weak_entity();
                    let target = reference.clone();
                    let accent = Color::Accent.color(cx);
                    let mut checkbox = Checkbox::new(SharedString::from(format!("pi-enabled-{reference}")), selected.into())
                        .label(label)
                        .label_color(if selected { Color::Default } else { Color::Muted })
                        .disabled(self.busy)
                        .on_click(move |_, window, cx| {
                            let _ = weak.update(cx, |this, cx| this.toggle_model(target.clone(), !selected, window, cx));
                        });
                    // Only a checked model is tinted, so the active set stands out.
                    if selected { checkbox = checkbox.style(ui::ToggleStyle::Custom(accent)).fill(); }
                    grid = grid.child(div().w(px(260.)).flex_none().child(checkbox));
                }
                card = card.child(div().w_full().child(grid));
            }
            rows.push(card.into_any_element());
        }
        v_flex().gap_3().pt_4().border_t_1().border_color(cx.theme().colors().border_variant)
            .child(h_flex().gap_2().justify_between()
                .child(Label::new(format!("Enabled models · {enabled}/{total}")))
                .child(h_flex().gap_2()
                    .child(Button::new("pi-enabled-all", "Enable all").disabled(self.busy)
                        .on_click(cx.listener(|this, _, window, cx| this.request(json!({"operation":"enabled-models", "all":true}), window, cx))))
                    .child(Button::new("pi-enabled-none", "Disable all").disabled(self.busy || enabled == 0)
                        .on_click(cx.listener(|this, _, window, cx| this.request(json!({"operation":"enabled-models", "enabled":Vec::<String>::new()}), window, cx))))))
            .child(Label::new("Stored as pi's enabledModels allowlist. Disabled models disappear from the composer and from task model choices. Expand a provider to pick individual models.").size(LabelSize::Small).color(Color::Muted))
            .children(rows)
            .into_any_element()
    }

    fn tools_page(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let effective = self.data.as_ref().and_then(|data| data["defaultToolsEffective"].as_array()).cloned().unwrap_or_default();
        let mut effective_row = h_flex().gap_2().flex_wrap();
        for name in effective.iter().filter_map(Value::as_str) {
            effective_row = effective_row.child(Label::new(name.to_owned()).size(LabelSize::Small)
                .color(if name == "codemode" { Color::Accent } else { Color::Default }));
        }
        let servers: Vec<String> = self.extensions.as_ref().and_then(|data| data["mcp"].as_array())
            .map(|rows| rows.iter().filter(|row| row["builtin"] != true).filter_map(|row| row["name"].as_str().map(str::to_owned)).collect())
            .unwrap_or_default();
        v_flex().gap_3()
            .child(text_field(self.default_tools.clone(), cx))
            .child(Label::new("Leave blank for read, edit, write, find, grep, ls and bash. Enter [] for no default file or shell tools.").size(LabelSize::Small).color(Color::Muted))
            .child(Label::new("Use tool names separated by commas. +codemode and -edit adjust pi defaults. Plugins keep their own defaults; agent roles still apply.").size(LabelSize::Small).color(Color::Muted))
            .child(Button::new("pi-save-tool-defaults", "Save Tool Defaults").style(ButtonStyle::Outlined).disabled(self.busy)
                .on_click(cx.listener(|this, _, window, cx| {
                    let text = this.default_tools.read(cx).text(cx);
                    let tools = if text.trim().is_empty() { Value::Null }
                        else if text.trim() == "[]" { json!([]) }
                        else { json!(text.split(',').map(str::trim).collect::<Vec<_>>()) };
                    let expected = this.data.as_ref().map(|data| data["defaultTools"].clone()).unwrap_or(Value::Null);
                    this.request(json!({"operation":"tool-defaults", "tools":tools, "expected":expected}), window, cx);
                })))
            .child(v_flex().gap_2().pt_4().border_t_1().border_color(cx.theme().colors().border_variant)
                .child(Label::new("Tools for new tasks"))
                .child(Label::new("Built-in and plugin tools resolved from pi defaults. codemode is highlighted because it carries the MCP tools.").size(LabelSize::XSmall).color(Color::Muted))
                .child(if effective.is_empty() { Label::new("No tools resolved yet. Choose Reload on the Models page.").size(LabelSize::Small).color(Color::Muted).into_any_element() }
                    else { effective_row.into_any_element() }))
            .child(v_flex().gap_2().pt_4().border_t_1().border_color(cx.theme().colors().border_variant)
                .child(Label::new("MCP tool namespaces"))
                .child(Label::new("Tools of an MCP server are named mcp__<server>__<tool>; codemode scripts call them by namespace.").size(LabelSize::XSmall).color(Color::Muted))
                .child(if servers.is_empty() { Label::new("No MCP servers configured. Add them on the MCP page.").size(LabelSize::Small).color(Color::Muted).into_any_element() }
                    else { servers.iter().fold(h_flex().gap_2().flex_wrap(), |row, name| row
                        .child(Label::new(format!("mcp__{name}__*")).size(LabelSize::Small).color(Color::Default))).into_any_element() }))
            .child(v_flex().gap_1().pt_4().border_t_1().border_color(cx.theme().colors().border_variant)
                .child(Label::new("Plugin tools"))
                .child(Label::new("Tools registered by a pi plugin cannot be listed before a task runs; the plugin announces them when it loads in a session.").size(LabelSize::XSmall).color(Color::Muted)))
            .into_any_element()
    }

    fn mcp_page(&self, _window: &mut Window, cx: &mut Context<Self>) -> gpui::AnyElement {
        let servers = self.extensions.as_ref().and_then(|data| data["mcp"].as_array()).cloned().unwrap_or_default();
        let loaded = self.extensions.is_some();
        let mut rows: Vec<gpui::AnyElement> = Vec::new();
        for server in servers.iter().filter(|server| server["builtin"] != true) {
            let name = server["name"].as_str().unwrap_or_default().to_owned();
            let enabled = server["enabled"] != false;
            let transport = server["transport"].as_str().unwrap_or_default();
            let exposure = server["exposure"].as_str().unwrap_or_default();
            let extra_exposure = server["toolExposure"].as_u64().unwrap_or(0);
            let route = match (transport.is_empty(), exposure.is_empty()) {
                (true, true) => String::new(),
                (false, true) => transport.to_owned(),
                (true, false) => format!("exposure: {exposure}"),
                (false, false) if extra_exposure > 0 => format!("{transport} · {exposure} · {extra_exposure} per-tool"),
                (false, false) => format!("{transport} · {exposure}"),
            };
            let oauth = server["oauth"] == true;
            let signed_in = server["signedIn"] == true;
            let runtime_only = server["runtimeOnly"] == true;
            let connections = server["connections"].as_array().cloned().unwrap_or_default();
            let mut label = "not connected in any open task".to_owned();
            let mut healthy = None;
            for (state, text) in [("connected", "connected"), ("connecting", "connecting"),
                ("needs-auth", "needs sign-in"), ("unavailable", "unavailable"),
                ("disconnected", "disconnected"), ("degraded", "degraded")] {
                if !connections.iter().any(|connection| connection["state"].as_str() == Some(state)) { continue; }
                label = text.to_owned();
                healthy = Some(state == "connected");
                if state == "connected" { break; }
            }
            // Parked sessions keep old settings; the page footnote explains /reload, so the row
            // only reports the connection state itself.
            let sessions = connections.iter().filter_map(|connection| connection["sessionId"].as_str()).collect::<HashSet<_>>().len();
            let status = if sessions > 1 { format!("{label} · {sessions} sessions") } else { label };
            let (status, healthy) = (status, healthy);
            let toggle_name = name.clone();
            let remove_name = name.clone();
            let signout_name = name.clone();
            let signin_name = name.clone();
            let mut row = h_flex().gap_2().items_center()
                .child(Label::new(name.clone()))
                .when(!route.is_empty(), |row| row.child(Label::new(route.clone()).size(LabelSize::XSmall).color(Color::Muted)))
                .child(Label::new(status).size(LabelSize::XSmall).color(match healthy {
                    Some(true) => Color::Success,
                    Some(false) => Color::Warning,
                    None => Color::Muted,
                }));
            if oauth {
                row = row.child(Label::new(if signed_in { "Signed in" } else { "Sign-in required" }).size(LabelSize::XSmall)
                    .color(if signed_in { Color::Muted } else { Color::Warning }));
            }
            row = row.child(div().flex_1());
            if !runtime_only {
                row = row.child(Button::new(SharedString::from(format!("pi-mcp-toggle-{name}")), if enabled { "Disable" } else { "Enable" })
                    .disabled(self.ext_busy)
                    .on_click(cx.listener(move |this, _, window, cx| this.ext_request(json!({"operation":"mcp-toggle", "name":toggle_name.clone(), "enabled":!enabled}), window, cx))));
            }
            if oauth {
                if signed_in {
                    row = row.child(Button::new(SharedString::from(format!("pi-mcp-sign-out-{name}")), "Sign out").disabled(self.ext_busy)
                        .on_click(cx.listener(move |this, _, window, cx| this.ext_request(json!({"operation":"mcp-signout", "name":signout_name.clone()}), window, cx))));
                } else {
                    row = row.child(Button::new(SharedString::from(format!("pi-mcp-sign-in-{name}")), "Sign in").disabled(self.ext_busy)
                        .on_click(cx.listener(move |this, _, window, cx| this.sign_in_mcp(signin_name.clone(), window, cx))));
                }
            }
            if !runtime_only {
                row = row.child(Button::new(SharedString::from(format!("pi-mcp-remove-{name}")), "Remove").disabled(self.ext_busy)
                    .on_click(cx.listener(move |this, _, window, cx| this.ext_request(json!({"operation":"mcp-remove", "name":remove_name.clone()}), window, cx))));
            }
            rows.push(row.into_any_element());
            if runtime_only {
                rows.push(Label::new("Provided by a pi plugin or an open task; it is not in mcp.json.").size(LabelSize::XSmall).color(Color::Muted).into_any_element());
            }
        }
        v_flex().gap_3()
            .child(h_flex().gap_2().justify_end()
                .child(Button::new("pi-mcp-refresh", "Refresh").disabled(self.ext_busy)
                    .on_click(cx.listener(|this, _, window, cx| this.ext_request(json!({"operation":"status"}), window, cx))))
                .child(Button::new("pi-mcp-add-toggle", if self.mcp_add_open { "Cancel" } else { "Add server" })
                    .start_icon(Icon::new(if self.mcp_add_open { IconName::ChevronDown } else { IconName::Plus }))
                    .disabled(self.ext_busy)
                    .on_click(cx.listener(|this, _, _, cx| { this.mcp_add_open = !this.mcp_add_open; cx.notify(); }))))
            .when(!loaded, |body| body.child(Label::new(if self.ext_busy { "Loading servers..." } else { "No server data yet. Choose Refresh." })
                .size(LabelSize::Small).color(Color::Muted)))
            .when(self.mcp_add_open, |body| body.child(v_flex().gap_2()
                .child(Label::new("Paste servers as JSON, for example { \"docs\": { \"url\": \"https://example.com/mcp\" } } or a full mcpServers object.").size(LabelSize::Small).color(Color::Muted))
                .child(text_field(self.mcp_text.clone(), cx))
                .child(h_flex().child(Button::new("pi-mcp-add", "Add servers").style(ButtonStyle::Filled).disabled(self.ext_busy)
                    .on_click(cx.listener(|this, _, window, cx| {
                        let text = this.mcp_text.read(cx).text(cx);
                        this.ext_request(json!({"operation":"mcp-add", "text":text}), window, cx);
                    }))))))
            .when(loaded && rows.is_empty(), |body| body
                .child(Label::new("No user-level MCP servers configured.").size(LabelSize::Small).color(Color::Muted)))
            .when_some(self.extensions.as_ref().map(|data| data["mcpCodemode"] == true), |body, codemode| body
                .child(Label::new(if codemode {
                    "Exposure: direct is declared to the model, codemode is reachable from scripts, deferred needs tool search, hidden is not exposed. Codemode activates automatically."
                } else {
                    "Exposure: direct is declared to the model, codemode is reachable from scripts, deferred needs tool search, hidden is not exposed. Codemode auto-activation is off in mcp.json."
                }).size(LabelSize::XSmall).color(Color::Muted)))
            .children(rows)
            .child(Label::new("Connections belong to open tasks. A dropped server reconnects on its next call, and mcp.json changes apply after /reload in the task.").size(LabelSize::XSmall).color(Color::Muted))
            .child(Label::new("Project servers live in the project's .pi/mcp.json and load only in a trusted project.").size(LabelSize::XSmall).color(Color::Muted))
            .into_any_element()
    }

    fn skills_page(&self, _window: &mut Window, cx: &mut Context<Self>) -> gpui::AnyElement {
        let skills = self.extensions.as_ref().and_then(|data| data["skills"].as_array()).cloned().unwrap_or_default();
        v_flex().gap_3()
            .child(h_flex().gap_2().justify_end()
                .child(Button::new("pi-skills-refresh", "Refresh").disabled(self.ext_busy)
                    .on_click(cx.listener(|this, _, window, cx| this.ext_request(json!({"operation":"status"}), window, cx)))))
            .when(self.extensions.is_none(), |body| body.child(Label::new(if self.ext_busy { "Loading skills..." } else { "Open this page to load the installed skills." }).size(LabelSize::Small).color(Color::Muted)))
            .when(self.extensions.is_some() && skills.is_empty(), |body| body
                .child(Label::new("No user-level skills installed. Skills from pi packages appear here automatically.").size(LabelSize::Small).color(Color::Muted)))
            .children(skills.iter().map(|skill| {
                let name = skill["name"].as_str().unwrap_or_default().to_owned();
                let description = skill["description"].as_str().unwrap_or_default()
                    .split_whitespace().collect::<Vec<_>>().join(" ");
                let path = skill["path"].as_str().unwrap_or_default().to_owned();
                let enabled = skill["enabled"] != false;
                let target = path.clone();
                v_flex().gap_1().py_1()
                    .child(h_flex().gap_2().items_center()
                        .child(Label::new(name))
                        .child(div().flex_1())
                        .child(Button::new(SharedString::from(format!("pi-skill-toggle-{}", path)), if enabled { "Disable" } else { "Enable" }).disabled(self.ext_busy)
                            .on_click(cx.listener(move |this, _, window, cx| this.ext_request(json!({"operation":"toggle-resource", "kind":"skills", "path":target, "enabled":!enabled}), window, cx)))))
                    .when(!description.is_empty(), |card| card.child(Label::new(description).size(LabelSize::Small).color(Color::Muted)))
                    .child(Label::new(path).size(LabelSize::XSmall).color(Color::Muted))
            }))
            .child(Label::new("Project skills live in .pi/skills/<name>/SKILL.md inside the project (.agents/skills also works) and load only in a trusted project. The list above shows user-level skills.").size(LabelSize::XSmall).color(Color::Muted))
            .into_any_element()
    }
}

fn visible_providers(providers: &[Value]) -> Vec<&Value> {
    providers.iter().filter(|provider| {
        provider["custom"] == true
            || provider["credential"].as_str().unwrap_or("none") != "none"
            || provider["models"].as_array().into_iter().flatten().any(|model| model["enabled"] == true)
    }).collect()
}

fn runtime_text(value: &Value) -> String {
    if value.is_null() { String::new() } else { value.as_str().map(str::to_owned).unwrap_or_else(|| value.to_string()) }
}
fn runtime_choice(value: &Value) -> String {
    match value {
        Value::Null => "Use pi default".into(), Value::Bool(true) => "On".into(), Value::Bool(false) => "Off".into(),
        Value::String(value) if value == "one-at-a-time" => "One at a time".into(),
        Value::String(value) if value == "all" => "All pending messages".into(),
        _ => runtime_text(value),
    }
}

fn field(label: &str, element: impl IntoElement) -> gpui::AnyElement {
    v_flex().gap_2().w_full().child(Label::new(label.to_owned()).size(LabelSize::Small)).child(element).into_any_element()
}
fn text_field(editor: Entity<Editor>, cx: &App) -> gpui::AnyElement {
    div().w_full().px_2().py_2().rounded_md().border_1().border_color(cx.theme().colors().border)
        .bg(cx.theme().colors().editor_background).child(editor).into_any_element()
}

enum AuthEvent { Started(ChildStdin), Url(String), Finished(Result<(), String>) }

fn run_mcp_sign_in(name: String, events: mpsc::Sender<AuthEvent>) {
    let result = (|| -> Result<(), String> {
        let root = std::env::var_os("ZEPI_ROOT").ok_or("Zepi runtime directory is unavailable.")?;
        let root = std::path::PathBuf::from(root);
        let agent = std::env::var_os("ZEPI_AGENT").map(std::path::PathBuf::from).filter(|p| p.is_file())
            .or_else(|| { let b = root.join("agent.exe"); b.is_file().then_some(b) });
        let mut child = match agent {
            Some(a) => { let mut c = util::command::new_std_command(a); c.arg("mcp-auth").arg(name); c }
            None => { let mut c = util::command::new_std_command(std::env::var_os("ZEPI_NODE").ok_or("Zepi Node.js runtime is unavailable.")?); c.arg(root.join("packages/runtime/bin/mcp-auth.mjs")).arg(name); c }
        }.current_dir(root)
            .stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null()).spawn()
            .map_err(|_| "Unable to start MCP sign-in.".to_owned())?;
        // Dropping the pipe cancels in pi and closes its callback listener.
        let input = child.stdin.take().ok_or("MCP sign-in input is unavailable.")?;
        if events.send(AuthEvent::Started(input)).is_err() {
            let _ = child.wait();
            return Ok(());
        }
        let output = child.stdout.take().ok_or("MCP sign-in output is unavailable.")?;
        let mut result = Err("MCP sign-in did not complete.".to_owned());
        for line in BufReader::new(output).lines() {
            let Ok(line) = line else { break; };
            let Ok(value) = serde_json::from_str::<Value>(&line) else { continue; };
            if let Some(url) = value["url"].as_str() {
                if !(url.starts_with("https://") || url.starts_with("http://")) { continue; }
                if events.send(AuthEvent::Url(url.to_owned())).is_err() { break; }
            } else if value["ok"] == true { result = Ok(()); }
            else if value["ok"] == false { result = Err(value["error"].as_str().unwrap_or_default().to_owned()); }
        }
        let _ = child.wait();
        result
    })();
    let _ = events.send(AuthEvent::Finished(result));
}

fn render_page(
    settings: &crate::SettingsWindow,
    section: &'static str,
    cx: &mut gpui::App,
) -> gpui::AnyElement {
    if let Some(view) = &settings.pi_settings { view.update(cx, |view, _| view.set_section(section)); }
    div().size_full().children(settings.pi_settings.clone()).into_any_element()
}

pub(crate) fn render_pi_general_page(
    settings: &crate::SettingsWindow,
    _: &gpui::ScrollHandle,
    _: &mut Window,
    cx: &mut Context<crate::SettingsWindow>,
) -> gpui::AnyElement {
    render_page(settings, sections::GENERAL, cx)
}

pub(crate) fn render_pi_models_page(
    settings: &crate::SettingsWindow,
    _: &gpui::ScrollHandle,
    _: &mut Window,
    cx: &mut Context<crate::SettingsWindow>,
) -> gpui::AnyElement {
    render_page(settings, sections::MODELS, cx)
}

pub(crate) fn render_pi_tools_page(
    settings: &crate::SettingsWindow,
    _: &gpui::ScrollHandle,
    _: &mut Window,
    cx: &mut Context<crate::SettingsWindow>,
) -> gpui::AnyElement {
    render_page(settings, sections::TOOLS, cx)
}

pub(crate) fn render_pi_mcp_page(
    settings: &crate::SettingsWindow,
    _: &gpui::ScrollHandle,
    _: &mut Window,
    cx: &mut Context<crate::SettingsWindow>,
) -> gpui::AnyElement {
    render_page(settings, sections::MCP, cx)
}

pub(crate) fn render_pi_skills_page(
    settings: &crate::SettingsWindow,
    _: &gpui::ScrollHandle,
    _: &mut Window,
    cx: &mut Context<crate::SettingsWindow>,
) -> gpui::AnyElement {
    render_page(settings, sections::SKILLS, cx)
}
