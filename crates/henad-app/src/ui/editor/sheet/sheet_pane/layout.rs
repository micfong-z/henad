//! Layout of the Sheet pane: framed sections, form rows with one label column, and tables.
//!
//! A *section* is a framed region with its title and its help at the top. A *form* lays its rows out as a label column
//! and a field column, every label on one left edge. A *table* has a header row of small upper-case labels, a hairline
//! under it and under each row, and the row under the pointer in the theme's hover fill.
//!
//! A table's columns take their natural widths while those fit, and the room left over goes to one column. When they do
//! not fit, columns give up width in a stated order, each down to its least width. A table that does not fit at its
//! least widths scrolls sideways on its own, so nothing draws past the pane's edge.

use std::hash::Hash;

use egui::{
    Align, Color32, CornerRadius, Frame, Id, Label, Layout, Margin, Rect, Response, ScrollArea, Shape, Stroke,
    TextStyle,
};
use egui::{Ui, UiBuilder, pos2, vec2};

use henad_graph::sheet::sentence::REMOVE_ROW_LABEL;

use crate::ui::editor::sheet::help::{Help, help_button};
use crate::ui::editor::sheet::widgets::{CHIP_HEIGHT, caption, field_message, region, region_title, text_control};
use crate::ui::mcs;

/// Height in points of one line of a form row or of a table cell. A field, a select or a chip sits centred in it.
pub(super) const LINE: f32 = 22.0;

/// Space in points between two columns of a form or a table.
pub(super) const COLUMN_GAP: f32 = 8.0;

/// Space in points between two items on one line of a cell or a field.
pub(super) const ITEM_GAP: f32 = 4.0;

/// Space in points between two lines of a cell.
const LINE_GAP: f32 = 4.0;

/// Padding in points above and below the contents of a table row.
const ROW_PADDING: f32 = 2.0;

/// Inset in points of a section's contents from its border, at its sides.
const SECTION_INSET: i8 = 10;

/// Share of a form's width its label column takes at most.
const LABEL_SHARE: f32 = 0.45;

/// Height in points a cell gives itself when nothing in it reaches further down.
const CELL_DEPTH: f32 = 4096.0;

/// Draws a section of the Sheet pane: a framed region with `title` and the help `help` at its top, then what `add`
/// draws.
pub(super) fn section(ui: &mut Ui, title: &str, help: Help, add: impl FnOnce(&mut Ui)) {
    region(ui, |ui| {
        Frame::NONE
            .inner_margin(Margin {
                left: SECTION_INSET,
                right: SECTION_INSET,
                top: 6,
                bottom: 10,
            })
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.spacing_mut().item_spacing.y = LINE_GAP;
                region_title(ui, title, |ui| help_button(ui, help));
                ui.add_space(2.0);
                add(ui);
            });
    });
}

/// Returns the width in points of `text` on one line in the body font.
pub(super) fn text_width(ui: &Ui, text: &str) -> f32 {
    let font = TextStyle::Body.resolve(ui.style());
    ui.painter()
        .layout_no_wrap(text.to_owned(), font, Color32::PLACEHOLDER)
        .size()
        .x
}

/// Draws `add` as one line of `ui`: a row as high as [`LINE`], left to right, its items centred on the line.
pub(super) fn line<R>(ui: &mut Ui, add: impl FnOnce(&mut Ui) -> R) -> R {
    ui.allocate_ui_with_layout(
        vec2(ui.available_width(), LINE),
        Layout::left_to_right(Align::Center),
        |ui| {
            ui.set_min_height(LINE);
            ui.spacing_mut().item_spacing.x = ITEM_GAP;
            add(ui)
        },
    )
    .inner
}

/// Draws `add` as lines of `ui` that wrap at its right edge, each as high as [`LINE`], its items centred on it.
pub(super) fn wrapped_line<R>(ui: &mut Ui, add: impl FnOnce(&mut Ui) -> R) -> R {
    ui.allocate_ui_with_layout(
        vec2(ui.available_width(), LINE),
        Layout::left_to_right(Align::Center).with_main_wrap(true),
        |ui| {
            ui.set_min_height(LINE);
            ui.spacing_mut().item_spacing = vec2(ITEM_GAP, LINE_GAP);
            ui.spacing_mut().interact_size.y = LINE;
            add(ui)
        },
    )
    .inner
}

