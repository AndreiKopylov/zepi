//! Headless spreadsheet raster for `zepi office render`: a sheet drawn the way the built-in
//! viewer draws it (geometry, colours, number formats, wrap and alignment), without gpui.

use anyhow::{anyhow, Result};
use gridcraft_core::CellRef;
use gridcraft_model::style::{BorderStyle, HAlign, PatternType, Underline, VAlign};
use gridcraft_model::{Sheet, Style, StyleId, Workbook};
use gridcraft_numfmt::{Formatted, NumberFormat, format_value};
use wordcraft_doc::props::Rgb;
use wordcraft_doc::Document;
use wordcraft_fonts::{FaceRef, FontDb, FontFace, ShapedGlyph};
use wordcraft_geom::Rect;
use wordcraft_layout::display::{Draw, Stroke};
use wordcraft_render::{RenderOptions, Rendered, render_items};

/// Header chrome sizes and colours: the viewer's own values.
const LEFT_W: f32 = 46.0;
const TOP_H: f32 = 22.0;
const GRID: Rgb = Rgb(0xd4, 0xd4, 0xd4);
const HEADER_BG: Rgb = Rgb(0xf0, 0xf0, 0xf0);
const TEXT: Rgb = Rgb(0x20, 0x20, 0x20);
const MUTED: Rgb = Rgb(0x60, 0x60, 0x60);
const BLACK: Rgb = Rgb(0, 0, 0);
const WHITE: Rgb = Rgb(0xff, 0xff, 0xff);

/// Font pt -> px at 96 dpi, the shipped `sheet.fontScale` default; the CLI has no settings.
const PT: f32 = 4.0 / 3.0;
/// Per-image pixel budget: a sheet is drawn 1:1, so a tall one needs more room than a page
/// but a 6000px-tall strip of a 60k-row sheet tells nobody anything.
const MAX_PIXELS: f64 = 4_000_000.0;
/// A sheet that does not fit one image is shown as up to this many sampled windows.
const MAX_WINDOWS: usize = 3;

/// PNG bytes of the workbook's sheets, drawn at `zoom` over the sheet's own 96-dpi pixel
/// geometry (the viewer's 100%): `sheet-N.png` per sheet, or `sheet-N-top.png`,
/// `sheet-N-middle.png`, `sheet-N-bottom.png` when the sheet is too big for one readable image.
/// `--pages` maps to sheets; `cells` (`A1:E40`, each side open: `A100:`, `:E40`) draws exactly
/// that box of every selected sheet instead.
pub fn sheet_pngs(bytes: &[u8], zoom: f64, sheets: Option<&[usize]>, cells: Option<&Cells>) -> Result<Vec<(String, Vec<u8>)>> {
    let (wb, _) = gridcraft_xlsx::read_xlsx(bytes).map_err(|error| anyhow!("xlsx import: {error:?}"))?;
    let zoom = if zoom.is_finite() && zoom > 0.0 { zoom } else { 1.0 };
    let mut out = Vec::new();
    for index in selected(wb.sheets.len(), sheets) {
        let sheet = &wb.sheets[index];
        let (row0, col0, row1, col1) = sheet_bounds(sheet);
        let (draw_col0, draw_col1) = match cells {
            Some(cells) => cells.columns((col0, col1)),
            None => (col0, col1),
        };
        let windows = match cells {
            Some(cells) => vec![cells.within((row0, col0, row1, col1))],
            None => {
                let (col1, width) = fit_columns(sheet, col0, col1, zoom);
                let window_rows = rows_per_image(sheet, row0, row1, width, zoom);
                let windows = row_windows(sheet, (row0, col0, row1, col1), window_rows);
                if windows.len() > 1 {
                    eprintln!(
                        "sheet \"{}\": {} rows, shown as {} windows of up to {} rows (a lower --dpi shows more per image, --cells picks another box)",
                        sheet.name,
                        row1 - row0 + 1,
                        windows.len(),
                        window_rows,
                    );
                }
                windows
            }
        };
        let suffixes: &[&str] = match windows.len() {
            1 => &[""],
            2 => &["-top", "-bottom"],
            _ => &["-top", "-middle", "-bottom"],
        };
        for (window, suffix) in windows.into_iter().zip(suffixes) {
            let rendered = render_sheet(&wb, sheet, (window.0, draw_col0, window.1, draw_col1), zoom)?;
            let png = rendered.to_png();
            if png.is_empty() {
                anyhow::bail!("sheet {} did not render", index + 1);
            }
            out.push((format!("sheet-{}{suffix}.png", index + 1), png));
        }
    }
    Ok(out)
}

