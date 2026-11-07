//! Conversion of a sheet to a node graph: a copy of the graph the sheet lowers to, with frames that name the line
//! each node came from.
//!
//! A conversion runs one way. The copy is a graph file with an id and a name of its own, so it never hides the sheet,
//! and its code identity differs from the sheet's, since the id, name and description enter the semantic text. Its
//! `[editor]` region records the source in a [`ModelOrigin`], draws a frame around each origin group, labels the nodes
//! of named results and of reads of a property, and keeps the sheet's prose as notes. It places no node, and the
//! editor arranges the copy after loading it.
//!
//! - An *origin group* is a set of nodes that came from one place in the sheet: the nodes of one line, the nodes
//!   several lines share (the change of a property by several lines, and the read of a property those lines take), or
//!   the nodes of a part of the sheet that no line owns, such as a start, a measure, a colour rule or a button step.
//!   Each frame records its group's line ids, or the paths of its parts of the sheet, as `line` or `lines`.

use std::collections::HashMap;
use std::fmt;

use henad_core::explore::fingerprint::fnv1a64;
use henad_core::provenance::ModelCode;

use super::check::SheetReport;
use super::document::{CardRuns, ColorRuleKind, LineKind, MeasureRule, PropertyType, SheetDocument, SheetProperty};
use super::relations::CardRelations;
use super::sentence::{self, Sentences, SheetSection};
use super::{KIND_SHEET, NodeOwner, SheetFile};
use crate::editor::{EditorRegion, FrameLayout, ModelOrigin, NoteLayout};
use crate::file::{CODE_FORMAT, GraphFile, GraphFileError};
use crate::ids::NodeId;
use crate::value::InputValue;

/// MCS hue of the frame around the nodes of one line.
const LINE_FRAME_COLOR: &str = "blue";
/// MCS hue of the frame around the nodes several lines share.
const SHARED_FRAME_COLOR: &str = "purple";
/// MCS hue of the frame around the nodes of a part of the sheet.
const SHEET_FRAME_COLOR: &str = "gray";
/// Highest copy number [`ConvertOptions::new`] tries before it takes the one past it.
const MAX_COPY_NUMBER: u32 = 999;
/// First segments of the slot path of a node the sheet owns for a property of its kind of thing.
const PROPERTY_ROOTS: [&str; 2] = ["cells", "agents"];

/// Id, name, source file and date of a conversion.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct ConvertOptions {
    /// Model id of the copy.
    pub id: String,
    /// Name of the copy.
    pub name: String,
    /// File name of the sheet, relative to the copy's file, such as `game_of_life.toml`.
    pub file: String,
    /// Date of the conversion, such as `2026-11-07`.
    pub date: String,
}

impl ConvertOptions {
    /// Returns the options of the first copy of `sheet` whose id and name `taken` reports free, converted on `date`.
    ///
    /// The first copy takes the sheet's id with `_expert` added and its name with " (expert)" added, and copy `n`
    /// after it takes `_expert_<n>` and " (expert n)". `taken` returns whether a model already uses an id or a name,
    /// and the file is the sheet's id with `.toml` added.
    pub fn new(sheet: &SheetDocument, date: &str, taken: impl Fn(&str, &str) -> bool) -> Self {
        let names = |number: u32| {
            let id = if number <= 1 {
                format!("{}_expert", sheet.sheet.id)
            } else {
                format!("{}_expert_{number}", sheet.sheet.id)
            };
            (id, sentence::expert_copy_number_name(&sheet.sheet.name, number))
        };
        let number = (1..=MAX_COPY_NUMBER)
            .find(|&number| {
                let (id, name) = names(number);
                !taken(&id, &name)
            })
            .unwrap_or(MAX_COPY_NUMBER + 1);
        let (id, name) = names(number);
        Self {
            id,
            name,
            file: format!("{}.toml", sheet.sheet.id),
            date: date.to_owned(),
        }
    }
}

/// Reason a sheet does not convert.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum ConvertRefusal {
    /// The sheet holds an error, so it lowers to no graph.
    Errors,
    /// A line of the sheet is skipped, and the copy would leave it out.
    SkippedLines,
}

