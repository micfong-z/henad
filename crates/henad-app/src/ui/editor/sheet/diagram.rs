//! The Diagram: each card of a sheet drawn read-only as boxes and arrows, in place of its lines.
//!
//! A *box* stands for one line. It shows the line prefix, the result the line works out on the right, the line's
//! sentence as the card writes it, and on the left each result of a line above that the line reads. An *arrow* runs
//! from a result to each line that reads it, and a dashed *frame* labelled "Inside line N" surrounds the lines inside
//! each When, nested Whens included, with a connector from the When's box to its frame.
//!
//! The drawing is painted with the painter and holds no widget. Each card's drawing is one accessible image named by
//! its number of lines, and holds no control.

use std::collections::BTreeMap;

use egui::accesskit::Role;
use egui::epaint::CubicBezierShape;
use egui::{Align2, Color32, CornerRadius, FontId, Galley, Painter, Pos2, Rect, Sense, Shape, Stroke, StrokeKind, Ui};
use egui::{Button, ScrollArea, TextStyle, pos2, vec2};
use henad_graph::sheet::ConvertRefusal;
use henad_graph::sheet::check::SheetReport;
use henad_graph::sheet::document::{LineKind, SheetDocument};
use henad_graph::sheet::relations::CardRelations;
use henad_graph::sheet::sentence::{
    AddButton, ChipKind, DIAGRAM_NOTE, EDIT_AS_NODE_GRAPH, SentenceChip, SentencePart, Sentences, diagram_name,
    inside_line_frame, line_count,
};

use super::accessible_group;
use super::card_pane::card_lines;
use super::widgets::{KIND_BAR_WIDTH, caption, kind_bar};
use crate::init::semibold_font;
use crate::ui::mcs;
use crate::ui::sweep::layout::add_button;

/// Width in points of a box.
const BOX_WIDTH: f32 = 240.0;

/// Gap in points between two columns of boxes.
const COLUMN_GAP: f32 = 64.0;

/// Gap in points between two rows of boxes.
const ROW_GAP: f32 = 16.0;

/// Padding in points inside a box.
const PADDING: f32 = 8.0;

/// Height in points of one row of a box: its header, its result and each value it reads.
const ROW_HEIGHT: f32 = 20.0;

/// Margin in points between a frame and the boxes inside it, at its sides and its bottom.
const FRAME_MARGIN: f32 = 10.0;

/// Height in points of the label strip at the top of a frame.
const FRAME_LABEL: f32 = 22.0;

/// Radius in points of a pin.
const PIN_RADIUS: f32 = 4.0;

/// Size in points of the text of a box.
const TEXT_SIZE: f32 = 12.0;

/// One box of the Diagram: a line of a card.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiagramBox {
    /// Id of the line.
    pub id: String,
    /// Line prefix, such as "Line 2" or "Clear line 1".
    pub prefix: String,
    /// Number of the line on its card, from 1.
    pub number: usize,
    /// Sentence of the line as the card writes it.
    pub sentence: String,
    /// Name of the result the line works out, `None` for a line that works out none.
    pub result: Option<String>,
    /// Each result of a line on the card that the line reads, as the line's id and the result's name, in the order of
    /// the sentence.
    pub reads: Vec<(String, String)>,
    /// Id of the When the line sits inside, `None` for a line at the top of the card.
    pub parent: Option<String>,
    /// Whether the line is a When.
    pub when: bool,
    /// Group of the line's kind, whose hue the box's bar takes as the card's row does.
    pub group: AddButton,
}

/// Returns the boxes of card `card_index` of `document`, one per line in display order.
pub fn diagram_boxes(
    document: &SheetDocument,
    sentences: &Sentences<'_>,
    report: &SheetReport,
    card_index: usize,
) -> Vec<DiagramBox> {
    let Some(card) = document.cards.get(card_index) else {
        return Vec::new();
    };
    let relations = CardRelations::new(document, card);
    card_lines(document, sentences, report, card_index)
        .into_iter()
        .map(|line| {
            let line_relation = relations.line(&line.id);
            let mut reads: Vec<(String, String)> = Vec::new();
            for part in &line.parts {
                if let SentencePart::Chip(SentenceChip {
                    kind: ChipKind::Result(source),
                    name,
                    ..
                }) = part
                    && relations.index(source).is_some()
                    && !reads.iter().any(|(known, _)| known == source)
                {
                    let name = relations
                        .line(source)
                        .and_then(|source| source.name.clone())
                        .unwrap_or_else(|| name.clone());
                    reads.push((source.clone(), name));
                }
            }
            DiagramBox {
                prefix: line.prefix.clone(),
                number: line.number,
                sentence: line.sentence.clone(),
                result: line_relation.and_then(|relation| relation.name.clone()),
                reads,
                parent: relations.parent(&line.id).map(str::to_owned),
                when: line_relation.is_some_and(|relation| relation.kind == LineKind::When),
                group: line_relation.map_or(AddButton::Change, |relation| AddButton::for_kind(relation.kind)),
                id: line.id,
            }
        })
        .collect()
}