/// A box of cells from `--cells`: `B10:E40`, or with either side open (`A100:`, `:E40`, `A:E`),
/// each bound optional and taken from the sheet's used range when left out.
#[derive(Clone, Debug, Default)]
pub struct Cells {
    row0: Option<u32>,
    col0: Option<u32>,
    row1: Option<u32>,
    col1: Option<u32>,
}

impl Cells {
    /// `None` for text that is not a cell or cell box.
    pub fn parse(spec: &str) -> Option<Cells> {
        let (start, end) = spec.split_once(':').unwrap_or((spec, spec));
        let (row0, col0) = parse_side(start)?;
        let (row1, col1) = parse_side(end)?;
        if row0.is_none() && col0.is_none() && row1.is_none() && col1.is_none() {
            return None;
        }
        Some(Cells { row0, col0, row1, col1 })
    }

    /// The rows of the box: open sides come from `bounds`, closed ones are taken as asked (so a
    /// box past the used range shows empty cells, as in Excel, instead of silently moving).
    fn within(&self, (row0, _, row1, _): (u32, u32, u32, u32)) -> (u32, u32) {
        ordered(self.row0.unwrap_or(row0), self.row1.unwrap_or(row1), gridcraft_core::MAX_ROWS)
    }

    /// The columns of the box, resolved the same way.
    fn columns(&self, (col0, col1): (u32, u32)) -> (u32, u32) {
        ordered(self.col0.unwrap_or(col0), self.col1.unwrap_or(col1), gridcraft_core::MAX_COLS)
    }
}

/// One side of a `--cells` box: `(row, column)`, `None` where it was left open.
fn parse_side(side: &str) -> Option<(Option<u32>, Option<u32>)> {
    let side = side.trim().trim_start_matches('$');
    if side.is_empty() {
        return Some((None, None));
    }
    if let Some(cell) = CellRef::parse(side) {
        return Some((Some(cell.row), Some(cell.col)));
    }
    let letters: String = side.chars().filter(|c| c.is_ascii_alphabetic()).collect();
    let digits: String = side.chars().filter(|c| c.is_ascii_digit()).collect();
    if letters.is_empty() && digits.is_empty() {
        return None;
    }
    let col = (!letters.is_empty()).then(|| column_of(&letters)).flatten();
    let row = (!digits.is_empty()).then(|| digits.parse::<u32>().ok()).flatten().map(|row| row.saturating_sub(1));
    (col.is_some() || row.is_some()).then_some((row, col))
}

fn column_of(letters: &str) -> Option<u32> {
    let mut col = 0u32;
    for letter in letters.chars() {
        col = col.checked_mul(26)?.checked_add(u32::from(letter.to_ascii_uppercase() as u8 - b'A') + 1)?;
    }
    col.checked_sub(1)
}

/// `(start, end)` in order and inside the sheet's grid.
fn ordered(a: u32, b: u32, limit: u32) -> (u32, u32) {
    let (start, end) = (a.min(b), a.max(b));
    (start.min(limit - 1), end.min(limit - 1))
}