/// Adds `label` with its first line centred on a line of [`LINE`]. A label in a column wraps at its right edge unless it
/// says otherwise.
pub(super) fn text_cell(ui: &mut Ui, label: Label) -> egui::Response {
    let row = ui.text_style_height(&TextStyle::Body);
    ui.add_space(((LINE - row) / 2.0).max(0.0));
    ui.add(label)
}

/// A form: rows of a label column and a field column, the labels on one left edge.
pub(super) struct Form {
    /// Width in points of the label column.
    label_width: f32,
}

impl Form {
    /// Returns the form whose label column fits the widest of `labels`, but takes at most 45% of the width of `ui`. A
    /// longer label wraps.
    pub(super) fn new(ui: &Ui, labels: &[&str]) -> Self {
        let widest = labels.iter().map(|label| text_width(ui, label)).fold(0.0, f32::max);
        Self {
            label_width: widest.min(ui.available_width() * LABEL_SHARE).ceil(),
        }
    }

    /// Returns the form with its label column `extra` points wider, for labels with a swatch before them.
    pub(super) fn wider(self, extra: f32) -> Self {
        Self {
            label_width: self.label_width + extra,
        }
    }

    /// Draws a row: `label` in the label column and what `add` draws in the field column, left to right and wrapping
    /// at its right edge. Returns what `add` returns.
    pub(super) fn row<R>(&self, ui: &mut Ui, label: &str, add: impl FnOnce(&mut Ui) -> R) -> R {
        self.row_with(
            ui,
            |ui| {
                ui.add(Label::new(label).wrap());
            },
            add,
        )
    }

    /// Draws a row whose label `label` draws, with what `add` draws in the field column, and returns what `add`
    /// returns.
    pub(super) fn row_with<R>(&self, ui: &mut Ui, label: impl FnOnce(&mut Ui), add: impl FnOnce(&mut Ui) -> R) -> R {
        ui.horizontal_top(|ui| {
            ui.spacing_mut().item_spacing.x = COLUMN_GAP;
            ui.allocate_ui_with_layout(
                vec2(self.label_width, LINE),
                Layout::left_to_right(Align::Center),
                |ui| {
                    ui.set_min_size(vec2(self.label_width, LINE));
                    label(ui);
                },
            );
            wrapped_line(ui, add)
        })
        .inner
    }

    /// Draws `add` in the field column under the rows above, top to bottom, with nothing in the label column: a hint
    /// under its field or the message of a refused field. Returns what `add` returns.
    pub(super) fn under<R>(&self, ui: &mut Ui, add: impl FnOnce(&mut Ui) -> R) -> R {
        ui.horizontal_top(|ui| {
            ui.spacing_mut().item_spacing.x = COLUMN_GAP;
            let (_, _) = ui.allocate_exact_size(vec2(self.label_width, 1.0), egui::Sense::hover());
            ui.vertical(|ui| {
                ui.set_max_width(ui.available_width());
                ui.spacing_mut().item_spacing.y = LINE_GAP;
                add(ui)
            })
            .inner
        })
        .inner
    }
}

/// One column of a table.
#[derive(Debug, Clone, Copy)]
pub(super) struct Column {
    /// Header of the column, empty for none. A header runs over the columns after it that have none.
    pub(super) header: &'static str,
    /// Width in points the column takes while the table fits.
    pub(super) natural: f32,
    /// Least width in points the column gives up width to.
    pub(super) least: f32,
}

impl Column {
    /// Returns a column under `header` that takes `natural` points and gives up width down to `least`.
    pub(super) fn new(header: &'static str, natural: f32, least: f32) -> Self {
        Self {
            header,
            natural: natural.max(least),
            least,
        }
    }

    /// Returns a column of `width` points with no header. It gives up no width.
    pub(super) fn fixed(width: f32) -> Self {
        Self::new("", width, width)
    }
}

/// A table: its columns, the column that takes the room left over, and the order its columns give up width in.
pub(super) struct Table<'a> {
    /// Id of the table's sideways scroll.
    id: Id,
    /// The columns, left to right.
    columns: Vec<Column>,
    /// Index of the column that takes the room left over.
    grow: usize,
    /// Steps of giving up width, first to last: each the index of a column and the width it gives up width down to.
    /// A column can give up width in more than one step.
    give_up: &'a [(usize, f32)],
}

impl<'a> Table<'a> {
    /// Returns the table of `columns` keyed by `id`, the column `grow` taking the room left over and the steps of
    /// `give_up` giving up width in that order. A step never takes a column below its least width.
    pub(super) fn new(id: Id, columns: Vec<Column>, grow: usize, give_up: &'a [(usize, f32)]) -> Self {
        Self {
            id,
            columns,
            grow,
            give_up,
        }
    }

