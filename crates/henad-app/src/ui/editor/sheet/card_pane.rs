//! The Card pane: each card of the sheet under its heading, with its lines numbered from 1 in display order.
//!
//! A line reads as its sentence from [`henad_graph::sheet::sentence`], drawn part by part: words, a chosen value as a
//! *chip* coloured by what it refers to, an empty slot as a dashed *blank*, a select inside the line, the grey
//! "always" and the muted "otherwise" words. A line inside a When is indented one step per level, and the notes of
//! the sheet's check about a line sit under it.
//!
//! Each line is a group named "Line 2: " and its sentence, and each control inside it is named by its visible words,
//! a blank or a select by what it wants and where it is ("Line 2: choose the old state"). Hovering or focusing
//! "otherwise" or a result chip highlights the line it names.

use egui::accesskit::Role;
use egui::{
    Color32, CornerRadius, Frame, Label, Margin, Pos2, Rect, Response, RichText, Sense, Shape, Stroke, TextStyle, Ui,
    Vec2, WidgetInfo, WidgetType, vec2,
};
use henad_core::diagnostic::DiagnosticSeverity;
use henad_graph::sheet::check::{SheetDiagnostic, SheetReport};
use henad_graph::sheet::document::{CardRuns, SheetCard, SheetDocument, SheetProperty};
use henad_graph::sheet::relations::CardRelations;
use henad_graph::sheet::sentence::{
    ALWAYS, ALWAYS_TOOLTIP, ANY_STATE_ID, BLANK_TEXT, ChipKind, FOR_EACH_BLANK_WORDS, FROM_TOOLTIP, FROM_WORD, LAW,
    LineSelect, SentenceChip, SentencePart, Sentences, UNIT_PLACEHOLDER, always_name, for_each_optional_name,
    for_each_optional_tooltip, line_count, line_list_name, line_name, line_prefix, otherwise_tooltip, otherwise_words,
    result_below_tooltip, result_inside_tooltip, sentence_text, uses_result_below, uses_result_inside,
};

use super::accessible_group;
use super::help::{Help, help_button};
use crate::icons::material_design_icons::{MDI_CHEVRON_DOWN, MDI_TUNE_VERTICAL};
use crate::ui::mcs;
use crate::ui::sweep::layout::add_button;

/// Indent in points of a line for each block around it.
const INDENT: f32 = 20.0;

/// Width in points of the column of line numbers.
const NUMBER_WIDTH: f32 = 22.0;

/// Padding in points inside a chip, a blank or a select.
const CHIP_PADDING: Vec2 = vec2(4.0, 1.0);

/// Draws every card of `document`, in file order, with the notes of `report` under their lines.
pub fn card_pane(ui: &mut Ui, document: &SheetDocument, report: &SheetReport) {
    let sentences = Sentences::new(document);
    let highlight_id = ui.id().with("henad_card_highlight");
    let highlight: Vec<String> = ui.data(|data| data.get_temp(highlight_id)).unwrap_or_default();
    let mut hovered: Vec<String> = Vec::new();
    for (index, card) in document.cards.iter().enumerate() {
        if index > 0 {
            ui.add_space(16.0);
        }
        let lines = card_lines(document, &sentences, report, index);
        card_ui(ui, document, &sentences, card, &lines, &highlight, &mut hovered);
    }
    if hovered != highlight {
        ui.data_mut(|data| data.insert_temp(highlight_id, hovered));
        ui.ctx().request_repaint();
    }
}

/// Draws the heading of `card`, the law under a card that runs every tick, and the numbered `lines`.
///
/// `highlight` holds the ids of the lines to highlight, and the ids of the lines that a hovered or focused control
/// names are added to `hovered`.
fn card_ui(
    ui: &mut Ui,
    document: &SheetDocument,
    sentences: &Sentences<'_>,
    card: &SheetCard,
    lines: &[CardLine],
    highlight: &[String],
    hovered: &mut Vec<String>,
) {
    let each_tick = card.runs == CardRuns::EachTick;
    ui.horizontal_wrapped(|ui| {
        ui.label(RichText::new(sentences.card_heading(card)).strong());
        ui.weak(line_count(lines.len()));
        if each_tick {
            help_button(ui, Help::Lines);
        }
    });
    if each_tick {
        Frame::NONE
            .fill(mcs::GRAY_900)
            .inner_margin(Margin::symmetric(8, 6))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.add(Label::new(RichText::new(LAW).weak()).wrap());
            });
    }
    ui.add_space(4.0);
    let list_name = line_list_name(button_of(document, card));
    let list = ui.scope(|ui| {
        for line in lines {
            line_ui(ui, line, highlight, hovered);
        }
        ui.unique_id()
    });
    accessible_group(ui.ctx(), list.inner, Role::List, &list_name, list.response.rect);
}