fn selected(count: usize, sheets: Option<&[usize]>) -> Vec<usize> {
    match sheets {
        Some(list) => list.iter().copied().filter(|index| *index < count).collect(),
        None => (0..count).collect(),
    }
}

/// The columns one image shows (the width wins the budget over the rows) and their width.
fn fit_columns(sheet: &Sheet, col0: u32, col1: u32, zoom: f64) -> (u32, f32) {
    let max_side = f64::from(wordcraft_render::MAX_SIDE) / zoom;
    let mut width = f64::from(LEFT_W);
    let mut last = col0;
    for col in col0..=col1 {
        let col_width = f64::from(sheet.col_width(col));
        if width + col_width > max_side && col > col0 {
            break;
        }
        width += col_width;
        last = col;
    }
    if last != col1 {
        eprintln!("sheet \"{}\": columns {}..={} drawn, the sheet has {}", sheet.name, col0 + 1, last + 1, col1 + 1);
    }
    (last, width as f32)
}

/// How many rows of `width` fit one image at `zoom`.
fn rows_per_image(sheet: &Sheet, row0: u32, row1: u32, width: f32, zoom: f64) -> u32 {
    let allowed = (MAX_PIXELS / (f64::from(width) * zoom)).min(f64::from(wordcraft_render::MAX_SIDE) / zoom);
    let mut height = f64::from(TOP_H);
    let mut rows = 0u32;
    for row in row0..=row1 {
        let row_height = f64::from(sheet.row_height(row));
        if height + row_height > allowed && rows > 0 {
            break;
        }
        height += row_height;
        rows += 1;
    }
    rows.max(1)
}

/// Up to [`MAX_WINDOWS`] evenly spaced row ranges of `window_rows` rows each, a merge never cut
/// from its anchor.
fn row_windows(sheet: &Sheet, (row0, col0, row1, col1): (u32, u32, u32, u32), window_rows: u32) -> Vec<(u32, u32)> {
    let total = row1 - row0 + 1;
    let count = (total.div_ceil(window_rows) as usize).clamp(1, MAX_WINDOWS);
    if count == 1 {
        return vec![(row0, row1)];
    }
    let slack = total - window_rows;
    (0..count)
        .map(|index| {
            let mut start = row0 + (u64::from(slack) * index as u64 / (count as u64 - 1)) as u32;
            for m in &sheet.merges {
                if m.end.row >= start && m.start.row < start && m.end.col >= col0 && m.start.col <= col1 {
                    start = m.start.row;
                }
            }
            (start, (start + window_rows - 1).min(row1))
        })
        .collect()
}

