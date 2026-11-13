//! Docx preview: continuous page tape rasterized by wordcraft (vello_cpu), ctrl+wheel zoom,
//! drag pan on the backdrop, region selection with copy/send, editor-like autohide scrollbar.
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::Result;
use deckcraft_geom::{Affine as DAffine, Point as DPoint};
use crate::slides_viewer::{
    office_knob_f, scan_char_matches, ClearSelection, CopyRegionImage, PageNext, PagePrev, CopySelection, SearchNext, SearchPrev,
    SendRegionImage, SendSlideText, ToggleSearch,
};
use crate::slides_viewer::office_knob;
use wordcraft_doc::Document;
use wordcraft_layout::{DocLayout, LayoutCache, LayoutOptions, Placed, ViewMode};
use wordcraft_render::{RenderOptions, render_page};
use editor::{Editor, EditorEvent};
use gpui::prelude::*;
use gpui::{EntityId, 
    App, AppContext, ClipboardItem, Context, Entity, FocusHandle, ImageFormat,
    Focusable, IntoElement, InteractiveElement, MouseButton, ParentElement, Pixels,
    Point, SharedString,
    StatefulInteractiveElement, Styled, Task, Window, div, img, px,
};
use gpui::{ImageSource, ScrollHandle};
use project::{Project, ProjectEntryId, ProjectPath};
use theme::ActiveTheme;
use ui::{
    ButtonCommon, Clickable, Color, Icon, IconButton, IconName, IconSize, Label,
    LabelCommon, LabelSize, Tooltip, VisibleOnHover,
};
use workspace::item::{ItemBufferKind, Item, ProjectItem, TabContentParams};
use workspace::{Pane, Workspace, register_project_item, register_serializable_item};


const BASE_SUPERSAMPLE: f64 = 2.0;
fn slide_gap() -> f32 {
    office_knob_f("slideGap", 14.0) as f32
}
const TAPE_PAD: f32 = 24.0;
const CACHE_CAP: usize = 12;
const ZOOM_MIN: f64 = 0.25;
const ZOOM_MAX: f64 = 4.0;

pub fn init(cx: &mut App) {
    wordcraft_fonts::FontDb::global().load_system_fonts();
    register_project_item::<WordView>(cx);
    register_serializable_item::<WordView>(cx);
    cx.bind_keys([
        gpui::KeyBinding::new("ctrl-c", CopySelection, Some("OfficeView")),
        gpui::KeyBinding::new("escape", ClearSelection, Some("OfficeView")),
        gpui::KeyBinding::new("ctrl-f", ToggleSearch, Some("OfficeView")),
        gpui::KeyBinding::new("enter", SearchNext, Some("OfficeSearch")),
        gpui::KeyBinding::new("shift-enter", SearchPrev, Some("OfficeSearch")),
        gpui::KeyBinding::new("escape", ToggleSearch, Some("OfficeSearch")),
    ]);
}

fn raster_for(zoom: f64) -> f64 {
    (((zoom * BASE_SUPERSAMPLE) * 2.0).round() / 2.0).clamp(0.5, 4.0)
}

fn raster_key(zoom: f64, scale: f32) -> u32 {
    (raster_for(zoom) * scale.max(1.0) as f64 * 2.0).round() as u32
}

#[derive(Clone)]
struct LineGeom {
    start: usize,
    end: usize,
    top: f64,
    bottom: f64,
    caret_x: Vec<f64>,
}

#[derive(Clone)]
struct ParaGeom {
    text: String,
    lines: Vec<LineGeom>,
}

#[derive(Clone)]
struct TextBlock {
    x0: f32,
    y0: f32,
    x1: f32,
    y1: f32,
    text: String,
    paras: Vec<ParaGeom>,
    xform: DAffine,
}

#[derive(Clone, Copy)]
struct Hit {
    slide: usize,
    x0: f32,
    y0: f32,
    x1: f32,
    y1: f32,
}

fn page_blocks(doc: &Document, layout: &DocLayout, ix: usize) -> Vec<TextBlock> {
    let Some(page) = layout.pages.get(ix) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for item in &page.items {
        let Placed::Lines {
            story,
            path,
            para,
            l0,
            l1,
            x,
            y,
            ..
        } = item
        else {
            continue;
        };
        let Some(paragraph) = doc.para(*story, path) else {
            continue;
        };
        let text = paragraph.text.clone();
        if text.trim().is_empty() {
            continue;
        }
        let mut lines = Vec::new();
        let mut bx0 = f64::MAX;
        let mut bx1 = f64::MIN;
        for li in (*l0)..(*l1).min(para.lines.len()) {
            let line = &para.lines[li];
            let c0 = line.c0.min(para.clusters.len());
            let c1 = line.c1.min(para.clusters.len());
            if c1 <= c0 || line.xs.is_empty() {
                continue;
            }
            let b_start = para.clusters[c0].start.min(text.len());
            let b_end = para.clusters[c1 - 1].end.min(text.len());
            let start = text[..b_start].chars().count();
            let end = text[..b_end].chars().count();
            let caret = line.xs.iter().map(|xs| (x + xs) as f64).collect::<Vec<_>>();
            bx0 = bx0.min(caret.first().copied().unwrap_or(0.0));
            bx1 = bx1.max(caret.last().copied().unwrap_or(0.0));
            lines.push(LineGeom {
                start,
                end,
                top: (y + line.top) as f64,
                bottom: (y + line.top + line.height) as f64,
                caret_x: caret,
            });
        }
        if lines.is_empty() {
            continue;
        }
        let by0 = lines.first().map(|line| line.top).unwrap_or(0.0);
        let by1 = lines.last().map(|line| line.bottom).unwrap_or(0.0);
        out.push(TextBlock {
            x0: bx0 as f32,
            y0: by0 as f32,
            x1: bx1 as f32,
            y1: by1 as f32,
            text: text.clone(),
            paras: vec![ParaGeom { text, lines }],
            xform: DAffine::IDENTITY,
        });
    }
    out
}

pub struct WordItem {
    path: ProjectPath,
    entry_id: Option<ProjectEntryId>,
    abs_path: PathBuf,
    pub doc: Option<Arc<Document>>,
    pub layout: Option<Arc<DocLayout>>,
    pub import_error: Option<String>,
}

impl project::ProjectItem for WordItem {
    fn try_open(
        project: &Entity<Project>,
        path: &ProjectPath,
        cx: &mut App,
    ) -> Option<Task<Result<Entity<Self>>>> {
        let ext = path
            .path
            .extension()
            .map(|ext| ext.to_ascii_lowercase())
            .or_else(|| {
                project
                    .read(cx)
                    .absolute_path(path, cx)
                    .and_then(|abs| abs.extension().map(|ext| ext.to_string_lossy().to_ascii_lowercase()))
            })?;
        if ext != "docx" && ext != "doc" {
            return None;
        }
        let abs_path = project.read(cx).absolute_path(path, cx)?;
        let entry_id = project.read(cx).entry_for_path(path, cx).map(|entry| entry.id);
        let path = path.clone();
        let abs_for_err = abs_path.clone();
        Some(cx.spawn(async move |cx| {
            let loaded = cx
                .background_executor()
                .spawn(async move {
                    let bytes = std::fs::read(&abs_path)?;
                    let (doc, layout) = import_doc(&bytes)?;
                    anyhow::Ok((doc, layout, abs_path))
                })
                .await;
            Ok(match loaded {
                Ok((doc, layout, abs_path)) => cx.new(|_| WordItem {
                    path,
                    entry_id,
                    abs_path,
                    doc: Some(doc),
                    layout: Some(layout),
                    import_error: None,
                }),
                Err(error) => cx.new(|_| WordItem {
                    path,
                    entry_id,
                    abs_path: abs_for_err,
                    doc: None,
                    layout: None,
                    import_error: Some(format!("{error:#}")),
                }),
            })
        }))
    }

    fn entry_id(&self, _: &App) -> Option<ProjectEntryId> {
        self.entry_id
    }

    fn project_path(&self, _: &App) -> Option<ProjectPath> {
        Some(self.path.clone())
    }

    fn is_dirty(&self) -> bool {
        false
    }
}

