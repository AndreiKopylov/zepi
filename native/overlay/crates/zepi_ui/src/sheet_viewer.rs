use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, Instant};

use editor::Editor;
// imports double as the child sheet_render module supply via use super::*
#[allow(unused_imports)]
use gpui::{
    App, AppContext, Context, Entity, EntityId, FocusHandle, Focusable, Hsla,
    WindowTextSystem,
    InteractiveElement, IntoElement, MouseDownEvent, MouseButton, ParentElement, Point,
    KeyDownEvent, MouseMoveEvent, MouseUpEvent, Pixels, ScrollEvent, SharedString, Styled,
    Task, Window, actions, div, px,
};
use gridcraft_core::{CellRef, Value};
use gridcraft_model::style::{PatternType, StyleId};
use gridcraft_model::workbook::{RichRun, RichString, Workbook};
use gridcraft_model::{Cell, Formula};
use gridcraft_numfmt::{NumberFormat, format_value};
use gridcraft_xlsx::{read_xlsx, write_xlsx};
use project::{Project, ProjectEntryId, ProjectPath};
use theme::ActiveTheme;
#[allow(unused_imports)]
use ui::{
    ButtonCommon, Clickable, Color as UiColor, Icon, IconButton, IconName, Label, LabelCommon,
    LabelSize, ScrollAxes,
    ScrollableHandle, ScrollbarStyle, Scrollbars, Tooltip, WithScrollbar,
};
use workspace::item::{ItemBufferKind, Item, ProjectItem, SerializableItem, TabContentParams};
use workspace::notifications::{DetachAndPromptErr, NotificationId};
use workspace::{
    Pane, Toast, Workspace, register_project_item,
    register_serializable_item,
};

use crate::slides_viewer::{office_knob, office_knob_f, office_thumb_color};

mod sheet_render;

actions!(
    sheet_viewer,
    [
        Save,
        ZoomIn,
        ZoomOut,
        EditCell,
        CommitEdit,
        CancelEdit,
        MoveUp,
        MoveDown,
        MoveLeft,
        MoveRight,
        SheetSearch,
        SheetSearchNext,
        SheetSearchPrev,
        SheetCopy,
        SheetCut,
        SheetPaste,
        SheetUndo,
        SheetRedo,
        SheetSend,
        SheetEditNewline,
        SheetClear,
        SheetSelectAll,
        SheetZoomReset
    ]
);

const TOP_H: f32 = 22.0;
const LEFT_W: f32 = 46.0;
const ZOOM_MIN: f32 = 0.4;
const ZOOM_MAX: f32 = 4.0;

/// Save bytes via a sibling temp file, then rename over the target: a crash or
/// full disk mid-write must not truncate the user's workbook (Excel saves the same way).
fn save_workbook_atomic(abs: &std::path::Path, bytes: &[u8]) -> anyhow::Result<()> {
    let name = abs.file_name().unwrap_or_default().to_string_lossy();
    let temp = abs.with_file_name(format!("~${name}.{}.tmp", uuid::Uuid::new_v4().simple()));
    let file = std::fs::OpenOptions::new().write(true).create_new(true).open(&temp);
    let mut file = match file {
        Ok(file) => file,
        Err(error) => {
            let _ = std::fs::remove_file(&temp);
            return Err(error.into());
        }
    };
    // Excel also hides its save temp: Explorer and sync tools skip it during the write
    #[cfg(windows)]
    set_hidden(&temp, true);
    if let Err(error) = std::io::Write::write_all(&mut file, bytes) {
        drop(file);
        let _ = std::fs::remove_file(&temp);
        return Err(error.into());
    }
    drop(file);
    // attributes travel with the renamed file, so restore visibility before replacing the workbook
    #[cfg(windows)]
    set_hidden(&temp, false);
    if let Err(error) = std::fs::rename(&temp, abs) {
        let _ = std::fs::remove_file(&temp);
        // sharing violation or access denied: another process holds the file open
        if matches!(error.raw_os_error(), Some(5 | 32)) {
            anyhow::bail!("the file is in use by another process (close it and retry): {error}");
        }
        return Err(error.into());
    }
    Ok(())
}

#[cfg(windows)]
fn set_hidden(path: &std::path::Path, hidden: bool) {
    use std::os::windows::ffi::OsStrExt;
    use windows::Win32::Storage::FileSystem::{
        SetFileAttributesW, FILE_ATTRIBUTE_HIDDEN, FILE_ATTRIBUTE_NORMAL,
    };
    let mut wide: Vec<u16> = path.as_os_str().encode_wide().collect();
    wide.push(0);
    // best-effort: visibility of the short-lived temp is cosmetic
    unsafe {
        let _ = SetFileAttributesW(
            windows::core::PCWSTR::from_raw(wide.as_ptr()),
            if hidden { FILE_ATTRIBUTE_HIDDEN } else { FILE_ATTRIBUTE_NORMAL },
        );
    }
}

pub fn init(cx: &mut App) {
    register_project_item::<SheetView>(cx);
    register_serializable_item::<SheetView>(cx);
}

const NLCH: char = '\n';

/// Exact pixel width of a single line of text in the given font.
fn shape_width(ts: &WindowTextSystem, text: &str, font: &gpui::Font, size: Pixels) -> f32 {
    if text.is_empty() {
        return 0.0;
    }
    let run = gpui::TextRun {
        len: text.len(),
        font: font.clone(),
        color: gpui::black(),
        background_color: None,
        underline: None,
        strikethrough: None,
    };
    f32::from(
        ts.shape_line(SharedString::from(text), size, &[run], None)
            .width(),
    )
}

fn splice_runs(runs: &mut Vec<RichRun>, old: &str, new: &str) {
    let oc: Vec<char> = old.chars().collect();
    let nc: Vec<char> = new.chars().collect();
    let mut p = 0;
    while p < oc.len() && p < nc.len() && oc[p] == nc[p] { p += 1; }
    let mut s = 0;
    while s < oc.len() - p && s < nc.len() - p && oc[oc.len() - 1 - s] == nc[nc.len() - 1 - s] { s += 1; }
    let mut attrs: Vec<RichRun> = Vec::new();
    for r in runs.iter() { for _ in r.text.chars() { attrs.push(RichRun { text: String::new(), ..r.clone() }); } }
    while attrs.len() < oc.len() { attrs.push(runs.last().cloned().unwrap_or_default()); }
    let ins = if p > 0 { attrs[p - 1].clone() } else { attrs.first().cloned().unwrap_or_default() };
    let mut na: Vec<RichRun> = attrs[..p].to_vec();
    for _ in 0..(nc.len() - p - s) { na.push(ins.clone()); }
    na.extend_from_slice(&attrs[oc.len() - s..]);
    let mut out: Vec<RichRun> = Vec::new();
    let mut cur: Option<RichRun> = None;
    let mut buf = String::new();
    for (ch, at) in nc.iter().zip(na.iter()) {
        let same = cur.as_ref().map(|c| c.bold == at.bold && c.italic == at.italic && c.underline == at.underline && c.strike == at.strike && c.color == at.color && c.size == at.size && c.font == at.font).unwrap_or(false);
        if same { buf.push(*ch); } else { if let Some(c) = cur.take() { if !buf.is_empty() { out.push(RichRun { text: buf.clone(), ..c }); } } buf.clear(); buf.push(*ch); cur = Some(at.clone()); }
    }
    if let Some(c) = cur { if !buf.is_empty() { out.push(RichRun { text: buf, ..c }); } }
    *runs = out;
}

fn edit_font(name: &str, bold: bool, italic: bool) -> gpui::Font {
    gpui::Font {
        family: SharedString::from(name.to_string()),
        weight: if bold {
            gpui::FontWeight::BOLD
        } else {
            gpui::FontWeight::default()
        },
        style: if italic {
            gpui::FontStyle::Italic
        } else {
            gpui::FontStyle::default()
        },
        ..Default::default()
    }
}