impl ConvertRefusal {
    /// Returns the reason a sheet whose check gave `report` does not convert, `None` for a sheet that converts.
    ///
    /// An error comes before a skipped line.
    pub fn of(report: &SheetReport) -> Option<Self> {
        if report.has_errors() {
            Some(Self::Errors)
        } else if report.skipped().next().is_some() {
            Some(Self::SkippedLines)
        } else {
            None
        }
    }

    /// Tooltip of the disabled Convert.
    pub fn tooltip(self) -> &'static str {
        match self {
            Self::Errors => sentence::CONVERT_NEEDS_FIXES,
            Self::SkippedLines => sentence::CONVERT_NEEDS_FINISHED_LINES,
        }
    }
}

/// Reason a conversion fails.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum ConvertError {
    /// The sheet does not convert.
    Refused(ConvertRefusal),
    /// The copy's canonical text does not read back.
    Unreadable(GraphFileError),
}

/// Writes the tooltip of a refusal, or the reading error.
impl fmt::Display for ConvertError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Refused(refusal) => write!(f, "{}", refusal.tooltip()),
            Self::Unreadable(error) => write!(f, "the copy does not read back: {error}"),
        }
    }
}

impl std::error::Error for ConvertError {}

/// Returns the node graph copy of `sheet`, under the id and name of `options`.
///
/// The copy holds the graph the sheet lowers to, every node with its id and key, so it runs tick for tick like the
/// sheet at one seed. Its description says which sheet it came from, and its `[editor]` region holds the origin record,
/// the frames, the labels and the notes the module doc describes.
///
/// # Errors
///
/// Returns [`ConvertError::Refused`] for a sheet with a skipped line, and [`ConvertError::Unreadable`] for a copy whose
/// canonical text does not read back.
pub fn convert(sheet: &SheetFile, options: &ConvertOptions) -> Result<GraphFile, ConvertError> {
    let report = sheet.parsed().check();
    if let Some(refusal) = ConvertRefusal::of(&report) {
        return Err(ConvertError::Refused(refusal));
    }
    let header = &sheet.document().sheet;
    let description = sentence::converted_description(&header.name);
    let mut document = sheet.lowered().document().clone();
    document.id.clone_from(&options.id);
    document.name.clone_from(&options.name);
    document.description.clone_from(&description);
    document.target.id.clone_from(&options.id);
    document.target.name.clone_from(&options.name);
    document.target.description = description;
    let copy_code = ModelCode::new(CODE_FORMAT, crate::writer::semantic_text(&document));

    let mut editor = EditorRegion::default();
    editor.set_model_origin(&ModelOrigin {
        kind: KIND_SHEET.to_owned(),
        model: header.id.clone(),
        name: header.name.clone(),
        file: options.file.clone(),
        hash: source_hash(sheet),
        code: format!("{:016x}", sheet.code().hash()),
        copy_code: format!("{:016x}", copy_code.hash()),
        converted: options.date.clone(),
    });
    for (node, label) in labels(sheet) {
        editor.set_node_label(node, &label);
    }
    for frame in frames(sheet, &report) {
        editor.push_frame(&frame);
    }
    for note in notes(sheet.document()) {
        editor.add_note(&note);
    }
    GraphFile::from_document(&document, Some(&editor)).map_err(ConvertError::Unreadable)
}

/// Returns the FNV-1a hash of the canonical text of `sheet`, as 16 lowercase hexadecimal digits.
pub(super) fn source_hash(sheet: &SheetFile) -> String {
    format!("{:016x}", fnv1a64(sheet.to_toml().as_bytes()))
}

/// Returns the notes of `document`: its purpose, then each parameter's meaning, each titled.
fn notes(document: &SheetDocument) -> Vec<NoteLayout> {
    let purpose = document
        .sheet
        .purpose
        .iter()
        .filter(|purpose| !purpose.is_empty())
        .map(|purpose| NoteLayout::about_model(SheetSection::Purpose.heading(), purpose));
    let meanings = document.parameters.iter().filter_map(|parameter| {
        let means = parameter.means.as_deref().filter(|means| !means.is_empty())?;
        Some(NoteLayout::about_model(&parameter.name, means))
    });
    purpose.chain(meanings).collect()
}