#[derive(Clone, Copy)]
struct PanDrag {
    start_pos: Point<Pixels>,
    start_offset: Point<Pixels>,
}

#[derive(Clone, Copy)]
struct ThumbDrag {
    start_y: Pixels,
    start_top: f32,
    track: f32,
    max: f32,
}

#[derive(Clone, Copy)]
struct Region {
    slide: usize,
    x0: f32,
    y0: f32,
    x1: f32,
    y1: f32,
    live: bool,
}

impl Region {
    fn norm(&self) -> (f32, f32, f32, f32) {
        (
            self.x0.min(self.x1),
            self.y0.min(self.y1),
            self.x0.max(self.x1),
            self.y0.max(self.y1),
        )
    }
}

pub struct WordView {
    item: Entity<WordItem>,
    focus_handle: FocusHandle,
    zoom: f64,
    cache: HashMap<(usize, u32), Arc<gpui::Image>>,
    order: Vec<(usize, u32)>,
    pending: Vec<(usize, u32)>,
    blocks: HashMap<usize, Arc<Vec<TextBlock>>>,
    region: Option<Region>,
    pan: Option<PanDrag>,
    thumb_drag: Option<ThumbDrag>,
    scroll_reveal_until: Option<Instant>,
    scroll_timer_armed: bool,
    last_offset: Point<Pixels>,
    search_open: bool,
    search_editor: Option<Entity<Editor>>,
    page_editor: Option<Entity<Editor>>,
    page_editing: bool,
    focus_page_on_render: bool,
    focus_search_on_render: bool,
    focus_on_first_render: bool,
    search_query: String,
    hits: Vec<Hit>,
    hit_ix: usize,
    scroll: ScrollHandle,
    _watch_task: Option<Task<()>>,
    reload_again: bool,
    // previous generation of rasters, shown until the new one is ready (no blink)
    stale: HashMap<(usize, u32), Arc<gpui::Image>>,
    // bumped on every disk reload; keeps gpui image ids unique so the
    // texture cache cannot serve stale pixels after the file changed
    raster_gen: u32,
    image_tag: u32,
    // mtime+size of the bytes currently shown; the poll compares against it
    loaded_stamp: Option<(u64, u64)>,
    scale: f32,
}

fn arm_timer(
    view: &Entity<WordView>,
    cx: &mut App,
    which: fn(&mut WordView) -> &mut Option<Instant>,
    armed: fn(&mut WordView) -> &mut bool,
    millis: u64,
) {
    let deadline = Instant::now() + Duration::from_millis(millis);
    let spawn = view
        .update(cx, |this, cx| {
            *which(this) = Some(deadline);
            cx.notify();
            !*armed(this)
        })
        ;
    if !spawn {
        return;
    }
    let _ = view.update(cx, |this, _| *armed(this) = true);
    let weak = view.downgrade();
    cx.spawn(async move |cx| {
        cx.background_executor()
            .timer(Duration::from_millis(millis + 100))
            .await;
        let _ = weak.update(cx, |this, cx| {
            *armed(this) = false;
            let expired = which(this).is_some_and(|deadline| Instant::now() >= deadline);
            if expired {
                *which(this) = None;
                cx.notify();
            }
        });
    })
    .detach();
}

impl WordView {
    pub fn new(
        item: Entity<WordItem>,
        project: Entity<Project>,
        cx: &mut Context<Self>,
    ) -> Self {
        let loaded_stamp = crate::disk_stamp(&item.read(cx).abs_path);
        let mut this = Self {
            item,
            focus_handle: cx.focus_handle(),
            zoom: 1.0,
            cache: HashMap::default(),
            order: Vec::new(),
            pending: Vec::new(),
            blocks: HashMap::default(),
            region: None,
            pan: None,
            thumb_drag: None,
            scroll_reveal_until: None,
            scroll_timer_armed: false,
            last_offset: gpui::point(px(0.), px(0.)),
            search_open: false,
            search_editor: None,
            page_editor: None,
            page_editing: false,
            focus_page_on_render: false,
            focus_search_on_render: false,
            focus_on_first_render: true,
            search_query: String::new(),
            hits: Vec::new(),
            hit_ix: 0,
            scroll: ScrollHandle::default(),
            _watch_task: None,
            reload_again: false,
            stale: HashMap::default(),
            raster_gen: 0,
            image_tag: next_image_tag(),
            loaded_stamp,
            scale: 1.0,
        };
        this.ensure(0, cx);
        // Worktree events can be missed when a writer replaces the file (Office and
        // python-pptx save by rename-over), so the visible file is polled as well.
        cx.spawn(async move |this, cx| loop {
            cx.background_executor()
                .timer(Duration::from_secs(2))
                .await;
            let Ok((abs, loaded)) =
                this.read_with(cx, |this, cx| (this.item.read(cx).abs_path.clone(), this.loaded_stamp))
            else {
                break;
            };
            let Some(stamp) = crate::disk_stamp(&abs) else { continue };
            if Some(stamp) != loaded {
                this.update(cx, |this, cx| this.schedule_reload(cx)).ok();
            }
        })
        .detach();
        let entry_id = this.item.read(cx).entry_id;
        let watch_path = this.item.read(cx).path.clone();
        cx.subscribe(&project, move |this, _project, event, cx| {
            let project::Event::WorktreeUpdatedEntries(worktree_id, updates) = event else {
                return;
            };
            // entry id changes when the file is replaced (agent rewrites via rename),
            // so match the relative path inside the same worktree as well
            let touched = updates.iter().any(|(rel_path, id, change)| {
                let relevant = matches!(
                    change,
                    project::PathChange::Updated
                        | project::PathChange::Added
                        | project::PathChange::Loaded
                );
                relevant
                    && (Some(*id) == entry_id
                        || (*worktree_id == watch_path.worktree_id
                            && rel_path.as_ref() == watch_path.path.as_ref()))
            });
            if touched {
                this.schedule_reload(cx);
            }
        })
        .detach();
        this
    }

