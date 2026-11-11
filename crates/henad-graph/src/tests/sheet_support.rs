//! Sheets the tests read, and the helpers that edit, read, write, lower and run them: the sheet edit commands, lowered
//! sheets with their refusals, programs and host runs, the agent and grid sheets the lowering tests share, and the
//! hostile values and lines of the sheet mutation corpora.

use std::collections::BTreeSet;

use henad_compute::entry::ModelEntry;
use henad_core::diagnostic::CodeDiagnostic;
use henad_core::params::ParamValue;

use super::support::{HostileText, torus};
use crate::document::World;
use crate::host::{HostEvaluator, HostLanes, HostValue, RunContext};
use crate::node::Node;
use crate::sheet::document::{SheetChip, SheetDocument, SheetLine, SheetValue, Slot};
use crate::sheet::edit::{LineField, SheetBuilder, SheetEdit};
use crate::sheet::{ParsedSheet, SheetFile, SheetFileError, starters};
use crate::value::ConfigValue;
use crate::wgsl::{AgentProgram, GridProgram};

// The sheets the tests read, and their text edited, read and written.

/// Every sheet the tests read, by file name: the starters of [`starters`], then the test sheets.
pub(super) fn sheets() -> Vec<(&'static str, &'static str)> {
    let test_sheets = [
        ("sir_chance.toml", include_str!("sheets/sir_chance.toml")),
        ("mobile_sir.toml", MOBILE),
        ("sird.toml", include_str!("sheets/sird.toml")),
    ];
    starters()
        .iter()
        .map(|starter| (starter.file_name, starter.text))
        .chain(test_sheets)
        .collect()
}

/// Returns the sheet `file_name` of [`sheets`].
pub(crate) fn sheet_text(file_name: &str) -> &'static str {
    sheets()
        .into_iter()
        .find(|(name, _)| *name == file_name)
        .map(|(_, text)| text)
        .unwrap_or_else(|| panic!("no sheet {file_name}"))
}

/// Returns `text` with `old` replaced by `new` once, where `text` must hold `old`.
pub(super) fn edited(text: &str, old: &str, new: &str) -> String {
    assert!(text.contains(old), "the text holds no {old:?}");
    text.replacen(old, new, 1)
}

/// Returns `text` with `old` replaced by `new`, where `text` must hold `old` exactly once.
pub(super) fn edited_once(text: &str, old: &str, new: &str) -> String {
    assert_eq!(text.matches(old).count(), 1, "the text holds {old:?} other than once");
    text.replacen(old, new, 1)
}

/// Returns the sheet `file_name` with each `(old, new)` of `edits` applied by [`edited_once`].
pub(super) fn edited_sheet(file_name: &str, edits: &[(&str, &str)]) -> String {
    edits.iter().fold(sheet_text(file_name).to_owned(), |text, (old, new)| {
        edited_once(&text, old, new)
    })
}

/// Reads `text`, which must read, and returns its document.
pub(super) fn read(text: &str) -> SheetDocument {
    ParsedSheet::parse(text)
        .unwrap_or_else(|error| panic!("the sheet does not read: {error}\n{text}"))
        .into_document()
}

/// Writes `document`, which must hold only finite numbers.
pub(super) fn write(document: &SheetDocument) -> String {
    document
        .to_text()
        .unwrap_or_else(|error| panic!("the sheet does not write: {error}"))
}

// Sheet edit commands.

/// Returns a builder of the sheet `file_name`, with its keys seeded.
pub(super) fn builder_of(file_name: &str) -> SheetBuilder {
    SheetBuilder::new(read(sheet_text(file_name))).with_key_seed(12)
}

/// Runs `edit` on `builder`, which must accept it, and returns the ids it made.
pub(super) fn run(builder: &mut SheetBuilder, edit: SheetEdit) -> Vec<String> {
    let shown = format!("{edit:?}");
    builder
        .apply(edit)
        .unwrap_or_else(|error| panic!("{shown} is refused: {error}"))
}