/// Returns the name of the button that runs `card` once, `None` for a card that runs every tick or that no button
/// runs.
fn button_of<'a>(document: &'a SheetDocument, card: &SheetCard) -> Option<&'a str> {
    if card.runs == CardRuns::EachTick {
        return None;
    }
    document
        .buttons
        .iter()
        .find(|button| {
            button
                .does
                .iter()
                .any(|step| step.card.as_deref() == Some(card.id.as_str()))
        })
        .map(|button| button.name.as_str())
}

/// Colour of a note under a line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NoteColor {
    /// A note that asks the author to look again, in the theme's warn colour.
    Amber,
    /// A note of a line that cannot run because of what it uses, or of an error, in the theme's error colour.
    Red,
}

/// A note under a line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LineNote {
    /// Words of the note.
    pub text: String,
    /// Colour of the note.
    pub color: NoteColor,
}

/// A control inside a line: a chip, a blank, a select, "always", "otherwise" or the optional "for each ..." blank.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LineControl {
    /// Words the control shows.
    pub visible: String,
    /// Accessible name of the control.
    pub name: String,
    /// Whether the control is a select, whose visible words are its value rather than its label.
    pub select: bool,
}

/// One line of a card as the Card pane shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CardLine {
    /// Id of the line.
    pub id: String,
    /// Number of the line on its card, from 1.
    pub number: usize,
    /// Number of blocks around the line.
    pub depth: usize,
    /// Prefix of the line's accessible names, such as "Line 2" or "Clear line 1".
    pub prefix: String,
    /// Sentence of the line as plain text, with its "otherwise" words.
    pub sentence: String,
    /// Parts of the sentence, in order.
    pub parts: Vec<SentencePart>,
    /// Ids of the higher lines that override this line, empty for a line with no "otherwise".
    pub otherwise: Vec<String>,
    /// Tooltip and accessible description of the "otherwise" words, empty for a line with no "otherwise".
    pub otherwise_tooltip: String,
    /// Index in [`Self::parts`] of each result chip that the line cannot use, with the chip's tooltip.
    pub red_chips: Vec<(usize, String)>,
    /// Index in [`Self::parts`] of each state chip, with the colour of its state. A state that no property has is
    /// grey.
    pub swatches: Vec<(usize, Color32)>,
    /// Notes of the sheet's check about the line, in report order, a skip note first.
    pub notes: Vec<LineNote>,
}

impl CardLine {
    /// Returns the accessible name of the line, such as "Line 2: Become Alive from Dead, when Alive neighbours is 3".
    pub fn label(&self) -> String {
        line_name(&self.prefix, &self.sentence)
    }

    /// Returns the control that `part` of the line draws, with its visible words and its accessible name, `None` for
    /// words.
    pub fn control(&self, part: &SentencePart) -> Option<LineControl> {
        let (visible, name, select) = match part {
            SentencePart::Chip(chip) => (chip.text(), chip.text(), false),
            SentencePart::Blank(blank) => (blank.word().to_owned(), blank.name(&self.prefix), false),
            SentencePart::Select(select, words) => {
                (select_words(*select, words).to_owned(), select.name(&self.prefix), true)
            }
            SentencePart::Always => (ALWAYS.to_owned(), always_name(&self.prefix), false),
            SentencePart::Otherwise(lines) => (otherwise_words(lines), otherwise_words(lines), false),
            SentencePart::ForEachBlank(_) => (
                FOR_EACH_BLANK_WORDS.to_owned(),
                for_each_optional_name(&self.prefix),
                false,
            ),
            _ => return None,
        };
        Some(LineControl { visible, name, select })
    }