/// Returns the label of each node of the lowered graph of `sheet` that the copy names: the result of a named line
/// takes the line's name, and the read of one of my properties or of a neighbour's takes "my" or "their" and the
/// property's name.
fn labels(sheet: &SheetFile) -> Vec<(NodeId, String)> {
    let document = sheet.document();
    let map = sheet.line_map();
    let mut labels: HashMap<NodeId, String> = HashMap::new();
    for card in &document.cards {
        for (id, line) in &card.line_tables {
            if let (Some(name), Some(node)) = (&line.name, map.result_node(id)) {
                labels.insert(node, name.clone());
            }
        }
    }
    for pass in &sheet.lowered().document().passes {
        for node in &pass.body.nodes {
            if labels.contains_key(&node.id) {
                continue;
            }
            let lane = node.config.get("lane").and_then(crate::value::ConfigValue::as_str);
            let label = match (node.op.as_str(), lane, map.origin(node.id)) {
                ("lane.get", Some(lane), Some(origin))
                    if origin.owner == NodeOwner::Sheet && property_read(&origin.path) == Some(lane) =>
                {
                    sentence::my_property_item(&property_name(document, lane))
                }
                ("lane.get_at", Some(lane), Some(_)) => sentence::their_property_words(&property_name(document, lane)),
                _ => continue,
            };
            labels.insert(node.id, label);
        }
    }
    let mut labels: Vec<(NodeId, String)> = labels.into_iter().collect();
    labels.sort_by_key(|(node, _)| *node);
    labels
}

/// Returns the property the read at the slot path `path` takes, such as `status` for `cells.status`, `None` for a path
/// that is no read of a property.
fn property_read(path: &str) -> Option<&str> {
    let (root, property) = path.split_once('.')?;
    (PROPERTY_ROOTS.contains(&root) && !property.contains('.')).then_some(property)
}

/// Returns every property of `document`: the cells', each kind of agent's, then the ground's.
fn properties(document: &SheetDocument) -> impl Iterator<Item = &SheetProperty> {
    document
        .cells
        .iter()
        .flatten()
        .chain(document.agents.iter().flat_map(|kind| &kind.properties))
        .chain(document.ground.iter().flat_map(|ground| &ground.properties))
}

/// Returns the name of the property `id` of `document`, or `id` for a property it does not hold.
fn property_name(document: &SheetDocument, id: &str) -> String {
    properties(document)
        .find(|property| property.id == id)
        .map_or_else(|| id.to_owned(), |property| property.name.clone())
}

/// Origins of the nodes of one origin group.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Origins {
    /// Lines of one card, in display order.
    Lines(Vec<String>),
    /// Parts of the sheet that no line owns, by path, such as `measure.alive`.
    Sheet(Vec<String>),
}

/// Place of a line in the sheet: its card and its number on the card.
#[derive(Debug, Clone, Copy)]
struct LinePlace {
    /// Index of the card that holds the line.
    card: usize,
    /// Number of the line on its card, from 1.
    number: usize,
}

/// Lines of the cards of a sheet, with their places and relations.
struct SheetLines<'a> {
    /// The sheet.
    document: &'a SheetDocument,
    /// Relations of each card's lines, in card order.
    relations: Vec<CardRelations<'a>>,
    /// Place of each line, by id.
    places: HashMap<&'a str, LinePlace>,
    /// Report of the sheet's check.
    report: &'a SheetReport,
}

impl<'a> SheetLines<'a> {
    /// Returns the lines of `document`, whose check gave `report`.
    fn new(document: &'a SheetDocument, report: &'a SheetReport) -> Self {
        let relations: Vec<CardRelations<'a>> = document
            .cards
            .iter()
            .map(|card| CardRelations::new(document, card))
            .collect();
        let mut places = HashMap::new();
        for (card, card_relations) in relations.iter().enumerate() {
            for &id in card_relations.order() {
                if let Some(number) = card_relations.number(id) {
                    places.insert(id, LinePlace { card, number });
                }
            }
        }
        Self {
            document,
            relations,
            places,
            report,
        }
    }

    /// Returns the display order of line `id` across the sheet: its card's index, then its number.
    fn order(&self, id: &str) -> (usize, usize) {
        self.places
            .get(id)
            .map_or((usize::MAX, usize::MAX), |place| (place.card, place.number))
    }

