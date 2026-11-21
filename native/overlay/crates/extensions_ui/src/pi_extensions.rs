use std::{io::Write, process::Stdio};

use serde_json::{Value, json};
use super::*;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum ExtensionSource { Editor, Pi }

impl ExtensionSource {
    pub const ALL: [Self; 2] = [Self::Editor, Self::Pi];
    pub fn label(self) -> &'static str {
        match self { Self::Editor => "Editor Extensions", Self::Pi => "pi Plugins" }
    }
    pub fn searchable(self) -> bool { matches!(self, Self::Editor | Self::Pi) }
}

pub(super) struct PiExtensionsState {
    data: Value,
    results: Vec<Value>,
    loaded: bool,
    busy: bool,
    searching: bool,
    search_error: Option<String>,
    searched_query: Option<String>,
    pub(super) management_focus: gpui::FocusHandle,
    search_generation: u64,
    search_task: Option<Task<()>>,
    editor_query: String,
    query: String,
    package_input: Entity<Editor>,
    installing: bool,
    updates: Vec<String>,
    updates_checked: bool,
    notice: String,
    failed: bool,
}

impl PiExtensionsState {
    pub fn new(window: &mut Window, cx: &mut Context<ExtensionsPage>) -> Self {
        Self {
            data: json!({}), results: vec![], loaded: false, busy: false, searching: false,
            search_error: None, searched_query: None, management_focus: cx.focus_handle(),
            search_generation: 0, search_task: None, editor_query: String::new(), query: String::new(),
            package_input: cx.new(|cx| {
                let mut editor = Editor::single_line(window, cx);
                editor.set_placeholder_text("npm package name or absolute local package folder", window, cx);
                editor
            }),
            installing: false, updates: vec![], updates_checked: false,
            notice: String::new(), failed: false,
        }
    }

    fn finish_search(&mut self, generation: u64, result: Result<Value, String>) {
        if generation != self.search_generation { return; }
        self.searching = false;
        match result {
            Ok(data) => { self.results = array(&data, "results"); self.search_error = None; self.searched_query = Some(self.query.clone()); }
            Err(error) => self.search_error = Some(error),
        }
    }
}

fn string(value: &Value, key: &str) -> String { value[key].as_str().unwrap_or_default().to_owned() }
fn array(value: &Value, key: &str) -> Vec<Value> { value[key].as_array().cloned().unwrap_or_default() }

fn run_pi_request(request: Value) -> Result<Value, String> {
    let root = std::env::var_os("ZEPI_ROOT").ok_or("Zepi runtime directory is unavailable.")?;
    let root = std::path::PathBuf::from(root);
    let agent = std::env::var_os("ZEPI_AGENT").map(std::path::PathBuf::from).filter(|p| p.is_file())
        .or_else(|| { let b = root.join("agent.exe"); b.is_file().then_some(b) });
    let mut child = match agent {
        Some(a) => { let mut c = util::command::new_std_command(a); c.arg("pi-extensions"); c }
        None => { let mut c = util::command::new_std_command(std::env::var_os("ZEPI_NODE").ok_or("Zepi Node.js runtime is unavailable.")?); c.arg(root.join("packages/runtime/bin/pi-extensions.mjs")); c },
    }.current_dir(root)
        .stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null()).spawn()
        .map_err(|_| "Unable to manage pi extensions.".to_owned())?;
    if let Some(mut stdin) = child.stdin.take() {
        stdin.write_all(request.to_string().as_bytes()).map_err(|_| "Unable to send extension request.".to_owned())?;
    }
    let output = child.wait_with_output().map_err(|_| "Extension request did not complete.".to_owned())?;
    let response: Value = serde_json::from_slice(&output.stdout).map_err(|_| "Invalid extension response.".to_owned())?;
    if response["ok"] == true { Ok(response["data"].clone()) }
    else { Err(response["error"].as_str().unwrap_or("Extension operation failed.").to_owned()) }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum ResourceKind { Package, Extension }

pub(super) struct ResourceRow { kind: ResourceKind, data: Value, configured: bool }

fn resource_rows(data: &Value, results: &[Value], source: ExtensionSource, filter: ExtensionFilter, query: &str) -> Vec<ResourceRow> {
    let query = query.trim().to_lowercase();
    let matches = |value: &Value| query.is_empty() || ["name", "description", "source"].iter()
        .any(|key| string(value, key).to_lowercase().contains(&query));
    match source {
        ExtensionSource::Editor => vec![],
        ExtensionSource::Pi => {
            let packages = array(data, "packages");
            let mut rows = Vec::new();
            if filter != ExtensionFilter::NotInstalled {
                rows.extend(packages.iter().filter(|p| matches(p)).cloned()
                    .map(|data| ResourceRow { kind: ResourceKind::Package, data, configured: true }));
                // Package-owned extensions are managed by the package card; only standalone
                // extensions need their own row in the same list.
                rows.extend(array(data, "extensions").into_iter()
                    .filter(|p| p["metadata"]["origin"] != "package" && matches(p))
                    .map(|data| ResourceRow { kind: ResourceKind::Extension, data, configured: true }));
            }
            if filter != ExtensionFilter::Installed {
                rows.extend(results.iter().filter(|p| !packages.iter().any(|installed| installed["source"] == p["source"]
                    || (installed["name"].is_string() && installed["name"] == p["name"])))
                    .cloned().map(|data| ResourceRow { kind: ResourceKind::Package, data, configured: false }));
            }
            rows
        }
    }
}