    /// Returns every control of the line in order.
    #[cfg(test)]
    pub fn controls(&self) -> Vec<LineControl> {
        self.parts.iter().filter_map(|part| self.control(part)).collect()
    }
}

/// Returns the lines of card `card_index` of `document` in display order, each with its number, depth, sentence and
/// parts, and the notes of `report` about it.
pub fn card_lines(
    document: &SheetDocument,
    sentences: &Sentences<'_>,
    report: &SheetReport,
    card_index: usize,
) -> Vec<CardLine> {
    let Some(card) = document.cards.get(card_index) else {
        return Vec::new();
    };
    let button = button_of(document, card);
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
            let number = relations.number(line_id).unwrap_or(index + 1);
            let otherwise_numbers = relations.otherwise_numbers(line_id);
            let parts = sentences.line_parts(line_id, &otherwise_numbers);
            let red_chips = red_chips(&relations, line_id, index, &parts);
            let notes = line_notes(report, card_index, line_id, &red_chips);
            let swatches = swatches(document, &parts);
            CardLine {
                id: line_id.to_owned(),
                number,
                depth,
                prefix: line_prefix(button, number),
                sentence: sentence_text(&parts),
                otherwise: relations.otherwise_of(line_id).into_iter().map(str::to_owned).collect(),
                otherwise_tooltip: if otherwise_numbers.is_empty() {
                    String::new()
                } else {
                    otherwise_tooltip_of(document, &relations, line_id, &otherwise_numbers)
                },
                parts,
                red_chips: red_chips.into_iter().map(|red| (red.index, red.tooltip)).collect(),
                swatches,
                notes,
            }
        })
        .collect()
}

/// Returns the index of each state chip of `parts` with the colour of its state in `document`.
fn swatches(document: &SheetDocument, parts: &[SentencePart]) -> Vec<(usize, Color32)> {
    parts
        .iter()
        .enumerate()
        .filter_map(|(index, part)| {
            let SentencePart::Chip(SentenceChip {
                kind: ChipKind::State { property, state },
                ..
            }) = part
            else {
                return None;
            };
            let color = property
                .as_deref()
                .and_then(|property| property_of(document, property))
                .and_then(|property| property.states.iter().find(|candidate| candidate.id == *state))
                .map_or(mcs::GRAY_500, |state| {
                    let [red, green, blue, alpha] = state.color;
                    Color32::from_rgba_unmultiplied(red, green, blue, alpha)
                });
            Some((index, color))
        })
        .collect()
}

/// A result chip that its line cannot use.
struct RedChip {
    /// Index of the chip in the line's parts.
    index: usize,
    /// Tooltip of the chip.
    tooltip: String,
    /// Skip note the sheet's check writes for the chip.
    note: String,
}

/// Returns the result chips of line `line_id`, at `position` in display order, that name a result below it or a
/// result inside a When the line is not in.
fn red_chips(relations: &CardRelations<'_>, line_id: &str, position: usize, parts: &[SentencePart]) -> Vec<RedChip> {
    parts
        .iter()
        .enumerate()
        .filter_map(|(index, part)| {
            let SentencePart::Chip(SentenceChip {
                kind: ChipKind::Result(result),
                name,
                ..
            }) = part
            else {
                return None;
            };
            let result_name = relations
                .line(result)
                .and_then(|line| line.name.clone())
                .unwrap_or_else(|| name.clone());
            let result_position = relations.index(result)?;
            if result_position >= position {
                return Some(RedChip {
                    index,
                    tooltip: result_below_tooltip(&result_name),
                    note: uses_result_below(&result_name),
                });
            }
            let block = relations.parent(result)?;
            if relations.is_within(line_id, block) {
                return None;
            }
            let number = relations.number(block).unwrap_or(0);
            Some(RedChip {
                index,
                tooltip: result_inside_tooltip(&result_name),
                note: uses_result_inside(&result_name, number),
            })
        })
        .collect()
}