    /// Debounced reload after the pptx changes on disk (worktree fs watch).
    fn schedule_reload(&mut self, cx: &mut Context<Self>) {
        if self._watch_task.is_some() {
            // file changed again mid-reload - queue another pass
            self.reload_again = true;
            return;
        }
        let abs = self.item.read(cx).abs_path.clone();
        let item = self.item.clone();
        self._watch_task = Some(cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(Duration::from_millis(office_knob("reloadDebounceMs", 600)))
                .await;
            // PowerPoint/OneDrive may hold the file briefly after save - retry
            let mut imported = Err(anyhow::anyhow!("not attempted"));
            for attempt in 0..4u32 {
                if attempt > 0 {
                    cx.background_executor()
                        .timer(Duration::from_millis(400))
                        .await;
                }
                let abs_try = abs.clone();
                imported = cx
                    .background_executor()
                    .spawn(async move {
                        let bytes = std::fs::read(&abs_try)?;
                        anyhow::Ok(import_doc(&bytes)?)
                    })
                    .await;
                if imported.is_ok() {
                    break;
                }
                log::warn!(
                    "word_viewer: reload attempt {attempt} failed: {}",
                    imported.as_ref().err().map(|e| e.to_string()).unwrap_or_default()
                );
            }
            let _ = this.update(cx, |this, cx| {
                this._watch_task = None;
                let again = std::mem::take(&mut this.reload_again);
                match imported {
                    Ok((doc, layout)) => {
                        log::info!("word_viewer: document reloaded from disk");
                        item.update(cx, |item, _| {
                            item.doc = Some(doc);
                            item.layout = Some(layout);
                            item.import_error = None;
                        });
                        this.loaded_stamp = crate::disk_stamp(&item.read(cx).abs_path);
                        this.raster_gen = this.raster_gen.wrapping_add(1);
                        this.stale = std::mem::take(&mut this.cache);
                        this.order.clear();
                        this.pending.clear();
                        this.blocks.clear();
                        this.region = None;
                        this.hits.clear();
                        this.hit_ix = 0;
                        this.ensure_visible(cx);
                        cx.notify();
                    }
                    Err(error) => {
                        log::warn!("word_viewer: reload failed: {error:#}");
                    }
                }
                if again {
                    this.schedule_reload(cx);
                }
            });
        }));
    }

    fn slide_count(&self, cx: &App) -> usize {
        self.item.read(cx).layout.as_ref().map_or(0, |layout| layout.pages.len())
    }

    // Sections may be landscape, so every page keeps its own size on the tape.
    fn page_size_px(&self, ix: usize, cx: &App) -> (f32, f32) {
        let item = self.item.read(cx);
        let Some(page) = item.layout.as_ref().and_then(|layout| layout.pages.get(ix)) else {
            return (0.0, 0.0);
        };
        (page.w * self.zoom as f32, page.h * self.zoom as f32)
    }

    fn tape_width_px(&self, cx: &App) -> f32 {
        let item = self.item.read(cx);
        let zoom = self.zoom as f32;
        item.layout
            .as_ref()
            .map_or(0.0, |layout| layout.pages.iter().map(|page| page.w).fold(0.0, f32::max) * zoom)
    }

    // Tape top of page ix: page heights differ per section, so the offsets accumulate.
    fn page_top(&self, ix: usize, cx: &App) -> f32 {
        let item = self.item.read(cx);
        let Some(layout) = item.layout.as_ref() else {
            return TAPE_PAD;
        };
        let zoom = self.zoom as f32;
        layout
            .pages
            .iter()
            .take(ix)
            .fold(TAPE_PAD, |top, page| top + page.h * zoom + slide_gap())
    }

    fn index_at(&self, y: f32, cx: &App) -> usize {
        let count = self.slide_count(cx);
        let mut ix = 0;
        while ix + 1 < count && y >= self.page_top(ix + 1, cx) {
            ix += 1;
        }
        ix
    }

    fn blocks_for(&mut self, ix: usize, cx: &App) -> Arc<Vec<TextBlock>> {
        if let Some(blocks) = self.blocks.get(&ix) {
            return blocks.clone();
        }
        let item = self.item.read(cx);
        let Some((doc, layout)) = item.doc.as_ref().zip(item.layout.as_ref()) else {
            return Arc::new(Vec::new());
        };
        let blocks = Arc::new(page_blocks(doc, layout, ix));
        self.blocks.insert(ix, blocks.clone());
        blocks
    }

    fn slide_origin(&self, ix: usize, cx: &App) -> Point<Pixels> {
        let (slide_w, _) = self.page_size_px(ix, cx);
        let bounds = self.scroll.bounds();
        let offset = self.scroll.offset();
        let tape_w = self.tape_width_px(cx);
        let content_x =
            ((f32::from(bounds.size.width) - tape_w) / 2.0).max(0.0) + (tape_w - slide_w) / 2.0;
        let content_y = self.page_top(ix, cx);
        gpui::point(
            bounds.origin.x + px(content_x) + offset.x,
            bounds.origin.y + px(content_y) + offset.y,
        )
    }

    fn slide_pt(&self, ix: usize, pos: Point<Pixels>, cx: &App) -> (f32, f32) {
        let origin = self.slide_origin(ix, cx);
        (
            f32::from(pos.x - origin.x) / self.zoom as f32,
            f32::from(pos.y - origin.y) / self.zoom as f32,
        )
    }

    fn region_text(&mut self, region: Region, cx: &App) -> String {
        let (x0, y0, x1, y1) = region.norm();
        let blocks = self.blocks_for(region.slide, cx);
        blocks
            .iter()
            .filter(|b| b.x0 < x1 && b.x1 > x0 && b.y0 < y1 && b.y1 > y0)
            .map(|b| b.text.as_str())
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn region_image(&self, region: Region, _cx: &App) -> Option<gpui::Image> {
        let key = raster_key(self.zoom, self.scale);
        let raster = raster_for(self.zoom);
        let image = self.cache.get(&(region.slide, key))?;
        let decoded = image::load_from_memory(&image.bytes).ok()?;
        let (x0, y0, x1, y1) = region.norm();
        let cx0 = (x0 as f64 * raster) as u32;
        let cy0 = (y0 as f64 * raster) as u32;
        let cw = ((x1 - x0) as f64 * raster) as u32;
        let ch = ((y1 - y0) as f64 * raster) as u32;
        let (iw, ih) = (decoded.width(), decoded.height());
        if cx0 >= iw || cy0 >= ih || cw == 0 || ch == 0 {
            return None;
        }
        let cropped = decoded.crop_imm(cx0, cy0, cw.min(iw - cx0), ch.min(ih - cy0));
        let mut bytes = Vec::new();
        cropped
            .write_to(&mut std::io::Cursor::new(&mut bytes), image::ImageFormat::Png)
            .ok()?;
        Some(gpui::Image {
            format: ImageFormat::Png,
            bytes,
            id: 1 << 62
                | 1 << 63
                | (self.image_tag as u64 & 0x3FFF) << 48
                | (self.raster_gen as u64 & 0xFFFF) << 32
                | (region.slide as u64 & 0xFF_FFFF) << 8
                | key as u64 & 0xFF,
        })
    }

    fn send_text_to_agent(&self, text: &str, window: &mut Window, cx: &mut App) {
        let Some(workspace) = window.root::<Workspace>().flatten().or_else(|| {
            window.root::<workspace::MultiWorkspace>().flatten().and_then(|multi| {
                multi.read(cx).workspaces().next().cloned()
            })
        }) else {
            log::error!("slides_viewer send: no workspace root");
            return;
        };
        let Some(panel) = workspace.read(cx).panel::<agent_ui::AgentPanel>(cx) else {
            log::error!("slides_viewer send: no agent panel");
            return;
        };
        let Some(conversation) = panel.read(cx).active_conversation_view().cloned() else {
            log::error!("slides_viewer send: no active conversation");
            return;
        };
        let Some(thread_view) = conversation.read(cx).root_thread_view() else {
            log::error!("slides_viewer send: no root thread view");
            return;
        };
        thread_view.update(cx, |thread_view, cx| {
            thread_view
                .message_editor
                .update(cx, |editor, cx| editor.zepi_append_text(text, window, cx));
        });
        log::info!("slides_viewer send: appended {} chars to composer", text.len());
    }

    fn source_note(&self, slide: usize, region: Option<(f32, f32, f32, f32)>, cx: &App) -> String {
        let path = self.item.read(cx).abs_path.display();
        let total = self.slide_count(cx);
        match region {
            Some((x0, y0, x1, y1)) => format!(
                "\n[source: {path} | slide {}/{} | region ({x0:.0},{y0:.0})-({x1:.0},{y1:.0}) pt]",
                slide + 1,
                total
            ),
            None => format!("\n[source: {path} | slide {}/{}]", slide + 1, total),
        }
    }

    fn set_zoom(&mut self, zoom: f64, cx: &mut Context<Self>) {
        let zoom = zoom.clamp(ZOOM_MIN, ZOOM_MAX);
        if (zoom - self.zoom).abs() < f64::EPSILON {
            return;
        }
        self.zoom = zoom;
        self.region = None;
        cx.notify();
    }

    fn goto_prev(&mut self, cx: &mut Context<Self>) {
        let cur = self.current_index(cx);
        if cur > 0 {
            self.scroll_to_slide(cur - 1, cx);
        }
    }

    fn goto_next(&mut self, cx: &mut Context<Self>) {
        let cur = self.current_index(cx);
        let last = self.slide_count(cx).saturating_sub(1);
        if cur < last {
            self.scroll_to_slide(cur + 1, cx);
        }
    }

    fn current_index(&self, cx: &App) -> usize {
        let count = self.slide_count(cx);
        if count == 0 {
            return 0;
        }
        self.index_at(-f32::from(self.scroll.offset().y), cx)
    }

    fn create_page_editor(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Entity<Editor> {
        let editor = cx.new(|cx| Editor::single_line(window, cx));
        cx.subscribe(&editor, move |this, editor, event: &EditorEvent, cx| {
            match event {
                EditorEvent::BufferEdited => {
                    let text = editor.read(cx).text(cx);
                    if let Ok(n) = text.trim().parse::<usize>() {
                        if n >= 1 {
                            let last = this.slide_count(cx).saturating_sub(1);
                            this.scroll_to_slide((n - 1).min(last), cx);
                        }
                    }
                }
                EditorEvent::Blurred => {
                    this.page_editing = false;
                    cx.notify();
                }
                _ => {}
            }
        })
        .detach();
        editor
    }
    fn scroll_to_slide(&mut self, ix: usize, cx: &mut Context<Self>) {
        let top = self.page_top(ix, cx);
        self.scroll.set_offset(gpui::point(px(0.), -px(top)));
        self.ensure(ix, cx);
        cx.notify();
    }

    fn recompute_matches(&mut self, cx: &mut Context<Self>) {
        let query = self.search_query.trim().to_lowercase();
        self.hits.clear();
        self.hit_ix = 0;
        if query.is_empty() {
            cx.notify();
            return;
        }
        let doc = self.item.read(cx).doc.clone();
        let layout = self.item.read(cx).layout.clone();
        if doc.is_none() || layout.is_none() {
            cx.notify();
            return;
        }
        let view = cx.entity();
        cx.spawn(async move |_this, cx| {
            let hits = cx
                .background_executor()
                .spawn(async move {
                  std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    let (Some(doc), Some(layout)) = (doc, layout) else {
                        return Vec::new();
                    };
                    let mut hits = Vec::new();
                    for slide in 0..layout.pages.len() {
                        for block in page_blocks(&doc, &layout, slide) {
                            for para in &block.paras {
                                for (ms, me) in scan_char_matches(&para.text, &query) {
                                    for line in &para.lines {
                                        let seg_start = ms.max(line.start);
                                        let seg_end = me.min(line.end);
                                        if seg_start >= seg_end {
                                            continue;
                                        }
                                        let caret = |ch: usize| -> f64 {
                                            let idx = (ch - line.start).min(line.caret_x.len().saturating_sub(1));
                                            line.caret_x.get(idx).copied().unwrap_or(0.0)
                                        };
                                        let local = [
                                            DPoint::new(caret(seg_start), line.top),
                                            DPoint::new(caret(seg_end), line.top),
                                            DPoint::new(caret(seg_end), line.bottom),
                                            DPoint::new(caret(seg_start), line.bottom),
                                        ];
                                        let (mut hx0, mut hy0, mut hx1, mut hy1) =
                                            (f64::MAX, f64::MAX, f64::MIN, f64::MIN);
                                        for point in local {
                                            let mapped = block.xform * point;
                                            hx0 = hx0.min(mapped.x);
                                            hy0 = hy0.min(mapped.y);
                                            hx1 = hx1.max(mapped.x);
                                            hy1 = hy1.max(mapped.y);
                                        }
                                        hits.push(Hit {
                                            slide,
                                            x0: hx0 as f32,
                                            y0: hy0 as f32,
                                            x1: hx1 as f32,
                                            y1: hy1 as f32,
                                        });
                                    }
                                }
                            }
                        }
                    }
                    hits
                  }))
                  .unwrap_or_else(|_| {
                    log::error!("word_viewer: match geometry panicked, dropping hits");
                    Vec::new()
                  })
                })
                .await;
            let _ = view.update(cx, |this, cx| {
                this.hits = hits;
                this.hit_ix = 0;
                if !this.hits.is_empty() {
                    let first = this.hits[0].slide;
                    this.scroll_to_slide(first, cx);
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn create_search_editor(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let editor = cx.new(|cx| Editor::single_line(window, cx));
        editor.update(cx, |editor, cx| {
            editor.set_placeholder_text("Search in presentation...", window, cx);
        });
        cx.subscribe_in(&editor, window, |this, _editor, event, _window, cx| {
            if matches!(event, EditorEvent::BufferEdited) {
                this.sync_query(cx);
            }
        })
        .detach();
        self.search_editor = Some(editor);
    }

    fn sync_query(&mut self, cx: &mut Context<Self>) {
        let query = self
            .search_editor
            .as_ref()
            .map(|editor| editor.read(cx).text(cx))
            .unwrap_or_default();
        if query != self.search_query {
            self.search_query = query;
            self.recompute_matches(cx);
        }
    }

    fn send_slide_text(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let region_text = self.region.map(|region| (region.slide, self.region_text(region, cx)));
        let (slide, text) = match region_text {
            // a sliver or empty selection must not swallow the send
            Some((slide, text)) if !text.trim().is_empty() => (slide, text),
            _ => {
                let ix = self
                    .region
                    .map(|region| region.slide)
                    .unwrap_or_else(|| self.center_slide(cx));
                let blocks = self.blocks_for(ix, cx);
                cx.notify();
                (
                    ix,
                    blocks
                        .iter()
                        .map(|block| block.text.as_str())
                        .collect::<Vec<_>>()
                        .join("\n")
                )
            }
        };
        if text.is_empty() {
            return;
        }
        let note = self.source_note(slide, None, cx);
        self.send_text_to_agent(&format!("{text}{note}"), window, cx);
        // the selection served its purpose - drop the outline
        self.region = None;
        cx.notify();
    }

    pub fn deploy_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.search_open = true;
        if self.search_editor.is_none() {
            self.create_search_editor(window, cx);
        }
        self.focus_search_on_render = true;
        cx.notify();
    }

    fn copy_region_image(&mut self, cx: &mut Context<Self>) {
        let Some(region) = self.region else {
            return;
        };
        let Some(image) = self.region_image(region, cx) else {
            return;
        };
        cx.write_to_clipboard(ClipboardItem::new_image(&image));
    }

    fn send_region_image(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(region) = self.region else {
            return;
        };
        let Some(image) = self.region_image(region, cx) else {
            return;
        };
        let dir = crate::temp_dir();
        if std::fs::create_dir_all(&dir).is_err() {
            return;
        }
        let file = dir.join(format!(
            "slide-{}-{}.png",
            region.slide,
            Instant::now().elapsed().as_millis()
        ));
        if std::fs::write(&file, &image.bytes).is_err() {
            return;
        }
        let (x0, y0, x1, y1) = region.norm();
        let note = self.source_note(region.slide, Some((x0, y0, x1, y1)), cx);
        let text = format!("[slide image: {}]{note}", file.display());
        self.send_text_to_agent(&text, window, cx);
        self.region = None;
        cx.notify();
    }

    fn step_match(&mut self, delta: isize, cx: &mut Context<Self>) {
        if self.hits.is_empty() {
            return;
        }
        let len = self.hits.len() as isize;
        self.hit_ix = ((self.hit_ix as isize + delta).rem_euclid(len)) as usize;
        self.scroll_to_slide(self.hits[self.hit_ix].slide, cx);
    }

    fn sel_drag_active(&self) -> bool {
        self.pan.is_some()
            || self.thumb_drag.is_some()
            || self.region.is_some_and(|region| region.live)
    }

    fn center_slide(&self, cx: &App) -> usize {
        let viewport = self.scroll.bounds().size.height;
        let y = -f32::from(self.scroll.offset().y) + f32::from(viewport) / 2.0;
        self.index_at(y, cx)
    }

    fn ensure(&mut self, ix: usize, cx: &mut Context<Self>) {
        let key = raster_key(self.zoom, self.scale);
        let slot = (ix, key);
        if self.cache.contains_key(&slot) || self.pending.contains(&slot) {
            return;
        }
        if ix >= self.slide_count(cx) {
            return;
        }
        self.pending.push(slot);
        let (Some(doc), Some(layout)) = (
            self.item.read(cx).doc.clone(),
            self.item.read(cx).layout.clone(),
        ) else {
            return;
        };
        let raster_gen = self.raster_gen;
        let id_upper = ((self.image_tag as u64 & 0x3FFF) << 48) | ((raster_gen as u64 & 0xFFFF) << 32);
        let view = cx.entity();
        cx.spawn(async move |_this, cx| {
            let rendered = cx
                .background_executor()
                .spawn(async move { rasterize_page(doc, layout, ix, key, id_upper) })
                .await;
            let _ = cx.update(|cx| {
                let _ = view.update(cx, |this, cx| {
                    this.pending.retain(|pending| pending != &slot);
                    if this.raster_gen != raster_gen {
                        return;
                    }
                    if let Err(error) = &rendered {
                        log::error!("slides_viewer rasterize slide {} failed: {error:#}", slot.0 + 1);
                    }
                    if let Ok(image) = rendered {
                        this.cache.insert(slot, Arc::new(image));
                        this.order.push(slot);
                        let cap = office_knob("rasterCacheCap", CACHE_CAP as u64) as usize;
                        while this.order.len() > cap {
                            let oldest = this.order.remove(0);
                            this.cache.remove(&oldest);
                        }
                    }
                    cx.notify();
                });
            });
        })
        .detach();
    }

    fn ensure_visible(&mut self, cx: &mut Context<Self>) {
        let count = self.slide_count(cx);
        if count == 0 {
            return;
        }
        let viewport = self.scroll.bounds().size.height;
        let offset = f32::from(-self.scroll.offset().y);
        let first = self.index_at(offset, cx).saturating_sub(1);
        let bottom = offset + f32::from(viewport) + slide_gap();
        for ix in first..count {
            if self.page_top(ix, cx) > bottom {
                break;
            }
            self.ensure(ix, cx);
        }
    }
}


static RASTER_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
// gpui textures are cached globally by Image id, so every view needs its own tag
static IMAGE_TAG: AtomicU32 = AtomicU32::new(0x51ED_270B);

fn next_image_tag() -> u32 {
    IMAGE_TAG.fetch_add(1, Ordering::Relaxed)
}

fn import_doc(bytes: &[u8]) -> Result<(Arc<Document>, Arc<DocLayout>)> {
    let doc = if bytes.starts_with(&[0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1]) {
        wordcraft_docbin::read_with_password(bytes, None).map_err(|error| anyhow::anyhow!("doc import: {error:?}"))?
    } else if crate::html_doc::looks_like_html(bytes) {
        crate::html_doc::read(bytes)?
    } else {
        wordcraft_docx::read_with_password(bytes, None).map_err(|error| anyhow::anyhow!("docx import: {error:?}"))?
    };
    let mut cache = LayoutCache::new();
    let layout = wordcraft_layout::layout(&doc, &mut cache, &LayoutOptions::default());
    Ok((Arc::new(doc), Arc::new(layout)))
}

fn rasterize_page(
    doc: Arc<Document>,
    layout: Arc<DocLayout>,
    index: usize,
    key: u32,
    id_upper: u64,
) -> Result<gpui::Image> {
    let _guard = RASTER_LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    let rendered = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let page = &layout.pages[index];
        let mut scale = key as f64 / 2.0;
        let max_pixels = office_knob("maxRasterPixels", 16_000_000) as f64;
        let area = page.w as f64 * page.h as f64 * scale * scale;
        if area > max_pixels {
            scale *= (max_pixels / area).sqrt();
        }
        scale = scale.min(16000.0 / page.h as f64).min(16000.0 / page.w as f64);
        render_page(&doc, page, scale as f32, &RenderOptions::default())
    }))
    .map_err(|_| anyhow::anyhow!("wordcraft panicked on page {}", index + 1))?;
    let rgba = image::RgbaImage::from_raw(rendered.width, rendered.height, rendered.pixels)
        .ok_or_else(|| anyhow::anyhow!("bad raster size"))?;
    let mut bytes = Vec::new();
    image::DynamicImage::ImageRgba8(rgba)
        .write_to(&mut std::io::Cursor::new(&mut bytes), image::ImageFormat::Png)?;
    Ok(gpui::Image {
        format: ImageFormat::Png,
        bytes,
        // bit 63 tags word rasters so gpui texture cache cannot serve slide pixels
        id: 1 << 63 | id_upper | (index as u64 & 0xFF_FFFF) << 8 | key as u64 & 0xFF,
    })
}