/// Adds `line` to the card `card` of `builder`, inside the line `inside` or at the card's end, and returns the ids it
/// made.
pub(super) fn add_line(builder: &mut SheetBuilder, card: &str, inside: Option<&str>, line: SheetLine) -> Vec<String> {
    run(
        builder,
        SheetEdit::AddLine {
            card: card.to_owned(),
            inside: inside.map(str::to_owned),
            line: Box::new(line),
        },
    )
}

/// Fills the slot at `path` of the line `line` of `builder` with `value`, and returns the ids it made.
pub(super) fn set_slot(builder: &mut SheetBuilder, line: &str, path: &str, value: Slot) -> Vec<String> {
    run(
        builder,
        SheetEdit::SetSlot {
            line: line.to_owned(),
            path: path.to_owned(),
            value,
        },
    )
}

/// Sets `field` of the line `line` of `builder`, and returns the ids it made.
pub(super) fn set_field(builder: &mut SheetBuilder, line: &str, field: LineField) -> Vec<String> {
    run(
        builder,
        SheetEdit::SetField {
            line: line.to_owned(),
            phrase: String::new(),
            field,
        },
    )
}

/// Returns a chip value.
pub(super) fn chip(chip: SheetChip) -> Slot {
    Slot::Filled(SheetValue::Chip(chip))
}

/// Returns a phrase value.
pub(super) fn phrase(line: SheetLine) -> Slot {
    Slot::Filled(SheetValue::Phrase(Box::new(line)))
}

/// Returns a result chip of line `line`.
pub(super) fn result(line: &str) -> SheetChip {
    SheetChip::Result {
        line: line.to_owned(),
        part: None,
        property: None,
    }
}

/// Returns the line `line_id` of `builder`'s sheet.
pub(super) fn line<'a>(builder: &'a SheetBuilder, line_id: &str) -> &'a SheetLine {
    builder
        .document()
        .line(line_id)
        .map(|(_, line)| line)
        .unwrap_or_else(|| panic!("no line {line_id}"))
}

// Lowered sheets, their refusals, their programs and their runs on the host.

/// Lowers `text`, which must lower.
pub(super) fn lowered(text: &str) -> SheetFile {
    SheetFile::parse(text).unwrap_or_else(|error| panic!("the sheet does not lower: {error}\n{text}"))
}

/// Returns the errors of lowering `text`, which must hold some.
pub(super) fn refusals(text: &str) -> Vec<CodeDiagnostic> {
    match SheetFile::parse(text) {
        Ok(_) => panic!("the sheet lowers:\n{text}"),
        Err(SheetFileError::Invalid(errors)) => errors.as_slice().to_vec(),
        Err(other) => panic!("the sheet does not read: {other}"),
    }
}

/// Returns the error of lowering `text`, which must hold exactly one.
pub(super) fn only_refusal(text: &str) -> CodeDiagnostic {
    let mut errors = refusals(text);
    assert_eq!(errors.len(), 1, "{errors:#?}");
    errors.remove(0)
}

/// Returns the bytes of `text` that `diagnostic` points at.
pub(super) fn placed_at<'a>(diagnostic: &CodeDiagnostic, text: &'a str) -> &'a str {
    let span = diagnostic
        .span
        .clone()
        .unwrap_or_else(|| panic!("{diagnostic} has no place"));
    &text[span]
}

/// Returns the program of `file`, a sheet of agents, compiled for the GPU with its fault names.
pub(super) fn agent_program_of(file: &SheetFile) -> AgentProgram {
    AgentProgram::compile_with_fault_names(file.lowered(), file.fault_names())
        .unwrap_or_else(|problems| panic!("{problems:#?}"))
}

/// Returns the entry of `file` compiled for its world, a sheet of agents with its fault names.
pub(super) fn entry_of(file: &SheetFile) -> ModelEntry {
    match file.lowered().document().world {
        World::Grid(_) => GridProgram::compile(file.lowered())
            .unwrap_or_else(|problems| panic!("{problems:#?}"))
            .entry(),
        World::Agents(_) => agent_program_of(file).entry(),
    }
}