/// 0-based column index to spreadsheet letters (A, B, ..., AA, ...).
fn col_name(col: u32) -> String {
    let mut n = col + 1;
    let mut out = String::new();
    while n > 0 {
        n -= 1;
        out.insert(0, (b'A' + (n % 26) as u8) as char);
        n /= 26;
    }
    out
}

fn cell_ref_text(row: u32, col: u32) -> String {
    format!("{}{}", col_name(col), row + 1)
}

fn theme_tint(rgb: u32, tint: i16) -> u32 {
    let t = f32::from(tint) / 1000.0;
    let map = |c: u32| -> u32 {
        let v = if t > 0.0 {
            c as f32 + (255.0 - c as f32) * t
        } else {
            c as f32 * (1.0 + t)
        };
        v.clamp(0.0, 255.0) as u32
    };
    (map((rgb >> 16) & 0xFF) << 16) | (map((rgb >> 8) & 0xFF) << 8) | map(rgb & 0xFF)
}

fn model_color_hsla(color: &gridcraft_model::style::Color, fallback: Hsla, theme: &[u32; 12]) -> Hsla {
    match color {
        gridcraft_model::style::Color::Rgb(rgb) => gpui::rgb(*rgb).into(),
        gridcraft_model::style::Color::Theme(index, tint) => {
            // shortcut: RGB blend instead of OOXML HSL luminance tint, close enough on screen
            gpui::rgb(theme_tint(theme[usize::from(*index)], *tint)).into()
        }
        gridcraft_model::style::Color::Auto => fallback,
    }
}

/// Formula-bar text to a cell: numbers and booleans by value, "=..." as a formula
/// (cached value stays empty until the file is recalculated elsewhere), rest as text.
fn parse_cell_text(text: &str, style: StyleId) -> Cell {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return Cell {
            value: Value::Empty,
            formula: None,
            style,
        };
    }
    if let Some(formula) = trimmed.strip_prefix('=') {
        return Cell {
            value: Value::Empty,
            formula: Some(Arc::new(Formula::new(formula))),
            style,
        };
    }
    if trimmed.eq_ignore_ascii_case("true") {
        return Cell {
            value: Value::Bool(true),
            formula: None,
            style,
        };
    }
    if trimmed.eq_ignore_ascii_case("false") {
        return Cell {
            value: Value::Bool(false),
            formula: None,
            style,
        };
    }
    if let Ok(number) = trimmed.parse::<f64>() {
        return Cell {
            value: Value::Number(number),
            formula: None,
            style,
        };
    }
    Cell {
        value: Value::Text(Arc::from(trimmed)),
        formula: None,
        style,
    }
}

#[derive(Clone)]
enum SheetOp {
    Cells(Vec<(CellRef, Option<Cell>, Option<Cell>)>),
    Col(u32, Option<f32>, Option<f32>),
    Row(u32, Option<f32>, Option<f32>),
}

#[derive(Clone, Default)]
struct SheetScrollHandle(Rc<RefCell<SheetScrollInner>>);

#[derive(Default)]
struct SheetScrollInner {
    offset: Point<Pixels>,
    viewport: gpui::Size<Pixels>,
    max_offset: Point<Pixels>,
    zoom: f32,
}

impl SheetScrollHandle {
    fn pos(&self) -> (f32, f32) {
        let inner = self.0.borrow();
        let zoom = inner.zoom.max(0.1);
        (-f32::from(inner.offset.x) / zoom, -f32::from(inner.offset.y) / zoom)
    }

    fn set_pos(&self, x: f32, y: f32) {
        let mut inner = self.0.borrow_mut();
        let zoom = inner.zoom.max(0.1);
        let ox = (x * zoom).clamp(0.0, f32::from(inner.max_offset.x));
        let oy = (y * zoom).clamp(0.0, f32::from(inner.max_offset.y));
        inner.offset = Point::new(px(-ox), px(-oy));
    }

    fn set_metrics(&self, viewport: gpui::Size<Pixels>, content: gpui::Size<Pixels>, zoom: f32) {
        let mut inner = self.0.borrow_mut();
        inner.viewport = viewport;
        inner.zoom = zoom;
        inner.max_offset = Point::new(
            px((f32::from(content.width) - f32::from(viewport.width)).max(0.0)),
            px((f32::from(content.height) - f32::from(viewport.height)).max(0.0)),
        );
        let off = inner.offset;
        inner.offset = Point::new(
            px(f32::from(off.x).clamp(-f32::from(inner.max_offset.x), 0.0)),
            px(f32::from(off.y).clamp(-f32::from(inner.max_offset.y), 0.0)),
        );
    }
}

impl ScrollableHandle for SheetScrollHandle {
    fn max_offset(&self) -> Point<Pixels> {
        self.0.borrow().max_offset
    }
    fn set_offset(&self, point: Point<Pixels>) {
        let mut inner = self.0.borrow_mut();
        inner.offset = Point::new(
            px(f32::from(point.x).clamp(-f32::from(inner.max_offset.x), 0.0)),
            px(f32::from(point.y).clamp(-f32::from(inner.max_offset.y), 0.0)),
        );
    }
    fn offset(&self) -> Point<Pixels> {
        self.0.borrow().offset
    }
    fn viewport(&self) -> gpui::Bounds<Pixels> {
        let inner = self.0.borrow();
        gpui::Bounds {
            origin: Point::default(),
            size: inner.viewport,
        }
    }
}

pub struct SheetItem {
    path: ProjectPath,
    entry_id: Option<ProjectEntryId>,
    abs_path: PathBuf,
    wb: Option<Workbook>,
    import_error: Option<String>,
    dirty: bool,
}

