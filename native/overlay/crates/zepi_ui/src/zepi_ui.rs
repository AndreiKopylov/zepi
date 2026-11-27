use std::{path::PathBuf, sync::Arc};

use agent_ui::{AgentPanel, ConversationView};
use gpui::{App, Context, Entity, KeyBinding, RenderImage, Window, actions};
use project_panel::ProjectPanel;
use util::ResultExt as _;
use workspace::{MultiWorkspace, Toast, Workspace, notifications::NotificationId};

mod sheet_formula;
pub mod sheet_raster;
pub mod sheet_viewer;
pub mod slides_viewer;
pub mod html_doc;
mod word_viewer;

actions!(zepi, [FocusComposer, Submit]);

pub(crate) enum OfficeContent {
    Text(String),
    Selection {
        path: PathBuf,
        location: String,
        text: String,
    },
    Image {
        path: PathBuf,
        location: String,
        image: gpui::Image,
    },
}

fn window_workspace(window: &Window, cx: &App) -> Option<Entity<Workspace>> {
    window.root::<Workspace>().flatten().or_else(|| {
        window
            .root::<MultiWorkspace>()
            .flatten()
            .map(|multi| multi.read(cx).workspace().clone())
    })
}

fn active_conversation(workspace: &Workspace, cx: &App) -> Option<Entity<ConversationView>> {
    let panel = workspace.panel::<AgentPanel>(cx)?;
    let panel = panel.read(cx);
    panel
        .visible_conversation_view()
        .or_else(|| panel.active_conversation_view())
        .cloned()
}

pub(crate) fn send_office_to_agent(content: OfficeContent, window: &mut Window, cx: &mut App) -> bool {
    let thread = window_workspace(window, cx)
        .and_then(|workspace| active_conversation(workspace.read(cx), cx))
        .and_then(|conversation| conversation.read(cx).active_thread().cloned());
    let Some(thread) = thread else {
        log::error!("office viewer send: no active agent thread");
        return false;
    };
    let editor = thread.read(cx).message_editor.clone();
    editor.update(cx, |editor, cx| match content {
        OfficeContent::Text(text) => {
            editor.zepi_append_text(&text, window, cx);
            true
        }
        OfficeContent::Selection { path, location, text } => {
            editor.zepi_append_office_text(path, location, text, window, cx)
        }
        OfficeContent::Image { path, location, image } => {
            let file = path.file_name().unwrap_or(path.as_os_str()).to_string_lossy();
            let name = format!("{file} · {location}");
            editor.zepi_append_image(image, name.into(), None, window, cx)
        }
    })
}

pub fn attach_image_region_to_agent(
    path: PathBuf,
    rect: [u32; 4],
    image: Arc<RenderImage>,
    window: &mut Window,
    cx: &mut App,
) -> bool {
    let Some(workspace) = window_workspace(window, cx) else {
        return false;
    };
    let Some(thread) = active_conversation(workspace.read(cx), cx)
        .and_then(|conversation| conversation.read(cx).active_thread().cloned())
    else {
        show_region_error(&workspace, "Open an agent thread to attach the image region.", cx);
        return false;
    };
    let editor = thread.read(cx).message_editor.downgrade();
    let workspace = workspace.downgrade();
    let crop = cx.background_executor().spawn(async move { crop_rendered_region(&image, rect) });
    window
        .spawn(cx, async move |cx| {
            let cropped = crop.await;
            cx.update(|window, cx| {
                let Some(editor) = editor.upgrade() else { return };
                let Some(image) = cropped else {
                    if let Some(workspace) = workspace.upgrade() {
                        show_region_error(&workspace, "Could not crop the selected image region.", cx);
                    }
                    return;
                };
                let [left, top, right, bottom] = rect;
                let coords = format!("({left}, {top}, {right}, {bottom})");
                let file = path.file_name().unwrap_or(path.as_os_str()).to_string_lossy();
                let name = format!("{file} · {coords}");
                let source = format!(
                    "[Image source: {} | pixel box {coords} (left, top, right, bottom; right/bottom exclusive; after EXIF orientation)]",
                    path.display(),
                );
                editor.update(cx, |editor, cx| {
                    editor.zepi_append_image(image, name.into(), Some(&source), window, cx);
                });
            })
            .log_err();
        })
        .detach();
    true
}

fn show_region_error(workspace: &Entity<Workspace>, message: &str, cx: &mut App) {
    struct ImageRegionError;
    workspace.update(cx, |workspace, cx| {
        workspace.show_toast(
            Toast::new(NotificationId::unique::<ImageRegionError>(), message.to_owned()),
            cx,
        );
    });
}