/// Returns the column of each box: one past the column of every box it reads and of its When, 0 for a box that reads
/// none and sits in none.
fn columns(boxes: &[DiagramBox]) -> Vec<usize> {
    let index: BTreeMap<&str, usize> = boxes
        .iter()
        .enumerate()
        .map(|(position, line)| (line.id.as_str(), position))
        .collect();
    let mut columns: Vec<usize> = vec![0; boxes.len()];
    // Display order puts every result a line can read and every When above the line, so one pass settles the columns.
    // A box that reads one below it is placed as if it did not.
    for (position, line) in boxes.iter().enumerate() {
        let sources = line
            .reads
            .iter()
            .map(|(source, _)| source.as_str())
            .chain(line.parent.as_deref());
        columns[position] = sources
            .filter_map(|source| index.get(source).copied())
            .filter(|&source| source < position)
            .map(|source| columns[source] + 1)
            .max()
            .unwrap_or(0);
    }
    columns
}

/// Returns whether the box `line` sits inside the When `block`, directly or through other Whens.
fn is_inside(boxes: &[DiagramBox], line: &DiagramBox, block: &str) -> bool {
    let mut parent = line.parent.as_deref();
    let mut steps = 0;
    while let Some(id) = parent
        && steps <= boxes.len()
    {
        if id == block {
            return true;
        }
        parent = boxes
            .iter()
            .find(|known| known.id == id)
            .and_then(|known| known.parent.as_deref());
        steps += 1;
    }
    false
}

/// A box placed on the drawing, with its laid-out sentence.
struct PlacedBox {
    /// Rectangle of the box, relative to the drawing's top left.
    rect: Rect,
    /// Sentence, laid out within the box.
    sentence: std::sync::Arc<Galley>,
}

impl PlacedBox {
    /// Centre of the pin of the box's result, on its right edge.
    fn output_pin(&self) -> Pos2 {
        pos2(self.rect.right(), self.rect.top() + ROW_HEIGHT * 1.5)
    }

    /// Top of the rows of the values the box reads.
    fn reads_top(&self, has_result: bool) -> f32 {
        let rows = if has_result { 2.0 } else { 1.0 };
        self.rect.top() + ROW_HEIGHT * rows + self.sentence.size().y + PADDING
    }
}

/// Returns the height of a box holding `sentence` with `rows` rows besides its header.
fn box_height(sentence: &Galley, rows: usize) -> f32 {
    ROW_HEIGHT * (1 + rows) as f32 + sentence.size().y + PADDING * 2.0
}

/// Draws the Diagram of every card of `document`: the note and Edit as node graph, then each card's heading and
/// drawing. Returns whether Edit as node graph was pressed.
///
/// Edit as node graph is disabled with the tooltip of `refusal` while the sheet does not convert.
pub fn diagram_ui(
    ui: &mut Ui,
    document: &SheetDocument,
    report: &SheetReport,
    refusal: Option<ConvertRefusal>,
) -> bool {
    let mut edit = false;
    ui.horizontal_wrapped(|ui| {
        ui.weak(DIAGRAM_NOTE);
        let button = add_button(
            ui,
            refusal.is_none(),
            Button::new(EDIT_AS_NODE_GRAPH),
            EDIT_AS_NODE_GRAPH,
        );
        let button = match refusal {
            Some(refusal) => button.on_disabled_hover_text(refusal.tooltip()),
            None => button,
        };
        edit = button.clicked();
    });
    let sentences = Sentences::new(document);
    for (index, card) in document.cards.iter().enumerate() {
        ui.add_space(12.0);
        let boxes = diagram_boxes(document, &sentences, report, index);
        ui.horizontal_wrapped(|ui| {
            let font = semibold_font(ui.ctx(), TextStyle::Body.resolve(ui.style()).size);
            ui.label(
                egui::RichText::new(sentences.card_heading(card))
                    .font(font)
                    .color(mcs::GRAY_100),
            );
            caption(ui, &line_count(boxes.len()));
        });
        if boxes.is_empty() {
            continue;
        }
        ScrollArea::horizontal()
            .id_salt(("henad_diagram_scroll", index))
            .show(ui, |ui| card_diagram(ui, &boxes));
    }
    edit
}