fn render_sheet(wb: &Workbook, sheet: &Sheet, (row0, col0, row1, col1): (u32, u32, u32, u32), zoom: f64) -> Result<Rendered> {
    let xs: Vec<f32> = (col0..=col1 + 1).map(|c| sheet.col_left(c) as f32).collect();
    let ys: Vec<f32> = (row0..=row1 + 1).map(|r| sheet.row_top(r) as f32).collect();
    let (gx0, gy0) = (xs[0], ys[0]);
    let (gw, gh) = (xs[xs.len() - 1] - gx0, ys[ys.len() - 1] - gy0);
    let (w, h) = (LEFT_W + gw, TOP_H + gh);
    let scale = fit_zoom(zoom, w, h);
    let db = FontDb::global();
    let mut items: Vec<Draw> = Vec::new();

    // page, header bands and header separators
    items.push(fill_rect(0.0, 0.0, w, h, WHITE));
    items.push(fill_rect(LEFT_W, 0.0, gw, TOP_H, HEADER_BG));
    items.push(fill_rect(0.0, TOP_H, LEFT_W, gh, HEADER_BG));
    for x in &xs {
        items.push(seg(LEFT_W + x - gx0, 0.0, LEFT_W + x - gx0, TOP_H, 1.0, GRID));
    }
    for y in &ys {
        items.push(seg(0.0, TOP_H + y - gy0, LEFT_W, TOP_H + y - gy0, 1.0, GRID));
    }
    items.push(seg(LEFT_W, 0.0, LEFT_W, h, 1.0, GRID));
    items.push(seg(0.0, TOP_H, w, TOP_H, 1.0, GRID));

    // header labels, centered in their cells
    let base = wb.styles.get(StyleId::DEFAULT);
    let head = db.face(&base.font.name, "Regular");
    let head_size = 11.0;
    let k = head_size / head.upem.max(1.0) as f32;
    let (asc, desc) = head.vertical_metrics();
    let mid = (asc as f32 - desc as f32) * k / 2.0;
    for c in col0..=col1 {
        let width = sheet.col_width(c);
        if width <= 0.0 {
            continue;
        }
        let left = LEFT_W + (sheet.col_left(c) as f32 - gx0);
        push_label(&mut items, &head, head_size, MUTED, &col_name(c), left, left + width, TOP_H / 2.0 + mid);
    }
    for r in row0..=row1 {
        let height = sheet.row_height(r);
        if height <= 0.0 {
            continue;
        }
        let top = TOP_H + (sheet.row_top(r) as f32 - gy0);
        push_label(&mut items, &head, head_size, MUTED, &(r + 1).to_string(), 0.0, LEFT_W, top + height / 2.0 + mid);
    }

    // cells: the viewer's drawing rules, merge anchors spanning their range
    for r in row0..=row1 {
        let height = sheet.row_height(r);
        if height <= 0.0 {
            continue;
        }
        for c in col0..=col1 {
            let width = sheet.col_width(c);
            if width <= 0.0 {
                continue;
            }
            let at = CellRef::new(r, c);
            let (x, y, cw, ch) = if let Some(m) = sheet.merge_at(at) {
                if at != m.start {
                    continue;
                }
                let x = LEFT_W + (sheet.col_left(m.start.col) as f32 - gx0);
                let y = TOP_H + (sheet.row_top(m.start.row) as f32 - gy0);
                let cw = (sheet.col_left(m.end.col + 1) - sheet.col_left(m.start.col)) as f32;
                let ch = (sheet.row_top(m.end.row + 1) - sheet.row_top(m.start.row)) as f32;
                (x, y, cw, ch)
            } else {
                (LEFT_W + (sheet.col_left(c) as f32 - gx0), TOP_H + (sheet.row_top(r) as f32 - gy0), width, height)
            };
            let style = wb.styles.get(sheet.style_id(at));
            if style.fill.pattern == PatternType::Solid {
                items.push(fill_rect(x, y, cw, ch, model_rgb(&style.fill.fg, HEADER_BG, &wb.theme)));
            }
            let b = &style.borders;
            let has_border = !b.top.is_none() || !b.right.is_none() || !b.bottom.is_none() || !b.left.is_none();
            if !has_border {
                items.push(seg(x + cw, y, x + cw, y + ch, 1.0, GRID));
                items.push(seg(x, y + ch, x + cw, y + ch, 1.0, GRID));
            }
            // shared edges drawn once: right/bottom yield to the neighbor's left/top
            let nb_right_left = !wb.styles.get(sheet.style_id(CellRef::new(r, c + 1))).borders.left.is_none();
            let nb_below_top = !wb.styles.get(sheet.style_id(CellRef::new(r + 1, c))).borders.top.is_none();
            for (side, line) in [(0usize, b.top), (1, b.right), (2, b.bottom), (3, b.left)] {
                let width = line.style.width();
                if line.is_none() || width <= 0.0 {
                    continue;
                }
                if side == 1 && nb_right_left || side == 2 && nb_below_top {
                    continue;
                }
                let color = model_rgb(&line.color, BLACK, &wb.theme);
                let (x0, y0, x1, y1) = match side {
                    0 => (x, y, x + cw, y),
                    1 => (x + cw, y, x + cw, y + ch),
                    2 => (x, y + ch, x + cw, y + ch),
                    _ => (x, y, x, y + ch),
                };
                if line.style == BorderStyle::Double {
                    items.extend(double_border(x0, y0, x1, y1, color));
                } else {
                    items.push(seg(x0, y0, x1, y1, width, color));
                }
            }
            let Some(cell) = sheet.cell(at) else { continue };
            let fmt = NumberFormat::parse_cached(&style.num_fmt.0);
            let formatted = format_value(&cell.value, &fmt, wb.date_system);
            if formatted.text.is_empty() {
                continue;
            }
            push_cell_text(wb, style, &formatted, x, y, cw, ch, &mut items);
        }
    }

    let doc = Document::default();
    let area = Rect { x: 0.0, y: 0.0, w, h };
    Ok(render_items(&doc, &items, area, scale, &RenderOptions::default()))
}