impl project::ProjectItem for SheetItem {
    fn try_open(
        project: &Entity<Project>,
        path: &ProjectPath,
        cx: &mut App,
    ) -> Option<Task<anyhow::Result<Entity<Self>>>> {
        let ext = path
            .path
            .extension()
            .map(|ext| ext.to_ascii_lowercase())
            .or_else(|| {
                project.read(cx).absolute_path(path, cx).and_then(|abs| {
                    abs.extension().map(|ext| ext.to_string_lossy().to_ascii_lowercase())
                })
            })?;
        if !matches!(ext.as_str(), "xlsx" | "xlsm" | "xls") {
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
                    let (mut wb, _report) = read_xlsx(&bytes)?;
                    SheetView::autofit_display_rows(&mut wb);
                    anyhow::Ok((wb, abs_path))
                })
                .await;
            Ok(match loaded {
                Ok((wb, abs_path)) => cx.new(|_| SheetItem {
                    path,
                    entry_id,
                    abs_path,
                    wb: Some(wb),
                    import_error: None,
                    dirty: false,
                }),
                Err(error) => cx.new(|_| SheetItem {
                    path,
                    entry_id,
                    abs_path: abs_for_err,
                    wb: None,
                    import_error: Some(format!("{error:#}")),
                    dirty: false,
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
        self.dirty
    }
}

pub struct SheetView {
    item: Entity<SheetItem>,
    focus_handle: FocusHandle,
    sheet: usize,
    scroll: SheetScrollHandle,
    zoom: f32,
    sel_row: u32,
    sel_col: u32,
    anchor: Option<(u32, u32)>,
    drag_select: bool,
    edit_row: u32,
    edit_col: u32,
    resize: Option<(u8, u32, f32, f32)>,
    undo_stack: Vec<SheetOp>,
    redo_stack: Vec<SheetOp>,
    editing: bool,
    edit_buffer: String,
    edit_caret: usize,
    edit_sel_anchor: Option<usize>,
    edit_undo: Option<(String, usize, Option<usize>, Vec<RichRun>)>,
    search_open: bool,
    search_editor: Option<Entity<Editor>>,
    cell_editor: Option<Entity<Editor>>,
    edit_runs: Vec<RichRun>,
    edit_prev: String,
    focus_search_on_render: bool,
    refocus_root: bool,
    search_matches: Vec<(u32, u32, usize, usize)>,
    search_current: usize,
    search_query: String,
    grid_origin: Point<gpui::Pixels>,
    grid_size: Point<gpui::Pixels>,
    self_write: Option<Instant>,
    watch_task: Option<Task<()>>,
    reload_again: bool,
    // mtime+size of the bytes currently shown; the poll compares against it
    loaded_stamp: Option<(u64, u64)>,
    saving: bool,
    edit_rect_cache: Option<(f32, f32, f32, f32)>,
    edit_font_px: f32,
    edit_font_cur: gpui::Font,
    caret_on: bool,
    edit_drag: bool,
    edit_align: u8,
    edit_layout: Vec<(f32, f32, Vec<(usize, usize, f32, f32, Vec<f32>)>)>,
    edit_origin: std::sync::Arc<std::sync::Mutex<(f32, f32)>>,
    edit_mouse: Option<(f32, f32)>,
    caret_task: Option<Task<()>>,
}



impl SheetView {
    pub fn new(item: Entity<SheetItem>, project: &Entity<Project>, cx: &mut Context<Self>) -> Self {
        let sheet = item
            .read(cx)
            .wb
            .as_ref()
            .map(|wb| wb.active_sheet().min(wb.sheets.len().saturating_sub(1)))
            .unwrap_or(0);
        let loaded_stamp = crate::disk_stamp(&item.read(cx).abs_path);
        let this = Self {
            item,
            focus_handle: cx.focus_handle(),
            sheet,
            scroll: SheetScrollHandle::default(),
            zoom: 1.0,
            sel_row: 0,
            sel_col: 0,
            anchor: None,
            drag_select: false,
            edit_row: 0,
            edit_col: 0,
            resize: None,
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
            editing: false,
            edit_buffer: String::new(),
            edit_caret: 0,
            edit_sel_anchor: None,
            edit_undo: None,
            search_open: false,
            search_editor: None,
            cell_editor: None,
            edit_runs: Vec::new(),
            edit_prev: String::new(),
            focus_search_on_render: false,
            refocus_root: false,
            search_matches: Vec::new(),
            search_current: 0,
            search_query: String::new(),
            grid_origin: Point::default(),
            grid_size: Point::default(),
            self_write: None,
            watch_task: None,
            reload_again: false,
            loaded_stamp,
            saving: false,
            edit_rect_cache: None,
            edit_font_px: 12.0,
            edit_font_cur: gpui::Font::default(),
            caret_on: true,
            edit_drag: false,
            edit_align: 0,
            edit_layout: Vec::new(),
            edit_origin: std::sync::Arc::new(std::sync::Mutex::new((0.0, 0.0))),
            edit_mouse: None,
            caret_task: None,
        };
        // Worktree events can be missed when a writer replaces the file (Office and
        // python-pptx save by rename-over), so the visible file is polled as well.
        cx.spawn(async move |this, cx| loop {
            cx.background_executor()
                .timer(Duration::from_secs(2))
                .await;
            let Ok((abs, loaded, writing)) = this.read_with(cx, |this, cx| {
                (
                    this.item.read(cx).abs_path.clone(),
                    this.loaded_stamp,
                    this.self_write.is_some_and(|at| at.elapsed() < Duration::from_secs(3)),
                )
            }) else {
                break;
            };
            if writing {
                continue;
            }
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
                // our own save echoes back through the fs watch
                if this.self_write.is_some_and(|at| at.elapsed() < Duration::from_secs(2)) {
                    return;
                }
                this.schedule_reload(cx);
            }
        })
        .detach();
        this
    }

    /// Debounced reimport after the workbook changes on disk (worktree fs watch).
    fn schedule_reload(&mut self, cx: &mut Context<Self>) {
        // unsaved edits win over external changes: skip reload instead of clobbering them
        if self.item.read(cx).dirty {
            log::warn!("sheet_viewer: skip reload, sheet has unsaved edits");
            return;
        }
        if self.watch_task.is_some() {
            self.reload_again = true;
            return;
        }
        let abs = self.item.read(cx).abs_path.clone();
        let item = self.item.clone();
        self.watch_task = Some(cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(Duration::from_millis(office_knob("reloadDebounceMs", 600)))
                .await;
            let mut imported = Err(anyhow::anyhow!("not attempted"));
            for attempt in 0..4u32 {
                if attempt > 0 {
                    cx.background_executor().timer(Duration::from_millis(400)).await;
                }
                let abs_try = abs.clone();
                imported = cx
                    .background_executor()
                    .spawn(async move {
                        let bytes = std::fs::read(&abs_try)?;
                        let (mut wb, _report) = read_xlsx(&bytes)?;
                    SheetView::autofit_display_rows(&mut wb);
                        anyhow::Ok(wb)
                    })
                    .await;
                if imported.is_ok() {
                    break;
                }
            }
            let _ = this.update(cx, |this, cx| {
                this.watch_task = None;
                let again = std::mem::take(&mut this.reload_again);
                match imported {
                    Ok(wb) => {
                        item.update(cx, |item, cx| {
                            item.wb = Some(wb);
                            item.import_error = None;
                            item.dirty = false;
                            cx.notify();
                        });
                        this.loaded_stamp = crate::disk_stamp(&item.read(cx).abs_path);
                        let last = item
                            .read(cx)
                            .wb
                            .as_ref()
                            .map(|wb| wb.sheets.len())
                            .unwrap_or(1)
                            .saturating_sub(1);
                        this.sheet = this.sheet.min(last);
                        this.cancel_edit(cx);
                        this.undo_stack.clear();
                        this.redo_stack.clear();
                        this.clamp_sel(cx);
                        cx.notify();
                    }
                    Err(error) => log::warn!("sheet_viewer: reload failed: {error:#}"),
                }
                if again {
                    this.schedule_reload(cx);
                }
            });
        }));
    }

    fn used_bounds(&self, cx: &App) -> (u32, u32) {
        let item = self.item.read(cx);
        let Some(wb) = item.wb.as_ref() else {
            return (0, 0);
        };
        let Some(sheet) = wb.sheets.get(self.sheet) else {
            return (0, 0);
        };
        match sheet.used_range() {
            Some(range) => (range.end.row, range.end.col),
            None => (0, 0),
        }
    }

    fn clamp_sel(&mut self, cx: &App) {
        let (last_row, last_col) = self.used_bounds(cx);
        self.sel_row = self.sel_row.min(last_row);
        self.sel_col = self.sel_col.min(last_col);
    }

    fn move_sel(&mut self, dr: i32, dc: i32, window: &mut Window, cx: &mut Context<Self>) {
        if self.editing {
            return;
        }
        let old = (self.sel_row, self.sel_col);
        let (last_row, last_col) = self.used_bounds(cx);
        self.sel_row = ((self.sel_row as i64 + dr as i64).clamp(0, last_row as i64)) as u32;
        self.sel_col = ((self.sel_col as i64 + dc as i64).clamp(0, last_col as i64)) as u32;
        if window.modifiers().shift {
            if self.anchor.is_none() {
                self.anchor = Some(old);
            }
        } else {
            self.anchor = Some((self.sel_row, self.sel_col));
        }
        cx.notify();
    }

    fn sel_range(&self) -> (u32, u32, u32, u32) {
        match self.anchor {
            Some((ar, ac)) => (
                ar.min(self.sel_row),
                ar.max(self.sel_row),
                ac.min(self.sel_col),
                ac.max(self.sel_col),
            ),
            None => (self.sel_row, self.sel_row, self.sel_col, self.sel_col),
        }
    }

    fn tsv_of_range(&self, r0: u32, r1: u32, c0: u32, c1: u32, cx: &App) -> String {
        let mut lines = Vec::new();
        for row in r0..=r1 {
            let cols: Vec<String> = (c0..=c1).map(|col| self.cell_bar_text(row, col, cx)).collect();
            lines.push(cols.join("\t"));
        }
        lines.join("\n")
    }

