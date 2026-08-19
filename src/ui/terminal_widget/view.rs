use alacritty_terminal::index::Side;
use egui::{
    Area, Color32, CornerRadius, EventFilter, FontFamily, FontId, Frame, IMEPurpose, ImeEvent, Key,
    Modifiers, Order, Pos2, Rect, Sense, Stroke, TextEdit, TextStyle, Ui, Vec2,
};

use crate::config::shortcuts::AppShortcuts;

use super::TerminalEmulator;

const SCROLLBAR_WIDTH: f32 = 10.0;
const SCROLLBAR_GAP: f32 = 4.0;

pub struct TerminalView<'a> {
    pub emulator: &'a mut TerminalEmulator,
    pub shortcuts: &'a AppShortcuts,
}

impl<'a> TerminalView<'a> {
    pub fn show(mut self, ui: &mut Ui) {
        let font_id = FontId::new(13.0, FontFamily::Monospace);
        let row_height = ui.fonts_mut(|f| f.row_height(&font_id)).max(14.0);
        let cell_width = ui.fonts_mut(|f| f.glyph_width(&font_id, 'M')).max(7.0);

        let history_size = self.emulator.history_size();
        let scrollbar_reserve = if history_size > 0 {
            SCROLLBAR_WIDTH + SCROLLBAR_GAP
        } else {
            0.0
        };

        let avail = ui.available_size();
        let term_avail_w = (avail.x - scrollbar_reserve).max(cell_width * 20.0);
        let cols = ((term_avail_w / cell_width).floor() as u16).max(20);
        let rows = ((avail.y / row_height).floor() as u16).max(5);
        self.emulator.resize(cols, rows);

        if self.emulator.find.open {
            self.emulator.recompute_find_matches();
        }

        let snapshot = self.emulator.snapshot();
        let term_w = cols as f32 * cell_width;
        let term_h = rows as f32 * row_height;

        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 0.0;

            let (response, painter) = ui.allocate_painter(
                Vec2::new(term_w, term_h),
                Sense::click_and_drag(),
            );

        // While the terminal has focus, capture Tab, arrows, and Escape so they reach the
        // remote shell instead of being handled by egui (focus navigation, cancel, etc.).
        if response.has_focus() {
            ui.memory_mut(|m| {
                m.set_focus_lock_filter(
                    response.id,
                    EventFilter {
                        tab: true,
                        horizontal_arrows: true,
                        vertical_arrows: true,
                        escape: true,
                    },
                );
            });
        }

        let origin = response.rect.min;
        let bg_default = Color32::from_rgb(0x12, 0x12, 0x14);
        let selection_bg = Color32::from_rgb(0x33, 0x55, 0x88);
        painter.rect_filled(response.rect, 0.0, bg_default);

        for (row_idx, row) in snapshot.rows.iter().enumerate() {
            for (col_idx, cell) in row.iter().enumerate() {
                let x = origin.x + col_idx as f32 * cell_width;
                let y = origin.y + row_idx as f32 * row_height;
                let cell_rect =
                    Rect::from_min_size(Pos2::new(x, y), Vec2::new(cell_width, row_height));
                let bg = if cell.selected {
                    selection_bg
                } else {
                    Color32::from_rgb(cell.bg[0], cell.bg[1], cell.bg[2])
                };
                if bg != bg_default {
                    painter.rect_filled(cell_rect, 0.0, bg);
                }
                // Wide-char spacers occupy a grid column but must not paint a glyph.
                if !cell.spacer && cell.ch != ' ' && cell.ch != '\0' {
                    let fg = Color32::from_rgb(cell.fg[0], cell.fg[1], cell.fg[2]);
                    let glyph_font = FontId::new(
                        font_id.size,
                        if cell.bold {
                            FontFamily::Monospace
                        } else {
                            font_id.family.clone()
                        },
                    );
                    painter.text(
                        Pos2::new(x, y),
                        egui::Align2::LEFT_TOP,
                        cell.ch,
                        glyph_font,
                        fg,
                    );
                }
                if cell.underline && !cell.spacer {
                    let underline_w = if cell.wide {
                        cell_width * 2.0
                    } else {
                        cell_width
                    };
                    painter.line_segment(
                        [
                            Pos2::new(x, y + row_height - 1.0),
                            Pos2::new(x + underline_w, y + row_height - 1.0),
                        ],
                        Stroke::new(1.0, Color32::from_rgb(cell.fg[0], cell.fg[1], cell.fg[2])),
                    );
                }
            }
        }

        let now = ui.ctx().input(|i| i.time);
        let typing_key = egui::Id::new(("term_last_typing", self.emulator as *const _ as usize));
        let mut typing_active = ui.data(|d| {
            let last = d.get_temp::<f64>(typing_key).unwrap_or(0.0);
            now - last < 1.0
        });

        if self.emulator.find.open && !self.emulator.find.matches.is_empty() {
            let display_offset = self.emulator.display_offset() as i32;
            let match_bg = Color32::from_rgba_unmultiplied(255, 220, 0, 60);
            let current_bg = Color32::from_rgba_unmultiplied(255, 140, 0, 160);
            let current_idx = self.emulator.find.current;
            let matches_snapshot: Vec<_> = self.emulator.find.matches.clone();
            for (i, m) in matches_snapshot.iter().enumerate() {
                let start = m.start();
                let end = m.end();
                if start.line != end.line {
                    continue;
                }
                let viewport_row = start.line.0 + display_offset;
                if viewport_row < 0 || viewport_row >= rows as i32 {
                    continue;
                }
                let y = origin.y + viewport_row as f32 * row_height;
                let x0 = origin.x + start.column.0 as f32 * cell_width;
                let x1 = origin.x + (end.column.0 + 1) as f32 * cell_width;
                let color = if Some(i) == current_idx {
                    current_bg
                } else {
                    match_bg
                };
                painter.rect_filled(
                    Rect::from_min_max(Pos2::new(x0, y), Pos2::new(x1, y + row_height)),
                    0.0,
                    color,
                );
            }
        }

        if let Some(reason) = &self.emulator.closed {
            painter.text(
                response.rect.center(),
                egui::Align2::CENTER_CENTER,
                format!("Session closed: {reason}"),
                TextStyle::Heading.resolve(ui.style()),
                Color32::LIGHT_RED,
            );
            if history_size > 0 {
                ui.add_space(SCROLLBAR_GAP);
                render_terminal_scrollbar(ui, self.emulator, term_h, row_height);
            }
            return;
        }

        if response.clicked() {
            response.request_focus();
        }

        // Auto-focus the terminal whenever this tab becomes visible (first
        // appearance or after switching back to it) so keystrokes reach the
        // remote shell immediately.  We track the last frame this terminal
        // was rendered: if it wasn't rendered in the immediately preceding
        // frame the tab was hidden (switched away or first appearance) and
        // we request focus.  This avoids stealing focus from other widgets
        // (like Settings) while both are simultaneously visible in a split.
        let appeared_key = egui::Id::new(("term_appeared", self.emulator as *const _ as usize));
        let frame_nr = ui.ctx().cumulative_frame_nr();
        let last_rendered_frame = ui.data(|d| d.get_temp::<u64>(appeared_key).unwrap_or(0));
        ui.data_mut(|d| d.insert_temp(appeared_key, frame_nr));
        let was_visible_last_frame = last_rendered_frame == frame_nr.saturating_sub(1);
        if !was_visible_last_frame && !response.has_focus() && !self.emulator.find.open {
            response.request_focus();
        }

        let to_cell = |pos: Pos2, display_offset: usize| -> (i32, usize, Side) {
            let col_f = (pos.x - origin.x).max(0.0) / cell_width;
            let col = (col_f as usize).min((cols as usize).saturating_sub(1));
            let frac = col_f - col_f.floor();
            let side = if frac < 0.5 { Side::Left } else { Side::Right };
            let row_f = (pos.y - origin.y) / row_height;
            let row = row_f
                .floor()
                .clamp(0.0, (rows as f32 - 1.0).max(0.0)) as usize;
            let line = row as i32 - display_offset as i32;
            (line, col, side)
        };

        let term_rect = response.rect;
        let edge_scroll_zone = row_height * 1.5;
        let drag_selecting = response.dragged();

        // Capture the pointer-press position so we can use it as the
        // selection anchor.  `drag_started()` fires only after the pointer
        // has moved past a threshold, so `interact_pointer_pos()` at that
        // point may already be on the *next* character.  By recording the
        // press position we guarantee the cell the user originally clicked
        // is included in the selection.
        let press_key = egui::Id::new(("term_press_pos", self.emulator as *const _ as usize));
        let primary_pressed = ui
            .ctx()
            .input(|i| i.pointer.button_pressed(egui::PointerButton::Primary));
        if primary_pressed {
            if let Some(pos) = ui.ctx().input(|i| i.pointer.interact_pos()) {
                if response.rect.contains(pos) {
                    ui.data_mut(|d| d.insert_temp(press_key, pos));
                }
            }
        }

        let shift_click = ui.input(|i| i.modifiers.shift);

        if response.drag_started() {
            // Use the stored press position for the anchor, falling back to
            // the current pointer position if unavailable.
            let anchor_pos: Option<Pos2> = ui.data(|d| d.get_temp(press_key));
            let pos = anchor_pos.or(response.interact_pointer_pos());
            if let Some(pos) = pos {
                let display_offset = self.emulator.display_offset();
                let (line, col, side) = to_cell(pos, display_offset);
                if shift_click {
                    // Shift+drag extends an existing selection (or starts one
                    // from the cursor) instead of replacing the anchor.
                    self.emulator.extend_selection(line, col, side);
                } else {
                    // Always anchor on the Left side so the character under the
                    // cursor is included when dragging forward.
                    self.emulator.begin_selection(line, col, Side::Left);
                }
            }
        } else if response.dragged() {
            if let Some(pos) = ui
                .ctx()
                .pointer_interact_pos()
                .or(response.interact_pointer_pos())
            {
                if pos.y < term_rect.min.y + edge_scroll_zone {
                    self.emulator.scroll(1);
                    ui.ctx().request_repaint();
                } else if pos.y > term_rect.max.y - edge_scroll_zone {
                    self.emulator.scroll(-1);
                    ui.ctx().request_repaint();
                }
                let display_offset = self.emulator.display_offset();
                let (line, col, side) = to_cell(pos, display_offset);
                self.emulator.update_selection(line, col, side);
            }
        }

        let single_click = response.clicked() && !response.dragged();
        if single_click {
            let dblclick_key =
                egui::Id::new(("term_last_click", self.emulator as *const _ as usize));
            let now = ui.ctx().input(|i| i.time);
            let anchor_pos: Option<Pos2> = ui.data(|d| d.get_temp(press_key));
            let pos = anchor_pos.or(response.interact_pointer_pos());
            if let Some(pos) = pos {
                let display_offset = self.emulator.display_offset();
                let (line, col, side) = to_cell(pos, display_offset);

                if shift_click {
                    self.emulator.extend_selection(line, col, side);
                } else {
                    let prev: Option<(f64, i32, usize)> = ui.data(|d| d.get_temp(dblclick_key));
                    let is_double = prev
                        .map(|(t, pl, pc)| (now - t) < 0.4 && pl == line && pc == col)
                        .unwrap_or(false);
                    ui.data_mut(|d| d.insert_temp(dblclick_key, (now, line, col)));

                    if is_double {
                        self.emulator.begin_semantic_selection(line, col);
                    } else {
                        self.emulator.clear_selection();
                    }
                }
            } else if !shift_click {
                self.emulator.clear_selection();
            }
        }

        if response.hovered() && !drag_selecting {
            let scroll_delta = ui.ctx().input(|i| i.smooth_scroll_delta.y);
            if scroll_delta.abs() > 0.5 {
                let lines = (scroll_delta / row_height).round() as i32;
                if lines != 0 {
                    self.emulator.scroll(lines);
                }
            }
        }

        let find_sc = self.shortcuts.find_in_terminal_shortcut();
        if response.has_focus() && ui.ctx().input_mut(|i| i.consume_shortcut(&find_sc)) {
            self.emulator.open_find();
        }

        let clear_sc = self.shortcuts.clear_terminal_shortcut();
        if response.has_focus() && ui.ctx().input_mut(|i| i.consume_shortcut(&clear_sc)) {
            self.emulator.clear_scrollback();
        }

        if self.emulator.find.open {
            self.render_find_bar(ui, response.rect);
        }

        if response.has_focus() {
            let ime_key = egui::Id::new(("term_ime_preedit", self.emulator as *const _ as usize));
            let mut preedit = ui.data(|d| d.get_temp::<String>(ime_key).unwrap_or_default());

            let (cy, cx) = snapshot.cursor;
            let cursor_rect = Rect::from_min_size(
                Pos2::new(
                    origin.x + cx as f32 * cell_width,
                    origin.y + cy as f32 * row_height,
                ),
                Vec2::new(cell_width, row_height),
            );
            // Tell winit to attach the OS IME (速成 / Pinyin / etc.) to this widget.
            // Without this, composition keys are delivered as Event::Text and leak
            // radicals into the PTY one-by-one.
            ui.output_mut(|o| {
                o.mutable_text_under_cursor = true;
                o.ime = Some(egui::output::IMEOutput {
                    purpose: IMEPurpose::Terminal,
                    rect: response.rect,
                    cursor_rect,
                    should_interrupt_composition: false,
                });
            });

            let (events, mods) = ui
                .ctx()
                .input(|input| (input.events.clone(), input.modifiers));
            let paste_text = resolve_paste_text(&events, mods);
            let paste_active = paste_text.as_ref().is_some_and(|t| !t.is_empty());
            let ime_commits = ime_commit_texts(&events);
            for ev in &events {
                match ev {
                    egui::Event::Ime(ImeEvent::Preedit { text, .. }) => preedit = text.clone(),
                    egui::Event::Ime(ImeEvent::Commit(_)) => preedit.clear(),
                    _ => {}
                }
            }

            let mut buf: Vec<u8> = Vec::new();
            // Keys that may also produce `Event::Text`: defer one frame to avoid duplicate bytes
            // when both Key and Text fire (e.g. `:`), but still emit if Text is missing (Vim `:`).
            let mut pending_printable: Option<u8> = None;

            let copy_combo = (mods.command && !mods.shift && !mods.alt)
                || (mods.ctrl && mods.shift && !mods.alt && !mods.command);

            for ev in &events {
                match ev {
                    egui::Event::Copy => {
                        if let Some(text) = self.emulator.selection_text() {
                            if !text.is_empty() {
                                ui.ctx().copy_text(text);
                            }
                        }
                    }
                    egui::Event::Text(text) => {
                        if paste_active {
                            continue;
                        }
                        if mods.command && (text == "c" || text == "C") {
                            continue;
                        }
                        // IME is composing, or this text is the same commit delivered twice.
                        if !preedit.is_empty() || ime_commits.iter().any(|c| c == text) {
                            continue;
                        }
                        if text.len() == 1 {
                            let b = text.as_bytes()[0];
                            if pending_printable == Some(b) {
                                pending_printable = None;
                                buf.push(b);
                            } else {
                                if let Some(p) = pending_printable.take() {
                                    buf.push(p);
                                }
                                buf.push(b);
                            }
                        } else {
                            if let Some(p) = pending_printable.take() {
                                buf.push(p);
                            }
                            buf.extend_from_slice(text.as_bytes());
                        }
                    }
                    egui::Event::Paste(_) => continue,
                    egui::Event::Ime(ImeEvent::Preedit { text, .. }) => {
                        preedit = text.clone();
                    }
                    egui::Event::Ime(ImeEvent::Commit(text)) => {
                        preedit.clear();
                        if paste_active || text.is_empty() {
                            continue;
                        }
                        if let Some(p) = pending_printable.take() {
                            buf.push(p);
                        }
                        if text.len() >= PASTE_BURST_MIN_LEN {
                            if !buf.is_empty() {
                                self.emulator.scroll_to_bottom();
                                self.emulator.send_input(std::mem::take(&mut buf));
                                typing_active = true;
                                ui.data_mut(|d| d.insert_temp(typing_key, now));
                            }
                            self.emulator.scroll_to_bottom();
                            self.emulator.send_paste(text);
                            typing_active = true;
                            ui.data_mut(|d| d.insert_temp(typing_key, now));
                        } else {
                            buf.extend_from_slice(text.as_bytes());
                        }
                    }
                    egui::Event::Ime(_) => {}
                    egui::Event::Key {
                        key,
                        pressed: true,
                        modifiers,
                        ..
                    } => {
                        if !preedit.is_empty()
                            && !modifiers.command
                            && !modifiers.ctrl
                            && !modifiers.mac_cmd
                        {
                            // Let the IME consume composition keystrokes (radicals, numbers).
                            continue;
                        }
                        if is_paste_key(*key, *modifiers) {
                            continue;
                        }
                        if copy_combo && matches!(key, Key::C) {
                            if let Some(text) = self.emulator.selection_text() {
                                if !text.is_empty() {
                                    ui.ctx().copy_text(text);
                                }
                            }
                            continue;
                        }
                        if matches!(key, Key::PageUp) && modifiers.shift {
                            if let Some(p) = pending_printable.take() {
                                buf.push(p);
                            }
                            self.emulator.scroll(rows as i32);
                            continue;
                        }
                        if matches!(key, Key::PageDown) && modifiers.shift {
                            if let Some(p) = pending_printable.take() {
                                buf.push(p);
                            }
                            self.emulator.scroll(-(rows as i32));
                            continue;
                        }
                        if matches!(key, Key::Home) && modifiers.shift {
                            if let Some(p) = pending_printable.take() {
                                buf.push(p);
                            }
                            self.emulator.scroll(i32::MAX / 2);
                            continue;
                        }
                        if matches!(key, Key::End) && modifiers.shift {
                            if let Some(p) = pending_printable.take() {
                                buf.push(p);
                            }
                            self.emulator.scroll_to_bottom();
                            continue;
                        }
                        if let Some(seq) =
                            key_to_bytes(*key, *modifiers, self.emulator.app_cursor_keys())
                        {
                            if let Some(p) = pending_printable.take() {
                                buf.push(p);
                            }
                            buf.extend_from_slice(&seq);
                        } else if let Some(b) = deferred_printable_key_byte(*key, *modifiers) {
                            if let Some(p) = pending_printable.take() {
                                buf.push(p);
                            }
                            pending_printable = Some(b);
                        }
                    }
                    _ => {}
                }
            }

            if let Some(p) = pending_printable.take() {
                buf.push(p);
            }

            if let Some(text) = paste_text {
                if !text.is_empty() {
                    self.emulator.scroll_to_bottom();
                    self.emulator.send_paste(&text);
                    typing_active = true;
                    ui.data_mut(|d| d.insert_temp(typing_key, now));
                }
            }

            if !buf.is_empty() {
                self.emulator.scroll_to_bottom();
                self.emulator.send_input(buf);
                typing_active = true;
                ui.data_mut(|d| d.insert_temp(typing_key, now));
            }

            ui.data_mut(|d| d.insert_temp(ime_key, preedit.clone()));
            if !preedit.is_empty() {
                paint_ime_preedit(
                    &painter,
                    origin,
                    cell_width,
                    row_height,
                    snapshot.cursor,
                    &preedit,
                    &font_id,
                );
                typing_active = true;
            }
        }

        if snapshot.cursor_visible && snapshot.display_offset == 0 {
            let focused = response.has_focus();
            let cursor_blink_on = if focused {
                if typing_active {
                    true
                } else {
                    ui.ctx().request_repaint();
                    let phase = now % 1.0;
                    phase < 0.5
                }
            } else {
                true
            };
            if cursor_blink_on {
                let (cy, cx) = snapshot.cursor;
                let cursor_rect = Rect::from_min_size(
                    Pos2::new(
                        origin.x + cx as f32 * cell_width,
                        origin.y + cy as f32 * row_height,
                    ),
                    Vec2::new(cell_width, row_height),
                );
                if focused {
                    painter.rect_filled(
                        cursor_rect,
                        0.0,
                        Color32::from_rgba_unmultiplied(0xea, 0xea, 0xea, 0xb0),
                    );
                } else {
                    painter.rect_stroke(
                        cursor_rect,
                        0.0,
                        Stroke::new(1.0, Color32::from_rgb(0x66, 0x66, 0x6a)),
                        egui::StrokeKind::Inside,
                    );
                }
            }
        }

            if history_size > 0 {
                ui.add_space(SCROLLBAR_GAP);
                render_terminal_scrollbar(ui, self.emulator, term_h, row_height);
            }
        });
    }

    fn render_find_bar(&mut self, ui: &mut Ui, term_rect: Rect) {
        let anchor = Pos2::new(term_rect.max.x - 12.0, term_rect.min.y + 8.0);
        let just_opened = std::mem::replace(&mut self.emulator.find.just_opened, false);
        let mut close_requested = false;
        let mut goto_next = false;
        let mut goto_prev = false;
        let mut edit_focused = false;

        let enter = ui.ctx().input(|i| i.key_pressed(Key::Enter));
        let shift_held = ui.ctx().input(|i| i.modifiers.shift);

        let pane_salt: usize = self.emulator as *const _ as usize;
        let area_id = egui::Id::new(("terminal-find-bar", pane_salt));
        let edit_id = egui::Id::new(("terminal-find-textedit", pane_salt));

        Area::new(area_id)
            .order(Order::Foreground)
            .fixed_pos(anchor - Vec2::new(340.0, 0.0))
            .show(ui.ctx(), |ui| {
                Frame::popup(ui.style())
                    .fill(Color32::from_rgb(0x20, 0x20, 0x24))
                    .stroke(Stroke::new(1.0, Color32::from_rgb(0x44, 0x44, 0x48)))
                    .corner_radius(CornerRadius::same(6))
                    .inner_margin(6.0)
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            let edit = TextEdit::singleline(&mut self.emulator.find.query)
                                .desired_width(220.0)
                                .hint_text("Find")
                                .font(TextStyle::Monospace);
                            let resp = ui.add(edit.id(edit_id));
                            edit_focused = resp.has_focus();
                            if just_opened {
                                resp.request_focus();
                                edit_focused = true;
                                if let Some(mut state) = TextEdit::load_state(ui.ctx(), edit_id) {
                                    let end = self.emulator.find.query.len();
                                    state.cursor.set_char_range(Some(
                                        egui::text::CCursorRange::two(
                                            egui::text::CCursor::new(0),
                                            egui::text::CCursor::new(end),
                                        ),
                                    ));
                                    state.store(ui.ctx(), edit_id);
                                }
                            }
                            if resp.changed() {
                                self.emulator.recompute_find_matches();
                                self.emulator.find_scroll_to_current();
                            }
                            if resp.has_focus() && enter {
                                if shift_held {
                                    goto_prev = true;
                                } else {
                                    goto_next = true;
                                }
                            }

                            let total = self.emulator.find.matches.len();
                            let idx = self.emulator.find.current.map(|i| i + 1).unwrap_or(0);
                            let counter = if total == 0 {
                                "0/0".to_string()
                            } else {
                                format!("{}/{}", idx, total)
                            };
                            ui.label(
                                egui::RichText::new(counter)
                                    .monospace()
                                    .color(Color32::from_rgb(0xa0, 0xa0, 0xa0)),
                            );

                            if ui
                                .small_button("Prev")
                                .on_hover_text("Previous match (Shift+Enter)")
                                .clicked()
                            {
                                goto_prev = true;
                            }
                            if ui
                                .small_button("Next")
                                .on_hover_text("Next match (Enter)")
                                .clicked()
                            {
                                goto_next = true;
                            }
                            if ui.small_button("x").on_hover_text("Close (Esc)").clicked() {
                                close_requested = true;
                            }
                        });
                    });
            });

        if edit_focused {
            let g_sc_next = egui::KeyboardShortcut::new(Modifiers::COMMAND, Key::G);
            let g_sc_prev =
                egui::KeyboardShortcut::new(Modifiers::COMMAND | Modifiers::SHIFT, Key::G);
            if ui.ctx().input_mut(|i| i.consume_shortcut(&g_sc_prev)) {
                goto_prev = true;
            } else if ui.ctx().input_mut(|i| i.consume_shortcut(&g_sc_next)) {
                goto_next = true;
            }
            if ui.ctx().input(|i| i.key_pressed(Key::Escape)) {
                close_requested = true;
            }
        }

        if goto_next {
            self.emulator.find_goto(true);
        } else if goto_prev {
            self.emulator.find_goto(false);
        }
        if close_requested {
            self.emulator.close_find();
        }
    }
}