/// The sheet's drawn range: the used range, extended for merged anchors crossing it, or a
/// small blank grid for an empty sheet.
fn sheet_bounds(sheet: &Sheet) -> (u32, u32, u32, u32) {
    let Some(used) = sheet.used_range() else { return (0, 0, 29, 19) };
    let (mut row0, mut col0) = (used.start.row, used.start.col);
    for m in &sheet.merges {
        if m.end.row >= row0 && m.start.row <= used.end.row && m.end.col >= col0 && m.start.col <= used.end.col {
            row0 = row0.min(m.start.row);
            col0 = col0.min(m.start.col);
        }
    }
    (row0, col0, used.end.row, used.end.col)
}

fn fit_zoom(zoom: f64, w: f32, h: f32) -> f32 {
    let zoom = if zoom.is_finite() && zoom > 0.0 { zoom.clamp(0.25, 4.0) } else { 1.0 };
    let area = f64::from(w) * f64::from(h) * zoom * zoom;
    let by_area = if area > MAX_PIXELS { zoom * (MAX_PIXELS / area).sqrt() } else { zoom };
    // the raster engine stops at MAX_SIDE pixels a side
    let side = f64::from(w.max(h)) * by_area;
    (if side > f64::from(wordcraft_render::MAX_SIDE) { by_area * f64::from(wordcraft_render::MAX_SIDE) / side } else { by_area }) as f32
}

fn fill_rect(x: f32, y: f32, w: f32, h: f32, color: Rgb) -> Draw {
    Draw::Fill { rect: Rect { x, y, w, h }, color, alpha: 1.0, text_bg: false }
}

fn seg(x0: f32, y0: f32, x1: f32, y1: f32, width: f32, color: Rgb) -> Draw {
    // a 1px line centred on a whole coordinate straddles two pixels; snap it onto one
    let (x0, x1, y0, y1) = if x0 == x1 {
        (x0 + 0.5, x1 + 0.5, y0, y1)
    } else if y0 == y1 {
        (x0, x1, y0 + 0.5, y1 + 0.5)
    } else {
        (x0, x1, y0, y1)
    };
    Draw::Line { x0, y0, x1, y1, width, color, stroke: Stroke::Solid, alpha: 1.0, text_bg: None }
}

/// Excel's double border: two hairlines either side of the edge.
fn double_border(x0: f32, y0: f32, x1: f32, y1: f32, color: Rgb) -> [Draw; 2] {
    let (dx, dy) = if y0 == y1 { (0.0, 1.0) } else { (1.0, 0.0) };
    [
        seg(x0 - dx, y0 - dy, x1 - dx, y1 - dy, 1.0, color),
        seg(x0 + dx, y0 + dy, x1 + dx, y1 + dy, 1.0, color),
    ]
}

fn model_rgb(color: &gridcraft_model::style::Color, fallback: Rgb, theme: &gridcraft_model::Theme) -> Rgb {
    match color.resolve(theme) {
        Some([r, g, b]) => Rgb(r, g, b),
        None => fallback,
    }
}

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