    fn selection_text(&self, cx: &App) -> String {
        let item = self.item.read(cx);
        let Some(wb) = item.wb.as_ref() else {
            return String::new();
        };
        let Some(sheet) = wb.sheets.get(self.sheet) else {
            return String::new();
        };
        let (r0, r1, c0, c1) = self.sel_range();
        let body = self.tsv_of_range(r0, r1, c0, c1, cx);
        let path = item.abs_path.display();
        let name = sheet.name.clone();
        format!(
            "{} :: {} :: {}:{}\n{}",
            path,
            name,
            cell_ref_text(r0, c0),
            cell_ref_text(r1, c1),
            body
        )
    }

    fn send_selection(&self, window: &mut Window, cx: &mut App) {
        let text = self.selection_text(cx);
        if text.trim().is_empty() {
            return;
        }
        let Some(workspace) = window.root::<Workspace>().flatten().or_else(|| {
            window.root::<workspace::MultiWorkspace>().flatten().and_then(|multi| {
                Some(multi.read(cx).workspace().clone())
            })
        }) else {
            log::error!("sheet_viewer send: no workspace root");
            return;
        };
        let Some(panel) = workspace.read(cx).panel::<agent_ui::AgentPanel>(cx) else {
            log::error!("sheet_viewer send: no agent panel");
            return;
        };
        let panel_ref = panel.read(cx);
        let Some(conversation) = panel_ref
            .visible_conversation_view()
            .or_else(|| panel_ref.active_conversation_view())
            .cloned() else {
            log::error!("sheet_viewer send: no active conversation");
            return;
        };
        let Some(thread_view) = conversation.read(cx).active_thread().cloned() else {
            log::error!("sheet_viewer send: no root thread view");
            return;
        };
        thread_view.update(cx, |thread_view, cx| {
            thread_view
                .message_editor
                .update(cx, |editor, cx| editor.zepi_append_text(&text, window, cx));
        });
    }

    fn push_op(&mut self, op: SheetOp) {
        if self.undo_stack.len() >= 200 {
            self.undo_stack.remove(0);
        }
        self.undo_stack.push(op);
        self.redo_stack.clear();
    }

    fn sheet_update<F: FnOnce(&mut gridcraft_model::sheet::Sheet)>(&mut self, cx: &mut Context<Self>, f: F) {
        self.item.update(cx, |item, cx| {
            let Some(wb) = item.wb.as_mut() else { return };
            let Some(arc_sheet) = wb.sheets.get_mut(self.sheet) else { return };
            let sheet = Arc::make_mut(arc_sheet);
            f(sheet);
            item.dirty = true;
            cx.notify();
        });
    }

    fn apply_op(&mut self, op: &SheetOp, cx: &mut Context<Self>, forward: bool) {
        match op {
            SheetOp::Cells(changes) => {
                self.sheet_update(cx, |sheet| {
                    for (at, old, new) in changes {
                        let target = if forward { new } else { old };
                        match target {
                            Some(cell) => sheet.set_cell(*at, cell.clone()),
                            None => sheet.set_cell(*at, Cell::default()),
                        }
                    }
                });
            }
            SheetOp::Col(idx, old, new) => {
                let size = if forward { *new } else { *old };
                self.sheet_update(cx, |sheet| {
                    let info = sheet.cols.entry(*idx).or_default();
                    info.size = size;
                    info.custom = true;
                });
            }
            SheetOp::Row(idx, old, new) => {
                let size = if forward { *new } else { *old };
                self.sheet_update(cx, |sheet| {
                    let info = sheet.rows.entry(*idx).or_default();
                    info.size = size;
                    info.custom = true;
                });
            }
        }
    }

    fn undo(&mut self, cx: &mut Context<Self>) {
        let Some(op) = self.undo_stack.pop() else { return };
        self.apply_op(&op, cx, false);
        self.redo_stack.push(op);
    }

    fn redo(&mut self, cx: &mut Context<Self>) {
        let Some(op) = self.redo_stack.pop() else { return };
        self.apply_op(&op, cx, true);
        self.undo_stack.push(op);
    }

    fn copy_selection(&self, cx: &mut App) {
        let (r0, r1, c0, c1) = self.sel_range();
        let tsv = self.tsv_of_range(r0, r1, c0, c1, cx);
        cx.write_to_clipboard(gpui::ClipboardItem::new_string(tsv));
    }

    fn paste_clipboard(&mut self, cx: &mut Context<Self>) -> u32 {
        let Some(clip) = cx.read_from_clipboard() else { return 0 };
        let Some(text) = clip.text() else { return 0 };
        if text.is_empty() {
            return 0;
        }
        let (sel_r0, sel_r1, sel_c0, sel_c1) = self.sel_range();
        let clip: Vec<Vec<&str>> = text
            .split('\n')
            .map(|line| line.trim_end_matches('\r').split('\t').collect())
            .collect();
        let clip_rows = clip.len();
        let clip_cols = clip.iter().map(|row| row.len()).max().unwrap_or(0);
        // Excel: a single copied cell fills the whole selected range
        let fill = clip_rows == 1 && clip_cols == 1 && (sel_r1 > sel_r0 || sel_c1 > sel_c0);
        let (start_row, start_col) = (self.sel_row, self.sel_col);
        let (last_row, last_col) = self.used_bounds(cx);
        let mut changes = Vec::new();
        let mut last = (start_row, start_col);
        let mut skipped = 0u32;
        self.sheet_update(cx, |sheet| {
            let tr1 = if fill { sel_r1 } else { start_row + clip_rows as u32 - 1 };
            let tc1 = if fill { sel_c1 } else { start_col + clip_cols as u32 - 1 };
            for row in sel_r0.min(start_row)..=tr1 {
                for col in sel_c0.min(start_col)..=tc1 {
                    let part = if fill {
                        clip[0][0]
                    } else {
                        let ri = (row - start_row) as usize;
                        let ci = (col - start_col) as usize;
                        match clip.get(ri).and_then(|r| r.get(ci)) {
                            Some(v) => *v,
                            None => continue,
                        }
                    };
                    if row > last_row || col > last_col {
                        skipped += 1;
                        continue;
                    }
                    let at = CellRef::new(row, col);
                    let old = sheet.cell(at).cloned();
                    let style = sheet.style_id(at);
                    sheet.set_cell(at, parse_cell_text(part, style));
                    changes.push((at, old, sheet.cell(at).cloned()));
                    last = (row, col);
                }
            }
        });
        if !changes.is_empty() {
            self.push_op(SheetOp::Cells(changes));
            self.anchor = Some((start_row, start_col));
            self.sel_row = last.0;
            self.sel_col = last.1;
            cx.notify();
        }
        skipped
    }

    fn clear_selection(&mut self, cx: &mut Context<Self>) {
        let (r0, r1, c0, c1) = self.sel_range();
        let mut changes = Vec::new();
        self.sheet_update(cx, |sheet| {
            for row in r0..=r1 {
                for col in c0..=c1 {
                    let at = CellRef::new(row, col);
                    let old = sheet.cell(at).cloned();
                    sheet.set_cell(at, Cell::default());
                    changes.push((at, old, None));
                }
            }
        });
        if !changes.is_empty() {
            self.push_op(SheetOp::Cells(changes));
        }
    }

    fn cut_selection(&mut self, cx: &mut Context<Self>) {
        self.copy_selection(cx);
        self.clear_selection(cx);
    }

    /// Screen rect of the cell currently being edited, without visibility checks.
    fn edit_cell_rect(&self, cx: &App) -> Option<(f32, f32, f32, f32)> {
        let item = self.item.read(cx);
        let wb = item.wb.as_ref()?;
        let sheet = wb.sheets.get(self.sheet)?;
        let (sx, sy) = self.scroll.pos();
        let left = LEFT_W + (sheet.col_left(self.edit_col) as f32 - sx) * self.zoom;
        let top = TOP_H + (sheet.row_top(self.edit_row) as f32 - sy) * self.zoom;
        let width = sheet.col_width(self.edit_col) * self.zoom;
        let height = sheet.row_height(self.edit_row) * self.zoom;
        Some((left, top, width, height))
    }