impl ProjectItem for WordView {
    type Item = WordItem;

    fn for_project_item(
        project: Entity<Project>,
        _pane: Option<&Pane>,
        item: Entity<Self::Item>,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        Self::new(item, project, cx)
    }
}

impl gpui::EventEmitter<()> for WordView {}

impl gpui::Focusable for WordView {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Item for WordView {
    type Event = ();
    fn buffer_kind(&self, _cx: &App) -> ItemBufferKind {
        ItemBufferKind::Singleton
    }

    fn for_each_project_item(&self, cx: &App, f: &mut dyn FnMut(EntityId, &dyn project::ProjectItem)) {
        f(self.item.entity_id(), self.item.read(cx));
    }


    fn tab_content_text(&self, _detail: usize, cx: &App) -> SharedString {
        self.item
            .read(cx)
            .abs_path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| "Document".to_string())
            .into()
    }

    fn tab_content(&self, params: TabContentParams, _window: &Window, cx: &App) -> gpui::AnyElement {
        Label::new(self.tab_content_text(params.detail.unwrap_or_default(), cx))
            .single_line()
            .color(params.text_color())
            .into_any_element()
    }
}

impl workspace::item::SerializableItem for WordView {
    fn serialized_item_kind() -> &'static str {
        "WordView"
    }

    fn cleanup(
        _workspace_id: workspace::WorkspaceId,
        _alive_items: Vec<workspace::ItemId>,
        _window: &mut Window,
        _cx: &mut App,
    ) -> Task<anyhow::Result<()>> {
        Task::ready(Ok(()))
    }

    fn deserialize(
        project: Entity<Project>,
        _workspace: gpui::WeakEntity<Workspace>,
        workspace_id: workspace::WorkspaceId,
        item_id: workspace::ItemId,
        window: &mut Window,
        cx: &mut App,
    ) -> Task<anyhow::Result<Entity<Self>>> {
        let db = WordViewerDb::global(cx);
        window.spawn(cx, async move |cx| {
            let path = match db.get_doc_path(item_id, workspace_id)? {
                Some(path) => path,
                None => anyhow::bail!("no doc path in db"),
            };
            let project_path =
                project.update(cx, |project, cx| project.find_project_path(path, cx));
            let Some(project_path) = project_path else {
                anyhow::bail!("slide path not in project");
            };
            let item_task = cx.update(|_window, cx| {
                use project::ProjectItem as _;
                WordItem::try_open(&project, &project_path, cx)
            })?;
            let Some(item_task) = item_task else {
                anyhow::bail!("slides try_open returned none");
            };
            let item = item_task.await?;
            Ok(cx.update(|_window, cx| cx.new(|cx| WordView::new(item, project, cx)))?)
        })
    }

    fn serialize(
        &mut self,
        workspace: &mut Workspace,
        item_id: workspace::ItemId,
        _closing: bool,
        cx: &mut Context<Self>,
    ) -> Option<Task<anyhow::Result<()>>> {
        let workspace_id = workspace.database_id()?;
        let path = self.item.read(cx).abs_path.clone();
        let db = WordViewerDb::global(cx);
        Some(cx.background_spawn(async move {
            db.save_doc_path(item_id, workspace_id, path).await
        }))
    }

    fn should_serialize(&self, _event: &()) -> bool {
        true
    }
}