/// Returns the notes of `report` about line `line_id` of card `card_index`, the skip note first.
///
/// A skip note about a chip in `red_chips` and an error are red, and any other note amber.
fn line_notes(report: &SheetReport, card_index: usize, line_id: &str, red_chips: &[RedChip]) -> Vec<LineNote> {
    let about_line = report
        .diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.card == Some(card_index) && diagnostic.line.as_deref() == Some(line_id));
    let (skips, others): (Vec<_>, Vec<_>) = about_line.partition(|diagnostic| diagnostic.skips_line);
    skips
        .into_iter()
        .chain(others)
        .map(|diagnostic| {
            let text = diagnostic.diagnostic.message.clone();
            let red = diagnostic.diagnostic.severity == DiagnosticSeverity::Error
                || red_chips.iter().any(|chip| chip.note == text);
            LineNote {
                text,
                color: if red { NoteColor::Red } else { NoteColor::Amber },
            }
        })
        .collect()
}

/// Returns whether the card pane draws `diagnostic` under a line, which it does for a problem about a line in the
/// display order of its card.
///
/// Note that the checker also places a problem at a line the card never shows, such as a line table no lines list
/// names. Such a problem stays in the list under the panes.
pub fn draws_diagnostic(document: &SheetDocument, diagnostic: &SheetDiagnostic) -> bool {
    let (Some(card), Some(line)) = (diagnostic.card, diagnostic.line.as_deref()) else {
        return false;
    };
    document
        .cards
        .get(card)
        .is_some_and(|card| CardRelations::new(document, card).index(line).is_some())
}

/// Returns the tooltip of the "otherwise" words of line `line_id`, overridden by the lines numbered `numbers`.
///
/// It names the property the lines change, and the old state a cell must be in for both to apply.
fn otherwise_tooltip_of(
    document: &SheetDocument,
    relations: &CardRelations<'_>,
    line_id: &str,
    numbers: &[usize],
) -> String {
    let line = relations.line(line_id);
    let property = line
        .and_then(|line| line.property.as_deref())
        .and_then(|id| property_of(document, id));
    let state = line
        .and_then(|line| line.start_state.as_deref())
        .filter(|state| *state != ANY_STATE_ID)
        .and_then(|state| {
            property
                .and_then(|property| property.states.iter().find(|candidate| candidate.id == state))
                .map(|state| state.name.as_str())
        });
    let property_name = property.map_or(BLANK_TEXT, |property| property.name.as_str());
    otherwise_tooltip(numbers, property_name, state)
}

/// Returns the property `id` of any kind of thing of `document`.
fn property_of<'a>(document: &'a SheetDocument, id: &str) -> Option<&'a SheetProperty> {
    document
        .cells
        .iter()
        .flatten()
        .chain(document.agents.iter().flat_map(|kind| &kind.properties))
        .chain(document.ground.iter().flat_map(|ground| &ground.properties))
        .find(|property| property.id == id)
}

/// Draws `line`: its number, its sentence part by part, and its notes under it.
///
/// A line whose id `highlight` holds is drawn on a highlight, and the ids of the lines that a hovered or focused
/// "otherwise" or result chip names are added to `hovered`.
fn line_ui(ui: &mut Ui, line: &CardLine, highlight: &[String], hovered: &mut Vec<String>) {
    let background = ui.painter().add(Shape::Noop);
    let mut drawing = LineDrawing {
        hovered,
        for_each_shapes: Vec::new(),
        focused: false,
    };
    let row = ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = 0.0;
        ui.add_space(INDENT * line.depth as f32);
        ui.add_sized(
            [NUMBER_WIDTH, ui.spacing().interact_size.y],
            Label::new(RichText::new(line.number.to_string()).weak()),
        );
        for (index, part) in line.parts.iter().enumerate() {
            part_ui(ui, line, index, part, &mut drawing);
        }
        ui.unique_id()
    });
    let rect = row.response.rect;
    accessible_group(ui.ctx(), row.inner, Role::Group, &line.label(), rect);
    if highlight.contains(&line.id) {
        ui.painter()
            .set(background, Shape::rect_filled(rect, CornerRadius::ZERO, mcs::BLUE_950));
    }
    // The empty "for each ..." blank stays in the accessibility tree, and is painted only on a hovered or focused line.
    if drawing.focused || ui.rect_contains_pointer(rect) {
        for (shape, blank) in drawing.for_each_shapes {
            ui.painter()
                .set(shape, Shape::Vec(blank_shapes(ui, blank, FOR_EACH_BLANK_WORDS)));
        }
    }
    for note in &line.notes {
        let color = match note.color {
            NoteColor::Amber => ui.visuals().warn_fg_color,
            NoteColor::Red => ui.visuals().error_fg_color,
        };
        ui.horizontal(|ui| {
            ui.add_space(INDENT * line.depth as f32 + NUMBER_WIDTH);
            ui.add(Label::new(RichText::new(note.text.as_str()).color(color)).wrap());
        });
    }
}