fn render_terminal_scrollbar(
    ui: &mut Ui,
    emulator: &mut TerminalEmulator,
    height: f32,
    row_height: f32,
) {
    let history = emulator.history_size();
    if history == 0 {
        return;
    }

    let display_offset = emulator.display_offset();
    let screen_rows = (height / row_height).floor().max(1.0) as usize;

    let (response, painter) = ui.allocate_painter(
        Vec2::new(SCROLLBAR_WIDTH, height),
        Sense::click_and_drag(),
    );

    let track = response.rect;
    let track_bg = Color32::from_rgb(0x18, 0x18, 0x1c);
    let track_border = Color32::from_rgb(0x2e, 0x2e, 0x34);
    painter.rect_filled(track, 0.0, track_bg);
    painter.rect_stroke(
        track,
        0.0,
        Stroke::new(1.0, track_border),
        egui::StrokeKind::Inside,
    );

    let total_lines = history + screen_rows;
    let thumb_h = (track.height() * screen_rows as f32 / total_lines as f32)
        .clamp(20.0, track.height());
    let scroll_range = (track.height() - thumb_h).max(1.0);
    let frac = display_offset as f32 / history as f32;
    let thumb_top = track.min.y + (1.0 - frac) * scroll_range;
    let thumb_rect = Rect::from_min_size(
        Pos2::new(track.min.x + 1.0, thumb_top),
        Vec2::new(track.width() - 2.0, thumb_h),
    );

    let active = response.hovered() || response.dragged();
    let thumb_color = if active {
        Color32::from_rgb(0x90, 0x90, 0x98)
    } else if display_offset > 0 {
        Color32::from_rgb(0x68, 0x68, 0x72)
    } else {
        Color32::from_rgb(0x48, 0x48, 0x50)
    };
    painter.rect_filled(thumb_rect, 2.0, thumb_color);

    if response.clicked() {
        if let Some(pos) = response.interact_pointer_pos() {
            if pos.y < thumb_rect.min.y {
                emulator.page_up();
            } else if pos.y > thumb_rect.max.y {
                emulator.page_down();
            }
        }
    }

    if response.dragged() {
        if let Some(pos) = response.interact_pointer_pos() {
            let rel = ((pos.y - track.min.y) / scroll_range).clamp(0.0, 1.0);
            let offset = ((1.0 - rel) * history as f32).round() as usize;
            emulator.scroll_to_display_offset(offset);
            ui.ctx().request_repaint();
        }
    }

    if response.hovered() || response.dragged() {
        let scroll_delta = ui.ctx().input(|i| i.smooth_scroll_delta.y);
        if scroll_delta.abs() > 0.5 {
            let lines = (scroll_delta / row_height).round() as i32;
            if lines != 0 {
                emulator.scroll(lines);
            }
        }
    }

    if response.hovered() {
        let label = if display_offset > 0 {
            format!("scrollback {display_offset} / {history}")
        } else {
            "live".to_string()
        };
        response.on_hover_text(label);
    }
}

