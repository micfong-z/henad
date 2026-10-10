//! Code view of the Editor tab: one text field over a whole model file, with no wrapping, and a gutter that numbers
//! its lines.
//!
//! The gutter reads the rows of the field's galley. With wrapping off, each row is one line of the text, and a text
//! that ends in a line break ends in an empty row of its own. The field scrolls in both directions inside one scroll
//! area, and the gutter sits outside it. The numbers follow the rows up and down and stay in place while a long line
//! scrolls sideways.

use std::sync::Arc;

use egui::text::{CCursor, CCursorRange, LayoutJob};
use egui::{Align, Align2, Color32, FontId, Galley, Id, ScrollArea, Sense, TextBuffer, TextEdit, TextStyle, Ui, vec2};

/// Salt of the code field's id. A press on a diagnostic moves the field's cursor through it.
const CODE_ID_SALT: &str = "henad_editor_code";

/// Space in points between the gutter's numbers and the field.
const GUTTER_PADDING: f32 = 8.0;

/// Width in points past the longest line, which keeps the cursor at a line's end in view.
const LINE_END_SLACK: f32 = 16.0;

/// Returns the id of the code field.
pub fn code_id() -> Id {
    Id::new(CODE_ID_SALT)
}

/// Draws the code field over `text` beside its gutter, filling the space left in `ui`, and returns the field's
/// response.
///
/// The number of each line in `marked_lines`, counted from 1, is drawn in the error colour. `scroll_to` is a
/// character of `text` the view scrolls to.
pub fn code_view(ui: &mut Ui, text: &mut String, marked_lines: &[u32], scroll_to: Option<usize>) -> egui::Response {
    let font_id = TextStyle::Monospace.resolve(ui.style());
    let line_height = ui.fonts_mut(|fonts| fonts.row_height(&font_id)) + ui.spacing().extra_text_line_spacing;
    let color = ui.visuals().text_color();
    let mut layouter = |ui: &Ui, buffer: &dyn TextBuffer, _wrap_width: f32| -> Arc<Galley> {
        ui.fonts_mut(|fonts| fonts.layout_job(unwrapped_job(buffer.as_str(), &font_id, color, line_height)))
    };

    let lines = text.bytes().filter(|&byte| byte == b'\n').count() + 1;
    let digit_width = ui.fonts_mut(|fonts| fonts.glyph_width(&font_id, '0'));
    let gutter_width = digit_count(lines) as f32 * digit_width + GUTTER_PADDING;
    let text_width = layouter(ui, &*text, f32::INFINITY).size().x;

    ui.horizontal_top(|ui| {
        ui.spacing_mut().item_spacing.x = 0.0;
        let (gutter, _) = ui.allocate_exact_size(vec2(gutter_width, ui.available_height()), Sense::hover());
        let output = ScrollArea::both()
            .id_salt("henad_editor_code_scroll")
            .auto_shrink([false; 2])
            .show(ui, |ui| {
                // As wide as the longest line, so a long line scrolls into view where a narrower field would clip it,
                // and as tall as the view, so a press below the last line still focuses the field.
                let width = (text_width + LINE_END_SLACK).max(ui.available_width());
                let height = ui.available_height();
                let output = ui
                    .allocate_ui(vec2(width, height), |ui| {
                        TextEdit::multiline(text)
                            .id(code_id())
                            .code_editor()
                            .desired_width(width)
                            .min_size(vec2(width, height))
                            .layouter(&mut layouter)
                            .show(ui)
                    })
                    .inner;
                if let Some(index) = scroll_to {
                    let cursor = output
                        .galley
                        .pos_from_cursor(CCursor::new(index))
                        .translate(output.galley_pos.to_vec2());
                    ui.scroll_to_rect(cursor.expand(line_height), Some(Align::Center));
                }
                output
            })
            .inner;

        let numbers = gutter_numbers(output.galley.rows.iter().map(|row| row.ends_with_newline));
        // The rows sit where the scroll area put them, and the painter clips the numbers past the view.
        let painter = ui.painter_at(gutter);
        for (row, number) in output.galley.rows.iter().zip(numbers) {
            let top = output.galley_pos.y + row.pos.y;
            if top > gutter.bottom() {
                break;
            }
            let Some(number) = number.filter(|_| top + line_height >= gutter.top()) else {
                continue;
            };
            let marked = u32::try_from(number).is_ok_and(|line| marked_lines.contains(&line));
            let color = if marked {
                ui.visuals().error_fg_color
            } else {
                ui.visuals().weak_text_color()
            };
            painter.text(
                egui::pos2(gutter.right() - GUTTER_PADDING, top),
                Align2::RIGHT_TOP,
                number.to_string(),
                font_id.clone(),
                color,
            );
        }
        output.response.response
    })
    .inner
}

/// Returns the layout of `text` in `font_id` and `color`, on rows `line_height` high, with no wrapping.
fn unwrapped_job(text: &str, font_id: &FontId, color: Color32, line_height: f32) -> LayoutJob {
    let mut job = LayoutJob::simple(text.to_owned(), font_id.clone(), color, f32::INFINITY);
    // Trailing spaces are part of the code, and the cursor has to reach them.
    job.keep_trailing_whitespace = true;
    for section in &mut job.sections {
        section.format.line_height = Some(line_height);
    }
    job
}