impl gpui::Render for WordView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.scale = window.scale_factor();
        if let Some(error) = self.item.read(cx).import_error.clone() {
            return div()
                .size_full()
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .gap_2()
                .bg(cx.theme().colors().panel_background)
                .child(
                    Icon::new(IconName::FileGeneric)
                        .size(IconSize::XLarge)
                        .color(Color::Muted),
                )
                .child(Label::new("Cannot open this file as a Word document").size(LabelSize::Large))
                .child(
                    div()
                        .max_w(px(560.))
                        .child(Label::new(error).size(LabelSize::Small).color(Color::Muted)),
                )
                .child(
                    Label::new("The file may be corrupted, password-protected, or not a real .docx")
                        .size(LabelSize::Small)
                        .color(Color::Muted),
                );
        }
        self.ensure_visible(cx);
        let offset = self.scroll.offset();
        if offset != self.last_offset {
            self.last_offset = offset;
            // entity update during paint corrupts gpui state; defer past the frame
            let view = cx.entity();
            cx.defer(move |cx| {
                arm_timer(
                    &view,
                    cx,
                    |this| &mut this.scroll_reveal_until,
                    |this| &mut this.scroll_timer_armed,
                    1200,
                )
            });
        }
        let count = self.slide_count(cx);
        let tape_w = self.tape_width_px(cx);
        let zoom = self.zoom as f32;
        let border = cx.theme().colors().border;
        let mut selection_bg = cx.theme().players().local().selection;
        // Themes may ship an opaque player selection; keep the overlay see-through.
        selection_bg.a = selection_bg.a.min(0.25);
        let mut region_bubble: Option<gpui::Div> = None;
        let mut tape = ui::v_flex()
            .w(px(tape_w))
            // explicit centering: auto margins overflow differently across taffy versions
            .ml(px(((f32::from(self.scroll.bounds().size.width) - tape_w) / 2.0).max(0.0)))
            .gap(px(slide_gap()))
            .py(px(TAPE_PAD));
        for ix in 0..count {
            let slot = (ix, raster_key(self.zoom, self.scale));
            let image = self
                .cache
                .get(&slot)
                .or_else(|| self.stale.get(&slot))
                .cloned();
            let (slide_w, slide_h) = self.page_size_px(ix, cx);
            let mut slide = div()
                .w(px(slide_w))
                .h(px(slide_h))
                .ml(px((tape_w - slide_w) / 2.0))
                .flex_none()
                .relative()
                .shadow_lg()
                .overflow_hidden()
                .cursor_crosshair();
            slide = match image {
                Some(image) => slide.child(img(ImageSource::from(image)).w(px(slide_w)).h(px(slide_h))),
                None => slide.child(
                    div()
                        .size_full()
                        .flex()
                        .items_center()
                        .justify_center()
                        .bg(cx.theme().colors().panel_background)
                        .child(
                            Label::new(format!("slide {}", ix + 1))
                                .size(LabelSize::Small)
                                .color(Color::Muted),
                        ),
                ),
            };
            let mut highlights: Vec<((f32, f32, f32, f32), bool)> = Vec::new();
            if let Some(region) = self.region.filter(|r| r.slide == ix) {
                highlights.push((region.norm(), true));
            }
            for (hi, hit) in self.hits.iter().enumerate() {
                if hit.slide == ix {
                    highlights.push(((hit.x0, hit.y0, hit.x1, hit.y1), hi == self.hit_ix));
                }
            }
            for ((x0, y0, x1, y1), current) in highlights {
                slide = slide.child(
                    div()
                        .absolute()
                        .left(px(x0 * zoom))
                        .top(px(y0 * zoom))
                        .w(px((x1 - x0) * zoom))
                        .h(px((y1 - y0) * zoom))
                        .bg(if current {
                            selection_bg
                        } else {
                            cx.theme().colors().search_match_background
                        }),
                );
            }
            let view = cx.entity();
            slide = slide.on_mouse_down(MouseButton::Left, move |event, window, cx| {
                // stop_propagation keeps the tape from panning during region drag,
                // but it also blocks the root focus handler - focus the view here
                cx.stop_propagation();
                let focus = view.read(cx).focus_handle(cx);
                window.focus(&focus, cx);
                let _ = view.update(cx, |this, cx| {
                    let (x, y) = this.slide_pt(ix, event.position, cx);
                    this.region = Some(Region {
                        slide: ix,
                        x0: x,
                        y0: y,
                        x1: x,
                        y1: y,
                        live: true,
                    });
                    cx.notify();
                });
            });
            tape = tape.child(slide);
        }

        let view = cx.entity();
        let wrapper = ui::v_flex()
            .cursor_grab()
            .on_scroll({
                let view = view.clone();
                move |event, _, cx| {
                    if !event.modifiers.control {
                        return;
                    }
                    let delta = event.delta.pixel_delta(px(20.)).y;
                    let _ = view.update(cx, |this, cx| {
                        let step = office_knob_f("zoomStep", 10.0) / 100.0;
                    let factor = if f32::from(delta) > 0.0 { 1.0 + step } else { 1.0 / (1.0 + step) };
                        this.set_zoom(this.zoom * factor, cx);
                    });
                    cx.stop_propagation();
                }
            })
            .on_mouse_down(MouseButton::Left, {
                let view = view.clone();
                move |event, _, cx| {
                    let offset = view.read(cx).scroll.offset();
                    let drag = PanDrag {
                        start_pos: event.position,
                        start_offset: offset,
                    };
                    let _ = view.update(cx, |this, cx| {
                        this.pan = Some(drag);
                        cx.notify();
                    });
                }
            })
            .child(tape);

        let scroll_area = div()
            .id("slides-scroll")
            .size_full()
            .overflow_scroll()
            .track_scroll(&self.scroll)
            .child(wrapper);

        // editor-like autohide thumb on the right edge
        let thumb = {
            let bounds = self.scroll.bounds().size.height;
            let max = f32::from(self.scroll.max_offset().y);
            let offset_y = f32::from(self.scroll.offset().y);
            let reveal = self
                .scroll_reveal_until
                .is_some_and(|deadline| Instant::now() < deadline)
                || self.thumb_drag.is_some();
            if max > 0.0 && f32::from(bounds) > 0.0 {
                let viewport = f32::from(bounds);
                let ratio = viewport / (viewport + max);
                let thumb_h = (viewport * ratio).max(24.0).min(viewport);
                let progress = (-offset_y / max).clamp(0.0, 1.0);
                let top = progress * (viewport - thumb_h);
                let track = (viewport - thumb_h).max(1.0);
                Some(
                    div()
                        .id("slides-thumb")
                        .absolute()
                        .right(px(2.))
                        .top(px(top))
                        .w(px(6.))
                        .h(px(thumb_h))
                        .rounded_full()
                        .bg(cx.theme().colors().scrollbar_thumb_background)
                        .when(!reveal, |this| this.visible_on_hover("slides-thumb-zone"))
                        .on_mouse_down(MouseButton::Left, {
                            let view = view.clone();
                            move |event, _, cx| {
                                cx.stop_propagation();
                                let _ = view.update(cx, |this, cx| {
                                    this.thumb_drag = Some(ThumbDrag {
                                        start_y: event.position.y,
                                        start_top: top,
                                        track,
                                        max,
                                    });
                                    cx.notify();
                                });
                            }
                        }),
                )
            } else {
                None
            }
        };

        if let Some(region) = self.region.filter(|r| !r.live) {
            let origin = self.slide_origin(region.slide, cx);
            let body_origin = self.scroll.bounds().origin;
            let origin = gpui::point(origin.x - body_origin.x, origin.y - body_origin.y);
            let view = cx.entity();
            let mut bubble = ui::h_flex()
                .gap_1()
                .px_1p5()
                .py_1()
                .rounded_md()
                .border_1()
                .border_color(border)
                .bg(cx.theme().colors().element_background)
                .shadow_lg()
                .on_mouse_down(MouseButton::Left, |_event, _, cx| {
                    cx.stop_propagation();
                });
            bubble = bubble.child(
                IconButton::new(("region-send-text", region.slide as u64), IconName::ZedAssistant)
                    .icon_size(IconSize::Small)
                    .tooltip({
                        let focus = self.focus_handle.clone();
                        move |_window, cx| {
                            Tooltip::for_action_in("Send region text to agent", &SendSlideText, &focus, cx)
                        }
                    })
                    .on_click({
                        let view = view.clone();
                        move |_, window, cx| {
                            let _ = view.update(cx, |this, cx| this.send_slide_text(window, cx));
                        }
                    }),
            );
            bubble = bubble.child(
                IconButton::new(("region-send-image", region.slide as u64), IconName::Paperclip)
                    .icon_size(IconSize::Small)
                    .tooltip({
                        let focus = self.focus_handle.clone();
                        move |_window, cx| {
                            Tooltip::for_action_in("Send region image to agent", &SendRegionImage, &focus, cx)
                        }
                    })
                    .on_click({
                        let view = view.clone();
                        move |_, window, cx| {
                            let _ = view.update(cx, |this, cx| this.send_region_image(window, cx));
                        }
                    }),
            );
            bubble = bubble.child(
                IconButton::new(("region-copy-text", region.slide as u64), IconName::Copy)
                    .icon_size(IconSize::Small)
                    .tooltip({
                        let focus = self.focus_handle.clone();
                        move |_window, cx| {
                            Tooltip::for_action_in("Copy text in region", &CopySelection, &focus, cx)
                        }
                    })
                    .on_click({
                        let view = view.clone();
                        move |_, _, cx| {
                            let _ = view.update(cx, |this, cx| {
                                if let Some(region) = this.region {
                                    let text = this.region_text(region, cx);
                                    cx.notify();
                                    if !text.is_empty() {
                                        cx.write_to_clipboard(ClipboardItem::new_string(text));
                                    }
                                }
                            });
                        }
                    }),
            );
            bubble = bubble.child(
                IconButton::new(("region-copy-image", region.slide as u64), IconName::Image)
                    .icon_size(IconSize::Small)
                    .tooltip({
                        let focus = self.focus_handle.clone();
                        move |_window, cx| {
                            Tooltip::for_action_in("Copy region image", &CopyRegionImage, &focus, cx)
                        }
                    })
                    .on_click({
                        let view = view.clone();
                        move |_, _, cx| {
                            let _ = view.update(cx, |this, cx| this.copy_region_image(cx));
                        }
                    }),
            );
            region_bubble = Some(
                div()
                    .absolute()
                    // bubble follows the cursor at the end of the selection
                    .left(origin.x + px(region.x1 * zoom) - px(4.))
                    .top((origin.y + px(region.y1 * zoom) - px(28.)).max(origin.y))
                    .child(bubble),
            );
        }

        if self.focus_on_first_render {
            self.focus_on_first_render = false;
            let handle = self.focus_handle.clone();
            window.defer(cx, move |window, cx| window.focus(&handle, cx));
        }
        if self.focus_page_on_render && self.page_editing {
            self.focus_page_on_render = false;
            if let Some(editor) = self.page_editor.clone() {
                let handle = editor.read(cx).focus_handle(cx);
                window.defer(cx, move |window, cx| {
                    window.focus(&handle, cx);
                    editor.update(cx, |e, cx| {
                        e.select_all(&editor::actions::SelectAll, window, cx)
                    });
                });
            }
        }
        if self.focus_search_on_render && self.search_open {
            self.focus_search_on_render = false;
            // focus after the frame: the input element must be painted
            // (registered in the focus tree) before it can take focus
            if let Some(editor) = self.search_editor.clone() {
                let handle = editor.read(cx).focus_handle(cx);
                window.defer(cx, move |window, cx| window.focus(&handle, cx));
            }
        }
        if self.page_editor.is_none() {
            let editor = self.create_page_editor(window, cx);
            self.page_editor = Some(editor);
        }
        let page_ed = self.page_editor.clone().expect("page editor just created");
        let current = self.current_index(cx) + 1;
        if !page_ed.read(cx).focus_handle(cx).is_focused(window) {
            let want = current.to_string();
            if page_ed.read(cx).text(cx) != want {
                page_ed.update(cx, |editor, cx| editor.set_text(want, window, cx));
            }
        }
        let page_slot = if self.page_editing {
            div().id("page-edit").w(px(28.)).child(page_ed.clone())
        } else {
            div()
                .id("page-label")
                .cursor_pointer()
                .tooltip(Tooltip::text("Click to jump to a page"))
                .on_click({
                    let view = view.clone();
                    move |_, _, cx| {
                        let _ = view.update(cx, |this, cx| {
                            this.page_editing = true;
                            this.focus_page_on_render = true;
                            cx.notify();
                        });
                    }
                })
                .child(
                    Label::new(SharedString::from(current.to_string()))
                        .size(LabelSize::Small)
                        .color(Color::Muted),
                )
        };
        let subheader = ui::h_flex()
            .flex_none()
            .h(px(34.))
            .px_2()
            .gap_2()
            .border_b_1()
            .border_color(border)
            .bg(cx.theme().colors().tab_bar_background)
            .child(
                Label::new(self.tab_content_text(0, cx))
                    .size(LabelSize::Small)
                    .color(Color::Muted),
            )
            .child(div().flex_1())
            .child(
                IconButton::new("page-prev", IconName::ChevronUp)
                    .icon_size(IconSize::XSmall)
                    .tooltip({
                        let focus = self.focus_handle.clone();
                        move |_window, cx| {
                            Tooltip::for_action_in("Previous page", &PagePrev, &focus, cx)
                        }
                    })
                    .on_click({
                        let view = view.clone();
                        move |_, _, cx| {
                            let _ = view.update(cx, |this, cx| this.goto_prev(cx));
                        }
                    }),
            )
            .child(page_slot)
            .child(
                Label::new(SharedString::from(format!("/ {}", count)))
                    .size(LabelSize::Small)
                    .color(Color::Muted),
            )
            .child(
                IconButton::new("page-next", IconName::ChevronDown)
                    .icon_size(IconSize::XSmall)
                    .tooltip({
                        let focus = self.focus_handle.clone();
                        move |_window, cx| {
                            Tooltip::for_action_in("Next page", &PageNext, &focus, cx)
                        }
                    })
                    .on_click({
                        let view = view.clone();
                        move |_, _, cx| {
                            let _ = view.update(cx, |this, cx| this.goto_next(cx));
                        }
                    }),
            )
            .child(ui::Divider::vertical())
            .child(
                IconButton::new("zoom-out", IconName::Dash)
                    .icon_size(IconSize::XSmall)
                    .tooltip(Tooltip::text("Zoom out"))
                    .on_click({
                        let view = view.clone();
                        move |_, _, cx| {
                            let _ = view.update(cx, |this, cx| this.set_zoom(this.zoom / 1.25, cx));
                        }
                    }),
            )
            .child(
                div()
                    .id("zoom-reset")
                    .px_1()
                    .cursor_pointer()
                    .tooltip(Tooltip::text("Reset zoom"))
                    .on_click({
                        let view = view.clone();
                        move |_, _, cx| {
                            let _ = view.update(cx, |this, cx| this.set_zoom(1.0, cx));
                        }
                    })
                    .child(
                        Label::new(SharedString::from(format!("{}%", (self.zoom * 100.0).round() as u32)))
                            .size(LabelSize::XSmall)
                            .color(Color::Muted),
                    ),
            )
            .child(
                IconButton::new("zoom-in", IconName::Plus)
                    .icon_size(IconSize::XSmall)
                    .tooltip(Tooltip::text("Zoom in"))
                    .on_click({
                        let view = view.clone();
                        move |_, _, cx| {
                            let _ = view.update(cx, |this, cx| this.set_zoom(this.zoom * 1.25, cx));
                        }
                    }),
            )
            .child(
                IconButton::new("slides-search", IconName::MagnifyingGlass)
                    .icon_size(IconSize::Small)
                    .tooltip({
                        let focus = self.focus_handle.clone();
                        move |_window, cx| {
                            Tooltip::for_action_in("Search in presentation", &ToggleSearch, &focus, cx)
                        }
                    })
                    .on_click({
                        let view = view.clone();
                        move |_, window, cx| {
                            let _ = view.update(cx, |this, cx| {
                                this.search_open = !this.search_open;
                                if !this.search_open {
                                    this.hits.clear();
                                }
                                if this.search_open {
                                    this.deploy_search(window, cx);
                                }
                                cx.notify();
                            });
                        }
                    }),
            )
            .child(
                IconButton::new("slides-assist", IconName::ZedAssistant)
                    .icon_size(IconSize::Small)
                    .tooltip({
                        let focus = self.focus_handle.clone();
                        move |_window, cx| {
                            Tooltip::for_action_in("Send slide text to agent", &SendSlideText, &focus, cx)
                        }
                    })
                    .on_click({
                        let view = view.clone();
                        move |_, window, cx| {
                            let _ = view.update(cx, |this, cx| this.send_slide_text(window, cx));
                        }
                    }),
            );

        let match_label = format!(
            "{}/{}",
            if self.hits.is_empty() { 0 } else { self.hit_ix + 1 },
            self.hits.len()
        );
        let search_bar = self.search_open.then(|| {
            let mut bar = ui::h_flex()
                .key_context("OfficeSearch")
                .flex_none()
                .h(px(32.))
                .px_2()
                .gap_2()
                .items_center()
                .border_b_1()
                .border_color(border)
                .bg(cx.theme().colors().editor_background);
            if let Some(editor) = self.search_editor.clone() {
                bar = bar.child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .h(px(22.))
                        // root view refocuses itself on left mouse down; without
                        // stopping the bubble the editor loses focus on click
                        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                        .child(editor),
                );
            }
            bar = bar
                .child(
                    Label::new(SharedString::from(match_label))
                        .size(LabelSize::XSmall)
                        .color(Color::Muted),
                )
                .child(
                    IconButton::new("search-prev", IconName::ChevronUp)
                        .icon_size(IconSize::XSmall)
                        .tooltip({
                            let focus = self.focus_handle.clone();
                            move |_window, cx| {
                                Tooltip::for_action_in("Previous match", &SearchPrev, &focus, cx)
                            }
                        })
                        .on_click({
                            let view = view.clone();
                            move |_, _, cx| {
                                let _ = view.update(cx, |this, cx| {
                                    this.step_match(-1, cx);
                                });
                            }
                        }),
                )
                .child(
                    IconButton::new("search-next", IconName::ChevronDown)
                        .icon_size(IconSize::XSmall)
                        .tooltip({
                            let focus = self.focus_handle.clone();
                            move |_window, cx| {
                                Tooltip::for_action_in("Next match", &SearchNext, &focus, cx)
                            }
                        })
                        .on_click({
                            let view = view.clone();
                            move |_, _, cx| {
                                let _ = view.update(cx, |this, cx| {
                                    this.step_match(1, cx);
                                });
                            }
                        }),
                )
                .child(
                    IconButton::new("search-close", IconName::Close)
                        .icon_size(IconSize::XSmall)
                        .on_click({
                            let view = view.clone();
                            move |_, _, cx| {
                                let _ = view.update(cx, |this, cx| {
                                    this.search_open = false;
                                    this.hits.clear();
                                    cx.notify();
                                });
                            }
                        }),
                );
            bar
        });

        let pan_canvas = gpui::canvas(
            |_, _, _| (),
            {
                let view = view.clone();
                move |_, _, window: &mut Window, cx: &mut App| {
                    let active = view.read(cx).sel_drag_active();
                    if !active {
                        return;
                    }
                    window.on_mouse_event({
                        let view = view.clone();
                        move |event: &gpui::MouseMoveEvent, phase, _, cx: &mut App| {
                            if phase != gpui::DispatchPhase::Bubble {
                                return;
                            }
                            let _ = view.update(cx, |this, cx| {
                                if let Some(drag) = this.thumb_drag {
                                    let travelled = f32::from(event.position.y - drag.start_y);
                                    let progress =
                                        ((drag.start_top + travelled) / drag.track).clamp(0.0, 1.0);
                                    this.scroll
                                        .set_offset(gpui::point(px(0.), -px(progress * drag.max)));
                                    cx.notify();
                                    return;
                                }
                                if let Some(region) = this.region.filter(|r| r.live) {
                                    let (x, y) = this.slide_pt(region.slide, event.position, cx);
                                    if let Some(current) = this.region.as_mut() {
                                        current.x1 = x;
                                        current.y1 = y;
                                    }
                                    cx.notify();
                                    return;
                                }
                                if let Some(drag) = this.pan {
                                    let max = this.scroll.max_offset();
                                    let current =
                                        drag.start_offset + (event.position - drag.start_pos);
                                    let clamped = gpui::point(
                                        current.x.clamp(-max.x, px(0.)),
                                        current.y.clamp(-max.y, px(0.)),
                                    );
                                    this.scroll.set_offset(clamped);
                                    cx.notify();
                                }
                            });
                        }
                    });
                    window.on_mouse_event({
                        let view = view.clone();
                        move |_: &gpui::MouseUpEvent, phase, _, cx: &mut App| {
                            if phase != gpui::DispatchPhase::Bubble {
                                return;
                            }
                            let _ = view.update(cx, |this, cx| {
                                this.pan = None;
                                this.thumb_drag = None;
                                if let Some(region) = this.region.as_mut() {
                                    if region.live {
                                        region.live = false;
                                        let (x0, y0, x1, y1) = region.norm();
                                        let tiny = (x1 - x0) * (this.zoom as f32) < 6.0
                                            || (y1 - y0) * (this.zoom as f32) < 6.0;
                                        if tiny {
                                            this.region = None;
                                        }
                                    }
                                }
                                cx.notify();
                            });
                        }
                    });
                }
            },
        );

        ui::v_flex()
            .size_full()
            .key_context("OfficeView")
            .track_focus(&self.focus_handle)
            .on_mouse_down(MouseButton::Left, {
                let focus = self.focus_handle.clone();
                move |_, window, cx| window.focus(&focus, cx)
            })
            .on_action(cx.listener(|this, _: &CopySelection, _, cx| {
                if let Some(region) = this.region {
                    let text = this.region_text(region, cx);
                    if !text.is_empty() {
                        cx.write_to_clipboard(ClipboardItem::new_string(text));
                    }
                }
            }))
            .on_action(cx.listener(|this, _: &ClearSelection, _, cx| {
                this.region = None;
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &ToggleSearch, window, cx| {
                this.search_open = !this.search_open;
                if !this.search_open {
                    this.hits.clear();
                }
                if this.search_open {
                    this.deploy_search(window, cx);
                }
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &PagePrev, _, cx| this.goto_prev(cx)))
            .on_action(cx.listener(|this, _: &PageNext, _, cx| this.goto_next(cx)))
            .on_action(cx.listener(|this, _: &SendSlideText, window, cx| {
                this.send_slide_text(window, cx);
            }))
            .on_action(cx.listener(|this, _: &CopyRegionImage, _, cx| {
                this.copy_region_image(cx);
            }))
            .on_action(cx.listener(|this, _: &SendRegionImage, window, cx| {
                this.send_region_image(window, cx);
            }))
            .on_action(cx.listener(|this, _: &SearchNext, _, cx| {
                this.step_match(1, cx);
            }))
            .on_action(cx.listener(|this, _: &SearchPrev, _, cx| {
                this.step_match(-1, cx);
            }))
            .child(subheader)
            .children(search_bar)
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .relative()
                    .overflow_hidden()
                    .bg(cx.theme().colors().panel_background)
                    .child(scroll_area)
                    .children(thumb)
                    .children(region_bubble)
                    .child(
                        div()
                            .absolute()
                            .right_0()
                            .top_0()
                            .bottom_0()
                            .w(px(12.))
                            .group("slides-thumb-zone")
                            .on_mouse_move({
                                let view = view.clone();
                                move |_, _, cx| {
                                    arm_timer(
                                        &view,
                                        cx,
                                        |this| &mut this.scroll_reveal_until,
                                        |this| &mut this.scroll_timer_armed,
                                        1200,
                                    );
                                }
                            }),
                    )

                    .child(pan_canvas),
            )
    }
}