    /// Returns the ids of the lines on the card `card_id` that write `property` and run, in display order.
    ///
    /// A Become, Set, Steer or Keep between line writes its own property, and a Move line writes the Position and the
    /// Heading of its kind of agent.
    fn writers(&self, card_id: &str, property: &str) -> Vec<String> {
        let Some(card) = self.document.cards.iter().position(|card| card.id == card_id) else {
            return Vec::new();
        };
        let moved = self
            .document
            .agents
            .iter()
            .find(|kind| kind.id == self.document.cards[card].every)
            .and_then(|kind| kind.properties.iter().find(|known| known.id == property))
            .is_some_and(|known| matches!(known.property_type, PropertyType::Position | PropertyType::Heading));
        self.relations[card]
            .order()
            .iter()
            .filter(|&&id| {
                self.relations[card].line(id).is_some_and(|line| {
                    let writes = match line.kind {
                        LineKind::Become | LineKind::Set | LineKind::Steer | LineKind::KeepBetween => {
                            line.property.as_deref() == Some(property)
                        }
                        LineKind::Move => moved,
                        _ => false,
                    };
                    writes && !self.report.is_skipped(id)
                })
            })
            .map(|&id| id.to_owned())
            .collect()
    }

    /// Returns the name of the button that runs the card at `card`, `None` for a card that runs each tick.
    ///
    /// A card that runs once with no button that runs it takes its id as the button's name, as its heading does.
    fn button(&self, card: usize) -> Option<&'a str> {
        let card = &self.document.cards[card];
        if card.runs != CardRuns::Once {
            return None;
        }
        let button = self.document.buttons.iter().find(|button| {
            button
                .does
                .iter()
                .any(|step| step.card.as_deref() == Some(card.id.as_str()))
        });
        Some(button.map_or(card.id.as_str(), |button| button.name.as_str()))
    }
}

/// Returns one frame per origin group of the lowered graph of `sheet`, whose check gave `report`, pass by pass in stack
/// order.
///
/// In a pass, the frames of single lines come first in display order, then the frames that several lines share, then
/// the frames of parts of the sheet.
pub(super) fn frames(sheet: &SheetFile, report: &SheetReport) -> Vec<FrameLayout> {
    let document = sheet.document();
    let lines = SheetLines::new(document, report);
    let sentences = Sentences::new(document);
    let mut frames = Vec::new();
    for pass in &sheet.lowered().document().passes {
        let scope = format!("pass.{}", pass.id);
        let mut groups: Vec<(Origins, Vec<NodeId>)> = Vec::new();
        for (node, origins) in pass_origins(sheet, &lines, &pass.id) {
            match groups.iter_mut().find(|(known, _)| *known == origins) {
                Some((_, members)) => members.push(node),
                None => groups.push((origins, vec![node])),
            }
        }
        groups.sort_by_key(|(origins, members)| match origins {
            Origins::Lines(ids) if ids.len() == 1 => (0, lines.order(&ids[0]), members[0]),
            Origins::Lines(ids) => (1, lines.order(&ids[0]), members[0]),
            Origins::Sheet(_) => (2, (0, 0), members[0]),
        });
        for (origins, members) in groups {
            let (title, color, ids) = match origins {
                Origins::Lines(ids) => {
                    let title = lines_title(sheet, &lines, &sentences, &ids, &members);
                    let color = if ids.len() == 1 {
                        LINE_FRAME_COLOR
                    } else {
                        SHARED_FRAME_COLOR
                    };
                    (title, color, ids)
                }
                Origins::Sheet(paths) => (sheet_title(document, &sentences, &paths), SHEET_FRAME_COLOR, paths),
            };
            frames.push(FrameLayout::new(&scope, &title, color, members).with_lines(ids));
        }
    }
    frames
}