/// Minimum combined `Event::Text` length in one frame to treat as a paste burst
/// (some platforms inject clipboard content as many single-char Text events).
const PASTE_BURST_MIN_LEN: usize = 16;

fn ime_commit_texts(events: &[egui::Event]) -> Vec<String> {
    events
        .iter()
        .filter_map(|ev| match ev {
            egui::Event::Ime(ImeEvent::Commit(text)) if !text.is_empty() => Some(text.clone()),
            _ => None,
        })
        .collect()
}

fn paint_ime_preedit(
    painter: &egui::Painter,
    origin: Pos2,
    cell_width: f32,
    row_height: f32,
    cursor: (usize, usize),
    preedit: &str,
    font_id: &FontId,
) {
    let (cy, cx) = cursor;
    let mut col = cx as f32;
    let y = origin.y + cy as f32 * row_height;
    let fg = Color32::from_rgb(0xea, 0xea, 0xea);
    let bg = Color32::from_rgba_unmultiplied(0x55, 0x55, 0x88, 0xb0);
    for ch in preedit.chars() {
        if ch == '\n' || ch == '\r' {
            continue;
        }
        let wide = unicode_char_wide(ch);
        let w = if wide { cell_width * 2.0 } else { cell_width };
        let x = origin.x + col * cell_width;
        let rect = Rect::from_min_size(Pos2::new(x, y), Vec2::new(w, row_height));
        painter.rect_filled(rect, 0.0, bg);
        painter.text(Pos2::new(x, y), egui::Align2::LEFT_TOP, ch, font_id.clone(), fg);
        painter.line_segment(
            [
                Pos2::new(x, y + row_height - 1.0),
                Pos2::new(x + w, y + row_height - 1.0),
            ],
            Stroke::new(1.0, fg),
        );
        col += if wide { 2.0 } else { 1.0 };
    }
}