fn style_str(bold: bool, italic: bool) -> &'static str {
    match (bold, italic) {
        (true, true) => "Bold Italic",
        (true, false) => "Bold",
        (false, true) => "Italic",
        _ => "Regular",
    }
}

/// A centered one-line header label; [a, b] is the cell it centers in.
fn push_label(items: &mut Vec<Draw>, face: &std::sync::Arc<FontFace>, size: f32, color: Rgb, text: &str, a: f32, b: f32, baseline: f32) {
    let k = size / face.upem.max(1.0) as f32;
    let shaped = wordcraft_fonts::shape(face, text, &[], |c| c);
    let lw: f32 = shaped.iter().map(|g| g.x_advance as f32 * k).sum();
    let mut glyphs = Vec::with_capacity(shaped.len());
    let mut pen = a + ((b - a) - lw) / 2.0;
    for g in &shaped {
        glyphs.push((g.gid, pen + g.x_offset as f32 * k, baseline - g.y_offset as f32 * k));
        pen += g.x_advance as f32 * k;
    }
    items.push(Draw::Glyphs {
        face: FaceRef::of(face),
        size,
        glyphs,
        color,
        alpha: 1.0,
        synth_bold: false,
        synth_italic: false,
        text: text.to_string(),
        link: None,
        ranges: Vec::new(),
        text_bg: None,
        x_scale: 1.0,
    });
}

/// One word of a cell's text: its face, metrics and shaped glyphs.
struct Word {
    face: FaceRef,
    size: f32,
    k: f32,
    asc: f32,
    h: f32,
    adv: f32,
    color: Rgb,
    underline: bool,
    strike: bool,
    shaped: Vec<ShapedGlyph>,
}

