use agent_ui::AgentPanel;
use gpui::{App, Context, KeyBinding, Window, actions};
use project_panel::ProjectPanel;
use workspace::{MultiWorkspace, Workspace};

mod sheet_formula;
pub mod sheet_raster;
pub mod sheet_viewer;
pub mod slides_viewer;
pub mod html_doc;
mod word_viewer;

actions!(zepi, [FocusComposer, Submit]);

/// `%LOCALAPPDATA%\Zepi\temp`: slide and page images handed to the agent.
pub(crate) fn temp_dir() -> std::path::PathBuf {
    match std::env::var_os("LOCALAPPDATA") {
        Some(dir) => std::path::PathBuf::from(dir).join("Zepi").join("temp"),
        None => std::env::temp_dir().join("zepi-temp"),
    }
}

/// Cheap identity of a file on disk: modification time in nanoseconds plus size.
/// The viewers poll it because worktree watch events are missed when a writer
/// replaces the file (python-pptx and Office save by rename-over).
pub(crate) fn disk_stamp(path: &std::path::Path) -> Option<(u64, u64)> {
    let metadata = std::fs::metadata(path).ok()?;
    let modified = metadata
        .modified()
        .ok()?
        .duration_since(std::time::UNIX_EPOCH)
        .ok()?
        .as_nanos() as u64;
    Some((modified, metadata.len()))
}

/// Validates a 1-based `--pages` selection and returns indices in the requested order.
pub fn office_page_indices(spec: Option<&str>, count: usize) -> anyhow::Result<Vec<usize>> {
    use anyhow::{Context as _, ensure};

    ensure!(count > 0, "document has no pages or sheets to render");
    let Some(spec) = spec else {
        return Ok((0..count).collect());
    };
    let mut pages = Vec::new();
    for part in spec.split(',').map(str::trim).filter(|part| !part.is_empty()) {
        let invalid = || format!("invalid --pages selection {part:?}; valid numbers are 1..={count}");
        let (first, last) = part.split_once('-').unwrap_or((part, part));
        let first: usize = first.trim().parse().with_context(invalid)?;
        let last: usize = last.trim().parse().with_context(invalid)?;
        ensure!(first > 0 && first <= last && last <= count, "{}", invalid());
        pages.extend(first - 1..last);
    }
    ensure!(!pages.is_empty(), "--pages got no page numbers");
    Ok(pages)
}

pub fn init(cx: &mut App) {
    sheet_viewer::init(cx);
    slides_viewer::init(cx);
    word_viewer::init(cx);
    if std::env::var_os("ZEPI_ROOT").is_some() {
        bind_keys(cx);
    }
}

pub fn bind_keys(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("cmd-shift-j", FocusComposer, None),
        KeyBinding::new("cmd-enter", Submit, Some("AcpThread > Editor")),
    ]);
}

pub fn attach(workspace: &mut Workspace, window: &mut Window, cx: &mut Context<Workspace>) {
    if std::env::var_os("ZEPI_ROOT").is_none() || workspace.zepi_attached() {
        return;
    }
    workspace.set_zepi_attached(cx);
    register_actions(workspace);
    if let Some(panel) = workspace.panel::<AgentPanel>(cx) {
        panel.update(cx, |panel, cx| panel.zepi_initialize(window, cx));
    }
}

/// Opens the agent dock, the project dock and the threads sidebar once per installation.
/// Afterwards the persisted dock state is the only source of truth, so a dock the user
/// closed stays closed.
pub fn apply_default_layout(
    workspace: &mut Workspace,
    window: &mut Window,
    cx: &mut Context<Workspace>,
) {
    if std::env::var_os("ZEPI_ROOT").is_none() {
        return;
    }
    let marker = paths::state_dir().join("zepi-layout-initialized");
    if marker.exists() {
        return;
    }
    workspace.open_panel::<AgentPanel>(window, cx);
    workspace.open_panel::<ProjectPanel>(window, cx);
    if let Some(multi_workspace) = window.root::<MultiWorkspace>().flatten() {
        if !multi_workspace.read(cx).sidebar_open() {
            // sidebar serialization reads the workspace, so it cannot run inside its own update
            cx.defer(move |cx| {
                multi_workspace.update(cx, |multi_workspace, cx| multi_workspace.open_sidebar(cx));
            });
        }
    }
    if let Some(parent) = marker.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Err(error) = std::fs::write(&marker, b"1") {
        log::warn!(target: "zepi", "failed to persist layout marker: {error}");
    }
}

fn register_actions(workspace: &mut Workspace) {
    workspace.register_action(|workspace, _: &FocusComposer, window, cx| {
        if let Some(panel) = workspace.panel::<AgentPanel>(cx) {
            panel.update(cx, |panel, cx| panel.zepi_focus_composer(window, cx));
        }
    });
    workspace.register_action(|workspace, _: &Submit, _, cx| {
        if let Some(thread) = workspace
            .panel::<AgentPanel>(cx)
            .and_then(|panel| panel.read(cx).active_thread_view(cx))
        {
            thread
                .read(cx)
                .message_editor
                .clone()
                .update(cx, |editor, cx| editor.send(cx));
        }
    });
}

#[cfg(test)]
mod tests {
    use super::office_page_indices;

    #[test]
    fn office_pages_validate_all_indices_before_rendering() {
        assert_eq!(office_page_indices(None, 3).unwrap(), [0, 1, 2]);
        assert_eq!(office_page_indices(Some("3,1-2"), 3).unwrap(), [2, 0, 1]);
        for invalid in ["0", "1,4", "1-4", "3-2", "nope"] {
            let error = office_page_indices(Some(invalid), 3).unwrap_err();
            assert!(error.to_string().contains("1..=3"));
        }
        let huge = format!("1-{}", usize::MAX);
        assert!(office_page_indices(Some(&huge), 3).is_err());
        assert!(office_page_indices(Some(""), 3).is_err());
        assert!(office_page_indices(None, 0).is_err());
    }
}