fn unicode_char_wide(ch: char) -> bool {
    // East Asian wide / fullwidth — enough for CJK IME preedit.
    matches!(ch as u32, 0x1100..=0x115F
        | 0x2329..=0x232A
        | 0x2E80..=0xA4CF
        | 0xAC00..=0xD7A3
        | 0xF900..=0xFAFF
        | 0xFE10..=0xFE19
        | 0xFE30..=0xFE6F
        | 0xFF00..=0xFF60
        | 0xFFE0..=0xFFE6
        | 0x1F300..=0x1FAFF)
}

fn is_paste_key(key: Key, modifiers: Modifiers) -> bool {
    match key {
        Key::V if (modifiers.command && !modifiers.shift) || (modifiers.ctrl && modifiers.shift) => {
            true
        }
        Key::Insert if modifiers.shift => true,
        Key::Paste => true,
        _ => false,
    }
}

fn read_clipboard_text() -> Option<String> {
    arboard::Clipboard::new()
        .ok()
        .and_then(|mut cb| cb.get_text().ok())
        .map(|s| s.replace("\r\n", "\n").replace('\r', "\n"))
        .filter(|s| !s.is_empty())
}

fn collect_burst_text(events: &[egui::Event], mods: Modifiers) -> Option<String> {
    if mods.command || mods.ctrl || mods.mac_cmd {
        return None;
    }
    let mut burst = String::new();
    for ev in events {
        if let egui::Event::Text(t) = ev {
            burst.push_str(t);
        }
    }
    if burst.len() >= PASTE_BURST_MIN_LEN {
        Some(burst)
    } else {
        None
    }
}