    /// Caret bar position inside the edit overlay, measured by text shaping.
    fn edit_lines(&self, inner_w: f32, window: &Window) -> Vec<String> {
        let ts = window.text_system();
        let font = &self.edit_font_cur;
        let size = px(self.edit_font_px.max(6.0));
        let mut out = Vec::new();
        for para in self.edit_buffer.split('\n') {
            let mut cur = String::new();
            let mut curw = 0.0f32;
            for word in para.split_inclusive(' ') {
                let w = shape_width(ts, word, font, size);
                if curw + w > inner_w && !cur.is_empty() { out.push(cur.clone()); cur.clear(); curw = 0.0; }
                cur.push_str(word);
                curw += w;
            }
            out.push(cur);
        }
        out
    }

    fn caret_pos(&self, left: f32, top: f32, width: f32, font_size: f32, window: &Window) -> (f32, f32) {
        let ts = window.text_system();
        let font = &self.edit_font_cur;
        let size = px(font_size);
        let inner_w = (width - 6.0).max(10.0);
        let line_h = font_size * 1.25;
        let lines = self.edit_lines(inner_w, window);
        let mut pos = 0usize;
        for (idx, line) in lines.iter().enumerate() {
            let lc = line.chars().count();
            if self.edit_caret <= pos + lc {
                let prefix: String = self.edit_buffer.chars().skip(pos).take(self.edit_caret - pos).collect();
                let w = shape_width(ts, &prefix, font, size);
                return (left + 3.0 + w, top + 1.0 + idx as f32 * line_h);
            }
            pos += lc;
            if self.edit_buffer.chars().nth(pos) == Some('\n') { pos += 1; }
        }
        (left + 3.0, top + 1.0 + lines.len().saturating_sub(1) as f32 * line_h)
    }

    /// Map a click inside the edit overlay to a caret offset using the same wrapped layout as the render.
    fn click_caret(&self, x: f32, y: f32, _width: f32, _window: &Window, _cx: &App) -> usize {
        let n = self.edit_buffer.chars().count();
        for (ly, lh, words) in &self.edit_layout {
            if y >= *ly && y < *ly + *lh {
                for (cs, ce, wx, ww, xs) in words {
                    if x >= *wx && x < *wx + *ww {
                        let rel = x - wx;
                        let mut k = xs.iter().rposition(|&v| v <= rel).unwrap_or(0);
                        if k + 1 < xs.len() && (rel - xs[k]) > (xs[k + 1] - rel) { k += 1; }
                        return (cs + k).min(*ce).min(n);
                    }
                }
                if let Some(last) = words.last() { return last.1.min(n); }
                if let Some(first) = words.first() { return first.0.min(n); }
            }
        }
        if y < 0.0 { return 0; }
        n
    }

    fn zoom_reset(&mut self, cx: &mut Context<Self>) {
        self.set_zoom(1.0, cx);
    }

    fn select_all(&mut self, cx: &mut Context<Self>) {
        let (last_row, last_col) = self.used_bounds(cx);
        self.anchor = Some((0, 0));
        self.sel_row = last_row;
        self.sel_col = last_col;
        cx.notify();
    }

    fn autofit(&mut self, axis: u8, idx: u32, cx: &mut Context<Self>) {
        let (last_row, last_col) = self.used_bounds(cx);
        let size = if axis == 1 {
            let mut widest = 40.0f32;
            for row in 0..=last_row {
                let len = self.cell_bar_text(row, idx, cx).chars().count() as f32;
                widest = widest.max(len * 8.0 + 12.0);
            }
            widest.min(800.0)
        } else {
            let mut lines = 1.0f32;
            for col in 0..=last_col {
                let len = self.cell_bar_text(idx, col, cx).chars().count() as f32;
                let cap = (self.col_width_at(col, cx) / 8.0).max(4.0);
                lines = lines.max((len / cap).ceil());
            }
            (lines * 20.0 + 8.0).min(600.0)
        };
        let old = self.line_size(axis, idx, cx);
        self.apply_resize(axis, idx, size, cx);
        self.push_op(if axis == 1 { SheetOp::Col(idx, old, Some(size)) } else { SheetOp::Row(idx, old, Some(size)) });
    }

    fn line_size(&self, axis: u8, idx: u32, cx: &App) -> Option<f32> {
        let item = self.item.read(cx);
        let wb = item.wb.as_ref()?;
        let sheet = wb.sheets.get(self.sheet)?;
        if axis == 1 { sheet.cols.get(&idx).and_then(|i| i.size) } else { sheet.rows.get(&idx).and_then(|i| i.size) }
    }

    fn col_width_at(&self, col: u32, cx: &App) -> f32 {
        let item = self.item.read(cx);
        item.wb
            .as_ref()
            .and_then(|wb| wb.sheets.get(self.sheet))
            .map(|sh| sh.col_width(col))
            .unwrap_or(64.0)
    }

    fn apply_resize(&mut self, axis: u8, idx: u32, size: f32, cx: &mut Context<Self>) {
        self.item.update(cx, |item, cx| {
            let Some(wb) = item.wb.as_mut() else { return };
            let Some(arc_sheet) = wb.sheets.get_mut(self.sheet) else { return };
            let sheet = Arc::make_mut(arc_sheet);
            if axis == 0 {
                let info = sheet.rows.entry(idx).or_default();
                info.size = Some(size);
                info.custom = true;
            } else {
                let info = sheet.cols.entry(idx).or_default();
                info.size = Some(size);
                info.custom = true;
            }
            item.dirty = true;
            cx.notify();
        });
    }

    fn set_zoom(&mut self, zoom: f32, cx: &mut Context<Self>) {
        self.zoom = zoom.clamp(ZOOM_MIN, ZOOM_MAX);
        cx.notify();
    }

    fn cell_bar_text(&self, row: u32, col: u32, cx: &App) -> String {
        let item = self.item.read(cx);
        let Some(wb) = item.wb.as_ref() else {
            return String::new();
        };
        let Some(sheet) = wb.sheets.get(self.sheet) else {
            return String::new();
        };
        sheet
            .cell(CellRef::new(row, col))
            .map(|cell| cell.bar_text())
            .unwrap_or_default()
    }

    fn bar_text(&self, cx: &App) -> String {
        self.cell_bar_text(self.sel_row, self.sel_col, cx)
    }

