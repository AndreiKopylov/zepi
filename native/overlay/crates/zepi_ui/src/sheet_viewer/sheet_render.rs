
use gpui::TextAlign;
use gridcraft_model::style::HAlign;
use ui::prelude::Disableable;

use super::formula_highlights::{FormulaHighlight, formula_highlights};
use super::*;

impl gpui::Render for SheetView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.refocus_root {
            self.refocus_root = false;
            let handle = self.focus_handle.clone();
            window.on_next_frame(move |window, cx| window.focus(&handle, cx));
        }
        if self.focus_search_on_render {
            self.focus_search_on_render = false;
            if let Some(editor) = self.search_editor.as_ref() {
                let handle = editor.read(cx).focus_handle(cx);
                window.focus(&handle, cx);
            }
        }
        let colors = cx.theme().colors().clone();
        let item = self.item.read(cx);
        if let Some(error) = item.import_error.as_ref() {
            let message = error.clone();
            return div()
                .size_full()
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .gap_2()
                .bg(colors.panel_background)
                .child(Icon::new(IconName::FileGeneric).size(ui::IconSize::XLarge).color(UiColor::Muted))
                .child(Label::new("Cannot open this file as a spreadsheet").size(LabelSize::Large))
                .child(
                    div()
                        .max_w(px(560.))
                        .child(Label::new(message).size(LabelSize::Small).color(UiColor::Muted)),
                )
                .child(
                    Label::new("The file may be corrupted, password-protected or not a real .xlsx/.xls package.")
                        .size(LabelSize::Small)
                        .color(UiColor::Muted),
                )
                .into_any_element();
        }
        let Some(wb) = item.wb.as_ref() else {
            return div().size_full().bg(colors.panel_background).into_any_element();
        };
        let sheet_count = wb.sheets.len();
        let self_sheet = self.sheet.min(sheet_count.saturating_sub(1));
        let Some(sheet) = wb.sheets.get(self_sheet) else {
            return div().size_full().bg(colors.panel_background).into_any_element();
        };
        let zoom = self.zoom;
        let (scroll_x, scroll_y) = self.scroll.pos();
        let (active_row, active_col) = self.active_cell();
        let date_system = wb.date_system;
        let styles = &wb.styles;

        let grid_line: Hsla = gpui::rgb(0xd4d4d4).into();
        let header_bg: Hsla = gpui::rgb(0xf3f3f3).into();
        let header_sel: Hsla = gpui::rgb(0xd9e9df).into();
        let select_border: Hsla = gpui::rgb(0x217346).into();
        let text_color: Hsla = gpui::rgb(0x202020).into();
        let cell_bg: Hsla = gpui::rgb(0xffffff).into();
        let bar_muted: Hsla = gpui::rgb(0x606060).into();
        let tab_muted: Hsla = gpui::rgb(0x606060).into();
        let tab_hover: Hsla = gpui::rgb(0xe4e4e4).into();
        let editing_formula = self.editing && self.edit_buffer.starts_with('=');
        let formula_refs = if editing_formula {
            formula_highlights(&self.edit_buffer, &sheet.name)
        } else {
            Vec::new()
        };

        let edit_rect = if self.editing {
            self.edit_cell_rect(cx).filter(|&(left, top, width, height)| {
                left < f32::from(self.grid_size.x) && top < f32::from(self.grid_size.y)
                    && left + width > LEFT_W && top + height > TOP_H
            })
        } else {
            None
        };

        // header chrome like the word/slides viewers
        let file_name = item
            .path
            .path
            .file_name()
            .map(|name| name.to_string())
            .unwrap_or_else(|| "spreadsheet".to_string());
        let mut header = div()
            .flex()
            .flex_row()
            .items_center()
            .gap_1()
            .px_2()
            .h(px(32.))
            .border_b_1()
            .border_color(colors.border_variant)
            .bg(colors.panel_background)
            .child(
                Label::new(SharedString::from(file_name))
                    .size(LabelSize::Small)
                    .color(UiColor::Muted),
            )
            .child(div().flex_1());
        let partial_import = risks_data_loss(&item.import_report);
        if !item.import_report.warnings.is_empty() {
            header = header.child(
                ui::Button::new("sheet-import-warnings", if partial_import { "Partial import" } else { "Import notes" })
                    .on_click(cx.listener(|this, _, window, cx| this.show_import_warnings(window, cx))),
            );
        }
        if partial_import || item.revision.is_dirty() {
            header = header.child(
                ui::Button::new("sheet-save-copy", "Save a Copy")
                    .on_click(cx.listener(|this, _, window, cx| this.save_copy(window, cx))),
            );
        }
        let view_for_header = cx.entity();
        header = header
            .child(
                IconButton::new("sheet-zoom-out", IconName::Dash)
                    .icon_size(ui::IconSize::XSmall)
                    .tooltip(Tooltip::for_action_title("Zoom out", &ZoomOut))
                    .on_click({
                        let view = view_for_header.clone();
                        move |_, _window, cx| {
                            let _ = view.update(cx, |this, cx| this.set_zoom(this.zoom - 0.1, cx));
                        }
                    }),
            )
            .child(
                Label::new(format!("{}%", (zoom * 100.0) as u32))
                    .size(LabelSize::XSmall)
                    .color(UiColor::Muted),
            )
            .child(
                IconButton::new("sheet-zoom-in", IconName::Plus)
                    .icon_size(ui::IconSize::XSmall)
                    .tooltip(Tooltip::for_action_title("Zoom in", &ZoomIn))
                    .on_click({
                        let view = view_for_header.clone();
                        move |_, _window, cx| {
                            let _ = view.update(cx, |this, cx| this.set_zoom(this.zoom + 0.1, cx));
                        }
                    }),
            )
            .child(
                IconButton::new("sheet-send", IconName::ZedAssistant)
                    .icon_size(ui::IconSize::Small)
                    .tooltip(Tooltip::for_action_title("Send selection to agent", &SheetSend))
                    .on_click({
                        let view = view_for_header.clone();
                        move |_, window, cx| {
                            let _ = view.update(cx, |this, cx| this.send_selection(window, cx));
                        }
                    }),
            )
            .child(
                IconButton::new("sheet-search", IconName::MagnifyingGlass)
                    .icon_size(ui::IconSize::Small)
                    .tooltip({ let f = self.focus_handle.clone(); move |_w, cx| Tooltip::for_action_in("Search", &SheetSearch, &f, cx) })
                    .on_click({
                        let view = view_for_header.clone();
                        move |_, window, cx| {
                            let _ = view.update(cx, |this, cx| this.toggle_search(window, cx));
                        }
                    }),
            );

        let search_bar = self.search_open.then(|| {
            let n = self.search_matches.len();
            let label = if n == 0 { "No results".to_string() } else { format!("{}/{}", self.search_current + 1, n) };
            let view = cx.entity();
            let search_focus = self.focus_handle.clone();
            let mut bar = ui::h_flex()
                .key_context("SheetSearch")
                .flex_none()
                .h(px(32.))
                .px_2()
                .gap_2()
                .items_center()
                .border_b_1()
                .border_color(colors.border_variant)
                .bg(colors.panel_background)
                .on_action(cx.listener(|this, _: &SheetSearchNext, _w, cx| this.search_next(cx)))
                .on_action(cx.listener(|this, _: &SheetSearchPrev, _w, cx| this.search_prev(cx)))
                .on_action(cx.listener(|this, _: &SheetSearch, w, cx| this.toggle_search(w, cx)));
            if let Some(editor) = self.search_editor.clone() {
                bar = bar.child(div().flex_1().min_w_0().h(px(22.)).on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation()).child(editor));
            }
            bar.child(Label::new(SharedString::from(label)).size(LabelSize::XSmall).color(UiColor::Muted))
                .child(IconButton::new("sheet-search-prev", IconName::ChevronUp).icon_size(ui::IconSize::XSmall).tooltip({ let f = search_focus.clone(); move |_w, cx| Tooltip::for_action_in("Previous match", &SheetSearchPrev, &f, cx) }).on_click({ let v = view.clone(); move |_, _, cx| { let _ = v.update(cx, |this, cx| this.search_prev(cx)); } }))
                .child(IconButton::new("sheet-search-next", IconName::ChevronDown).icon_size(ui::IconSize::XSmall).tooltip({ let f = search_focus.clone(); move |_w, cx| Tooltip::for_action_in("Next match", &SheetSearchNext, &f, cx) }).on_click({ let v = view.clone(); move |_, _, cx| { let _ = v.update(cx, |this, cx| this.search_next(cx)); } }))
                .child(IconButton::new("sheet-search-close", IconName::Close).icon_size(ui::IconSize::XSmall).tooltip({ let f = search_focus.clone(); move |_w, cx| Tooltip::for_action_in("Close search", &SheetSearch, &f, cx) }).on_click({ let v = view; move |_, w, cx| { let _ = v.update(cx, |this, cx| this.toggle_search(w, cx)); } }))
        });

        // formula bar
        let bar_text = if self.editing { self.edit_buffer.clone() } else { self.bar_text(cx) };
        let bar_content = div()
            .flex_1()
            .min_w_0()
            .overflow_hidden()
            .whitespace_nowrap()
            .id("sheet-bar-content")
            .cursor_pointer()
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _event, window, cx| this.start_edit(None, window, cx)),
            )
            .child(Label::new(if bar_text.is_empty() { " ".into() } else { SharedString::from(bar_text) })
                .size(LabelSize::Default).color(UiColor::Custom(text_color)));
        let formula_action_color = UiColor::Custom(if self.editing { bar_muted } else { gpui::rgb(0xa0a0a0).into() });
        let formula_bar = div()
            .flex()
            .flex_row()
            .items_center()
            .gap_1()
            .px_2()
            .h(px(32.))
            .bg(header_bg)
            .border_b_1()
            .border_color(grid_line)
            .child(
                div()
                    .w(px(56.))
                    .h(px(24.))
                    .bg(cell_bg)
                    .border_1()
                    .border_color(grid_line)
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(
                        Label::new(cell_ref_text(active_row, active_col))
                            .size(LabelSize::Small)
                            .color(UiColor::Custom(text_color)),
                    ),
            )
            .child(IconButton::new("sheet-cancel-edit", IconName::Close)
                .icon_size(ui::IconSize::XSmall)
                .icon_color(formula_action_color)
                .disabled(!self.editing)
                .tooltip(Tooltip::for_action_title("Cancel edit", &CancelEdit))
                .on_click(cx.listener(|this, _, _, cx| this.cancel_edit(cx))))
            .child(IconButton::new("sheet-commit-edit", IconName::Check)
                .icon_size(ui::IconSize::XSmall)
                .icon_color(formula_action_color)
                .disabled(!self.editing)
                .tooltip(Tooltip::for_action_title("Confirm edit", &CommitEdit))
                .on_click(cx.listener(|this, _, _, cx| this.commit_edit(cx))))
            .child(Label::new("ƒx").size(LabelSize::Small).color(UiColor::Custom(bar_muted)))
            .child(div().flex_1().min_w_0().h(px(24.0)).flex().items_center().px_1()
                .bg(cell_bg).border_1().border_color(grid_line).child(bar_content));

        // visible range
        let view_w = if f32::from(self.grid_size.x) > 1.0 {
            f32::from(self.grid_size.x)
        } else {
            f32::from(window.viewport_size().width)
        };
        let view_h = if f32::from(self.grid_size.y) > 1.0 {
            f32::from(self.grid_size.y)
        } else {
            f32::from(window.viewport_size().height) - 60.0
        };
        let (last_row, last_col) = self.used_bounds(cx);
        let first_col = sheet.col_at(scroll_x as f64);
        let last_visible_col = sheet
            .col_at((scroll_x + (view_w - LEFT_W) / zoom) as f64)
            .min(last_col);
        let first_row = sheet.row_at(scroll_y as f64);
        let last_visible_row = sheet
            .row_at((scroll_y + (view_h - TOP_H) / zoom) as f64)
            .min(last_row);

        let mut grid = div()
            .flex_1()
            .relative()
            .overflow_hidden()
            .bg(cell_bg)
            .on_scroll(cx.listener(|this, event, _window, cx| this.on_wheel(event, cx)))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, event: &MouseDownEvent, window, cx| {
                    let gx = f32::from(event.position.x - this.grid_origin.x);
                    let gy = f32::from(event.position.y - this.grid_origin.y);
                    if gx > f32::from(this.grid_size.x) - 10.0 || gy > f32::from(this.grid_size.y) - 10.0 {
                        return;
                    }
                    let Some((row, col)) = this.cell_at(event.position, cx) else {
                        return;
                    };
                    if this.editing {
                        // a click inside the edited cell moves the caret, like Excel
                        if let Some((l, t, w, h)) = this.edit_rect_cache {
                            if gx >= l && gx <= l + w && gy >= t && gy <= t + h { return; }
                        }
                        this.commit_edit(cx);
                    }
                    if event.modifiers.shift {
                        if this.anchor.is_none() {
                            this.anchor = Some((this.sel_row, this.sel_col));
                        }
                    } else {
                        this.anchor = Some((row, col));
                    }
                    this.sel_row = row;
                    this.sel_col = col;
                    this.clamp_sel(cx);
                    this.drag_select = true;
                    if event.click_count == 2 {
                        this.start_edit(None, window, cx);
                    } else {
                        window.focus(&this.focus_handle, cx);
                    }
                    cx.notify();
                }),
            )
            .on_mouse_move(cx.listener(|this, event: &MouseMoveEvent, _window, cx| {
                // in-cell caret drags belong to the edit overlay, whose canvas origin
                // differs from the cell rect by the border and vertical alignment
                if !this.drag_select {
                    return;
                }
                let Some((row, col)) = this.cell_at(event.position, cx) else {
                    return;
                };
                if row != this.sel_row || col != this.sel_col {
                    this.sel_row = row;
                    this.sel_col = col;
                    this.clamp_sel(cx);
                    cx.notify();
                }
            }))
            .child(
                div()
                    .absolute()
                    .left_0()
                    .top_0()
                    .size_full()
                    .child(gpui::canvas(
                        {
                            let view = cx.entity();
                            move |bounds: gpui::Bounds<gpui::Pixels>, _window, cx: &mut App| {
                                let origin = bounds.origin;
                                let size = bounds.size;
                                view.update(cx, |this, cx| {
                                    let new_size = Point::new(size.width, size.height);
                                    if this.grid_origin != origin || this.grid_size != new_size {
                                        this.grid_origin = origin;
                                        this.grid_size = new_size;
                                        cx.notify();
                                    }
                                });
                            }
                        },
                        |_, _, _, _| (),
                    )
                    .size_full()),
            );

        // corner
        grid = grid.child(
            div()
                .absolute()
                .left_0()
                .top_0()
                .w(px(LEFT_W))
                .h(px(TOP_H))
                .bg(header_bg)
                .border_r_1()
                .border_b_1()
                .border_color(grid_line),
        );

        // column headers
        for col in first_col..=last_visible_col {
            let width = sheet.col_width(col) * zoom;
            if width <= 0.0 {
                continue;
            }
            let left = LEFT_W + (sheet.col_left(col) as f32 - scroll_x) * zoom;
            grid = grid.child(
                div()
                    .absolute()
                    .left(px(left))
                    .top_0()
                    .w(px(width))
                    .h(px(TOP_H))
                    .bg(if col == active_col { header_sel } else { header_bg })
                    .border_r_1()
                    .border_b_1()
                    .border_color(grid_line)
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(Label::new(col_name(col)).size(LabelSize::Small).color(UiColor::Custom(if col == active_col { select_border } else { bar_muted }))),
            );
            grid = grid.child(
                div()
                    .absolute()
                    .left(px(left + width - 3.0))
                    .top_0()
                    .w(px(6.))
                    .h(px(TOP_H))
                    .cursor_col_resize()
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, event: &MouseDownEvent, _window, cx| {
                            let w = this
                                .item
                                .read(cx)
                                .wb
                                .as_ref()
                                .and_then(|wb| wb.sheets.get(this.sheet))
                                .map(|sh| sh.col_width(col))
                                .unwrap_or(0.0);
                            if event.click_count == 2 {
                                this.autofit(1, col, cx);
                                return;
                            }
                            this.resize = Some((1, col, f32::from(event.position.x), w));
                        }),
                    ),
            );
        }

        // row headers
        for row in first_row..=last_visible_row {
            let height = sheet.row_height(row) * zoom;
            if height <= 0.0 {
                continue;
            }
            let top = TOP_H + (sheet.row_top(row) as f32 - scroll_y) * zoom;
            grid = grid.child(
                div()
                    .absolute()
                    .left_0()
                    .top(px(top))
                    .w(px(LEFT_W))
                    .h(px(height))
                    .bg(if row == active_row { header_sel } else { header_bg })
                    .border_r_1()
                    .border_b_1()
                    .border_color(grid_line)
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(Label::new((row + 1).to_string()).size(LabelSize::Small).color(UiColor::Custom(if row == active_row { select_border } else { bar_muted }))),
            );
            grid = grid.child(
                div()
                    .absolute()
                    .left_0()
                    .top(px(top + height - 3.0))
                    .w(px(LEFT_W))
                    .h(px(6.))
                    .cursor_row_resize()
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, event: &MouseDownEvent, _window, cx| {
                            let h = this
                                .item
                                .read(cx)
                                .wb
                                .as_ref()
                                .and_then(|wb| wb.sheets.get(this.sheet))
                                .map(|sh| sh.row_height(row))
                                .unwrap_or(0.0);
                            if event.click_count == 2 {
                                this.autofit(0, row, cx);
                                return;
                            }
                            this.resize = Some((0, row, f32::from(event.position.y), h));
                        }),
                    ),
            );
        }

        // cells (extend to merge anchors intersecting the viewport so merged text survives scroll)
        let mut draw_row0 = first_row;
        let mut draw_col0 = first_col;
        for m in &sheet.merges {
            if m.end.row >= first_row
                && m.start.row <= last_visible_row
                && m.end.col >= first_col
                && m.start.col <= last_visible_col
            {
                draw_row0 = draw_row0.min(m.start.row);
                draw_col0 = draw_col0.min(m.start.col);
            }
        }
        let mut edit_el: Option<gpui::Div> = None;
        let mut edit_text: Option<(f32, gpui::Div)> = None;
        for row in draw_row0..=last_visible_row {
            let mut height = sheet.row_height(row) * zoom;
            if height <= 0.0 {
                continue;
            }
            let top = TOP_H + (sheet.row_top(row) as f32 - scroll_y) * zoom;
            for col in draw_col0..=last_visible_col {
                let mut width = sheet.col_width(col) * zoom;
                if width <= 0.0 {
                    continue;
                }
                let left = LEFT_W + (sheet.col_left(col) as f32 - scroll_x) * zoom;
                let at = CellRef::new(row, col);
                // merged range: draw the anchor across the whole span, skip covered cells
                if let Some(m) = sheet.merge_at(at) {
                    if at != m.start {
                        continue;
                    }
                    width = (sheet.col_left(m.end.col + 1) - sheet.col_left(m.start.col)) as f32 * zoom;
                    height = (sheet.row_top(m.end.row + 1) - sheet.row_top(m.start.row)) as f32 * zoom;
                }
                let style_id = sheet.style_id(at);
                let style = styles.get(style_id);
                let has_border = !style.borders.top.is_none()
                    || !style.borders.right.is_none()
                    || !style.borders.bottom.is_none()
                    || !style.borders.left.is_none();
                let mut cell_div = div()
                    .absolute()
                    .left(px(left))
                    .top(px(top))
                    .w(px(width))
                    .h(px(height))
                    .bg(cell_bg)
                    .overflow_hidden();
                if !has_border {
                    cell_div = cell_div.border_r_1().border_b_1().border_color(grid_line);
                }
                let fill_hsla: Option<Hsla> = match style.fill.pattern {
                    PatternType::Solid => Some(model_color_hsla(&style.fill.fg, header_bg, &wb.theme.colors)),
                    _ => None,
                };
                if let Some(fill) = fill_hsla {
                    cell_div = cell_div.bg(fill);
                }
                let b = &style.borders;
                // shared edges drawn once: right/bottom yield to the neighbor's left/top
                let nb_right_left = !styles.get(sheet.style_id(CellRef::new(row, col + 1))).borders.left.is_none();
                let nb_below_top = !styles.get(sheet.style_id(CellRef::new(row + 1, col))).borders.top.is_none();
                for (side, line) in [(0usize, b.top), (1, b.right), (2, b.bottom), (3, b.left)] {
                    let w = line.style.width();
                    if line.is_none() || w <= 0.0 {
                        continue;
                    }
                    if side == 1 && nb_right_left {
                        continue;
                    }
                    if side == 2 && nb_below_top {
                        continue;
                    }
                    let color = model_color_hsla(&line.color, gpui::rgb(0x000000).into(), &wb.theme.colors);
                    let edge = div().absolute().bg(color);
                    let edge = match side {
                        0 => edge.left_0().top_0().right_0().h(px(w)),
                        1 => edge.top_0().right_0().bottom_0().w(px(w)),
                        2 => edge.left_0().bottom_0().right_0().h(px(w)),
                        _ => edge.top_0().left_0().bottom_0().w(px(w)),
                    };
                    cell_div = cell_div.child(edge);
                }
                let is_edited = self.editing && row == self.edit_row && col == self.edit_col;
                if let Some(value) = sheet.value_ref(at).or_else(|| is_edited.then_some(&Value::Empty)) {
                    let fmt = NumberFormat::parse_cached(&style.num_fmt.0);
                    let formatted = format_value(value, &fmt, date_system);
                    if !formatted.text.is_empty() || is_edited {
                        let font_size = (style.font.size * office_knob_f("sheet.fontScale", 4.0 / 3.0) as f32 * zoom).clamp(1.0, 40.0);
                        let color = model_color_hsla(&style.font.color, text_color, &wb.theme.colors);
                        let align = cell_editor_align(style.align.h, formatted.numeric, is_edited && editing_formula);
                        let right_padding = if style.align.h == HAlign::General {
                            f32::from(window.rem_size()) * 0.25
                        } else {
                            2.0
                        };
                        let mut text_div = div()
                            .w_full()
                            .line_height(px(font_size * 1.2))
                            .text_size(px(font_size))
                            .text_color(color)
                            .font_family(SharedString::from(style.font.name.clone()));
                        text_div = match align {
                            TextAlign::Center => text_div.text_center(),
                            TextAlign::Right => text_div.text_right().pr(px(right_padding)),
                            TextAlign::Left => text_div.text_left().pl(px(2.0)),
                        };
                        if style.align.wrap {
                            text_div = text_div.whitespace_normal();
                        } else {
                            text_div = text_div.whitespace_nowrap().overflow_hidden();
                        }
                        if style.font.bold {
                            text_div = text_div.font_weight(gpui::FontWeight::BOLD);
                        }
                        if style.font.italic {
                            text_div = text_div.italic();
                        }
                        if style.font.strike {
                            text_div = text_div.line_through();
                        }
                        if style.font.underline != gridcraft_model::style::Underline::None {
                            text_div = text_div.underline();
                        }
                        let mut rich_total: Option<f32> = None;
                        let cell_hits: Vec<(usize, usize)> = if self.search_open { self.search_matches.iter().filter(|m| m.0 == row && m.1 == col).map(|m| (m.2, m.3)).collect() } else { Vec::new() };
                        let cur_hit: Option<(usize, usize)> = if self.search_open { self.search_matches.get(self.search_current).filter(|m| m.0 == row && m.1 == col).map(|m| (m.2, m.3)) } else { None };
                        let editing_this = self.editing && row == self.edit_row && col == self.edit_col;
                        let edit_rs;
                        let rs_opt = if editing_this {
                            edit_rs = gridcraft_model::workbook::RichString { text: self.edit_buffer.clone(), runs: self.edit_runs.clone() };
                            Some(&edit_rs)
                        } else {
                            wb.rich.iter().find(|r| r.text == formatted.text)
                        };
                        if let Some(rs) = rs_opt {
                            let knob = office_knob_f("sheet.fontScale", 4.0 / 3.0) as f32;
                            let family = SharedString::from(style.font.name.clone());
                            let base_color = color;
                            let mut runs: Vec<(String, f32, Hsla, bool, bool, bool, bool)> = Vec::new();
                            for run in &rs.runs {
                                runs.push((run.text.clone(), (run.size.unwrap_or(style.font.size) * knob * zoom).clamp(1.0, 40.0), run.color.map(|cc| gpui::rgb(cc).into()).unwrap_or(base_color), run.bold || style.font.bold, run.italic || style.font.italic, run.underline, run.strike));
                            }
                            let inner_w = (width + 2.0).max(10.0);
                            let right_edge = width - right_padding - if has_border { 0.0 } else { 1.0 };
                            let mut lines: Vec<Vec<(String, f32, Hsla, bool, bool, bool, bool, f32, f32, usize)>> = vec![Vec::new()];
                            let mut cur_w = 0.0f32;
                            let mut off = 0usize;
                            for (text, sz, col, b, i, u, st) in &runs {
                                let mut first_seg = true;
                                for seg in text.split('\n') {
                                    if !first_seg { lines.push(Vec::new()); cur_w = 0.0; off += 1; }
                                    first_seg = false;
                                    for word in seg.split_inclusive(' ').flat_map(|w| w.split_inclusive('-')) {
                                        if word.is_empty() { continue; }
                                        let font = gpui::Font { family: family.clone(), weight: if *b { gpui::FontWeight::BOLD } else { gpui::FontWeight::default() }, style: if *i { gpui::FontStyle::Italic } else { gpui::FontStyle::default() }, ..Default::default() };
                                        let run = gpui::TextRun { len: word.len(), font, color: *col, background_color: None, underline: if *u { Some(Default::default()) } else { None }, strikethrough: if *st { Some(Default::default()) } else { None } };
                                        let shaped = window.text_system().shape_line(SharedString::from(word.to_string()), px(*sz), &[run], None);
                                        let w = f32::from(shaped.width).round();
                                        let h = f32::from(shaped.ascent + shaped.descent);
                                        if cur_w + w > inner_w && cur_w > 0.0 { lines.push(Vec::new()); cur_w = 0.0; }
                                        lines.last_mut().unwrap().push((word.to_string(), *sz, *col, *b, *i, *u, *st, w, h, off));
                                        cur_w += w;
                                        off += word.chars().count();
                                    }
                                }
                            }
                            let base_h = {
                                let f = gpui::Font { family: family.clone(), weight: if style.font.bold { gpui::FontWeight::BOLD } else { gpui::FontWeight::default() }, style: if style.font.italic { gpui::FontStyle::Italic } else { gpui::FontStyle::default() }, ..Default::default() };
                                let r = gpui::TextRun { len: 1, font: f, color: base_color, background_color: None, underline: None, strikethrough: None };
                                let sh = window.text_system().shape_line(SharedString::from(" "), px(font_size), &[r], None);
                                f32::from(sh.ascent + sh.descent)
                            };
                            let line_geometry: Vec<_> = lines.iter().map(|line| {
                                let width = line.iter().map(|word| word.7).sum();
                                let height = if line.is_empty() { base_h } else {
                                    line.iter().map(|word| word.8).fold(0.0f32, f32::max)
                                };
                                (line_start_px(align, inner_w, right_edge, width), height)
                            }).collect();
                            let total_h: f32 = line_geometry.iter().map(|(_, height)| height).sum();
                            rich_total = Some(total_h);
                            if editing_this {
                                let mut lay = Vec::new();
                                let mut yy = 0.0f32;
                                for (line, &(mut xx, lh)) in lines.iter().zip(&line_geometry) {
                                    let mut words = Vec::new();
                                    for w in line {
                                        let ce = w.9 + w.0.chars().count();
                                        let font = gpui::Font { family: family.clone(), weight: if w.3 { gpui::FontWeight::BOLD } else { gpui::FontWeight::default() }, style: if w.4 { gpui::FontStyle::Italic } else { gpui::FontStyle::default() }, ..Default::default() };
                                        let run = gpui::TextRun { len: w.0.len(), font, color: w.2, background_color: None, underline: None, strikethrough: None };
                                        let shaped = window.text_system().shape_line(SharedString::from(w.0.clone()), px(w.1), &[run], None);
                                        words.push((w.9, ce, xx, w.7, char_xs(&shaped, &w.0)));
                                        xx += w.7;
                                    }
                                    lay.push((yy, lh, words));
                                    yy += lh;
                                }
                                self.edit_layout = lay;
                            }
                            let hits = cell_hits;
                            let cur = cur_hit;
                            let e_caret = if editing_this { Some(self.edit_caret) } else { None };
                            let e_mouse = if editing_this { self.edit_mouse } else { None };
                            let e_drag = editing_this && self.edit_drag;
                            let e_sel = if editing_this { self.edit_sel_anchor.map(|a| (a.min(self.edit_caret), a.max(self.edit_caret))) } else { None };
                            let eo = self.edit_origin.clone();
                            let et = editing_this;
                            let refs = if editing_this { formula_refs.clone() } else { Vec::new() };
                            let painter = gpui::canvas(
                                |_, _, _| {},
                                move |bounds, _, window, cx| {
                                    if et { *eo.lock().unwrap() = (f32::from(bounds.origin.x), f32::from(bounds.origin.y)); }
                                    let mut y = 0.0f32;
                                    for (line, &(mut x, lh)) in lines.iter().zip(&line_geometry) {
                                        for (word, sz, col, b, i, u, st, w, _h, cs) in line {
                                            let ce = cs + word.chars().count();
                                            let font = gpui::Font { family: family.clone(), weight: if *b { gpui::FontWeight::BOLD } else { gpui::FontWeight::default() }, style: if *i { gpui::FontStyle::Italic } else { gpui::FontStyle::default() }, ..Default::default() };
                                            let run = gpui::TextRun { len: word.len(), font, color: *col, background_color: None, underline: if *u { Some(Default::default()) } else { None }, strikethrough: if *st { Some(Default::default()) } else { None } };
                                            let colored = formula_word_runs(word, *cs, &run, &refs);
                                            let runs = if colored.is_empty() { std::slice::from_ref(&run) } else { &colored };
                                            let shaped = window.text_system().shape_line(SharedString::from(word.clone()), px(*sz), runs, None);
                                            for (hs, he) in hits.iter().chain(cur.iter()) {
                                                let aa = (*hs).max(*cs);
                                                let bb = (*he).min(ce);
                                                if aa >= bb { continue; }
                                                if let Some((hx0, hx1)) = hit_span(&shaped, word, *cs, aa, bb) {
                                                    let is_cur = cur.map(|(ca, cb)| *hs == ca && *he == cb).unwrap_or(false);
                                                    let bg = if is_cur { gpui::hsla(0.58, 0.9, 0.5, 0.6) } else { gpui::hsla(0.58, 0.8, 0.6, 0.3) };
                                                    window.paint_quad(gpui::fill(gpui::Bounds { origin: gpui::point(bounds.origin.x + px(x + hx0), bounds.origin.y + px(y)), size: gpui::size(px(hx1 - hx0), px(lh)) }, bg));
                                                }
                                            }
                                            if let Some((sa, sb)) = e_sel {
                                                let aa = sa.max(*cs); let bb = sb.min(ce);
                                                if aa < bb { if let Some((mut hx0, mut hx1)) = hit_span(&shaped, word, *cs, aa, bb) {
                                                    if e_drag { if let Some((mx, my)) = e_mouse { if my >= y && my < y + lh { if let Some(h) = e_caret { if h == sb && bb == sb { hx1 = (mx - x).clamp(hx0, *w); } if h == sa && aa == sa { hx0 = (mx - x).clamp(0.0, hx1); } } } } }
                                                    window.paint_quad(gpui::fill(gpui::Bounds { origin: gpui::point(bounds.origin.x + px(x + hx0), bounds.origin.y + px(y)), size: gpui::size(px(hx1 - hx0), px(lh)) }, gpui::hsla(0.0, 0.0, 0.82, 1.0)));
                                                } }
                                            }
                                            if let Some(cc) = e_caret { if !e_drag && cc >= *cs && cc <= ce { let cx0 = if cc == ce { *w } else { hit_span(&shaped, word, *cs, cc, (cc + 1).min(ce)).map(|(a, _)| a).unwrap_or(*w) }; window.paint_quad(gpui::fill(gpui::Bounds { origin: gpui::point(bounds.origin.x + px(x + cx0), bounds.origin.y + px(y)), size: gpui::size(px(1.0), px(lh)) }, gpui::black())); } }
                                            let _ = shaped.paint(gpui::point(bounds.origin.x + px(x), bounds.origin.y + px(y)), px(lh), TextAlign::Left, None, window, cx);
                                            x += w;
                                        }
                                        y += lh;
                                    }
                                },
                            );
                            text_div = div().w_full().h(px(total_h)).child(painter);
                        } else if !cell_hits.is_empty() {
                            text_div = text_div.child(search_styled(&formatted.text, &cell_hits, cur_hit, color, &style));
                        } else {
                            text_div = text_div.child(SharedString::from(formatted.text.clone()));
                        }
                        // Excel clips overflow at the bottom (shows the start), so top-align when wrapped text is taller than the cell
                        let est_lines = ((formatted.text.chars().count() as f32 * font_size * 0.5) / width.max(1.0)).ceil().max(1.0);
                        let overflow = style.align.wrap && rich_total.unwrap_or(est_lines * font_size * 1.2) > height;
                        let mut v_div = div().size_full().flex().flex_col();
                        v_div = if overflow {
                            v_div.justify_start()
                        } else {
                            match style.align.v {
                                gridcraft_model::style::VAlign::Top => v_div.justify_start(),
                                gridcraft_model::style::VAlign::Center => v_div.justify_center(),
                                _ => v_div.justify_end(),
                            }
                        };
                        if is_edited { edit_text = Some((rich_total.unwrap_or(height), text_div)); } else { cell_div = cell_div.child(v_div.child(text_div)); }
                    }
                }
                grid = grid.child(cell_div);
            }
        }

        if !formula_refs.is_empty() {
            let mut layer = div().absolute().left(px(LEFT_W)).top(px(TOP_H)).right_0().bottom_0().overflow_hidden();
            let visible = RangeRef::new(CellRef::new(first_row, first_col), CellRef::new(last_visible_row, last_visible_col));
            let mut drawn = std::collections::HashSet::new();
            for reference in &formula_refs {
                let range = reference.cells;
                if !range.intersects(&visible) || !drawn.insert(range) {
                    continue;
                }
                let x = sheet.col_left(range.start.col) as f32;
                let y = sheet.row_top(range.start.row) as f32;
                let width = (sheet.col_left(range.end.col + 1) as f32 - x) * zoom;
                let height = (sheet.row_top(range.end.row + 1) as f32 - y) * zoom;
                let color = reference_color(reference.index);
                layer = layer.child(div().absolute()
                    .left(px((x - scroll_x) * zoom)).top(px((y - scroll_y) * zoom))
                    .w(px(width)).h(px(height)).border_2().border_color(color)
                    .bg(Hsla { a: 0.08, ..color }));
            }
            grid = grid.child(layer);
        }

        if let Some(cut) = self.pending_cut.as_ref().filter(|cut| cut.sheet == self_sheet && cut.is_current(item)) {
            let left = LEFT_W + (sheet.col_left(cut.range.start.col) as f32 - scroll_x) * zoom;
            let top = TOP_H + (sheet.row_top(cut.range.start.row) as f32 - scroll_y) * zoom;
            let width = (sheet.col_left(cut.range.end.col + 1) - sheet.col_left(cut.range.start.col)) as f32 * zoom;
            let height = (sheet.row_top(cut.range.end.row + 1) - sheet.row_top(cut.range.start.row)) as f32 * zoom;
            grid = grid.child(div().absolute().left(px(left)).top(px(top)).w(px(width)).h(px(height)).border_1().border_color(select_border));
        }

        let (r0, r1, c0, c1) = self.sel_range();
        if r0 != r1 || c0 != c1 {
            let left = LEFT_W + (sheet.col_left(c0) as f32 - scroll_x) * zoom;
            let top = TOP_H + (sheet.row_top(r0) as f32 - scroll_y) * zoom;
            let right = LEFT_W + (sheet.col_left(c1) as f32 + sheet.col_width(c1) - scroll_x) * zoom;
            let bottom = TOP_H + (sheet.row_top(r1) as f32 + sheet.row_height(r1) - scroll_y) * zoom;
            grid = grid.child(
                div()
                    .absolute()
                    .left(px(left))
                    .top(px(top))
                    .w(px((right - left).max(1.0)))
                    .h(px((bottom - top).max(1.0)))
                    .bg(Hsla { a: 0.12, ..select_border })
                    .border_1()
                    .border_color(select_border),
            );
        }

        // Draw the active cell on top without shifting its text.
        grid = grid.child(
            div()
                .absolute()
                .left(px(LEFT_W + (sheet.col_left(active_col) as f32 - scroll_x) * zoom))
                .top(px(TOP_H + (sheet.row_top(active_row) as f32 - scroll_y) * zoom))
                .w(px((sheet.col_width(active_col) * zoom).max(1.0)))
                .h(px((sheet.row_height(active_row) * zoom).max(1.0)))
                .border_2()
                .border_color(select_border),
        );

        if self.editing {
            if let Some((left, top, width, height)) = edit_rect {
                let estyle = styles.get(sheet.style_id(CellRef::new(self.edit_row, self.edit_col)));
                let font_size = (estyle.font.size * office_knob_f("sheet.fontScale", 4.0 / 3.0) as f32 * zoom).clamp(1.0, 40.0);
                let st_bold = estyle.font.bold;
                let st_italic = estyle.font.italic;
                let edit_bg = if estyle.fill.pattern == PatternType::Solid {
                    model_color_hsla(&estyle.fill.fg, cell_bg, &wb.theme.colors)
                } else {
                    cell_bg
                };
                self.edit_font_px = font_size;
                self.edit_font_cur = edit_font(&estyle.font.name, st_bold, st_italic);
                let (th, tdiv) = edit_text.take().unwrap_or((height, div().w_full()));
                let h = height.max(th + 2.0);
                self.edit_rect_cache = Some((left, top, width, h));
                let inner = div().w_full().child(tdiv);
                let aligned = if th <= height { match estyle.align.v { gridcraft_model::style::VAlign::Center => div().size_full().flex().flex_col().justify_center().child(inner), gridcraft_model::style::VAlign::Bottom => div().size_full().flex().flex_col().justify_end().child(inner), _ => div().size_full().child(inner) } } else { div().size_full().child(inner) };
                let eo = self.edit_origin.clone();
                let eo2 = eo.clone();
                let handle = cx.entity().downgrade();
                let h2 = handle.clone();
                let h3 = handle.clone();
                edit_el = Some(
                    div().absolute().left(px(left)).top(px(top)).w(px(width)).h(px(h)).overflow_hidden().bg(edit_bg).cursor_text()
                        .child(aligned)
                        .child(div().absolute().top_0().left_0().size_full().border_2().border_color(select_border))
                        .on_mouse_down(gpui::MouseButton::Left, move |ev: &gpui::MouseDownEvent, window, cx| {
                            let (ox, oy) = *eo.lock().unwrap();
                            let x = f32::from(ev.position.x) - ox;
                            let y = f32::from(ev.position.y) - oy;
                            if let Some(v) = handle.upgrade() {
                                v.update(cx, |this, cx| {
                                    let c = this.click_caret(x, y);
                                    this.edit_caret = c;
                                    this.edit_sel_anchor = Some(c);
                                    this.edit_drag = true;
                                    this.edit_mouse = Some((x, y));
                                    let fh = this.focus_handle.clone();
                                    window.focus(&fh, cx);
                                    cx.notify();
                                });
                            }
                            cx.stop_propagation();
                        })
                        .on_mouse_move(move |ev: &MouseMoveEvent, _window, cx| {
                            let (ox, oy) = *eo2.lock().unwrap();
                            let x = f32::from(ev.position.x) - ox;
                            let y = f32::from(ev.position.y) - oy;
                            if let Some(v) = h2.upgrade() {
                                v.update(cx, |this, cx| {
                                    if this.edit_drag && this.editing {
                                        this.edit_caret = this.click_caret(x, y);
                                        this.edit_mouse = Some((x, y));
                                        cx.notify();
                                    }
                                });
                            }
                        })
                        .on_mouse_up(gpui::MouseButton::Left, move |_ev: &MouseUpEvent, _window, cx| {
                            if let Some(v) = h3.upgrade() {
                                v.update(cx, |this, cx| {
                                    this.edit_drag = false;
                                    this.edit_mouse = None;
                                    cx.notify();
                                });
                            }
                        }),
                );
            }
        }

        if let Some(e) = edit_el { grid = grid.child(e); }

        // native scrollbars metrics
        let (content_w, content_h) = self.content_size(cx);
        let grid_w = f32::from(self.grid_size.x);
        let grid_h = f32::from(self.grid_size.y);
        self.scroll.set_metrics(
            gpui::Size::new(px((grid_w - LEFT_W).max(1.0)), px((grid_h - TOP_H).max(1.0))),
            gpui::Size::new(px(content_w * zoom), px(content_h * zoom)),
            zoom,
        );

        // sheet tabs + zoom
        let mut bottom = div()
            .flex()
            .flex_row()
            .items_center()
            .gap_1()
            .px_1()
            .h(px(26.))
            .bg(header_bg)
            .border_t_1()
            .border_color(grid_line);
        for (index, sheet_item) in wb.sheets.iter().enumerate() {
            let name = SharedString::from(sheet_item.name.clone());
            let active = index == self_sheet;
            bottom = bottom.child(
                div()
                    .id(("sheet-tab", index))
                    .px_3()
                    .py(px(3.))
                    .cursor_pointer()
                    .bg(if active { cell_bg } else { gpui::transparent_black() })
                    .hover(move |style| {
                        if active {
                            style
                        } else {
                            style.bg(tab_hover)
                        }
                    })
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, _event, _window, cx| {
                            if this.editing {
                                this.commit_edit(cx);
                            }
                            this.select_sheet(index, cx);
                        }),
                    )
                    .child(
                        Label::new(name)
                            .size(LabelSize::Small)
                            .color(UiColor::Custom(if active { text_color } else { tab_muted })),
                    ),
            );
        }
        bottom = bottom.child(div().flex_1());

        div()
            .size_full()
            .flex()
            .flex_col()
            .bg(colors.panel_background)
            .track_focus(&self.focus_handle)
            .key_context(if self.editing { "SheetEditing" } else { "SheetView" })
            .on_key_down(cx.listener(|this, ev: &KeyDownEvent, window, cx| {
                if this.sheet_key(ev, window, cx) { cx.stop_propagation(); }
            }))
            .on_action(cx.listener(|this, _: &SheetSearchNext, _w, cx| this.search_next(cx)))
            .on_action(cx.listener(|this, _: &SheetSearchPrev, _w, cx| this.search_prev(cx)))
            
            .on_action(cx.listener(|this, _: &Save, window, cx| {
                this.save_changes(cx)
                    .detach_and_prompt_err("Failed to save", window, cx, |err, _, _| Some(format!("{err}")));
            }))
            .on_action(cx.listener(|this, _: &ZoomIn, _window, cx| this.set_zoom(this.zoom + 0.1, cx)))
            .on_action(cx.listener(|this, _: &ZoomOut, _window, cx| this.set_zoom(this.zoom - 0.1, cx)))
            
            .on_action(cx.listener(|this, _: &SheetEditNewline, _window, cx| this.edit_newline(cx)))
            .on_mouse_move(cx.listener(|this, event: &MouseMoveEvent, _window, cx| {
                let Some((axis, idx, start, start_size)) = this.resize else {
                    return;
                };
                let cur = if axis == 0 {
                    f32::from(event.position.y)
                } else {
                    f32::from(event.position.x)
                };
                let size = (start_size + (cur - start) / this.zoom).clamp(8.0, 2000.0);
                this.apply_resize(axis, idx, size, cx);
                cx.notify();
            }))
            .on_mouse_up(MouseButton::Left, cx.listener(|this, _event: &MouseUpEvent, _window, cx| {
                this.drag_select = false;
                this.edit_drag = false;
                if let Some((axis, idx, _start, start_size)) = this.resize.take() {
                    let new = this.line_size(axis, idx, cx);
                    if new != Some(start_size) {
                        this.push_op(if axis == 1 {
                            SheetOp::Col(idx, Some(start_size), new)
                        } else {
                            SheetOp::Row(idx, Some(start_size), new)
                        });
                    }
                }
            }))
            .on_action(cx.listener(|this, _: &SheetFillDown, window, cx| this.fill_selection(true, window, cx)))
            .on_action(cx.listener(|this, _: &SheetFillRight, window, cx| this.fill_selection(false, window, cx)))
            .on_action(cx.listener(|this, _: &SheetSearch, window, cx| this.toggle_search(window, cx)))
            .on_action(cx.listener(|this, _: &SheetSend, window, cx| this.send_selection(window, cx)))
            .on_action(cx.listener(|this, _: &SheetCopy, window, cx| {
                if !this.editing {
                    this.copy_selection(window, cx, false);
                }
            }))
            .on_action(cx.listener(|this, _: &SheetCut, window, cx| {
                if !this.editing {
                    this.copy_selection(window, cx, true);
                }
            }))
            .on_action(cx.listener(|this, _: &SheetPaste, window, cx| {
                if this.editing {
                    return;
                }
                let skipped = this.paste_clipboard(window, cx);
                if skipped > 0 {
                    let ws = window.root::<Workspace>().flatten();
                    this.show_toast(
                        ws,
                        cx,
                        format!("Paste clipped at sheet bounds, skipped cells: {skipped}"),
                    );
                }
            }))
            .on_action(cx.listener(|this, _: &SheetClear, _window, cx| {
                if !this.editing {
                    this.clear_selection(cx);
                }
            }))
            .on_action(cx.listener(|this, _: &SheetZoomReset, _window, cx| this.zoom_reset(cx)))
            .on_action(cx.listener(|this, _: &SheetSelectAll, _window, cx| {
                if !this.editing {
                    this.select_all(cx);
                }
            }))
            .on_action(cx.listener(|this, _: &SheetUndo, _window, cx| {
                if !this.editing {
                    this.undo(cx);
                }
            }))
            .on_action(cx.listener(|this, _: &SheetRedo, _window, cx| {
                if !this.editing {
                    this.redo(cx);
                }
            }))
            .on_action(cx.listener(|this, _: &EditCell, window, cx| {
                    if this.editing {
                        this.commit_edit(cx);
                    } else {
                        this.start_edit(None, window, cx);
                    }
                }))
            .on_action(cx.listener(|this, _: &CommitEdit, _window, cx| this.commit_edit(cx)))
            .on_action(cx.listener(|this, _: &CancelEdit, _window, cx| this.cancel_edit(cx)))
            .on_action(cx.listener(|this, _: &MoveUp, window, cx| this.move_sel(-1, 0, window, cx)))
            .on_action(cx.listener(|this, _: &MoveDown, window, cx| this.move_sel(1, 0, window, cx)))
            .on_action(cx.listener(|this, _: &MoveLeft, window, cx| this.move_sel(0, -1, window, cx)))
            .on_action(cx.listener(|this, _: &MoveRight, window, cx| this.move_sel(0, 1, window, cx)))
            .child(header)
            .children(search_bar)
            .child(formula_bar)
            .child(grid.custom_scrollbars(
                Scrollbars::new(ScrollAxes::Both)
                    .style(ScrollbarStyle::Editor)
                    .thumb_color(office_thumb_color())
                    .tracked_scroll_handle(&self.scroll)
                    .notify_content(),
                window,
                cx,
            ))
            .child(bottom)
            .into_any_element()
    }
}