impl ExtensionsPage {
    pub(super) fn change_source(&mut self, source: ExtensionSource, window: &mut Window, cx: &mut Context<Self>) {
        if self.source == source { return; }
        match self.source {
            ExtensionSource::Editor => self.pi.editor_query = self.query_editor.read(cx).text(cx),
            ExtensionSource::Pi => self.pi.query = self.query_editor.read(cx).text(cx),
            _ => {}
        }
        self.pi.search_generation += 1;
        self.pi.search_task = None;
        self.pi.searching = false;
        self.extension_fetch_task = None;
        self.source = source;
        let query = match source { ExtensionSource::Editor => self.pi.editor_query.clone(), ExtensionSource::Pi => self.pi.query.clone(), _ => String::new() };
        self.query_editor.update(cx, |editor, cx| {
            editor.set_placeholder_text(if source == ExtensionSource::Pi { "Search pi plugins..." } else { "Search extensions..." }, window, cx);
            editor.set_text(query, window, cx);
        });
        if source != ExtensionSource::Editor && !self.pi.loaded && !self.pi.busy {
            self.request_pi(json!({"operation":"status"}), window, cx);
        }
        // Registry check runs once per page; it only decides whether "Update" applies.
        if source == ExtensionSource::Pi && !self.pi.updates_checked {
            self.pi.updates_checked = true;
            self.load_pi_updates(cx);
        }
        if source.searchable() {
            self.query_editor.read(cx).focus_handle(cx).focus(window, cx);
            self.refresh_search(cx);
        } else { self.pi.management_focus.focus(window, cx); }
        self.scroll_to_top(cx);
    }