    fn toggle_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.search_open = !self.search_open;
        if self.search_open {
            if self.search_editor.is_none() {
                let editor = cx.new(|cx| Editor::single_line(window, cx));
                cx.subscribe(&editor, |this, _ed, ev, cx| {
                    if matches!(ev, editor::EditorEvent::BufferEdited) {
                        this.recompute_search(cx);
                    }
                }).detach();
                self.search_editor = Some(editor);
            }
            self.focus_search_on_render = true;
            self.recompute_search(cx);
        } else {
            self.search_editor = None;
            self.search_matches.clear();
            self.search_query.clear();
            window.focus(&self.focus_handle, cx);
        }
        cx.notify();
    }

    fn recompute_search(&mut self, cx: &mut Context<Self>) {
        let q = self.search_editor.as_ref().map(|e| e.read(cx).text(cx)).unwrap_or_default().trim().to_lowercase();
        self.search_query = q.clone();
        self.search_matches.clear();
        self.search_current = 0;
        if !q.is_empty() {
            let (last_row, last_col) = self.used_bounds(cx);
            if let Some(sheet) = self.item.read(cx).wb.as_ref().and_then(|wb| wb.sheets.get(self.sheet).cloned()) {
                for row in 0..=last_row {
                    for col in 0..=last_col {
                        let text = sheet.cell(CellRef::new(row, col)).map(|c| c.bar_text()).unwrap_or_default();
                        for (ms, me) in crate::slides_viewer::scan_char_matches(&text, &q) {
                            self.search_matches.push((row, col, ms, me));
                        }
                    }
                }
            }
        }
        if !self.search_matches.is_empty() {
            self.jump_to_match(cx);
        }
        cx.notify();
    }

    fn step_match(&mut self, delta: isize, cx: &mut Context<Self>) {
        let n = self.search_matches.len();
        if n == 0 { return; }
        self.search_current = (self.search_current as isize + delta).rem_euclid(n as isize) as usize;
        self.jump_to_match(cx);
        cx.notify();
    }

    fn search_next(&mut self, cx: &mut Context<Self>) { self.step_match(1, cx); }
    fn search_prev(&mut self, cx: &mut Context<Self>) { self.step_match(-1, cx); }

    fn jump_to_match(&mut self, cx: &mut Context<Self>) {
        let Some(&(row, col, _, _)) = self.search_matches.get(self.search_current) else { return };
        self.anchor = None;
        self.sel_row = row;
        self.sel_col = col;
        let item = self.item.read(cx);
        if let Some(wb) = item.wb.as_ref() {
            if let Some(sheet) = wb.sheets.get(self.sheet) {
                let (sx, sy) = self.scroll.pos();
                let view_w = (f32::from(self.grid_size.x) - LEFT_W).max(50.0) / self.zoom;
                let view_h = (f32::from(self.grid_size.y) - TOP_H).max(50.0) / self.zoom;
                let cl = sheet.col_left(col) as f32;
                let cw = sheet.col_width(col);
                let rt = sheet.row_top(row) as f32;
                let rh = sheet.row_height(row);
                let mut nx = sx;
                let mut ny = sy;
                if cl < nx { nx = cl; } else if cl + cw > nx + view_w { nx = cl + cw - view_w; }
                if rt < ny { ny = rt; } else if rt + rh > ny + view_h { ny = rt + rh - view_h; }
                self.scroll.set_pos(nx, ny);
            }
        }
    }

    fn start_edit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.item.read(cx).wb.is_none() || self.editing {
            return;
        }
        let text = self.bar_text(cx);
        self.edit_row = self.sel_row;
        self.edit_col = self.sel_col;
        self.edit_buffer = text.clone();
        self.edit_caret = self.edit_buffer.chars().count();
        self.edit_sel_anchor = None;
        self.caret_on = true;
        if let Some((_, _, _, _)) = self.edit_cell_rect(cx) {
            let item = self.item.read(cx);
            if let Some(wb) = item.wb.as_ref() {
                if let Some(sheet) = wb.sheets.get(self.sheet) {
                    let st = wb
                        .styles
                        .get(sheet.style_id(CellRef::new(self.edit_row, self.edit_col)));
                    self.edit_font_px = (st.font.size * 4.0 / 3.0 * self.zoom).clamp(8.0, 40.0);
                    self.edit_font_cur = edit_font(&st.font.name, st.font.bold, st.font.italic);
                    self.edit_align = match st.align.h { gridcraft_model::style::HAlign::Center | gridcraft_model::style::HAlign::CenterAcross => 1, gridcraft_model::style::HAlign::Right | gridcraft_model::style::HAlign::Fill | gridcraft_model::style::HAlign::Justify => 2, _ => 0 };
                }
            }
        }
        self.edit_runs = {
            let item = self.item.read(cx);
            let mut found = None;
            if let Some(wb) = item.wb.as_ref() {
                if let Some(rs) = wb.rich.iter().find(|r| r.text == text) { found = Some(rs.runs.clone()); }
            }
            found.unwrap_or_else(|| vec![RichRun { text: text.clone(), ..Default::default() }])
        };
        self.edit_prev = text.clone();
        self.edit_undo = Some((text.clone(), self.edit_caret, self.edit_sel_anchor, self.edit_runs.clone()));
        self.editing = true;
        let fh = self.focus_handle.clone();
        window.on_next_frame(move |window, cx| { window.focus(&fh, cx); });
        cx.notify();
    }

    fn commit_edit(&mut self, cx: &mut Context<Self>) {
        if !self.editing {
            cx.notify();
            return;
        }
        let text = self.edit_buffer.clone();
        self.cell_editor = None;
        self.edit_undo = None;
        self.editing = false;
        self.refocus_root = true;
        self.caret_task = None;
        self.edit_drag = false;
        self.item.update(cx, |item, cx| {
            let changed = match item.wb.as_mut().and_then(|wb| wb.sheets.get_mut(self.sheet)) {
                Some(arc_sheet) => {
                    let sheet = Arc::make_mut(arc_sheet);
                    let at = CellRef::new(self.edit_row, self.edit_col);
                    let current = sheet.cell(at).map(|cell| cell.bar_text()).unwrap_or_default();
                    if current == text {
                        false
                    } else {
                        let style = sheet.style_id(at);
                        let old = sheet.cell(at).cloned();
                        let new = parse_cell_text(&text, style);
                        sheet.set_cell(at, new.clone());
                        let wb = item.wb.as_mut().unwrap();
                        wb.rich.retain(|r| r.text != current && r.text != text);
                        let has_ov = self.edit_runs.iter().any(|r| r.bold || r.italic || r.underline || r.strike || r.color.is_some() || r.size.is_some() || r.font.is_some());
                        if has_ov && !text.is_empty() { wb.rich.push(RichString { text: text.clone(), runs: self.edit_runs.clone() }); }
                        self.undo_stack.push(SheetOp::Cells(vec![(at, old, Some(new))]));
                        self.redo_stack.clear();
                        item.dirty = true;
                        true
                    }
                }
                None => false,
            };
            if changed {
                cx.notify();
            }
            changed
        });
        cx.notify();
    }

    fn cancel_edit(&mut self, cx: &mut Context<Self>) {
        if !self.editing {
            return;
        }
        self.cell_editor = None;
        self.edit_undo = None;
        self.edit_buffer.clear();
        self.edit_caret = 0;
        self.edit_sel_anchor = None;
        self.editing = false;
        self.refocus_root = true;
        self.caret_task = None;
        self.edit_drag = false;
        cx.notify();
    }

    fn edit_newline(&mut self, cx: &mut Context<Self>) {
        if !self.editing {
            return;
        }
        let mut chars: Vec<char> = self.edit_buffer.chars().collect();
        let caret = self.edit_caret.min(chars.len());
        chars.insert(caret, '\n');
        self.edit_caret = caret + 1;
        self.edit_sel_anchor = Some(self.edit_caret);
        self.edit_buffer = chars.into_iter().collect();
        cx.notify();
    }

    fn edit_sel_range(&self) -> Option<(usize, usize)> {
        self.edit_sel_anchor.map(|a| (a.min(self.edit_caret), a.max(self.edit_caret))).filter(|(a, b)| a < b)
    }

    fn edit_set_text(&mut self, new: String, caret: usize, anchor: Option<usize>, cx: &mut Context<Self>) {
        splice_runs(&mut self.edit_runs, &self.edit_buffer, &new);
        self.edit_prev = new.clone();
        self.edit_buffer = new;
        self.edit_caret = caret;
        self.edit_sel_anchor = anchor;
        cx.notify();
    }

    fn edit_insert(&mut self, text: &str, cx: &mut Context<Self>) {
        let chars: Vec<char> = self.edit_buffer.chars().collect();
        let n = chars.len();
        let (s, e) = self.edit_sel_range().unwrap_or((self.edit_caret.min(n), self.edit_caret.min(n)));
        let mut nc: Vec<char> = chars[..s].to_vec();
        nc.extend(text.chars());
        nc.extend_from_slice(&chars[e..]);
        let caret = s + text.chars().count();
        self.edit_set_text(nc.into_iter().collect(), caret, None, cx);
    }

    fn edit_delete(&mut self, back: bool, cx: &mut Context<Self>) {
        let chars: Vec<char> = self.edit_buffer.chars().collect();
        let n = chars.len();
        let (s, e, caret) = if let Some((s, e)) = self.edit_sel_range() {
            (s, e, s)
        } else if back {
            let c = self.edit_caret.min(n);
            if c == 0 { return; }
            (c - 1, c, c - 1)
        } else {
            let c = self.edit_caret.min(n);
            if c == n { return; }
            (c, c + 1, c)
        };
        let mut nc: Vec<char> = chars[..s].to_vec();
        nc.extend_from_slice(&chars[e..]);
        self.edit_set_text(nc.into_iter().collect(), caret, None, cx);
    }

    fn edit_move(&mut self, dir: &str, shift: bool, _window: &Window, cx: &mut Context<Self>) {
        let n = self.edit_buffer.chars().count();
        let mut caret = self.edit_caret.min(n);
        let anchor = if shift { self.edit_sel_anchor.or(Some(self.edit_caret)) } else { None };
        let lay = &self.edit_layout;
        let cur = lay.iter().rposition(|(_, _, w)| w.first().map(|f| f.0 <= caret).unwrap_or(false)).unwrap_or(0);
        let first = lay[cur].2.first().map(|f| f.0).unwrap_or(0);
        let last = lay[cur].2.last().map(|f| f.1).unwrap_or(first);
        let col = caret.saturating_sub(first);
        match dir {
            "left" => caret = caret.saturating_sub(1),
            "right" => caret = (caret + 1).min(n),
            "home" => caret = first,
            "end" => caret = last,
            "up" => if cur > 0 { let t = &lay[cur - 1].2; let tf = t.first().map(|f| f.0).unwrap_or(0); let tl = t.last().map(|f| f.1).unwrap_or(tf); caret = (tf + col).min(tl); },
            "down" => if cur + 1 < lay.len() { let t = &lay[cur + 1].2; let tf = t.first().map(|f| f.0).unwrap_or(0); let tl = t.last().map(|f| f.1).unwrap_or(tf); caret = (tf + col).min(tl); },
            _ => {}
        }
        self.edit_caret = caret.min(n);
        self.edit_sel_anchor = anchor;
        cx.notify();
    }

    fn edit_key(&mut self, ev: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let k = &ev.keystroke;
        let m = k.modifiers;
        let key = k.key.as_str();
        if m.control || m.platform {
            match key {
                "a" => { self.edit_sel_anchor = Some(0); self.edit_caret = self.edit_buffer.chars().count(); cx.notify(); return true; }
                "c" | "x" => {
                    if let Some((s, e)) = self.edit_sel_range() {
                        let t: String = self.edit_buffer.chars().skip(s).take(e - s).collect();
                        cx.write_to_clipboard(gpui::ClipboardItem::new_string(t));
                        if key == "x" { self.edit_delete(false, cx); }
                    }
                    return true;
                }
                "v" => { if let Some(item) = cx.read_from_clipboard() { if let Some(t) = item.text() { self.edit_insert(&t, cx); } } return true; }
                // Excel-style in-cell Ctrl+Z reverts the whole cell edit in one step
                "z" if !m.shift => {
                    if let Some((text, caret, anchor, runs)) = self.edit_undo.clone() {
                        self.edit_buffer = text.clone();
                        self.edit_prev = text;
                        self.edit_caret = caret;
                        self.edit_sel_anchor = anchor;
                        self.edit_runs = runs;
                        cx.notify();
                    }
                    return true;
                }
                _ => return false,
            }
        }
        match key {
            "enter" | "tab" => { self.commit_edit(cx); true }
            "escape" => { self.cancel_edit(cx); true }
            "backspace" => { self.edit_delete(true, cx); true }
            "space" => { self.edit_insert(" ", cx); true }
            "delete" => { self.edit_delete(false, cx); true }
            "left" | "right" | "up" | "down" | "home" | "end" => { self.edit_move(key, m.shift, window, cx); true }
            _ => {
                if key.chars().count() == 1 {
                    let ch = key.chars().next().unwrap();
                    let ch = if m.shift { ch.to_uppercase().next().unwrap_or(ch) } else { ch };
                    self.edit_insert(&ch.to_string(), cx);
                    true
                } else { false }
            }
        }
    }

    fn show_toast(&self, ws: Option<Entity<Workspace>>, cx: &mut Context<Self>, message: String) {
        let Some(ws) = ws else { return };
        ws.update(cx, |ws, cx| {
            ws.show_toast(Toast::new(NotificationId::unique::<Self>(), message), cx);
        });
    }

    /// Serializing and writing the workbook happens off the UI thread: a 60k-row
    /// sheet would freeze the window for seconds otherwise.
    fn save(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.editing {
            self.commit_edit(cx);
        }
        if self.saving {
            return;
        }
        let Some(wb) = self.item.read(cx).wb.as_ref() else {
            return;
        };
        let wb = wb.clone();
        let abs = self.item.read(cx).abs_path.clone();
        self.saving = true;
        // serialize and write off the main thread, then finish state on it
        let abs_for_write = abs.clone();
        let written = cx.background_executor().spawn(async move {
            save_workbook_atomic(&abs_for_write, &write_xlsx(&wb)?)
        });
        // same notification shape zed uses for editor save failures
        cx.spawn(async move |this, cx| {
            let written = written.await;
            let _ = this.update(cx, |this, cx| {
                this.saving = false;
                match &written {
                    Ok(()) => {
                        this.self_write = Some(Instant::now());
                        this.loaded_stamp = crate::disk_stamp(&abs);
                        this.item.update(cx, |item, cx| {
                            item.dirty = false;
                            cx.notify();
                        });
                    }
                    Err(error) => log::error!("sheet_viewer: save failed: {error:#}"),
                }
                cx.notify();
            });
            written
        })
        .detach_and_prompt_err("Failed to save", window, cx, |err, _, _| Some(format!("{err}")));
    }

    /// Excel auto-fits rows to wrapped text when no explicit height is stored; mirror that (spread-style estimate).
    fn autofit_display_rows(wb: &mut Workbook) {
        for arc_sheet in wb.sheets.iter_mut() {
            let sheet = Arc::make_mut(arc_sheet);
            let Some(range) = sheet.used_range() else { continue };
            let (last_row, last_col) = (range.end.row, range.end.col);
            let mut computed: Vec<(u32, f32)> = Vec::new();
            for row in 0..=last_row {
                if sheet.rows.get(&row).is_some_and(|i| i.custom || i.size.is_some()) {
                    continue;
                }
                let mut max_h = 0.0f32;
                for col in 0..=last_col {
                    let at = CellRef::new(row, col);
                    let merge = sheet.merge_at(at);
                    if merge.is_some_and(|m| m.start != at) {
                        continue;
                    }
                    let Some(cell) = sheet.cell(at) else { continue };
                    let text = cell.bar_text();
                    if text.is_empty() {
                        continue;
                    }
                    let style = wb.styles.get(sheet.style_id(at));
                    if !style.align.wrap && !text.contains('\n') {
                        continue;
                    }
                    let width = match merge {
                        Some(m) => (sheet.col_left(m.end.col + 1) - sheet.col_left(m.start.col)) as f32,
                        None => sheet.col_width(col),
                    };
                    let fs = style.font.size * office_knob_f("sheet.fontScale", 1.25) as f32;
                    let cw = (fs * 0.5).max(4.0);
                    let avail = (width - 8.0).max(cw);
                    let mut lines = 0usize;
                    for seg in text.split('\n') {
                        let (mut cur, mut n) = (0.0f32, 1usize);
                        for _ in seg.chars() {
                            if cur > 0.0 && cur + cw > avail {
                                n += 1;
                                cur = 0.0;
                            }
                            cur += cw;
                        }
                        lines += n;
                    }
                    max_h = max_h.max(lines as f32 * fs * 1.2 + 4.0);
                }
                if max_h > sheet.default_row_height {
                    computed.push((row, max_h));
                }
            }
            for (row, h) in computed {
                sheet.rows.entry(row).or_default().size = Some(h);
            }
        }
    }

    fn content_size(&self, cx: &App) -> (f32, f32) {
        let item = self.item.read(cx);
        let Some(wb) = item.wb.as_ref() else {
            return (0.0, 0.0);
        };
        let Some(sheet) = wb.sheets.get(self.sheet) else {
            return (0.0, 0.0);
        };
        let (last_row, last_col) = self.used_bounds(cx);
        (
            sheet.col_left(last_col + 1) as f32,
            sheet.row_top(last_row + 1) as f32,
        )
    }

    fn on_wheel(&mut self, event: &ScrollEvent, cx: &mut Context<Self>) {
        if event.modifiers.control {
            let step = office_knob_f("zoomStep", 10.0) as f32 / 100.0;
            let factor = if f32::from(event.delta.pixel_delta(px(1.0)).y) > 0.0 {
                1.0 + step
            } else {
                1.0 / (1.0 + step)
            };
            self.set_zoom(self.zoom * factor, cx);
            return;
        }
        let delta = event.delta.pixel_delta(px(24.0));
        let (dx, dy) = if event.modifiers.shift {
            (f32::from(delta.y), 0.0)
        } else {
            (f32::from(delta.x), f32::from(delta.y))
        };
        let (sx, sy) = self.scroll.pos();
        self.scroll.set_pos(sx - dx / self.zoom, sy - dy / self.zoom);
        cx.notify();
    }

    fn cell_at(&self, pos: Point<gpui::Pixels>, cx: &App) -> Option<(u32, u32)> {
        let item = self.item.read(cx);
        let wb = item.wb.as_ref()?;
        let sheet = wb.sheets.get(self.sheet)?;
        let x = f32::from(pos.x - self.grid_origin.x);
        let y = f32::from(pos.y - self.grid_origin.y);
        if x < LEFT_W || y < TOP_H {
            return None;
        }
        let (sx, sy) = self.scroll.pos();
        let content_x = ((x - LEFT_W) / self.zoom + sx) as f64;
        let content_y = ((y - TOP_H) / self.zoom + sy) as f64;
        Some((sheet.row_at(content_y), sheet.col_at(content_x)))
    }
}