fn cell_text_align(align: HAlign, numeric: bool) -> TextAlign {
    match align {
        HAlign::Center | HAlign::CenterAcross => TextAlign::Center,
        HAlign::Right | HAlign::Fill | HAlign::Justify => TextAlign::Right,
        HAlign::General if numeric => TextAlign::Right,
        _ => TextAlign::Left,
    }
}

fn cell_editor_align(align: HAlign, numeric: bool, editing_formula: bool) -> TextAlign {
    if editing_formula { TextAlign::Left } else { cell_text_align(align, numeric) }
}

fn reference_color(index: usize) -> Hsla {
    let colors = [0x245ac5, 0xc23841, 0x8351a6, 0x177a72, 0x9a560f, 0xaf396d];
    gpui::rgb(colors[index % colors.len()]).into()
}

fn formula_word_runs(word: &str, start: usize, base: &gpui::TextRun, refs: &[FormulaHighlight]) -> Vec<gpui::TextRun> {
    if refs.is_empty() {
        return Vec::new();
    }
    let refs = &refs[refs.partition_point(|reference| reference.chars.end <= start)..];
    let end = start + word.chars().count();
    if refs.first().is_none_or(|reference| reference.chars.start >= end) {
        return Vec::new();
    }
    let bytes: Vec<_> = word.char_indices().map(|(byte, _)| byte).chain([word.len()]).collect();
    let mut runs = Vec::new();
    let mut next = 0;
    for reference in refs.iter().take_while(|reference| reference.chars.start < end) {
        let from = reference.chars.start.max(start);
        let to = reference.chars.end.min(end);
        if from >= to {
            continue;
        }
        let (from, to) = (bytes[from - start], bytes[to - start]);
        if next < from {
            runs.push(gpui::TextRun { len: from - next, ..base.clone() });
        }
        runs.push(gpui::TextRun { len: to - from, color: reference_color(reference.index), ..base.clone() });
        next = to;
    }
    if !runs.is_empty() && next < word.len() {
        runs.push(gpui::TextRun { len: word.len() - next, ..base.clone() });
    }
    runs
}