/// Draws the drawing of one card's `boxes`, and names it as an image.
fn card_diagram(ui: &mut Ui, boxes: &[DiagramBox]) {
    let columns = columns(boxes);
    let font = FontId::proportional(TEXT_SIZE);
    let text_width = BOX_WIDTH - PADDING * 2.0 - KIND_BAR_WIDTH;
    let galleys: Vec<std::sync::Arc<Galley>> = boxes
        .iter()
        .map(|line| {
            ui.painter()
                .layout(line.sentence.clone(), font.clone(), mcs::GRAY_200, text_width)
        })
        .collect();

    let (rects, frames, top) = place_boxes(boxes, &columns, &galleys);

    let width = rects
        .iter()
        .chain(frames.iter().map(|(_, rect)| rect))
        .map(|rect| rect.right())
        .fold(0.0, f32::max)
        + FRAME_MARGIN * 2.0;
    let (response, painter) = ui.allocate_painter(vec2(width, top), Sense::hover());
    let origin = response.rect.min.to_vec2();
    let placed: Vec<PlacedBox> = rects
        .iter()
        .zip(galleys)
        .map(|(rect, sentence)| PlacedBox {
            rect: rect.translate(origin),
            sentence,
        })
        .collect();

    for (position, frame) in frames.iter().rev() {
        paint_frame(&painter, frame.translate(origin), boxes[*position].number);
        let when = &placed[*position];
        let start = pos2(when.rect.right(), when.rect.top() + ROW_HEIGHT / 2.0);
        let end = frame.translate(origin).left_top() + vec2(0.0, FRAME_LABEL / 2.0);
        paint_arrow(&painter, start, end, mcs::GRAY_500);
    }
    let index: BTreeMap<&str, usize> = boxes
        .iter()
        .enumerate()
        .map(|(position, line)| (line.id.as_str(), position))
        .collect();
    for (position, line) in boxes.iter().enumerate() {
        let reads_top = placed[position].reads_top(line.result.is_some());
        for (row, (source, _)) in line.reads.iter().enumerate() {
            let Some(&source) = index.get(source.as_str()) else {
                continue;
            };
            let end = pos2(
                placed[position].rect.left(),
                reads_top + ROW_HEIGHT * (row as f32 + 0.5),
            );
            paint_arrow(&painter, placed[source].output_pin(), end, mcs::BLUE_400);
        }
    }
    for (line, placed) in boxes.iter().zip(&placed) {
        paint_box(&painter, line, placed, &font);
    }
    accessible_group(
        ui.ctx(),
        response.id,
        Role::Image,
        &diagram_name(boxes.len()),
        response.rect,
    );
}

/// Returns the rectangle of each of `boxes` in `columns`, holding the sentences `galleys`, the frame of each When with
/// lines inside it, by the When's position, and the height of the drawing.
fn place_boxes(
    boxes: &[DiagramBox],
    columns: &[usize],
    galleys: &[std::sync::Arc<Galley>],
) -> (Vec<Rect>, Vec<(usize, Rect)>, f32) {
    // Each box takes a row of its own, in display order. A frame opens with a label strip above the first line inside
    // its When and closes with a margin after the last.
    let mut rects: Vec<Rect> = Vec::with_capacity(boxes.len());
    let mut open: Vec<&str> = Vec::new();
    let mut top = 0.0;
    for (position, line) in boxes.iter().enumerate() {
        while let Some(block) = open.last()
            && !is_inside(boxes, line, block)
        {
            open.pop();
            top += FRAME_MARGIN + 4.0;
        }
        let mut chain: Vec<&str> = Vec::new();
        let mut parent = line.parent.as_deref();
        while let Some(id) = parent
            && !open.contains(&id)
            && chain.len() <= boxes.len()
        {
            chain.push(id);
            parent = boxes
                .iter()
                .find(|known| known.id == id)
                .and_then(|known| known.parent.as_deref());
        }
        for block in chain.into_iter().rev() {
            open.push(block);
            top += FRAME_LABEL + 4.0;
        }
        let rows = usize::from(line.result.is_some()) + line.reads.len();
        let height = box_height(&galleys[position], rows);
        let left = columns[position] as f32 * (BOX_WIDTH + COLUMN_GAP) + FRAME_MARGIN * 2.0;
        rects.push(Rect::from_min_size(pos2(left, top), vec2(BOX_WIDTH, height)));
        top += height + ROW_GAP;
    }
    top += open.len() as f32 * (FRAME_MARGIN + 4.0);

    // Frames, innermost first, each around its boxes and the frames inside it.
    let mut frames: Vec<(usize, Rect)> = Vec::new();
    for (position, block) in boxes.iter().enumerate().rev() {
        if !block.when {
            continue;
        }
        let inside: Vec<usize> = (0..boxes.len())
            .filter(|&member| is_inside(boxes, &boxes[member], &block.id))
            .collect();
        let Some(contents) = inside
            .iter()
            .map(|&member| rects[member])
            .chain(
                frames
                    .iter()
                    .filter(|(inner, _)| inside.contains(inner))
                    .map(|(_, rect)| *rect),
            )
            .reduce(Rect::union)
        else {
            continue;
        };
        let frame = Rect::from_min_max(
            contents.min - vec2(FRAME_MARGIN, FRAME_LABEL),
            contents.max + vec2(FRAME_MARGIN, FRAME_MARGIN),
        );
        frames.push((position, frame));
    }

    (rects, frames, top)
}