impl gpui::EventEmitter<()> for SheetView {}

impl Focusable for SheetView {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl ProjectItem for SheetView {
    type Item = SheetItem;

    fn for_project_item(
        project: Entity<Project>,
        _pane: Option<&Pane>,
        item: Entity<Self::Item>,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        Self::new(item, &project, cx)
    }
}

impl Item for SheetView {
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
            .unwrap_or_else(|| "Spreadsheet".to_string())
            .into()
    }

    fn tab_content(&self, params: TabContentParams, _window: &Window, cx: &App) -> gpui::AnyElement {
        Label::new(self.tab_content_text(params.detail.unwrap_or_default(), cx))
            .single_line()
            .color(params.text_color())
            .into_any_element()
    }

    fn is_dirty(&self, cx: &App) -> bool {
        self.item.read(cx).dirty
    }
}

impl SerializableItem for SheetView {
    fn serialized_item_kind() -> &'static str {
        "SheetView"
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
        let db = persistence::SheetViewerDb::global(cx);
        window.spawn(cx, async move |cx| {
            let (path, sheet, zoom) = match db.get_sheet(item_id, workspace_id)? {
                Some(row) => row,
                None => anyhow::bail!("no sheet path in db"),
            };
            let project_path =
                project.update(cx, |project, cx| project.find_project_path(path, cx));
            let Some(project_path) = project_path else {
                anyhow::bail!("sheet path not in project");
            };
            let item_task = cx.update(|_window, cx| {
                use project::ProjectItem as _;
                SheetItem::try_open(&project, &project_path, cx)
            })?;
            let Some(item_task) = item_task else {
                anyhow::bail!("sheet try_open returned none");
            };
            let item = item_task.await?;
            Ok(cx.update(|_window, cx| {
                cx.new(|cx| {
                    let mut view = SheetView::new(item, &project, cx);
                    view.sheet = sheet as usize;
                    view.zoom = zoom.clamp(ZOOM_MIN as f64, ZOOM_MAX as f64) as f32;
                    view.clamp_sel(cx);
                    view
                })
            })?)
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
        let sheet = self.sheet as i64;
        let zoom = self.zoom as f64;
        let db = persistence::SheetViewerDb::global(cx);
        Some(cx.background_spawn(async move {
            db.save_sheet(item_id, workspace_id, path, sheet, zoom).await
        }))
    }

    fn should_serialize(&self, _event: &()) -> bool {
        true
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

    pub struct SheetViewerDb(ThreadSafeConnection);

    impl Domain for SheetViewerDb {
        const NAME: &str = stringify!(SheetViewerDb);

        const MIGRATIONS: &[&str] = &[sql!(
            CREATE TABLE sheet_viewers (
                workspace_id INTEGER,
                item_id INTEGER UNIQUE,
                sheet_path BLOB,
                sheet INTEGER,
                zoom REAL,
                PRIMARY KEY(workspace_id, item_id),
                FOREIGN KEY(workspace_id) REFERENCES workspaces(workspace_id)
                ON DELETE CASCADE
            ) STRICT;
        )];
    }

    db::static_connection!(SheetViewerDb, [WorkspaceDb]);

    impl SheetViewerDb {
        query! {
            pub async fn save_sheet(
                item_id: ItemId,
                workspace_id: WorkspaceId,
                sheet_path: PathBuf,
                sheet: i64,
                zoom: f64
            ) -> Result<()> {
                INSERT OR REPLACE INTO sheet_viewers(item_id, workspace_id, sheet_path, sheet, zoom)
                VALUES (?, ?, ?, ?, ?)
            }
        }

        query! {
            pub fn get_sheet(item_id: ItemId, workspace_id: WorkspaceId) -> Result<Option<(PathBuf, i64, f64)>> {
                SELECT sheet_path, sheet, zoom
                FROM sheet_viewers
                WHERE item_id = ? AND workspace_id = ?
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn col_names_and_refs() {
        assert_eq!(col_name(0), "A");
        assert_eq!(col_name(25), "Z");
        assert_eq!(col_name(26), "AA");
        assert_eq!(col_name(701), "ZZ");
        assert_eq!(col_name(702), "AAA");
        assert_eq!(cell_ref_text(0, 0), "A1");
        assert_eq!(cell_ref_text(99, 26), "AA100");
    }

    #[test]
    fn tint_blends_towards_white_or_black() {
        assert_eq!(theme_tint(0x808080, 0), 0x808080);
        assert_eq!(theme_tint(0x000000, 1000), 0xFFFFFF);
        assert_eq!(theme_tint(0xFFFFFF, -1000), 0x000000);
    }

    #[test]
    fn cell_text_kinds() {
        let d = StyleId::DEFAULT;
        assert!(matches!(parse_cell_text("  ", d).value, Value::Empty));
        assert!(parse_cell_text("=SUM(A1:A2)", d).formula.is_some());
        assert!(matches!(parse_cell_text("TRUE", d).value, Value::Bool(true)));
        assert!(matches!(parse_cell_text("false", d).value, Value::Bool(false)));
        match parse_cell_text("42.5", d).value {
            Value::Number(n) => assert_eq!(n, 42.5),
            other => panic!("expected number, got {other:?}"),
        }
        assert!(matches!(parse_cell_text("hello", d).value, Value::Text(_)));
    }

    #[test]
    fn workbook_save_replaces_target_and_cleans_temp_up() {
        let dir = std::env::temp_dir().join(format!("zepi-sheet-save-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("book.xlsx");
        std::fs::write(&path, b"old").unwrap();
        save_workbook_atomic(&path, b"new").unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"new");
        // a failed rename (target is a directory) must not leave the temp file behind
        let target = dir.join("locked");
        std::fs::create_dir_all(&target).unwrap();
        assert!(save_workbook_atomic(&target, b"new").is_err());
        let leftovers: Vec<_> = std::fs::read_dir(&dir).unwrap().filter_map(std::result::Result::ok)
            .map(|entry| entry.file_name()).filter(|name| name.to_string_lossy().starts_with("~$")).collect();
        assert!(leftovers.is_empty(), "temp files left behind: {leftovers:?}");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
