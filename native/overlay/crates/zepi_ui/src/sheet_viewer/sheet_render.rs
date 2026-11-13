use gpui::{HighlightStyle, StyledText};

use super::*;

impl gpui::Render for SheetView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.focus_search_on_render {
            self.focus_search_on_render = false;
            if let Some(editor) = self.search_editor.as_ref() {
                let handle = editor.read(cx).focus_handle(cx);
                window.focus(&handle, cx);
            }
        }
        let colors = cx.theme().colors();
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
        let sel_row = self.sel_row;
        let sel_col = self.sel_col;
        let date_system = wb.date_system;
        let styles = &wb.styles;

        let grid_line: Hsla = gpui::rgb(0xd4d4d4).into();
        let header_bg: Hsla = gpui::rgb(0xf0f0f0).into();
        let header_sel: Hsla = gpui::rgb(0xcfcfcf).into();
        let text_color: Hsla = gpui::rgb(0x202020).into();
        let cell_bg: Hsla = gpui::rgb(0xffffff).into();
        let bar_muted: Hsla = gpui::rgb(0x606060).into();
        let tab_muted: Hsla = gpui::rgb(0x606060).into();
        let tab_hover: Hsla = gpui::rgb(0xe4e4e4).into();

        let edit_rect = if self.editing {
            let (last_row, last_col) = self.used_bounds(cx);
            if sel_row <= last_row && sel_col <= last_col {
                let left = LEFT_W + (sheet.col_left(sel_col) as f32 - scroll_x) * zoom;
                let top = TOP_H + (sheet.row_top(sel_row) as f32 - scroll_y) * zoom;
                let width = sheet.col_width(sel_col) * zoom;
                let height = sheet.row_height(sel_row) * zoom;
                let gw = f32::from(self.grid_size.x);
                let gh = f32::from(self.grid_size.y);
                if left < gw && top < gh && left + width > 0.0 && top + height > 0.0 {
                    Some((left, top, width, height))
                } else {
                    None
                }
            } else {
                None
            }
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
        if self.search_open {
            if let Some(editor) = self.search_editor.clone() {
                header = header.child(
                    div()
                        .w(px(240.))
                        .h(px(24.))
                        .key_context("SheetSearch")
                        .on_action(cx.listener(|this, _: &SheetSearchNext, _window, cx| this.search_next(cx)))
                        .on_action(cx.listener(|this, _: &SheetSearch, window, cx| this.toggle_search(window, cx)))
                        .child(editor),
                );
            }
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
                    .tooltip(Tooltip::for_action_title("Search", &SheetSearch))
                    .on_click({
                        let view = view_for_header.clone();
                        move |_, window, cx| {
                            let _ = view.update(cx, |this, cx| this.toggle_search(window, cx));
                        }
                    }),
            );

        // formula bar
        let bar_text = self.bar_text(cx);
        let bar_content = if self.editing {
            {
                div()
                    .flex_1()
                    .id("sheet-bar-editing")
                    .child(
                        Label::new(if self.edit_buffer.is_empty() {
                            SharedString::from(" ")
                        } else {
                            SharedString::from(self.edit_buffer.clone())
                        })
                        .size(LabelSize::Small)
                        .color(UiColor::Custom(text_color)),
                    )
            }
        } else {
            div()
                .flex_1()
                .id("sheet-bar-content")
                .cursor_pointer()
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, _event, window, cx| this.start_edit(window, cx)),
                )
                .child(
                    Label::new(if bar_text.is_empty() {
                        SharedString::from(" ")
                    } else {
                        SharedString::from(bar_text.clone())
                    })
                    .size(LabelSize::Small)
                    .color(if bar_text.is_empty() { UiColor::Muted } else { UiColor::Default }),
                )
        };
        let formula_bar = div()
            .flex()
            .flex_row()
            .items_center()
            .gap_2()
            .px_2()
            .h(px(30.))
            .bg(cell_bg)
            .border_b_1()
            .border_color(grid_line)
            .child(
                div()
                    .w(px(56.))
                    .h(px(20.))
                    .border_1()
                    .border_color(grid_line)
                    .rounded_sm()
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(
                        Label::new(cell_ref_text(sel_row, sel_col))
                            .size(LabelSize::Small)
                            .color(UiColor::Custom(text_color)),
                    ),
            )
            .child(Label::new("fx").size(LabelSize::Small).color(UiColor::Custom(bar_muted)))
            .child(bar_content);

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

        // sheet surface keeps the Excel light palette regardless of app theme
        let select_border: Hsla = gpui::rgb(0x3b82f6).into();

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
                            if gx >= l && gx <= l + w && gy >= t && gy <= t + h {
                                let click_caret =
                                    this.click_caret(gx - l, gy - t, w, window, cx);
                                if window.modifiers().shift {
                                    this.edit_sel_anchor.get_or_insert(this.edit_caret);
                                } else {
                                    this.edit_sel_anchor = Some(click_caret);
                                }
                                this.edit_caret = click_caret;
                                this.edit_drag = true;
                                cx.notify();
                                return;
                            }
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
                        this.start_edit(window, cx);
                        if let Some((l, t, w, _h)) = this.edit_cell_rect(cx) {
                            let at = this.click_caret(gx - l, gy - t, w, window, cx);
                            this.edit_caret = at;
                            this.edit_sel_anchor = Some(at);
                        }
                    } else {
                        window.focus(&this.focus_handle, cx);
                    }
                    cx.notify();
                }),
            )
            .on_mouse_move(cx.listener(|this, event: &MouseMoveEvent, window, cx| {
                if this.editing && this.edit_drag {
                    if let Some((l, t, w, h)) = this.edit_rect_cache {
                        let gx = f32::from(event.position.x - this.grid_origin.x);
                        let gy = f32::from(event.position.y - this.grid_origin.y);
                        if gx >= l && gx <= l + w && gy >= t && gy <= t + h {
                            this.edit_caret = this.click_caret(gx - l, gy - t, w, window, cx);
                            cx.notify();
                        }
                    }
                    return;
                }
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
                    .bg(if col == sel_col { header_sel } else { header_bg })
                    .border_r_1()
                    .border_b_1()
                    .border_color(grid_line)
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(Label::new(col_name(col)).size(LabelSize::Small).color(UiColor::Muted)),
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
                    .bg(if row == sel_row { header_sel } else { header_bg })
                    .border_r_1()
                    .border_b_1()
                    .border_color(grid_line)
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(Label::new((row + 1).to_string()).size(LabelSize::Small).color(UiColor::Muted)),
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

        // cells
        for row in first_row..=last_visible_row {
            let height = sheet.row_height(row) * zoom;
            if height <= 0.0 {
                continue;
            }
            let top = TOP_H + (sheet.row_top(row) as f32 - scroll_y) * zoom;
            for col in first_col..=last_visible_col {
                let width = sheet.col_width(col) * zoom;
                if width <= 0.0 {
                    continue;
                }
                let left = LEFT_W + (sheet.col_left(col) as f32 - scroll_x) * zoom;
                let at = CellRef::new(row, col);
                let cell = sheet.cell(at);
                let style_id = sheet.style_id(at);
                let style = styles.get(style_id);
                let mut cell_div = div()
                    .absolute()
                    .left(px(left))
                    .top(px(top))
                    .w(px(width))
                    .h(px(height))
                    .border_r_1()
                    .border_b_1()
                    .border_color(grid_line)
                    .bg(cell_bg)
                    .overflow_hidden();
                let fill_hsla: Option<Hsla> = match style.fill.pattern {
                    PatternType::Solid => Some(model_color_hsla(&style.fill.fg, header_bg, &wb.theme.colors)),
                    _ => None,
                };
                if let Some(fill) = fill_hsla {
                    cell_div = cell_div.bg(fill);
                }
                let b = &style.borders;
                for (side, line) in [(0usize, b.top), (1, b.right), (2, b.bottom), (3, b.left)] {
                    let w = line.style.width();
                    if line.is_none() || w <= 0.0 {
                        continue;
                    }
                    let color = model_color_hsla(&line.color, grid_line, &wb.theme.colors);
                    let edge = div().absolute().bg(color);
                    let edge = match side {
                        0 => edge.left_0().top_0().right_0().h(px(w)),
                        1 => edge.top_0().right_0().bottom_0().w(px(w)),
                        2 => edge.left_0().bottom_0().right_0().h(px(w)),
                        _ => edge.top_0().left_0().bottom_0().w(px(w)),
                    };
                    cell_div = cell_div.child(edge);
                }
                if let Some(cell) = cell {
                    let fmt = NumberFormat::parse_cached(&style.num_fmt.0);
                    let formatted = format_value(&cell.value, &fmt, date_system);
                    if !formatted.text.is_empty() {
                        let font_size = (style.font.size * 4.0 / 3.0 * zoom).clamp(8.0, 40.0);
                        let color = model_color_hsla(&style.font.color, text_color, &wb.theme.colors);
                        let numeric = formatted.numeric;
                        let mut text_div = div()
                            .size_full()
                            .flex()
                            .text_size(px(font_size))
                            .text_color(color)
                            .font_family(SharedString::from(style.font.name.clone()))
                            .child(SharedString::from(formatted.text.clone()));
                        text_div = match style.align.v {
                            gridcraft_model::style::VAlign::Top => text_div.items_start(),
                            gridcraft_model::style::VAlign::Center => text_div.items_center(),
                            _ => text_div.items_end(),
                        };
                        match style.align.h {
                            gridcraft_model::style::HAlign::Center
                            | gridcraft_model::style::HAlign::CenterAcross => {
                                text_div = text_div.justify_center();
                            }
                            gridcraft_model::style::HAlign::Right
                            | gridcraft_model::style::HAlign::Fill
                            | gridcraft_model::style::HAlign::Justify => {
                                text_div = text_div.justify_end().pr(px(2.));
                            }
                            gridcraft_model::style::HAlign::General if numeric => {
                                text_div = text_div.justify_end().pr_1();
                            }
                            _ => {
                                text_div = text_div.pl(px(2.));
                            }
                        }
                        if style.align.wrap {
                            text_div = text_div.whitespace_normal().overflow_hidden();
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
                        cell_div = cell_div.child(text_div);
                    }
                }
                grid = grid.child(cell_div);
            }
        }

        // selection frame as an overlay: a thicker border on the cell div shifts its text
        {
            let left = LEFT_W + (sheet.col_left(sel_col) as f32 - scroll_x) * zoom;
            let top = TOP_H + (sheet.row_top(sel_row) as f32 - scroll_y) * zoom;
            grid = grid.child(
                div()
                    .absolute()
                    .left(px(left))
                    .top(px(top))
                    .w(px((sheet.col_width(sel_col) * zoom).max(1.0)))
                    .h(px((sheet.row_height(sel_row) * zoom).max(1.0)))
                    .border_2()
                    .border_color(select_border),
            );
        }

        if let Some((ar, ac)) = self.anchor {
            let (r0, r1) = (ar.min(sel_row), ar.max(sel_row));
            let (c0, c1) = (ac.min(sel_col), ac.max(sel_col));
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
                        .bg(gpui::hsla(0.6, 0.8, 0.5, 0.15))
                        .border_1()
                        .border_color(select_border),
                );
            }
        }

        if self.editing {
            if let Some((left, top, width, height)) = edit_rect {
                // the caret is a separate 1px bar: a glyph in the text flow would
                // shift every character after it and reflow the wraps
                let bytes_up_to = |i: usize| -> usize {
                    self.edit_buffer.chars().take(i).map(|c| c.len_utf8()).sum()
                };
                let mut highlights = Vec::new();
                if let Some(a) = self.edit_sel_anchor {
                    if a != self.edit_caret {
                        let (s0, s1) = (a.min(self.edit_caret), a.max(self.edit_caret));
                        highlights.push((
                            bytes_up_to(s0)..bytes_up_to(s1),
                            HighlightStyle {
                                background_color: Some(gpui::hsla(0.6, 0.8, 0.5, 0.35)),
                                ..Default::default()
                            },
                        ));
                    }
                }
                let styled = StyledText::new(SharedString::from(self.edit_buffer.clone()))
                    .with_highlights(highlights);
                // mirror the cell own text style so entering edit does not jump
                let estyle = styles.get(sheet.style_id(CellRef::new(self.edit_row, self.edit_col)));
                let font_size = (estyle.font.size * 4.0 / 3.0 * zoom).clamp(8.0, 40.0);
                self.edit_rect_cache = Some((left, top, width, height));
                self.edit_font_px = font_size;
                let edit_color = model_color_hsla(&estyle.font.color, text_color, &wb.theme.colors);
                let mut edit_text = div()
                    .size_full()
                    .flex()
                    .whitespace_normal()
                    .overflow_hidden()
                    .text_size(px(font_size))
                    .text_color(edit_color)
                    .font_family(SharedString::from(estyle.font.name.clone()))
                    .child(styled);
                edit_text = match estyle.align.v {
                    gridcraft_model::style::VAlign::Top => edit_text.items_start(),
                    gridcraft_model::style::VAlign::Center => edit_text.items_center(),
                    _ => edit_text.items_end(),
                };
                match estyle.align.h {
                    gridcraft_model::style::HAlign::Center
                    | gridcraft_model::style::HAlign::CenterAcross => {
                        edit_text = edit_text.justify_center();
                    }
                    gridcraft_model::style::HAlign::Right
                    | gridcraft_model::style::HAlign::Fill
                    | gridcraft_model::style::HAlign::Justify => {
                        edit_text = edit_text.justify_end().pr(px(3.));
                    }
                    _ => {
                        edit_text = edit_text.pl(px(3.));
                    }
                }
                if estyle.font.bold {
                    edit_text = edit_text.font_weight(gpui::FontWeight::BOLD);
                }
                if estyle.font.italic {
                    edit_text = edit_text.italic();
                }
                let efont = edit_font(&estyle.font.name, estyle.font.bold, estyle.font.italic);
                self.edit_font_cur = efont.clone();
                let (caret_x, caret_y) = self.caret_pos(left, top, width, font_size, window);
                let mut overlay = div()
                    .absolute()
                    .left(px(left))
                    .top(px(top))
                    .w(px(width))
                    .h(px(height))
                    .bg(cell_bg)
                    .cursor_text()
                    .overflow_hidden()
                    .pt(px(1.))
                    .pb(px(1.))
                    .child(edit_text);
                if self.caret_on {
                    overlay = overlay.child(
                        div()
                            .absolute()
                            .left(px(caret_x - left))
                            .top(px(caret_y - top))
                            .w(px(1.0))
                            .h(px((font_size * 1.1).max(8.0)))
                            .bg(text_color),
                    );
                }
                grid = grid.child(overlay).child(
                    div()
                        .absolute()
                        .left(px(left))
                        .top(px(top))
                        .w(px(width))
                        .h(px(height))
                        .border_2()
                        .border_color(select_border),
                );
            }
        }

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
                            this.sheet = index;
                            this.scroll.set_pos(0.0, 0.0);
                            this.clamp_sel(cx);
                            cx.notify();
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
            .on_action(cx.listener(|this, _: &Save, window, cx| this.save(window, cx)))
            .on_action(cx.listener(|this, _: &ZoomIn, _window, cx| this.set_zoom(this.zoom + 0.1, cx)))
            .on_action(cx.listener(|this, _: &ZoomOut, _window, cx| this.set_zoom(this.zoom - 0.1, cx)))
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                if !this.editing {
                    return;
                }
                let ks = &event.keystroke;
                if ks.modifiers.control || ks.modifiers.alt || ks.modifiers.platform {
                    return;
                }
                let mut chars: Vec<char> = this.edit_buffer.chars().collect();
                let caret = this.edit_caret.min(chars.len());
                match ks.key.as_str() {
                    "backspace" => {
                        if caret > 0 {
                            chars.remove(caret - 1);
                            this.edit_caret = caret - 1;
                            this.edit_sel_anchor = Some(this.edit_caret);
                        }
                    }
                    "delete" => {
                        if caret < chars.len() {
                            chars.remove(caret);
                        }
                    }
                    "left" | "right" | "home" | "end" => {
                        let new_caret = match ks.key.as_str() {
                            "left" => caret.saturating_sub(1),
                            "right" => (caret + 1).min(chars.len()),
                            "home" => 0,
                            _ => chars.len(),
                        };
                        if window.modifiers().shift {
                            this.edit_sel_anchor.get_or_insert(caret);
                        } else {
                            this.edit_sel_anchor = Some(new_caret);
                        }
                        this.edit_caret = new_caret;
                    }
                    _ => {
                        let Some(ch) = ks.key_char.as_ref() else { return };
                        let mut it = ch.chars();
                        let (Some(c), None) = (it.next(), it.next()) else { return };
                        if c.is_control() {
                            return;
                        }
                        chars.insert(caret, c);
                        this.edit_caret = caret + 1;
                        this.edit_sel_anchor = Some(this.edit_caret);
                    }
                }
                this.edit_buffer = chars.into_iter().collect();
                cx.notify();
            }))
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
            .on_mouse_up(MouseButton::Left, cx.listener(|this, _event: &MouseUpEvent, window, cx| {
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
                        this.save(window, cx);
                    }
                }
            }))
            .on_action(cx.listener(|this, _: &SheetSearch, window, cx| this.toggle_search(window, cx)))
            .on_action(cx.listener(|this, _: &SheetSend, window, cx| this.send_selection(window, cx)))
            .on_action(cx.listener(|this, _: &SheetCopy, _window, cx| {
                if !this.editing {
                    this.copy_selection(cx);
                }
            }))
            .on_action(cx.listener(|this, _: &SheetCut, _window, cx| {
                if !this.editing {
                    this.cut_selection(cx);
                }
            }))
            .on_action(cx.listener(|this, _: &SheetPaste, window, cx| {
                if this.editing {
                    return;
                }
                let skipped = this.paste_clipboard(cx);
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
                        this.start_edit(window, cx);
                    }
                }))
            .on_action(cx.listener(|this, _: &CancelEdit, _window, cx| this.cancel_edit(cx)))
            .on_action(cx.listener(|this, _: &MoveUp, window, cx| this.move_sel(-1, 0, window, cx)))
            .on_action(cx.listener(|this, _: &MoveDown, window, cx| this.move_sel(1, 0, window, cx)))
            .on_action(cx.listener(|this, _: &MoveLeft, window, cx| this.move_sel(0, -1, window, cx)))
            .on_action(cx.listener(|this, _: &MoveRight, window, cx| this.move_sel(0, 1, window, cx)))
            .child(header)
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