fn resolve_paste_text(events: &[egui::Event], mods: Modifiers) -> Option<String> {
    for ev in events {
        if let egui::Event::Paste(text) = ev {
            if !text.is_empty() {
                return Some(text.replace("\r\n", "\n").replace('\r', "\n"));
            }
        }
    }

    let shortcut_pressed = events.iter().any(|ev| {
        matches!(
            ev,
            egui::Event::Key {
                key,
                pressed: true,
                modifiers,
                ..
            } if is_paste_key(*key, *modifiers)
        )
    });

    if shortcut_pressed {
        if let Some(text) = read_clipboard_text() {
            return Some(text);
        }
    }

    collect_burst_text(events, mods)
}

/// Single-byte keys where winit/egui often sends `Event::Text` as well as `Event::Key`.
/// When Text is missing (some hosts / IME paths), we still need the byte on the PTY.
fn deferred_printable_key_byte(key: Key, mods: Modifiers) -> Option<u8> {
    if mods.ctrl || mods.alt || mods.command || mods.mac_cmd {
        return None;
    }
    use Key::*;
    match key {
        Colon => Some(b':'),
        Semicolon if mods.shift => Some(b':'),
        Semicolon => Some(b';'),
        Slash if mods.shift => Some(b'?'),
        Slash => Some(b'/'),
        Questionmark => Some(b'?'),
        _ => None,
    }
}