/// A cell's text laid out like the viewer lays it out: rich runs tokenized on spaces and
/// hyphens, wrapped at the cell edge, aligned, clipped to the cell.
#[allow(clippy::too_many_arguments)]
fn push_cell_text(wb: &Workbook, style: &Style, formatted: &Formatted, x: f32, y: f32, w: f32, h: f32, items: &mut Vec<Draw>) {
    let base_color = model_rgb(&style.font.color, TEXT, &wb.theme);
    struct Run {
        text: String,
        size: f32,
        color: Rgb,
        bold: bool,
        italic: bool,
        underline: bool,
        strike: bool,
    }
    // rich runs match the cell's formatted text, as in the viewer
    let runs: Vec<Run> = if let Some(rs) = wb.rich.iter().find(|r| r.text == formatted.text) {
        rs.runs
            .iter()
            .map(|run| Run {
                text: run.text.clone(),
                size: (run.size.unwrap_or(style.font.size) * PT).clamp(1.0, 40.0),
                color: run.color.map(rgb_of).unwrap_or(base_color),
                bold: run.bold || style.font.bold,
                italic: run.italic || style.font.italic,
                underline: run.underline,
                strike: run.strike,
            })
            .collect()
    } else {
        vec![Run {
            text: formatted.text.clone(),
            size: (style.font.size * PT).clamp(1.0, 40.0),
            color: base_color,
            bold: style.font.bold,
            italic: style.font.italic,
            underline: style.font.underline != Underline::None,
            strike: style.font.strike,
        }]
    };
    if runs.is_empty() {
        return;
    }
    let db = FontDb::global();
    let inner_w = (w + 2.0).max(10.0);
    let mut lines: Vec<Vec<Word>> = vec![Vec::new()];
    let mut cur_w = 0.0f32;
    let mut base_h = 0.0f32;
    for run in &runs {
        let face = db.face(&style.font.name, style_str(run.bold, run.italic));
        let face_ref = FaceRef::of(&face);
        let k = run.size / face.upem.max(1.0) as f32;
        let (asc, desc) = face.vertical_metrics();
        let (asc, h) = (asc as f32 * k, (asc + desc) as f32 * k);
        if base_h == 0.0 {
            base_h = h;
        }
        let mut first = true;
        // a tab inside a cell is a gap in Excel, not a glyph
        for seg in run.text.replace('\t', " ").split('\n') {
            if !first {
                lines.push(Vec::new());
                cur_w = 0.0;
            }
            first = false;
            for word in seg.split_inclusive(' ').flat_map(|t| t.split_inclusive('-')) {
                if word.is_empty() {
                    continue;
                }
                let shaped = wordcraft_fonts::shape(&face, word, &[], |c| c);
                let adv = shaped.iter().map(|g| g.x_advance as f32 * k).sum::<f32>().round();
                if style.align.wrap && cur_w + adv > inner_w && cur_w > 0.0 {
                    lines.push(Vec::new());
                    cur_w = 0.0;
                }
                if let Some(last) = lines.last_mut() {
                    last.push(Word { face: face_ref, size: run.size, k, asc, h, adv, color: run.color, underline: run.underline, strike: run.strike, shaped });
                }
                cur_w += adv;
            }
        }
    }
    let line_hs: Vec<f32> = lines.iter().map(|l| if l.is_empty() { base_h } else { l.iter().map(|wd| wd.h).fold(0.0f32, f32::max) }).collect();
    let total: f32 = line_hs.iter().sum();
    // Excel clips overflow at the bottom (shows the start), so top-align wrapped text taller than the cell
    let overflow = style.align.wrap && total > h;
    let mut yy = if overflow {
        y
    } else {
        match style.align.v {
            VAlign::Top => y,
            VAlign::Center => y + ((h - total) / 2.0).max(0.0),
            _ => y + (h - total).max(0.0),
        }
    };
    let mut cell_items: Vec<Draw> = Vec::new();
    for (li, line) in lines.iter().enumerate() {
        let lh = line_hs[li];
        let lw: f32 = line.iter().map(|wd| wd.adv).sum();
        let mut pen = match style.align.h {
            HAlign::Center | HAlign::CenterAcross => x + 2.0 + ((inner_w - lw) / 2.0).max(0.0),
            HAlign::Right | HAlign::Fill | HAlign::Justify => x + w - 2.0 - lw,
            HAlign::General if formatted.numeric => x + w - 1.0 - lw,
            _ => x + 2.0,
        };
        for wd in line {
            let baseline = yy + wd.asc;
            let start = pen;
            let mut glyphs = Vec::with_capacity(wd.shaped.len());
            for g in &wd.shaped {
                glyphs.push((g.gid, pen + g.x_offset as f32 * wd.k, baseline - g.y_offset as f32 * wd.k));
                pen += g.x_advance as f32 * wd.k;
            }
            let adv = pen - start;
            if !glyphs.is_empty() {
                cell_items.push(Draw::Glyphs {
                    face: wd.face,
                    size: wd.size,
                    glyphs,
                    color: wd.color,
                    alpha: 1.0,
                    synth_bold: false,
                    synth_italic: false,
                    text: String::new(),
                    link: None,
                    ranges: Vec::new(),
                    text_bg: None,
                    x_scale: 1.0,
                });
            }
            if wd.underline || wd.strike {
                let lw_line = (wd.size * 0.06).max(1.0);
                if wd.underline {
                    let uy = baseline + (wd.size * 0.1).max(1.0);
                    cell_items.push(seg(start, uy, start + adv, uy, lw_line, wd.color));
                }
                if wd.strike {
                    let sy = baseline - wd.asc * 0.3;
                    cell_items.push(seg(start, sy, start + adv, sy, lw_line, wd.color));
                }
            }
        }
        yy += lh;
    }
    if !cell_items.is_empty() {
        items.push(Draw::Clip { rect: Rect { x, y, w, h }, items: cell_items });
    }
}