fn crop_rendered_region(
    image: &RenderImage,
    [left, top, right, bottom]: [u32; 4],
) -> Option<gpui::Image> {
    let size = image.size(0).map(u32::from);
    if left >= right || top >= bottom || right > size.width || bottom > size.height {
        return None;
    }
    let bytes = image.as_bytes(0)?;
    let mut rgba = Vec::with_capacity((right - left) as usize * (bottom - top) as usize * 4);
    for y in top..bottom {
        let row = y as usize * size.width as usize * 4;
        rgba.extend_from_slice(bytes.get(row + left as usize * 4..row + right as usize * 4)?);
    }
    for pixel in rgba.chunks_exact_mut(4) {
        pixel.swap(0, 2);
    }
    let cropped = image::RgbaImage::from_raw(right - left, bottom - top, rgba)?;
    let mut png = std::io::Cursor::new(Vec::new());
    cropped.write_to(&mut png, image::ImageFormat::Png).ok()?;
    Some(gpui::Image::from_bytes(gpui::ImageFormat::Png, png.into_inner()))
}

pub(crate) fn crop_office_region(
    bytes: &[u8],
    (x0, y0, x1, y1): (f32, f32, f32, f32),
    (page_width, page_height): (f64, f64),
) -> Option<gpui::Image> {
    if page_width <= 0.0 || page_height <= 0.0 {
        return None;
    }
    let image = image::load_from_memory(bytes).ok()?;
    let pixel = |coord, extent, limit: u32| {
        ((f64::from(coord) / extent * f64::from(limit)) as u32).min(limit)
    };
    let left = pixel(x0, page_width, image.width());
    let top = pixel(y0, page_height, image.height());
    let width = pixel(x1, page_width, image.width()).saturating_sub(left);
    let height = pixel(y1, page_height, image.height()).saturating_sub(top);
    if width == 0 || height == 0 {
        return None;
    }
    let mut png = std::io::Cursor::new(Vec::new());
    image
        .crop_imm(left, top, width, height)
        .write_to(&mut png, image::ImageFormat::Png)
        .ok()?;
    Some(gpui::Image::from_bytes(gpui::ImageFormat::Png, png.into_inner()))
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
    use super::{crop_office_region, crop_rendered_region, office_page_indices};

    #[test]
    fn image_region_crops_only_requested_pixels_in_rgba_order() {
        let pixels = image::RgbaImage::from_fn(4, 3, |x, y| image::Rgba([7, y as u8, x as u8, 123]));
        let render = gpui::RenderImage::new([image::Frame::new(pixels)]);
        let crop = crop_rendered_region(&render, [1, 1, 3, 3]).unwrap();
        let crop = image::load_from_memory(crop.bytes()).unwrap().to_rgba8();
        assert_eq!(crop.dimensions(), (2, 2));
        assert_eq!(crop.get_pixel(0, 0).0, [1, 1, 7, 123]);
        assert_eq!(crop.get_pixel(1, 1).0, [2, 2, 7, 123]);
        assert!(crop_rendered_region(&render, [2, 0, 5, 1]).is_none());
    }

    #[test]
    fn office_image_clips_to_the_selection_and_uses_a_content_id() {
        let image = image::RgbImage::from_fn(3, 2, |x, _| image::Rgb([x as u8, 0, 0]));
        let mut png = std::io::Cursor::new(Vec::new());
        image.write_to(&mut png, image::ImageFormat::Png).unwrap();
        let png = png.into_inner();

        let page = (1.5, 1.);
        let left = crop_office_region(&png, (-0.5, 0., 0.5, 0.5), page).unwrap();
        let right = crop_office_region(&png, (0.5, 0., 1.5, 1.), page).unwrap();
        let crop = image::load_from_memory(&left.bytes).unwrap().to_rgb8();
        assert_eq!(crop.dimensions(), (1, 1));
        assert_eq!(crop.get_pixel(0, 0), &image::Rgb([0, 0, 0]));
        let crop = image::load_from_memory(&right.bytes).unwrap().to_rgb8();
        assert_eq!(crop.dimensions(), (2, 2));
        assert_eq!(crop.get_pixel(1, 1), &image::Rgb([2, 0, 0]));
        assert_ne!(left.id, right.id);
        assert!(crop_office_region(&png, (-2., 0., -1., 1.), page).is_none());
    }

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