    /// Returns the width of each column in `available` points, and whether they fit.
    fn widths(&self, available: f32) -> (Vec<f32>, bool) {
        let gaps = COLUMN_GAP * self.columns.len().saturating_sub(1) as f32;
        let room = available - gaps;
        let mut widths: Vec<f32> = self.columns.iter().map(|column| column.natural).collect();
        let total: f32 = widths.iter().sum();
        if total <= room {
            if let Some(width) = widths.get_mut(self.grow) {
                *width += room - total;
            }
            return (widths, true);
        }
        let mut over = total - room;
        for &(index, floor) in self.give_up {
            let (Some(width), Some(column)) = (widths.get_mut(index), self.columns.get(index)) else {
                continue;
            };
            let given = (*width - floor.max(column.least)).clamp(0.0, over);
            *width -= given;
            over -= given;
            if over <= 0.0 {
                return (widths, true);
            }
        }
        (widths, false)
    }

    /// Draws the header row, then what `add` draws: the rows, each through [`TableRows::row`], and anything between
    /// them. A table that does not fit scrolls sideways.
    pub(super) fn show(self, ui: &mut Ui, add: impl FnOnce(&mut Ui, &TableRows)) {
        let available = ui.available_width();
        let (widths, fits) = self.widths(available);
        let width = widths.iter().sum::<f32>() + COLUMN_GAP * widths.len().saturating_sub(1) as f32;
        let rows = TableRows {
            headers: self.columns.iter().map(|column| column.header).collect(),
            widths,
            width,
            spacing: ui.spacing().item_spacing,
        };
        let body = |ui: &mut Ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            rows.header(ui);
            add(ui, &rows);
        };
        if fits {
            ui.scope(body);
        } else {
            ScrollArea::horizontal()
                .id_salt(self.id)
                .auto_shrink([false, true])
                .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysVisible)
                .show(ui, |ui| {
                    ui.set_width(width);
                    body(ui);
                });
        }
    }
}

/// Widths of a table's columns while its rows are drawn.
pub(super) struct TableRows {
    /// Header of each column.
    headers: Vec<&'static str>,
    /// Width in points of each column.
    widths: Vec<f32>,
    /// Width in points of the whole table.
    width: f32,
    /// Spacing between the items of a cell.
    spacing: egui::Vec2,
}

impl TableRows {
    /// Returns the left edge of the column `index`, from the table's left edge.
    fn left(&self, index: usize) -> f32 {
        self.widths.iter().take(index).sum::<f32>() + COLUMN_GAP * index as f32
    }

    /// Returns the width in points of the columns `first` to `last`, with the gaps between them.
    fn span(&self, first: usize, last: usize) -> f32 {
        self.left(last + 1) - self.left(first) - COLUMN_GAP
    }

    /// Draws the header row: each header as a small upper-case label over its column and the columns after it that
    /// have none, then a hairline. A table with no header draws none.
    fn header(&self, ui: &mut Ui) {
        if self.headers.iter().all(|header| header.is_empty()) {
            return;
        }
        let left = ui.cursor().left();
        let top = ui.cursor().top();
        let mut bottom = top;
        for (index, header) in self.headers.iter().enumerate() {
            if header.is_empty() {
                continue;
            }
            let last = (index + 1..self.headers.len())
                .take_while(|next| self.headers.get(*next).is_some_and(|next| next.is_empty()))
                .last()
                .unwrap_or(index);
            let rect = Rect::from_min_size(
                pos2(left + self.left(index), top),
                vec2(self.span(index, last), CELL_DEPTH),
            );
            let mut cell = ui.new_child(
                UiBuilder::new()
                    .id_salt(("henad_table_header", index))
                    .max_rect(rect)
                    .layout(Layout::top_down(Align::Min)),
            );
            caption(&mut cell, header);
            bottom = bottom.max(cell.min_rect().bottom());
        }
        let rect = Rect::from_min_max(pos2(left, top), pos2(left + self.width, bottom + 4.0));
        ui.advance_cursor_after_rect(rect);
        rule(ui, rect);
    }