/// What the parts of one line gather while they are drawn.
struct LineDrawing<'a> {
    /// Ids of the lines that a hovered or focused "otherwise" or result chip names.
    hovered: &'a mut Vec<String>,
    /// Shape and place of each empty "for each ..." blank, painted once the line is known to be hovered or focused.
    for_each_shapes: Vec<(egui::layers::ShapeIdx, Rect)>,
    /// Whether a control of the line holds the keyboard focus.
    focused: bool,
}

/// Draws `part`, part `index` of `line`, and gathers what `drawing` holds.
fn part_ui(ui: &mut Ui, line: &CardLine, index: usize, part: &SentencePart, drawing: &mut LineDrawing<'_>) {
    let name = line.control(part).map(|control| control.name).unwrap_or_default();
    let response = match part {
        SentencePart::Text(words) => {
            ui.label(words.as_str());
            return;
        }
        // A word that does not wrap moves to the next row whole when the row has no room left for it.
        SentencePart::ConditionWord(words) => {
            ui.add(Label::new(RichText::new(*words).strong()).extend());
            return;
        }
        SentencePart::From => {
            ui.add(Label::new(FROM_WORD).extend()).on_hover_text(FROM_TOOLTIP);
            return;
        }
        SentencePart::Chip(chip) => {
            let red = line
                .red_chips
                .iter()
                .find(|(red, _)| *red == index)
                .map(|(_, tooltip)| tooltip);
            let swatch = line
                .swatches
                .iter()
                .find(|(state, _)| *state == index)
                .map(|(_, color)| *color);
            let response = chip_ui(ui, chip, red.is_some(), swatch);
            let response = match red {
                Some(tooltip) => response.on_hover_text(tooltip.as_str()),
                None => response,
            };
            if let ChipKind::Result(result) = &chip.kind
                && (response.contains_pointer() || response.has_focus())
            {
                drawing.hovered.push(result.clone());
            }
            response
        }
        SentencePart::Blank(blank) => blank_ui(ui, blank.word(), &name, true).1,
        SentencePart::Select(select, words) => select_ui(ui, select_words(*select, words), &name, words.is_empty()),
        SentencePart::Always => add_button(
            ui,
            true,
            egui::Button::new(RichText::new(ALWAYS).color(mcs::GRAY_400)).frame(false),
            &name,
        )
        .on_hover_text(ALWAYS_TOOLTIP),
        SentencePart::Otherwise(lines) => {
            let words = otherwise_words(lines);
            let response = ui
                .add(Label::new(RichText::new(words.as_str()).weak()).sense(Sense::click()))
                .on_hover_text(line.otherwise_tooltip.as_str());
            ui.ctx().accesskit_node_builder(response.id, |node| {
                node.set_label(words);
                node.set_description(line.otherwise_tooltip.as_str());
            });
            if response.contains_pointer() || response.has_focus() {
                drawing.hovered.extend(line.otherwise.iter().cloned());
            }
            response
        }
        SentencePart::ForEachBlank(draw) => {
            ui.add_space(4.0);
            let (shape, response) = blank_ui(ui, FOR_EACH_BLANK_WORDS, &name, false);
            if let Some(shape) = shape {
                drawing.for_each_shapes.push((shape, response.rect));
            }
            response.on_hover_text(for_each_optional_tooltip(*draw))
        }
        _ => return,
    };
    drawing.focused |= response.has_focus();
}