fn rgb_of(v: u32) -> Rgb {
    Rgb(((v >> 16) & 0xFF) as u8, ((v >> 8) & 0xFF) as u8, (v & 0xFF) as u8)
}

#[cfg(test)]
mod tests {
    use super::*;
    use gridcraft_core::Value;
    use gridcraft_model::Cell;

    #[test]
    fn renders_cells_and_headers() {
        let mut wb = Workbook::new();
        let sheet = std::sync::Arc::make_mut(&mut wb.sheets[0]);
        sheet.cells.set(CellRef::new(0, 0), Cell::value(Value::Text("Hello".into())));
        sheet.cells.set(CellRef::new(1, 1), Cell::value(Value::Number(42.0)));
        let bytes = gridcraft_xlsx::write_xlsx(&wb).expect("write test workbook");
        let pngs = sheet_pngs(&bytes, 1.0, None, None).expect("render test workbook");
        assert_eq!(pngs.len(), 1);
        assert_eq!(pngs[0].0, "sheet-1.png");
        let img = image::load_from_memory(&pngs[0].1).expect("decode rendered png");
        assert!(img.width() > LEFT_W as u32 && img.height() > TOP_H as u32);
        let rgba = img.to_rgba8();
        let blank = rgba.enumerate_pixels().all(|(_, _, p)| p.0[..3] == [255, 255, 255][..]);
        assert!(!blank, "gridlines or cell text should be visible");
    }

    #[test]
    fn pages_select_sheets() {
        let mut wb = Workbook::new();
        wb.sheets.push(std::sync::Arc::new(gridcraft_model::Sheet::new("Sheet2")));
        let bytes = gridcraft_xlsx::write_xlsx(&wb).expect("write test workbook");
        let pngs = sheet_pngs(&bytes, 1.0, Some(&[1]), None).expect("render sheet 2");
        assert_eq!(pngs.len(), 1);
        assert_eq!(pngs[0].0, "sheet-2.png");
    }

    #[test]
    fn a_tall_sheet_renders_as_sampled_windows() {
        let mut wb = Workbook::new();
        let sheet = std::sync::Arc::make_mut(&mut wb.sheets[0]);
        for row in 0..2000 {
            sheet.cells.set(CellRef::new(row, 0), Cell::value(Value::Number(f64::from(row))));
        }
        let bytes = gridcraft_xlsx::write_xlsx(&wb).expect("write test workbook");
        let names: Vec<String> = sheet_pngs(&bytes, 1.0, None, None)
            .expect("render tall sheet")
            .into_iter()
            .map(|(name, _)| name)
            .collect();
        assert_eq!(names, ["sheet-1-top.png", "sheet-1-middle.png", "sheet-1-bottom.png"]);
    }

    #[test]
    fn cells_draws_the_requested_box() {
        let mut wb = Workbook::new();
        let sheet = std::sync::Arc::make_mut(&mut wb.sheets[0]);
        for row in 0..200 {
            sheet.cells.set(CellRef::new(row, 0), Cell::value(Value::Number(f64::from(row))));
        }
        let bytes = gridcraft_xlsx::write_xlsx(&wb).expect("write test workbook");
        let cells = Cells::parse("A50:A60").expect("parse a cell box");
        let pngs = sheet_pngs(&bytes, 1.0, None, Some(&cells)).expect("render a box");
        assert_eq!(pngs.len(), 1);
        let img = image::load_from_memory(&pngs[0].1).expect("decode rendered png");
        // eleven 20px rows plus the header band, not the whole 200-row sheet
        assert!((240..=320).contains(&img.height()), "height {}", img.height());
    }

    #[test]
    fn cells_parse_open_ends() {
        assert!(Cells::parse("B10:E40").is_some());
        assert!(Cells::parse("A100:").is_some());
        assert!(Cells::parse(":E40").is_some());
        assert!(Cells::parse("A:E").is_some());
        assert!(Cells::parse("nonsense").is_none());
    }
}