mod persistence {
    use std::path::PathBuf;

    use db::{
        query,
        sqlez::{domain::Domain, thread_safe_connection::ThreadSafeConnection},
        sqlez_macros::sql,
    };
    use workspace::{ItemId, WorkspaceDb, WorkspaceId};

    pub struct WordViewerDb(ThreadSafeConnection);

    impl Domain for WordViewerDb {
        const NAME: &str = stringify!(WordViewerDb);

        const MIGRATIONS: &[&str] = &[sql!(
            CREATE TABLE word_viewers (
                workspace_id INTEGER,
                item_id INTEGER UNIQUE,
                doc_path BLOB,
                PRIMARY KEY(workspace_id, item_id),
                FOREIGN KEY(workspace_id) REFERENCES workspaces(workspace_id)
                ON DELETE CASCADE
            ) STRICT;
        )];
    }

    db::static_connection!(WordViewerDb, [WorkspaceDb]);

    impl WordViewerDb {
        query! {
            pub async fn save_doc_path(
                item_id: ItemId,
                workspace_id: WorkspaceId,
                doc_path: PathBuf
            ) -> Result<()> {
                INSERT OR REPLACE INTO word_viewers(item_id, workspace_id, doc_path)
                VALUES (?, ?, ?)
            }
        }

        query! {
            pub fn get_doc_path(item_id: ItemId, workspace_id: WorkspaceId) -> Result<Option<PathBuf>> {
                SELECT doc_path
                FROM word_viewers
                WHERE item_id = ? AND workspace_id = ?
            }
        }
    }
}

use persistence::WordViewerDb;