/// Paints the box of `line` at `placed`.
fn paint_box(painter: &Painter, line: &DiagramBox, placed: &PlacedBox, font: &FontId) {
    let rect = placed.rect;
    painter.rect(
        rect,
        CornerRadius::ZERO,
        mcs::GRAY_900,
        Stroke::new(1.0, mcs::GRAY_600),
        StrokeKind::Inside,
    );
    let header = Rect::from_min_size(rect.min, vec2(rect.width(), ROW_HEIGHT));
    painter.rect_filled(header.shrink(1.0), CornerRadius::ZERO, mcs::GRAY_800);
    painter.text(
        header.left_center() + vec2(PADDING + KIND_BAR_WIDTH, 0.0),
        Align2::LEFT_CENTER,
        &line.prefix,
        FontId::proportional(TEXT_SIZE),
        mcs::GRAY_100,
    );
    kind_bar(painter, rect, line.group);
    let mut top = rect.top() + ROW_HEIGHT;
    if let Some(result) = &line.result {
        let pin = placed.output_pin();
        painter.text(
            pin - vec2(PADDING + PIN_RADIUS, 0.0),
            Align2::RIGHT_CENTER,
            result,
            font.clone(),
            mcs::BLUE_200,
        );
        painter.circle_filled(pin, PIN_RADIUS, mcs::BLUE_400);
        top += ROW_HEIGHT;
    }
    painter.galley(
        pos2(rect.left() + PADDING + KIND_BAR_WIDTH, top + PADDING / 2.0),
        placed.sentence.clone(),
        mcs::GRAY_200,
    );
    let reads_top = placed.reads_top(line.result.is_some());
    for (row, (_, name)) in line.reads.iter().enumerate() {
        let pin = pos2(rect.left(), reads_top + ROW_HEIGHT * (row as f32 + 0.5));
        painter.circle_filled(pin, PIN_RADIUS, mcs::BLUE_400);
        painter.text(
            pin + vec2(PADDING + PIN_RADIUS, 0.0),
            Align2::LEFT_CENTER,
            name,
            font.clone(),
            mcs::GRAY_300,
        );
    }
}

/// Paints the dashed frame `rect` around the lines inside line `number`, with its label.
fn paint_frame(painter: &Painter, rect: Rect, number: usize) {
    let corners = [
        rect.left_top(),
        rect.right_top(),
        rect.right_bottom(),
        rect.left_bottom(),
        rect.left_top(),
    ];
    painter.extend(Shape::dashed_line(&corners, Stroke::new(1.0, mcs::GRAY_500), 6.0, 4.0));
    painter.text(
        rect.left_top() + vec2(FRAME_MARGIN, FRAME_LABEL / 2.0),
        Align2::LEFT_CENTER,
        inside_line_frame(number),
        FontId::proportional(TEXT_SIZE),
        mcs::GRAY_300,
    );
}

/// Paints an arrow from `start` to `end` in `color`, leaving and entering horizontally.
fn paint_arrow(painter: &Painter, start: Pos2, end: Pos2, color: Color32) {
    let bend = ((end.x - start.x) / 2.0).max(24.0);
    let curve = CubicBezierShape::from_points_stroke(
        [start, start + vec2(bend, 0.0), end - vec2(bend, 0.0), end],
        false,
        Color32::TRANSPARENT,
        Stroke::new(1.5, color),
    );
    painter.add(curve);
}

#[cfg(test)]
mod tests {
    use henad_graph::sheet::sentence::AddButton;

    use super::{DiagramBox, columns};

    fn line(id: &str, reads: &[&str], parent: Option<&str>) -> DiagramBox {
        DiagramBox {
            id: id.to_owned(),
            prefix: String::new(),
            number: 0,
            sentence: String::new(),
            result: None,
            reads: reads
                .iter()
                .map(|source| ((*source).to_owned(), String::new()))
                .collect(),
            parent: parent.map(str::to_owned),
            when: false,
            group: AddButton::Change,
        }
    }

    #[test]
    fn a_box_sits_right_of_what_it_reads_and_of_its_when() {
        let boxes = [
            line("l1", &[], None),
            line("l2", &["l1"], None),
            line("l3", &[], Some("l2")),
            line("l4", &["l1", "l3"], None),
        ];
        assert_eq!(columns(&boxes), [0, 1, 2, 3]);
    }
}