/// Returns the nodes of the pass `pass` of `file`.
pub(super) fn pass_nodes<'a>(file: &'a SheetFile, pass: &str) -> &'a [Node] {
    &file
        .lowered()
        .document()
        .passes
        .iter()
        .find(|known| known.id == pass)
        .unwrap_or_else(|| panic!("no pass {pass}"))
        .body
        .nodes
}

/// Returns the names of the accumulators of the fold or region `node`, none when it has no `acc` array.
pub(super) fn accumulators(node: &Node) -> Vec<String> {
    node.config
        .get("acc")
        .and_then(ConfigValue::as_array)
        .map(|entries| {
            entries
                .iter()
                .filter_map(|entry| entry.as_table()?.get("name")?.as_str().map(str::to_owned))
                .collect()
        })
        .unwrap_or_default()
}

/// Returns the host evaluator of the graph `file` lowers to.
pub(super) fn host_of(file: &SheetFile) -> HostEvaluator {
    HostEvaluator::new(file.lowered()).expect("the host runs the lowered graph")
}

/// Returns `text` with a world of `side` by `side` cells in place of 1024 by 1024.
pub(super) fn sized(text: &str, side: u32) -> String {
    let text = edited_once(text, "width = 1024", &format!("width = {side}"));
    edited_once(&text, "height = 1024", &format!("height = {side}"))
}

/// Parameter values of Game of Life at their defaults: the initial density.
pub(super) const LIFE_PARAMS: [ParamValue; 1] = [ParamValue::F32(0.3)];

/// Returns the number of cells of the `health` lane of `lanes` in each of its first `states` states.
pub(super) fn health_counts(lanes: &HostLanes, states: usize) -> Vec<usize> {
    let health = lanes.column("health").expect("a health lane");
    let mut counts = vec![0; states];
    for cell in 0..health.len() {
        match health.get(cell) {
            Some(HostValue::Category(state)) if (state as usize) < states => counts[state as usize] += 1,
            other => panic!("cell {cell} holds {other:?}"),
        }
    }
    counts
}

/// Cells of an SIR sheet's run on the host in each of the three health states.
pub(super) struct Epidemic {
    /// Cells in each state at the start.
    pub start: Vec<usize>,
    /// Cells in each state after a press of New outbreak.
    pub pressed: Vec<usize>,
    /// Most cells infected at once over the ticks after the press.
    pub peak: usize,
    /// Cells in each state at the end.
    pub end: Vec<usize>,
}

/// Runs the SIR sheet `file` of 64 by 64 cells on the host from seed 5 with `params`: its start, a press of New
/// outbreak, then 200 ticks. Every cell must hold one of the three health states after the press and after each tick.
pub(super) fn epidemic(file: &SheetFile, params: &[ParamValue]) -> Epidemic {
    let evaluator = host_of(file);
    let mut lanes = evaluator.init(torus(64, 64), params, Some(5)).expect("the start runs");
    let start = health_counts(&lanes, 3);
    let mut stream = 0;
    evaluator
        .run_pass(
            "button_new_outbreak",
            &mut lanes,
            params,
            &RunContext::action(Some(5), 0, 1, 0),
            &mut stream,
        )
        .expect("the press runs");
    let pressed = health_counts(&lanes, 3);
    assert_eq!(pressed.iter().sum::<usize>(), 64 * 64, "{pressed:?}");
    let mut peak = 0;
    for tick in 0..200 {
        evaluator
            .run_pass(
                "tick_cell_tick",
                &mut lanes,
                params,
                &RunContext::tick(Some(5), tick),
                &mut stream,
            )
            .expect("a tick runs");
        let counts = health_counts(&lanes, 3);
        assert_eq!(counts.iter().sum::<usize>(), 64 * 64, "tick {tick}: {counts:?}");
        peak = peak.max(counts[1]);
    }
    Epidemic {
        start,
        pressed,
        peak,
        end: health_counts(&lanes, 3),
    }
}

// Agent sheets and their edits.

/// Text of the test sheet of chances once for each infected person nearby.
pub(super) const MOBILE: &str = include_str!("sheets/mobile_sir.toml");