    pub(super) fn search_pi_debounced(&mut self, cx: &mut Context<Self>) {
        if self.source != ExtensionSource::Pi { return; }
        if self.filter == ExtensionFilter::Installed {
            self.pi.search_generation += 1;
            self.pi.search_task = None;
            self.pi.searching = false;
            cx.notify();
            return;
        }
        let query = self.query_editor.read(cx).text(cx);
        if self.pi.searched_query.as_deref() == Some(query.as_str()) && self.pi.search_error.is_none() { return; }
        self.pi.query = query.clone();
        self.pi.search_generation += 1;
        let generation = self.pi.search_generation;
        self.pi.searched_query = None;
        self.pi.results.clear();
        self.pi.searching = true;
        self.pi.search_task = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(Duration::from_millis(250)).await;
            let result = cx.background_spawn(async move { run_pi_request(json!({"operation":"search", "query":query})) }).await;
            this.update(cx, |this, cx| {
                this.pi.finish_search(generation, result);
                cx.notify();
            }).ok();
        }));
        cx.notify();
    }

    fn request_pi(&mut self, request: Value, window: &mut Window, cx: &mut Context<Self>) {
        if self.pi.busy { return; }
        self.pi.busy = true; self.pi.notice.clear(); self.pi.failed = false;
        let operation = string(&request, "operation");
        let task = cx.background_spawn(async move { run_pi_request(request) });
        cx.spawn_in(window, async move |this, cx| {
            let result = task.await;
            this.update_in(cx, |this, window, cx| {
                this.pi.busy = false;
                match result {
                    Ok(data) => {
                        this.pi.data = data; this.pi.loaded = true;
                        if operation == "install" { this.pi.installing = false; }
                        if operation != "status" {
                            this.pi.notice = if array(&this.pi.data, "pending").is_empty() {
                                "Updated. New pi tasks use these resource settings."
                            } else { "Package changes are queued. Restart Zepi to apply them." }.into();
                        }
                    }
                    Err(error) => { this.pi.failed = true; this.pi.notice = error; }
                }
                cx.notify();
            }).ok();
        }).detach();
        cx.notify();
    }

    fn load_pi_updates(&mut self, cx: &mut Context<Self>) {
        let task = cx.background_spawn(async move { run_pi_request(json!({"operation":"updates"})) });
        cx.spawn(async move |this, cx| {
            let Ok(data) = task.await else { return };
            this.update(cx, |this, cx| {
                this.pi.updates = array(&data, "updates").iter().filter_map(|item| item.as_str().map(str::to_owned)).collect();
                cx.notify();
            }).ok();
        }).detach();
    }

    fn pi_action(&self, id: String, label: &str, request: Value, cx: &mut Context<Self>) -> Button {
        Button::new(SharedString::from(id), label.to_owned()).disabled(self.pi.busy)
            .on_click(cx.listener(move |this, _, window, cx| this.request_pi(request.clone(), window, cx)))
    }

    pub(super) fn render_source_actions(&self, cx: &mut Context<Self>) -> AnyElement {
        if self.source == ExtensionSource::Editor {
            return Button::new("install-dev-extension", "Install Dev Extension").style(ButtonStyle::Outlined)
                .size(ButtonSize::Medium).on_click(|_, window, cx| window.dispatch_action(Box::new(InstallDevExtension), cx))
                .into_any_element();
        }
        h_flex().gap_2()
            .when(self.source == ExtensionSource::Pi, |row| row.child(
                Button::new("install-pi-package", "Install Package…").style(ButtonStyle::Outlined)
                    .disabled(self.pi.busy).on_click(cx.listener(|this, _, window, cx| {
                        this.pi.installing = !this.pi.installing;
                        if this.pi.installing { this.pi.package_input.read(cx).focus_handle(cx).focus(window, cx); }
                        cx.notify();
                    }))))
            .child(Button::new("refresh-pi-resources", "Refresh").disabled(self.pi.busy)
                .on_click(cx.listener(|this, _, window, cx| {
                    this.request_pi(json!({"operation":"status"}), window, cx);
                    if this.source == ExtensionSource::Pi { this.pi.searched_query = None; this.search_pi_debounced(cx); }
                })))
            .into_any_element()
    }

    pub(super) fn pi_rows(&self, cx: &App) -> Vec<ResourceRow> {
        resource_rows(&self.pi.data, &self.pi.results, self.source, self.filter, &self.query_editor.read(cx).text(cx))
    }

    pub(super) fn render_source_entries(&mut self, range: Range<usize>, window: &mut Window, cx: &mut Context<Self>) -> Vec<ExtensionCard> {
        if self.source == ExtensionSource::Editor { return self.render_extensions(range, window, cx); }
        let rows = self.pi_rows(cx);
        range.filter_map(|index| rows.get(index)).map(|row| self.render_pi_card(row, cx)).collect()
    }

    fn render_pi_card(&self, row: &ResourceRow, cx: &mut Context<Self>) -> ExtensionCard {
        let data = &row.data;
        let name = string(data, "name");
        let enabled = data["enabled"] == true;
        let mut actions = [None, None, None];
        let (id, feature, description) = match row.kind {
            ResourceKind::Package => {
                let source = string(data, "source");
                let pending = array(&self.pi.data, "pending").iter().any(|p| p["source"] == data["source"]);
                if !pending {
                    if row.configured {
                        actions[0] = Some(self.pi_action(format!("toggle-{source}"), if enabled {"Disable"} else {"Enable"}, json!({"operation":"toggle-package","source":source,"enabled":!enabled}), cx));
                        if self.pi.updates.iter().any(|stale| stale == &source) {
                            actions[1] = Some(self.pi_action(format!("update-{source}"), "Update", json!({"operation":"update","source":source}), cx));
                        }
                        actions[2] = Some(self.pi_action(format!("remove-{source}"), "Uninstall", json!({"operation":"remove","source":source}), cx));
                    } else { actions[0] = Some(self.pi_action(format!("install-{source}"), "Install", json!({"operation":"install","source":source}), cx)); }
                }
                (source, "pi Plugin", string(data, "description"))
            }
            ResourceKind::Extension => {
                let path = string(data, "path");
                actions[0] = Some(self.pi_action(format!("toggle-{path}"), if enabled {"Disable"} else {"Enable"}, json!({"operation":"toggle-resource","kind":"extensions","path":path,"enabled":!enabled}), cx));
                let description = string(data, "description");
                (path.clone(), "Extension", if description.is_empty() {path} else {description})
            }
        };
        let status = (if array(&self.pi.data, "pending").iter().any(|p| p["source"].as_str() == Some(&id)) {
            "Pending · Restart Zepi to apply"
        } else if data["builtin"] == true && data["enabled"] == false { "Built-in · Unavailable" }
        else if data["builtin"] == true { "Built-in · Enabled" }
        else if !row.configured { "Available from npm" }
        else if data["signedIn"] == true && enabled { "Enabled · Signed in" }
        else if enabled && row.kind == ResourceKind::Package && self.pi.updates_checked
            && !self.pi.updates.iter().any(|stale| stale == &id) { "Enabled · Global · up to date" }
        else if enabled { "Enabled · Global" } else { "Disabled · Global" }).into();
        ExtensionCard::for_pi_resource(id, name, string(data, "version"), description, status, feature, actions)
    }

    pub(super) fn render_pi_status(&self, cx: &mut Context<Self>) -> AnyElement {
        let catalog = self.source == ExtensionSource::Pi && self.filter != ExtensionFilter::Installed;
        v_flex().px_4().py_2p5().gap_2().border_b_1().border_color(cx.theme().colors().border_variant)
            .when(self.pi.busy || (catalog && self.pi.searching), |body| body.child(Label::new(if self.pi.busy {"Updating resources…"} else {"Searching pi plugins…"}).size(LabelSize::Small).color(Color::Muted)))
            .when(catalog && self.pi.search_error.is_some(), |body| body.child(Label::new(self.pi.search_error.clone().unwrap_or_default()).size(LabelSize::Small).color(Color::Error)))
            .when(!self.pi.notice.is_empty(), |body| body.child(Label::new(self.pi.notice.clone()).size(LabelSize::Small).color(if self.pi.failed {Color::Error} else {Color::Muted})))
            .children(array(&self.pi.data, "pending").iter().map(|item| {
                let source = string(item, "source");
                h_flex().gap_2().child(Label::new(format!("{} · {source} · {}", string(item, "operation"), if item["error"].is_string() {"Failed"} else {"Pending restart"})).size(LabelSize::Small).truncate())
                    .when(item["error"].is_string(), |row| row.child(self.pi_action(format!("retry-{source}"), "Retry", json!({"operation":"retry"}), cx)))
                    .child(self.pi_action(format!("cancel-{source}"), "Cancel", json!({"operation":"cancel-pending","source":source}), cx))
            }))
            .when(self.source == ExtensionSource::Pi && self.pi.installing, |body| body.child(
                h_flex().gap_2().child(div().flex_1().min_w_0().p_2().border_1().border_color(cx.theme().colors().border).rounded_md()
                    .child(self.render_text_input(&self.pi.package_input, cx)))
                    .child(Button::new("confirm-pi-install", "Install").disabled(self.pi.busy).on_click(cx.listener(|this, _, window, cx| {
                        this.request_pi(json!({"operation":"install","source":this.pi.package_input.read(cx).text(cx)}), window, cx);
                    })))
                    .child(Button::new("cancel-pi-install", "Cancel").on_click(cx.listener(|this, _, _, cx| {this.pi.installing = false; cx.notify();})))))
            .into_any_element()
    }

    pub(super) fn render_pi_empty(&self, _cx: &mut Context<Self>) -> AnyElement {
        let catalog = self.source == ExtensionSource::Pi && self.filter != ExtensionFilter::Installed;
        let message = if self.pi.busy || (catalog && self.pi.searching) { "Loading resources…" }
            else if !self.pi.loaded || (catalog && self.pi.search_error.is_some()) { "Unable to load resources. Use Refresh to retry." }
            else { match self.source {
                ExtensionSource::Pi => match self.filter {
                    ExtensionFilter::Installed => "No installed pi plugins match this search.",
                    ExtensionFilter::NotInstalled => "No available pi plugins match this search.",
                    ExtensionFilter::All => "No pi plugins match this search.",
                },
                ExtensionSource::Editor => "No extensions.",
            }};
        v_flex().flex_1().justify_center().items_center().gap_2()
            .child(Icon::new(IconName::Blocks).size(IconSize::Medium).color(Color::Muted))
            .child(Label::new(message).size(LabelSize::Small).color(Color::Muted)).into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_zepi_pi_catalog_merges_installed_without_duplicate_actions() {
        let data = json!({"packages":[{"source":"npm:installed@1.0.0", "name":"installed", "enabled":false}],
            "extensions":[{"name":"owned", "metadata":{"origin":"package"}}, {"name":"local", "metadata":{"origin":"auto"}}]});
        let results = vec![json!({"source":"npm:installed", "name":"installed"}), json!({"source":"npm:available", "name":"available"})];
        let all = resource_rows(&data, &results, ExtensionSource::Pi, ExtensionFilter::All, "");
        assert_eq!(all.len(), 3);
        assert!(all[0].configured);
        assert_eq!(all[1].kind, ResourceKind::Extension);
        assert!(!all[2].configured);
        assert_eq!(resource_rows(&data, &results, ExtensionSource::Pi, ExtensionFilter::Installed, "installed").len(), 1);
        let available = resource_rows(&data, &results, ExtensionSource::Pi, ExtensionFilter::NotInstalled, "");
        assert_eq!(available.len(), 1);
        assert_eq!(available[0].data["name"], "available");
    }

    #[test]
    fn test_zepi_editor_extensions_keep_their_provides() {
        assert!(!editor_extension_provides(ExtensionProvides::ContextServers));
        assert!(!editor_extension_provides(ExtensionProvides::AgentServers));
        assert!(editor_extension_provides(ExtensionProvides::Languages));
    }

}