fn key_to_bytes(key: Key, mods: Modifiers, app_cursor: bool) -> Option<Vec<u8>> {
    use Key::*;
    // Option/Alt + horizontal arrows: readline / zsh emacs-style word motion (Meta-b / Meta-f).
    if mods.alt && !mods.ctrl && !mods.command && !mods.mac_cmd {
        match key {
            ArrowLeft => return Some(vec![b'\x1b', b'b']),
            ArrowRight => return Some(vec![b'\x1b', b'f']),
            _ => {}
        }
    }
    if mods.ctrl && !mods.shift && !mods.alt {
        if let Some(b) = ctrl_byte(key) {
            return Some(vec![b]);
        }
    }
    let bytes: &[u8] = if app_cursor {
        match key {
            ArrowUp => b"\x1bOA",
            ArrowDown => b"\x1bOB",
            ArrowRight => b"\x1bOC",
            ArrowLeft => b"\x1bOD",
            Home => b"\x1bOH",
            End => b"\x1bOF",
            _ => cursor_key_bytes_normal(key)?,
        }
    } else {
        cursor_key_bytes_normal(key)?
    };
    Some(bytes.to_vec())
}

fn cursor_key_bytes_normal(key: Key) -> Option<&'static [u8]> {
    use Key::*;
    Some(match key {
        Enter => b"\r",
        Tab => b"\t",
        Backspace => b"\x7f",
        Escape => b"\x1b",
        ArrowUp => b"\x1b[A",
        ArrowDown => b"\x1b[B",
        ArrowRight => b"\x1b[C",
        ArrowLeft => b"\x1b[D",
        Home => b"\x1b[H",
        End => b"\x1b[F",
        PageUp => b"\x1b[5~",
        PageDown => b"\x1b[6~",
        Delete => b"\x1b[3~",
        Insert => b"\x1b[2~",
        F1 => b"\x1bOP",
        F2 => b"\x1bOQ",
        F3 => b"\x1bOR",
        F4 => b"\x1bOS",
        F5 => b"\x1b[15~",
        F6 => b"\x1b[17~",
        F7 => b"\x1b[18~",
        F8 => b"\x1b[19~",
        F9 => b"\x1b[20~",
        F10 => b"\x1b[21~",
        F11 => b"\x1b[23~",
        F12 => b"\x1b[24~",
        _ => return None,
    })
}

fn ctrl_byte(key: Key) -> Option<u8> {
    use Key::*;
    let c = match key {
        A => b'a',
        B => b'b',
        C => b'c',
        D => b'd',
        E => b'e',
        F => b'f',
        G => b'g',
        H => b'h',
        I => b'i',
        J => b'j',
        K => b'k',
        L => b'l',
        M => b'm',
        N => b'n',
        O => b'o',
        P => b'p',
        Q => b'q',
        R => b'r',
        S => b's',
        T => b't',
        U => b'u',
        V => b'v',
        W => b'w',
        X => b'x',
        Y => b'y',
        Z => b'z',
        OpenBracket => b'[',
        CloseBracket => b']',
        Backslash => b'\\',
        _ => return None,
    };
    Some(c & 0x1f)
}