/// Returns each node of the pass `pass_id` of the lowered graph of `sheet` with the origins of its group, in id order.
///
/// A node a line owns belongs to that line. The change of a property belongs to the lines that write it, a start,
/// measure, colour rule or button step to its part of the sheet, and a count shared by measures to those measures.
/// Any other node the sheet owns, such as the read of one of my properties or the part of a card several lines share,
/// belongs to the group its readers share once each of them has one: the lines they belong to together, or one part of
/// the sheet. A node whose readers never all get a group, such as the last node of a chain that no node reads, takes
/// the group its feeders share. A node whose readers or feeders share neither stands alone.
fn pass_origins(sheet: &SheetFile, lines: &SheetLines<'_>, pass_id: &str) -> Vec<(NodeId, Origins)> {
    let map = sheet.line_map();
    let Some(pass) = sheet.lowered().document().passes.iter().find(|pass| pass.id == pass_id) else {
        return Vec::new();
    };
    let nodes = &pass.body.nodes;
    let index_of: HashMap<NodeId, usize> = nodes.iter().enumerate().map(|(index, node)| (node.id, index)).collect();
    let mut readers: Vec<Vec<usize>> = vec![Vec::new(); nodes.len()];
    let mut feeders: Vec<Vec<usize>> = vec![Vec::new(); nodes.len()];
    for (reader, node) in nodes.iter().enumerate() {
        for input in node.inputs.values() {
            if let InputValue::Link(link) = input
                && let Some(&feeder) = index_of.get(&link.node)
                && !feeders[reader].contains(&feeder)
            {
                feeders[reader].push(feeder);
                readers[feeder].push(reader);
            }
        }
    }
    let mut groups: Vec<Option<Origins>> = nodes
        .iter()
        .map(|node| {
            let Some(origin) = map.origin(node.id) else {
                return Some(Origins::Sheet(Vec::new()));
            };
            match &origin.owner {
                NodeOwner::Line(line) => Some(Origins::Lines(vec![line.clone()])),
                NodeOwner::Sheet => sheet_origins(sheet.document(), lines, &origin.path),
            }
        })
        .collect();
    let alone = |index: usize| {
        let path = map
            .origin(nodes[index].id)
            .map(|origin| origin.path.clone())
            .unwrap_or_default();
        Origins::Sheet(vec![path])
    };
    // Each round settles the nodes whose readers all have a group. A round that settles none settles the nodes whose
    // feeders share a group, and a round that settles none of those leaves each remaining node alone.
    loop {
        let open: Vec<usize> = (0..nodes.len()).filter(|&index| groups[index].is_none()).collect();
        if open.is_empty() {
            break;
        }
        let known =
            |linked: &[usize]| -> Vec<&Origins> { linked.iter().filter_map(|&index| groups[index].as_ref()).collect() };
        let mut settled: Vec<(usize, Origins)> = open
            .iter()
            .filter(|&&index| {
                !readers[index].is_empty() && readers[index].iter().all(|&reader| groups[reader].is_some())
            })
            .map(|&index| {
                let group = shared_origins(lines, &known(&readers[index])).unwrap_or_else(|| alone(index));
                (index, group)
            })
            .collect();
        if settled.is_empty() {
            settled = open
                .iter()
                .filter_map(|&index| Some((index, shared_origins(lines, &known(&feeders[index]))?)))
                .collect();
        }
        if settled.is_empty() {
            settled = open.iter().map(|&index| (index, alone(index))).collect();
        }
        for (index, group) in settled {
            groups[index] = Some(group);
        }
    }
    nodes
        .iter()
        .zip(groups)
        .filter_map(|(node, group)| Some((node.id, group?)))
        .collect()
}

/// Returns the origins of a node the sheet owns at the slot path `path`, `None` for a node whose origins come from the
/// nodes it is linked to: the read of one of my properties, the part of a card several lines share, and the change of
/// a property that no line of the card writes.
fn sheet_origins(document: &SheetDocument, lines: &SheetLines<'_>, path: &str) -> Option<Origins> {
    let segments: Vec<&str> = path.split('.').collect();
    let part = |count: usize| Origins::Sheet(vec![segments[..count.min(segments.len())].join(".")]);
    Some(match segments.as_slice() {
        [root, _] if PROPERTY_ROOTS.contains(root) => return None,
        ["card", _] => return None,
        ["card", card, property] => {
            let writers = lines.writers(card, property);
            if writers.is_empty() {
                return None;
            }
            Origins::Lines(writers)
        }
        [root, property, "count"] if PROPERTY_ROOTS.contains(root) => {
            let measures: Vec<String> = document
                .measures
                .iter()
                .filter(|measure| {
                    measure.rule == MeasureRule::CountInState && measure.property.as_deref() == Some(*property)
                })
                .map(|measure| format!("measure.{}", measure.id))
                .collect();
            if measures.is_empty() {
                part(3)
            } else {
                Origins::Sheet(measures)
            }
        }
        [root, _, ..] if PROPERTY_ROOTS.contains(root) => part(3),
        _ => part(2),
    })
}