    /// Draws one row keyed by `key`: `add` places its cells through [`TableRow::cell`]. The row takes the theme's hover
    /// fill under the pointer and a hairline under it.
    pub(super) fn row(&self, ui: &mut Ui, key: impl Hash + std::fmt::Debug, add: impl FnOnce(&mut TableRow<'_>)) {
        let left = ui.cursor().left();
        let top = ui.cursor().top();
        let fill = ui.painter().add(Shape::Noop);
        let mut row = TableRow {
            ui,
            rows: self,
            key: Id::new(key),
            left,
            top: top + ROW_PADDING,
            bottom: top + ROW_PADDING + LINE,
        };
        add(&mut row);
        let bottom = row.bottom;
        let rect = Rect::from_min_max(pos2(left, top), pos2(left + self.width, bottom + ROW_PADDING + 1.0));
        ui.advance_cursor_after_rect(rect);
        if ui.rect_contains_pointer(rect) {
            let hover = ui.visuals().widgets.hovered.weak_bg_fill;
            ui.painter()
                .set(fill, Shape::rect_filled(rect, CornerRadius::ZERO, hover));
        }
        rule(ui, rect);
    }
}

/// Paints a hairline along the bottom edge of `rect`, inside it.
fn rule(ui: &Ui, rect: Rect) {
    ui.painter()
        .hline(rect.x_range(), rect.bottom() - 0.5, Stroke::new(1.0, mcs::GRAY_800));
}

/// Adds the delete button of a row, a "×" named `name` in the weak text colour.
pub(super) fn delete_button(ui: &mut Ui, name: &str) -> Response {
    text_control(ui, None, REMOVE_ROW_LABEL, name, CHIP_HEIGHT)
}

/// One row of a table while its cells are drawn.
pub(super) struct TableRow<'a> {
    /// The `Ui` the table is drawn in.
    ui: &'a mut Ui,
    /// Widths of the table's columns.
    rows: &'a TableRows,
    /// Key of the row and of its cells.
    key: Id,
    /// Left edge of the table.
    left: f32,
    /// Top of the row's cells.
    top: f32,
    /// Bottom of the lowest cell drawn so far.
    bottom: f32,
}

impl TableRow<'_> {
    /// Returns the message under the field `field` after its text was refused, `None` while none shows.
    pub(super) fn message(&self, field: Id) -> Option<String> {
        field_message(self.ui, field)
    }

    /// Draws `add` in the cell of the column `index`, top to bottom from the row's top, and returns what it returns.
    pub(super) fn cell<R>(&mut self, index: usize, add: impl FnOnce(&mut Ui) -> R) -> R {
        self.cells(index, index, add)
    }

    /// Draws `add` across the cells of the columns `first` to `last`, top to bottom from the row's top, and returns
    /// what it returns.
    pub(super) fn cells<R>(&mut self, first: usize, last: usize, add: impl FnOnce(&mut Ui) -> R) -> R {
        let top = self.top;
        self.place(first, last, top, ("cell", first), add)
    }

    /// Draws the row's delete button, a [`delete_button`] named `name` at the right edge of the cell of the column
    /// `index`, and returns whether it was clicked.
    pub(super) fn delete_cell(&mut self, index: usize, name: &str) -> bool {
        self.cell(index, |ui| {
            line(ui, |ui| {
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    delete_button(ui, name).clicked()
                })
                .inner
            })
        })
    }

    /// Draws `add` under every cell drawn so far, from the left edge of the column `first` to the row's right edge, and
    /// returns what it returns: a line that belongs to the row as a whole, or the message of a refused field.
    pub(super) fn below<R>(&mut self, first: usize, add: impl FnOnce(&mut Ui) -> R) -> R {
        let top = self.bottom + LINE_GAP;
        let last = self.rows.widths.len().saturating_sub(1);
        self.place(first, last, top, ("below", first), add)
    }

    /// Draws `add` in the columns `first` to `last` from `top` down, in a child keyed by `salt`.
    fn place<R>(
        &mut self,
        first: usize,
        last: usize,
        top: f32,
        salt: (&str, usize),
        add: impl FnOnce(&mut Ui) -> R,
    ) -> R {
        let rect = Rect::from_min_size(
            pos2(self.left + self.rows.left(first), top),
            vec2(self.rows.span(first, last), CELL_DEPTH),
        );
        let mut cell = self.ui.new_child(
            UiBuilder::new()
                .id_salt((self.key, salt))
                .max_rect(rect)
                .layout(Layout::top_down(Align::Min)),
        );
        cell.spacing_mut().item_spacing = vec2(self.rows.spacing.x, LINE_GAP);
        let inner = add(&mut cell);
        self.bottom = self.bottom.max(cell.min_rect().bottom());
        inner
    }
}