/// Returns the words a select shows: its chosen words, or while empty [`UNIT_PLACEHOLDER`] for a unit and
/// [`BLANK_TEXT`] otherwise.
fn select_words(select: LineSelect, words: &str) -> &str {
    match (words.is_empty(), select) {
        (false, _) => words,
        (true, LineSelect::UnitOfTime) => UNIT_PLACEHOLDER,
        (true, _) => BLANK_TEXT,
    }
}

/// Colours of a chip: its fill, its text and its grey lead word.
struct ChipColors {
    /// Fill of the chip.
    fill: Color32,
    /// Colour of the chip's name.
    text: Color32,
    /// Colour of the lead word and of the slider mark.
    lead: Color32,
}

/// Returns the colours of a chip of `kind`, red for a result that its line cannot use.
fn chip_colors(kind: &ChipKind, red: bool) -> ChipColors {
    match (kind, red) {
        (_, true) => ChipColors {
            fill: mcs::RED_900,
            text: mcs::RED_200,
            lead: mcs::RED_300,
        },
        (ChipKind::Result(_), false) => ChipColors {
            fill: mcs::BLUE_900,
            text: mcs::BLUE_200,
            lead: mcs::BLUE_300,
        },
        _ => ChipColors {
            fill: mcs::GRAY_800,
            text: mcs::GRAY_100,
            lead: mcs::GRAY_400,
        },
    }
}

/// Adds the chip of `chip`, a button named by its visible text, and returns its response.
///
/// A result chip fills blue, or red when `red` holds. A parameter chip carries a slider mark, a state chip a swatch of
/// the colour `swatch`, and a property chip its lead word ("my") in grey. Clicking a chip does nothing.
fn chip_ui(ui: &mut Ui, chip: &SentenceChip, red: bool, swatch: Option<Color32>) -> Response {
    let colors = chip_colors(&chip.kind, red);
    let font = TextStyle::Body.resolve(ui.style());
    let painter = ui.painter();
    let mark = matches!(chip.kind, ChipKind::Parameter(_))
        .then(|| painter.layout_no_wrap(format!("{MDI_TUNE_VERTICAL} "), font.clone(), colors.lead));
    let lead =
        (!chip.lead.is_empty()).then(|| painter.layout_no_wrap(format!("{} ", chip.lead), font.clone(), colors.lead));
    let name = painter.layout_no_wrap(chip.name.clone(), font.clone(), colors.text);
    let swatch_side = font.size * 0.75;
    let swatch_width = if swatch.is_some() { swatch_side + 4.0 } else { 0.0 };
    let width = CHIP_PADDING.x * 2.0
        + swatch_width
        + mark.as_ref().map_or(0.0, |galley| galley.size().x)
        + lead.as_ref().map_or(0.0, |galley| galley.size().x)
        + name.size().x;
    let height = (name.size().y + CHIP_PADDING.y * 2.0).max(ui.spacing().interact_size.y - 2.0);
    let (rect, response) = ui.allocate_exact_size(vec2(width, height), Sense::click());
    response.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, chip.text()));
    if ui.is_rect_visible(rect) {
        let painter = ui.painter();
        painter.rect_filled(rect, CornerRadius::ZERO, colors.fill);
        if response.has_focus() || response.hovered() {
            painter.rect_stroke(
                rect,
                CornerRadius::ZERO,
                Stroke::new(1.0, colors.text),
                egui::StrokeKind::Inside,
            );
        }
        let mut x = rect.left() + CHIP_PADDING.x;
        let centre_y = rect.center().y;
        if let Some(color) = swatch {
            let square = Rect::from_center_size(Pos2::new(x + swatch_side / 2.0, centre_y), Vec2::splat(swatch_side));
            painter.rect_filled(square, CornerRadius::ZERO, color);
            painter.rect_stroke(
                square,
                CornerRadius::ZERO,
                Stroke::new(1.0, mcs::GRAY_600),
                egui::StrokeKind::Inside,
            );
            x += swatch_width;
        }
        for galley in [mark, lead, Some(name)].into_iter().flatten() {
            let size = galley.size();
            painter.galley(Pos2::new(x, centre_y - size.y / 2.0), galley, colors.text);
            x += size.x;
        }
    }
    response
}