/// Returns the group the readers `readers` of a node share: the union of their lines when every reader belongs to
/// lines, or their one part of the sheet. Returns `None` for no reader, or readers of several kinds or parts.
fn shared_origins(lines: &SheetLines<'_>, readers: &[&Origins]) -> Option<Origins> {
    let first = readers.first()?;
    if readers.iter().all(|reader| matches!(reader, Origins::Lines(_))) {
        let mut ids: Vec<String> = Vec::new();
        for reader in readers {
            if let Origins::Lines(reader_ids) = reader {
                for id in reader_ids {
                    if !ids.contains(id) {
                        ids.push(id.clone());
                    }
                }
            }
        }
        ids.sort_by_key(|id| lines.order(id));
        return Some(Origins::Lines(ids));
    }
    readers.iter().all(|reader| reader == first).then(|| (*first).clone())
}

/// Returns the title of the frame of the lines `ids`, whose members are `members`.
///
/// One line's frame takes the line's prefix and sentence. A frame several lines share takes their prefix and the words
/// of the property values it works out: the new value of each property it changes, or else each property it reads. A
/// shared frame that works out neither, such as the part of a card several looks share, takes the prefix alone.
fn lines_title(
    sheet: &SheetFile,
    lines: &SheetLines<'_>,
    sentences: &Sentences<'_>,
    ids: &[String],
    members: &[NodeId],
) -> String {
    let place = ids.first().and_then(|id| lines.places.get(id.as_str()).copied());
    let button = place.and_then(|place| lines.button(place.card));
    if let [id] = ids {
        let number = place.map_or(0, |place| place.number);
        let otherwise = place.map_or_else(Vec::new, |place| lines.relations[place.card].otherwise_numbers(id));
        return sentence::line_frame_title(&sentence::line_prefix(button, number), &sentences.line(id, &otherwise));
    }
    let numbers: Vec<usize> = ids
        .iter()
        .filter_map(|id| lines.places.get(id.as_str()).map(|place| place.number))
        .collect();
    let document = sheet.document();
    let mut changed: Vec<String> = Vec::new();
    let mut read: Vec<String> = Vec::new();
    for member in members {
        let Some(origin) = sheet.line_map().origin(*member) else {
            continue;
        };
        let segments: Vec<&str> = origin.path.split('.').collect();
        let (list, words) = match segments.as_slice() {
            ["card", _, property] => (
                &mut changed,
                sentence::new_value_words(&property_name(document, property)),
            ),
            _ => match property_read(&origin.path) {
                Some(property) => (
                    &mut read,
                    sentence::my_property_item(&property_name(document, property)),
                ),
                None => continue,
            },
        };
        if !list.contains(&words) {
            list.push(words);
        }
    }
    let words = if changed.is_empty() { read } else { changed };
    let prefix = sentence::lines_prefix(button, &numbers);
    if words.is_empty() {
        return prefix;
    }
    sentence::line_frame_title(&prefix, &sentence::word_list(&words))
}

