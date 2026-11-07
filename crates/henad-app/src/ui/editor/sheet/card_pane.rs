//! The Card pane: each card of the sheet under its heading, with its lines numbered from 1 in display order.
//!
//! A line reads as its sentence from [`henad_graph::sheet::sentence`], with the "otherwise" words of the higher lines
//! that change the same property first. A line inside a When is indented one step per level.

use egui::{Label, RichText};
use henad_graph::sheet::document::{SheetCard, SheetDocument};
use henad_graph::sheet::relations::CardRelations;
use henad_graph::sheet::sentence::Sentences;

/// Indent in points of a line for each block around it.
const INDENT: f32 = 20.0;

/// Draws every card of `document`, in file order.
pub fn card_pane(ui: &mut egui::Ui, document: &SheetDocument) {
    let sentences = Sentences::new(document);
    for (index, card) in document.cards.iter().enumerate() {
        if index > 0 {
            ui.add_space(12.0);
        }
        card_ui(ui, document, &sentences, card);
    }
}

/// Draws the heading of `card` and its numbered lines.
fn card_ui(ui: &mut egui::Ui, document: &SheetDocument, sentences: &Sentences<'_>, card: &SheetCard) {
    ui.label(RichText::new(sentences.card_heading(card)).strong());
    for line in card_lines(document, sentences, card) {
        ui.horizontal(|ui| {
            ui.add_space(INDENT * line.depth as f32);
            ui.weak(line.number.to_string());
            ui.add(Label::new(line.sentence).wrap());
        });
    }
}

/// One line of a card as the Card pane shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CardLine {
    /// Number of the line on its card, from 1.
    pub number: usize,
    /// Number of blocks around the line.
    pub depth: usize,
    /// Sentence of the line, with its "otherwise" words.
    pub sentence: String,
}

/// Returns the lines of `card` in display order, each with its number, depth and sentence.
pub fn card_lines(document: &SheetDocument, sentences: &Sentences<'_>, card: &SheetCard) -> Vec<CardLine> {
    let relations = CardRelations::new(document, card);
    relations
        .order()
        .iter()
        .enumerate()
        .map(|(index, &line_id)| {
            let mut depth = 0;
            let mut block = relations.parent(line_id);
            // Display order holds each block once, before its lines. The bound guards against a cycle all the same.
            while let Some(parent) = block
                && depth < relations.order().len()
            {
                depth += 1;
                block = relations.parent(parent);
            }
            CardLine {
                number: relations.number(line_id).unwrap_or(index + 1),
                depth,
                sentence: sentences.line(line_id, &relations.otherwise_numbers(line_id)),
            }
        })
        .collect()
}