/// Text of the test sheet whose Repeat of 4 rolls a number and becomes A below 0.5, and otherwise B.
pub(super) const ROLL: &str = include_str!("sheets/repeat_roll.toml");

/// Returns `text` with every Become of Status turned into a Set of a Number property Level starting at 0: to 1 for a
/// Become to A, and to 2 for a Become to B.
pub(super) fn with_sets(text: &str) -> String {
    let text = edited(
        text,
        "[[measure]]",
        "[[agents.property]]\nid = \"level\"\nname = \"Level\"\ntype = \"number\"\n\n[agents.property.starts_as]\n\
         rule = \"value\"\nvalue = 0\n\n[[measure]]",
    );
    let text = edited(
        &text,
        "property = \"status\"\nnew_state = \"a\"\nstart_state = \"s\"",
        "property = \"level\"\nvalue = 1",
    );
    let text = edited(
        &text,
        "property = \"status\"\nnew_state = \"b\"\nstart_state = \"s\"",
        "property = \"level\"\nvalue = 2",
    );
    text.replace("kind = \"become\"", "kind = \"set\"")
}

/// Returns the boids with the line table `table` added as line `l9` of the card that runs each tick.
pub(super) fn boids_with(table: &str) -> String {
    let text = edited(sheet_text("boids.toml"), "next_line = 9", "next_line = 10");
    let text = edited(
        &text,
        "lines = [\"l1\", \"l2\", \"l3\", \"l4\", \"l5\", \"l6\", \"l7\"]",
        "lines = [\"l1\", \"l2\", \"l3\", \"l4\", \"l5\", \"l6\", \"l7\", \"l9\"]",
    );
    edited(
        &text,
        "# Lines of Randomise headings",
        &format!("{table}\n\n# Lines of Randomise headings"),
    )
}

/// Returns the ants with `lines` added to the card that runs each tick, the line tables `tables` after the last table,
/// and `next_line` as the next line number.
pub(super) fn ants_with(next_line: u32, lines: &[&str], tables: &str) -> String {
    let text = edited(
        sheet_text("ants.toml"),
        "next_line = 17",
        &format!("next_line = {next_line}"),
    );
    let added: String = lines.iter().map(|line| format!(", \"{line}\"")).collect();
    let text = edited(&text, "\"l12\", \"l16\"]", &format!("\"l12\", \"l16\"{added}]"));
    format!("{text}\n{tables}")
}

// Grid sheets and their readers, shared by the grid lowering tests.

/// Text of the Game of Life starter.
pub(super) fn life() -> &'static str {
    starters()[0].text
}

/// Text of the Majority vote starter.
pub(super) fn vote() -> &'static str {
    sheet_text("vote.toml")
}

/// Game of Life with line `table` added to the card that runs each tick as line `l5`.
pub(super) fn life_with_line(table: &str) -> String {
    let text = edited(life(), "next_line = 5", "next_line = 6");
    let text = edited(
        &text,
        "lines = [\"l1\", \"l2\", \"l3\"]",
        "lines = [\"l1\", \"l2\", \"l3\", \"l5\"]",
    );
    edited(
        &text,
        "# Lines of Clear: run once, every cell, when pressed",
        &format!("{table}\n\n# Lines of Clear: run once, every cell, when pressed"),
    )
}

/// Returns the code of `text`, which must lower.
pub(super) fn code(text: &str) -> henad_core::provenance::ModelCode {
    lowered(text).code()
}

/// A grid sheet over the line kinds of this lowering: Counts by `grid.count` and by a fold, an all of holding a none
/// of, two Sets of one property, the outside test, a Set inside a When on an any of, and Becomes with one of, within
/// and `my_new`.
pub(super) const KINDS: &str = include_str!("sheets/kinds.toml");

/// A grid sheet over the lines that work out values from the cells around: three Look arounds, two with an `only` test
/// and one of them able to leave out every neighbour, covering every summary, a Count with an `only` test, a Calculate
/// over their parts, a Choose, a Depending on, and Sets and a Become reading them.
pub(super) const VALUES: &str = include_str!("sheets/values.toml");