/// Returns the title of the frame of the parts of the sheet at `paths`.
fn sheet_title(document: &SheetDocument, sentences: &Sentences<'_>, paths: &[String]) -> String {
    let first = paths.first().map(String::as_str).unwrap_or_default();
    let segments: Vec<&str> = first.split('.').collect();
    let words = match segments.as_slice() {
        ["measure", ..] => {
            let names: Vec<String> = paths
                .iter()
                .filter_map(|path| {
                    let id = path.strip_prefix("measure.")?;
                    document.measures.iter().find(|measure| measure.id == id)
                })
                .map(|measure| measure.name.clone())
                .collect();
            sentence::measure_frame_words(&names)
        }
        [root, property, "starts_as"] if PROPERTY_ROOTS.contains(root) => {
            sentence::starts_frame_words(&property_name(document, property))
        }
        [root, property, ..] if PROPERTY_ROOTS.contains(root) => {
            sentence::my_property_item(&property_name(document, property))
        }
        ["card", card, ..] => match document.cards.iter().find(|known| known.id == *card) {
            Some(known) => match segments.get(2) {
                Some(property) => sentence::new_value_words(&property_name(document, property)),
                None => sentences.card_heading(known),
            },
            None => sentence::SHEET_PART_WORDS.to_owned(),
        },
        ["button", id] => {
            let name = document
                .buttons
                .iter()
                .find(|button| button.id == *id)
                .map_or(*id, |button| button.name.as_str());
            sentence::button_frame_words(name)
        }
        ["color", thing] => {
            // A colour by heading names no property, and reads the Heading of the kind of agent.
            let headings = document
                .agents
                .iter()
                .filter(|kind| kind.id == *thing)
                .flat_map(|kind| &kind.properties)
                .filter(|property| property.property_type == PropertyType::Heading);
            let names: Vec<String> = document
                .colors
                .get(*thing)
                .map(|rule| {
                    let read: Vec<String> = rule
                        .property
                        .iter()
                        .map(String::as_str)
                        .chain(rule.shades.iter().map(|shade| shade.property.as_str()))
                        .map(|property| property_name(document, property))
                        .collect();
                    match rule.rule {
                        ColorRuleKind::ByHeading => headings.map(|property| property.name.clone()).collect(),
                        _ => read,
                    }
                })
                .unwrap_or_default();
            sentence::color_frame_words(&sentences.thing_word(thing), &sentence::word_list(&names))
        }
        _ => sentence::SHEET_PART_WORDS.to_owned(),
    };
    sentence::sheet_frame_title(&words)
}

#[cfg(test)]
mod tests {
    use super::{Origins, PropertyType, SheetLines, property_read, sheet_origins, sheet_title};
    use crate::sheet::ParsedSheet;
    use crate::sheet::sentence::Sentences;
    use crate::tests::sheet_document::sheet_text;

    /// Returns `ids` as owned strings.
    fn owned(ids: &[&str]) -> Vec<String> {
        ids.iter().map(|id| (*id).to_owned()).collect()
    }

    #[test]
    fn agent_paths_group_and_title_like_cell_paths() {
        let parsed = ParsedSheet::parse(sheet_text("boids.toml")).expect("the boids sheet reads");
        let report = parsed.check();
        let document = parsed.document();
        assert!(
            document.agents[0]
                .properties
                .iter()
                .any(|property| property.property_type == PropertyType::Heading)
        );
        let lines = SheetLines::new(document, &report);
        assert_eq!(lines.writers("bird_tick", "velocity"), owned(&["l3", "l4", "l5", "l6"]));
        assert_eq!(lines.writers("bird_tick", "position"), owned(&["l7"]));
        assert_eq!(lines.writers("bird_tick", "heading"), owned(&["l7"]));
        assert_eq!(lines.writers("randomise_headings", "velocity"), owned(&["l8"]));
        assert_eq!(lines.writers("randomise_headings", "heading"), owned(&[]));

        assert_eq!(sheet_origins(document, &lines, "agents.velocity"), None);
        assert_eq!(sheet_origins(document, &lines, "card.bird_tick"), None);
        assert_eq!(sheet_origins(document, &lines, "card.randomise_headings.heading"), None);
        assert_eq!(
            sheet_origins(document, &lines, "card.bird_tick.heading"),
            Some(Origins::Lines(owned(&["l7"])))
        );
        assert_eq!(
            sheet_origins(document, &lines, "agents.velocity.starts_as"),
            Some(Origins::Sheet(owned(&["agents.velocity.starts_as"])))
        );

        let sentences = Sentences::new(document);
        let title = |path: &str| sheet_title(document, &sentences, &[path.to_owned()]);
        assert_eq!(title("agents.velocity.starts_as"), "Sheet, Velocity starts as");
        assert_eq!(title("agents.velocity"), "Sheet, my Velocity");
        assert_eq!(title("card.bird_tick"), "Sheet, Each tick, every bird");
        assert_eq!(
            title("card.bird_tick.heading"),
            "Sheet, the new Heading (the first change that applies wins)"
        );
        assert_eq!(title("color.bird"), "Sheet, colour each bird by Heading");
        assert_eq!(title("button.randomise_headings"), "Sheet, button Randomise headings");

        assert_eq!(property_read("agents.velocity"), Some("velocity"));
        assert_eq!(property_read("cells.status"), Some("status"));
        assert_eq!(property_read("cells.status.starts_as"), None);
        assert_eq!(property_read("card.bird_tick"), None);
    }
}