/// Adds a dashed blank showing `word`, named `name`, and returns its response with the index of its shapes.
///
/// A blank with `painted` false is laid out and named but not painted, and returns the index of an empty shape that
/// the caller fills to paint it.
fn blank_ui(ui: &mut Ui, word: &str, name: &str, painted: bool) -> (Option<egui::layers::ShapeIdx>, Response) {
    let font = TextStyle::Body.resolve(ui.style());
    let galley = ui.painter().layout_no_wrap(word.to_owned(), font, mcs::GRAY_400);
    let size = galley.size() + CHIP_PADDING * 2.0;
    let height = size.y.max(ui.spacing().interact_size.y - 2.0);
    let (rect, response) = ui.allocate_exact_size(vec2(size.x, height), Sense::click());
    response.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, name));
    if painted {
        if ui.is_rect_visible(rect) {
            let shapes = blank_shapes(ui, rect, word);
            ui.painter().extend(shapes);
            if response.has_focus() {
                ui.painter().rect_stroke(
                    rect,
                    CornerRadius::ZERO,
                    Stroke::new(1.0, mcs::GRAY_200),
                    egui::StrokeKind::Inside,
                );
            }
        }
        (None, response)
    } else {
        (Some(ui.painter().add(Shape::Noop)), response)
    }
}

/// Returns the shapes of a dashed blank showing `word` in `rect`.
fn blank_shapes(ui: &Ui, rect: Rect, word: &str) -> Vec<Shape> {
    let font = TextStyle::Body.resolve(ui.style());
    let galley = ui.painter().layout_no_wrap(word.to_owned(), font, mcs::GRAY_400);
    let stroke = Stroke::new(1.0, mcs::GRAY_500);
    let corners = [
        rect.left_top(),
        rect.right_top(),
        rect.right_bottom(),
        rect.left_bottom(),
        rect.left_top(),
    ];
    let mut shapes = Shape::dashed_line(&corners, stroke, 3.0, 2.0);
    let size = galley.size();
    shapes.push(Shape::galley(
        Pos2::new(rect.left() + CHIP_PADDING.x, rect.center().y - size.y / 2.0),
        galley,
        mcs::GRAY_400,
    ));
    shapes
}

/// Adds a select inside a line showing `words`, named `name`, read-only, and returns its response. An `empty` select
/// shows its words in grey.
fn select_ui(ui: &mut Ui, words: &str, name: &str, empty: bool) -> Response {
    let font = TextStyle::Body.resolve(ui.style());
    let color = if empty { mcs::GRAY_400 } else { mcs::GRAY_100 };
    let painter = ui.painter();
    let text = painter.layout_no_wrap(words.to_owned(), font.clone(), color);
    let chevron = painter.layout_no_wrap(format!(" {MDI_CHEVRON_DOWN}"), font, mcs::GRAY_400);
    let width = text.size().x + chevron.size().x + CHIP_PADDING.x * 2.0;
    let height = (text.size().y + CHIP_PADDING.y * 2.0).max(ui.spacing().interact_size.y - 2.0);
    let (rect, response) = ui.allocate_exact_size(vec2(width, height), Sense::hover());
    response.widget_info(|| WidgetInfo {
        current_text_value: Some(words.to_owned()),
        ..WidgetInfo::labeled(WidgetType::ComboBox, false, name)
    });
    if ui.is_rect_visible(rect) {
        let painter = ui.painter();
        painter.rect_stroke(
            rect,
            CornerRadius::ZERO,
            Stroke::new(1.0, mcs::GRAY_700),
            egui::StrokeKind::Inside,
        );
        let text_size = text.size();
        let left = Pos2::new(rect.left() + CHIP_PADDING.x, rect.center().y - text_size.y / 2.0);
        painter.galley(left, text, color);
        let chevron_size = chevron.size();
        painter.galley(
            Pos2::new(left.x + text_size.x, rect.center().y - chevron_size.y / 2.0),
            chevron,
            mcs::GRAY_400,
        );
    }
    response
}