fn line_start_px(align: TextAlign, inner_width: f32, right_edge: f32, line_width: f32) -> f32 {
    match align {
        TextAlign::Left => 2.0,
        TextAlign::Center => 2.0 + ((inner_width - line_width) / 2.0).max(0.0),
        TextAlign::Right => (right_edge - line_width).max(2.0),
    }
}

#[allow(clippy::too_many_arguments)]
fn search_styled(text: &str, hits: &[(usize, usize)], cur: Option<(usize, usize)>, color: Hsla, style: &gridcraft_model::style::Style) -> gpui::StyledText {
    let n = text.chars().count();
    let mut b: Vec<usize> = vec![0, n];
    for (x, y) in hits { b.push(*x); b.push(*y); }
    if let Some((x, y)) = cur { b.push(x); b.push(y); }
    b.sort(); b.dedup();
    let byte: Vec<usize> = text.char_indices().map(|(i, _)| i).chain(std::iter::once(text.len())).collect();
    let font = gpui::Font { family: SharedString::from(style.font.name.clone()), weight: if style.font.bold { gpui::FontWeight::BOLD } else { gpui::FontWeight::default() }, style: if style.font.italic { gpui::FontStyle::Italic } else { gpui::FontStyle::default() }, ..Default::default() };
    let mut runs = Vec::new();
    for w in b.windows(2) {
        let (a, bb) = (w[0], w[1]);
        if a >= bb { continue; }
        let is_cur = cur.map(|(ca, cb)| a >= ca && bb <= cb).unwrap_or(false);
        let is_hit = hits.iter().any(|(ha, hb)| a >= *ha && bb <= *hb);
        let bg = if is_cur { Some(gpui::hsla(0.58, 0.9, 0.5, 0.6)) } else if is_hit { Some(gpui::hsla(0.58, 0.8, 0.6, 0.3)) } else { None };
        runs.push(gpui::TextRun { len: byte[bb] - byte[a], font: font.clone(), color, background_color: bg, underline: None, strikethrough: None });
    }
    gpui::StyledText::new(text.to_string()).with_runs(runs)
}