/// Number of decimal digits of `count`.
fn digit_count(count: usize) -> usize {
    count.checked_ilog10().map_or(1, |log| log as usize + 1)
}

/// Returns the line number the gutter shows beside each row of a galley, given whether each row ends in a line break.
///
/// A row starts a line when it is the first row or the row above it ends in a line break. A row that continues a
/// wrapped line gets `None`.
pub fn gutter_numbers(rows_end_with_newline: impl IntoIterator<Item = bool>) -> Vec<Option<usize>> {
    let mut line = 0;
    let mut starts_line = true;
    rows_end_with_newline
        .into_iter()
        .map(|ends_with_newline| {
            let number = starts_line.then(|| {
                line += 1;
                line
            });
            starts_line = ends_with_newline;
            number
        })
        .collect()
}

/// Returns the character index of `column` on `line` of `text`, both counted from 1, the column in characters.
///
/// A line past the end of `text` gives the end of `text`, and a column past the end of its line gives the line's end.
pub fn char_index_at(text: &str, line: u32, column: u32) -> usize {
    let mut index = 0;
    let mut lines = text.split('\n');
    for _ in 1..line {
        match lines.next() {
            Some(skipped) => index += skipped.chars().count() + 1,
            None => return text.chars().count(),
        }
    }
    let Some(target) = lines.next() else {
        return text.chars().count();
    };
    let column = usize::try_from(column.saturating_sub(1)).unwrap_or(usize::MAX);
    index + column.min(target.trim_end_matches('\r').chars().count())
}

/// Moves the cursor of the code field to character `index`, and focuses the field.
pub fn move_cursor(ctx: &egui::Context, index: usize) {
    let id = code_id();
    let mut state = TextEdit::load_state(ctx, id).unwrap_or_default();
    state
        .cursor
        .set_char_range(Some(CCursorRange::one(CCursor::new(index))));
    TextEdit::store_state(ctx, id, state);
    ctx.memory_mut(|memory| memory.request_focus(id));
}

/// Character index of the cursor of the code field, `None` before the field holds a cursor.
#[cfg(test)]
pub fn cursor_index(ctx: &egui::Context) -> Option<usize> {
    TextEdit::load_state(ctx, code_id())?
        .cursor
        .char_range()
        .map(|range| range.primary.index.0)
}

#[cfg(test)]
mod tests {
    use super::{char_index_at, digit_count, gutter_numbers, unwrapped_job};

    #[test]
    fn gutter_numbers_follow_galley_rows() {
        assert_eq!(gutter_numbers([false]), [Some(1)]);
        assert_eq!(gutter_numbers([true, true, false]), [Some(1), Some(2), Some(3)]);
        assert_eq!(
            gutter_numbers([false, true, false]),
            [Some(1), None, Some(2)],
            "a wrapped row continues its line"
        );

        // The rows of a real galley: a text that ends in a line break ends in an empty row, which gets its number.
        let ctx = egui::Context::default();
        let output = ctx.run_ui(egui::RawInput::default(), |ui| {
            let font = egui::TextStyle::Monospace.resolve(ui.style());
            let color = ui.visuals().text_color();
            let galley =
                ui.fonts_mut(|fonts| fonts.layout_job(unwrapped_job("fn a() {}\n\n  b\n", &font, color, 14.0)));
            let numbers = gutter_numbers(galley.rows.iter().map(|row| row.ends_with_newline));
            assert_eq!(numbers, [Some(1), Some(2), Some(3), Some(4)]);
            assert!(
                galley.rows.windows(2).all(|pair| pair[0].pos.y < pair[1].pos.y),
                "every line has a row of its own"
            );

            let long = "x".repeat(400);
            let galley = ui.fonts_mut(|fonts| fonts.layout_job(unwrapped_job(&long, &font, color, 14.0)));
            assert_eq!(galley.rows.len(), 1, "a long line does not wrap");
        });
        output.drop_without_applying_deltas();
    }

    #[test]
    fn a_place_maps_to_its_character() {
        let text = "ab\ncdé\r\nfg";
        assert_eq!(char_index_at(text, 1, 1), 0);
        assert_eq!(char_index_at(text, 2, 1), 3);
        assert_eq!(char_index_at(text, 2, 3), 5, "a column counts characters");
        assert_eq!(
            char_index_at(text, 2, 9),
            6,
            "a column past the line's end stops at its end"
        );
        assert_eq!(char_index_at(text, 3, 2), 9, "a CRLF line end is two characters");
        assert_eq!(char_index_at(text, 7, 1), text.chars().count());
        assert_eq!(char_index_at(text, 0, 0), 0);
    }

    #[test]
    fn a_count_has_its_digits() {
        assert_eq!(digit_count(0), 1);
        assert_eq!(digit_count(9), 1);
        assert_eq!(digit_count(10), 2);
        assert_eq!(digit_count(1000), 4);
    }
}