/// Two cells properties for a Game of Life sheet: `level`, a Number starting at a random number between 0 and 1, and
/// `mood`, a States property starting calm.
pub(super) const EXTRA_PROPERTIES: &str = "[[cells.property]]\nid = \"level\"\nname = \"Level\"\ntype = \"number\"\n\n\
                                [cells.property.starts_as]\nrule = \"value\"\nkey = 77\n\n\
                                [cells.property.starts_as.value]\nkind = \"random_number\"\nlow = 0\nhigh = 1\n\n\
                                [[cells.property]]\nid = \"mood\"\nname = \"Mood\"\ntype = \"states\"\nstates = [\n  \
                                { id = \"calm\", name = \"Calm\", color = \"#2060E0\" },\n  { id = \"excited\", name = \
                                \"Excited\", color = \"#E06020\" },\n]\n\n[cells.property.starts_as]\nrule = \"value\"\n\
                                value = { state = \"calm\" }\n\n";

/// Returns `text`, a Game of Life sheet, with [`EXTRA_PROPERTIES`] after its `status` property and `parts` before its
/// first card. `parts` holds measures, colours or buttons.
pub(super) fn life_with(text: &str, parts: &str) -> String {
    let text = edited(text, "[[parameter]]", &format!("{EXTRA_PROPERTIES}[[parameter]]"));
    edited(
        &text,
        "# Each tick, every cell\n",
        &format!("{parts}# Each tick, every cell\n"),
    )
}

/// Returns the value of the `level` lane at every cell.
pub(super) fn levels(lanes: &HostLanes) -> Vec<f32> {
    let column = lanes.column("level").expect("a level lane");
    (0..column.len())
        .map(|cell| match column.get(cell) {
            Some(HostValue::F32(value)) => value,
            other => panic!("level at {cell} is {other:?}"),
        })
        .collect()
}

/// Returns the alive cells of a Game of Life world `side` cells wide, as column and row.
pub(super) fn alive(lanes: &HostLanes, side: u32) -> BTreeSet<(u32, u32)> {
    let status = lanes.column("status").expect("a status lane");
    (0..side * side)
        .filter(|&cell| status.get(cell as usize) == Some(HostValue::Category(1)))
        .map(|cell| (cell % side, cell / side))
        .collect()
}

/// Returns the lanes of the lowered Game of Life on a world `side` cells wide after its start at `seed`.
pub(super) fn started(evaluator: &HostEvaluator, side: u32, seed: u64) -> HostLanes {
    let world = torus(side, side);
    evaluator.init(world, &LIFE_PARAMS, Some(seed)).expect("the start runs")
}

// Values and lines the edits of the sheet mutation corpora put in.

/// Values a token edit puts in place of a value.
const HOSTILE_VALUES: [&str; 24] = [
    "0",
    "-1",
    "4294967296",
    "9223372036854775807",
    "1e400",
    "-0.0",
    "nan",
    "\"\"",
    "\"l1\"",
    "\"any_state\"",
    "\"fortnight\"",
    "[]",
    "[[]]",
    "{}",
    "{ kind = \"always\" }",
    "{ result = \"l99\", part = \"amount\" }",
    "{ param = \"x\", my = \"y\" }",
    "true",
    "1979-05-27",
    "\"#GGGGGG\"",
    "{ percent = 30 }",
    "{ percent = \"30\" }",
    "{ percent = 1e400 }",
    "{ percent = 30, my = \"y\" }",
];

/// Lines an edit inserts.
const HOSTILE_LINES: [&str; 7] = [
    "kind = \"when\"",
    "[card.line.l1.condition]\nkind = \"always\"",
    "[[card.line.l1.items]]\nresult = \"l1\"",
    "[[card]]\nid = \"x\"\nevery = \"cells\"\nruns = \"once\"",
    "[card.line.l99]\nkind = \"repeat\"\nlines = [\"l99\"]",
    "version = 2",
    "percent = 30",
];

/// Values and lines the edits of the sheet corpora put in.
pub(super) const SHEET_HOSTILE: HostileText = HostileText {
    values: &HOSTILE_VALUES,
    lines: &HOSTILE_LINES,
};