fn char_xs(shaped: &gpui::ShapedLine, word: &str) -> Vec<f32> {
    let chars: Vec<usize> = word.char_indices().map(|(i, _)| i).collect();
    let n = word.chars().count();
    let mut xs = Vec::with_capacity(n + 1);
    for k in 0..=n {
        let b = if k < n { chars[k] } else { word.len() };
        let mut x: Option<f32> = None;
        for r in &shaped.runs { for g in &r.glyphs { if g.index >= b && x.is_none() { x = Some(f32::from(g.position.x)); } } }
        xs.push(x.unwrap_or_else(|| f32::from(shaped.width)));
    }
    xs
}

fn hit_span(shaped: &gpui::ShapedLine, word: &str, cs: usize, aa: usize, bb: usize) -> Option<(f32, f32)> {
    let chars: Vec<usize> = word.char_indices().map(|(i, _)| i).collect();
    let n = word.chars().count();
    let ba = if aa - cs < n { chars[aa - cs] } else { word.len() };
    let bbty = if bb - cs < n { chars[bb - cs] } else { word.len() };
    let mut x0: Option<f32> = None;
    let mut x1: Option<f32> = None;
    for r in &shaped.runs {
        for g in &r.glyphs {
            let gx = f32::from(g.position.x);
            if g.index >= ba && g.index < bbty && x0.is_none() { x0 = Some(gx); }
            if g.index >= bbty && x1.is_none() { x1 = Some(gx); }
        }
    }
    let x0 = x0?;
    let x1 = x1.unwrap_or_else(|| f32::from(shaped.width));
    if x1 > x0 { Some((x0, x1)) } else { None }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn general_number_stays_right_aligned_inside_the_edit_frame() {
        let format = NumberFormat::parse_cached("General");
        let date_system = gridcraft_core::DateSystem::D1900;
        let number = format_value(&Value::Number(1250.0), &format, date_system);
        let text = format_value(&Value::Text("1250".into()), &format, date_system);
        let align = cell_editor_align(HAlign::General, number.numeric, false);
        assert_eq!(align, TextAlign::Right);
        assert_eq!(cell_editor_align(HAlign::General, number.numeric, true), TextAlign::Left);
        assert_eq!(cell_text_align(HAlign::General, text.numeric), TextAlign::Left);
        assert_eq!(cell_text_align(HAlign::Left, number.numeric), TextAlign::Left);

        assert_eq!(line_start_px(align, 102.0, 95.0, 30.0), 65.0);
    }

    #[test]
    fn formula_colors_use_utf8_byte_lengths_for_shaping() {
        let word = "=B2-'Лист'!C2";
        let base = gpui::TextRun { len: word.len(), color: gpui::black(), ..Default::default() };
        let refs = formula_highlights(word, "Лист");
        let runs = formula_word_runs(word, 0, &base, &refs);
        assert_eq!(runs.iter().map(|r| r.len).collect::<Vec<_>>(), [1, 2, 1, "'Лист'!C2".len()]);
        assert_eq!(runs[1].color, reference_color(0));
        assert_eq!(runs[3].color, reference_color(1));
    }
}
