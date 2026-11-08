//! The sheet reader over the starters and the test sheets, its refusals, its blanks and a mutation corpus.
//!
//! The starters in the crate's `sheets/` directory are Game of Life, Majority vote, forest fire, random walk and
//! bounded confidence written from the models of henad-models' `tests/fixtures/lowered/round1/`, and SIR with rates,
//! boids and ants from the author format's examples. The test sheets under `sheets/` here hold SIR with chances written
//! from the same `round1/` models, and a sheet of chances once for each infected person nearby. Every sheet holds two
//! stored forms: every condition that always holds written `{ kind = "always" }`, and every Become from any state
//! written `start_state = "any_state"`. Game of Life's Clear is a button that runs a card once, and the SIR buttons are
//! named New outbreak.

use std::panic::{AssertUnwindSafe, catch_unwind};

use henad_core::authoring::primitives::rng::xorshift64;
use henad_core::diagnostic::CodeDiagnostic;
use henad_core::metadata::TimeUnit;

use crate::model_text::{ModelHeader, read_header};
use crate::sheet::document::{
    Around, ButtonStepRule, CardRuns, ChipRelation, CompareTest, Edges, LineKind, MadeOf, MeasureRule, PropertyType,
    SheetChip, SheetDocument, SheetLine, SheetLiteral, SheetValue, Slot, StartRule, SummaryKind,
};
use crate::sheet::limits::{MAX_LINES_PER_CARD, MAX_NESTING, MAX_SHEET_BYTES};
use crate::sheet::{ParsedSheet, SheetFileError, TablePlaces, starters};

/// Every sheet the tests read, by file name: the starters of [`starters`], then the test sheets.
pub(super) fn sheets() -> Vec<(&'static str, &'static str)> {
    let test_sheets = [
        ("sir_chance.toml", include_str!("sheets/sir_chance.toml")),
        ("mobile_sir.toml", include_str!("sheets/mobile_sir.toml")),
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

/// Reads `text`, which must read.
fn read(text: &str) -> ParsedSheet {
    ParsedSheet::parse(text).unwrap_or_else(|error| panic!("the sheet does not read: {error}"))
}

/// Returns the diagnostic of reading `text`, which must not read.
fn refusal(text: &str) -> SheetFileError {
    match ParsedSheet::parse(text) {
        Ok(_) => panic!("the sheet reads:\n{text}"),
        Err(error) => error,
    }
}

/// Checks that `diagnostic` is placed on `expected`, the bytes of `text` it points at.
fn assert_at(diagnostic: &CodeDiagnostic, text: &str, expected: &str) {
    let span = diagnostic
        .span
        .clone()
        .unwrap_or_else(|| panic!("{diagnostic} has no place"));
    assert_eq!(&text[span], expected, "{diagnostic}");
}

/// Returns the line `line_id` of `document`.
fn line<'a>(document: &'a SheetDocument, line_id: &str) -> &'a SheetLine {
    document.line(line_id).unwrap_or_else(|| panic!("no line {line_id}")).1
}

/// Returns the phrase that `slot` holds.
fn phrase(slot: &Slot) -> &SheetLine {
    match slot.value() {
        Some(SheetValue::Phrase(phrase)) => phrase,
        other => panic!("{other:?} is no phrase"),
    }
}

/// Checks that `places` is the table whose header is `header` in `text`.
fn assert_header(places: &TablePlaces, text: &str, header: &str) {
    assert_eq!(&text[places.table.clone()], header);
}

#[test]
fn every_sheet_reads() {
    for (file_name, text) in sheets() {
        let sheet = read(text);
        let document = sheet.document();
        assert!(!document.cards.is_empty(), "{file_name}");
        for (card_index, card) in document.cards.iter().enumerate() {
            for line_id in card.line_tables.keys() {
                let places = sheet
                    .places()
                    .line(card_index, line_id)
                    .unwrap_or_else(|| panic!("{file_name}: line {line_id} has no place"));
                assert_header(places, text, &format!("[card.line.{line_id}]"));
                assert!(places.key("kind").is_some(), "{file_name}: {line_id}");
            }
        }
    }

    // The ants hold the deepest nesting, well within the limit.
    let deepest = sheets()
        .into_iter()
        .map(|(_, text)| nesting_depth(text))
        .max()
        .unwrap_or(0);
    assert!(deepest * 2 <= MAX_NESTING, "a sheet nests {deepest} deep");

    // The ants' deepest table: a chip in the items of an all of in the condition of a Choose row.
    let text = sheet_text("ants.toml");
    let sheet = read(text);
    let next_cell = line(sheet.document(), "l7");
    assert_eq!(next_cell.kind, LineKind::Choose);
    assert_eq!(next_cell.rows.len(), 2);
    let all_of = phrase(&next_cell.rows[0].condition);
    assert_eq!(all_of.kind, LineKind::AllOf);
    assert_eq!(all_of.items.len(), 3);
    assert_eq!(
        all_of.items[1],
        SheetValue::Chip(SheetChip::Result {
            line: "l6".to_owned(),
            part: Some("open".to_owned()),
            property: None,
        })
    );
    let SheetValue::Phrase(chance) = &all_of.items[2] else {
        panic!("{:?} is no phrase", all_of.items[2]);
    };
    assert_eq!(chance.kind, LineKind::Chance);
    assert_eq!(phrase(&next_cell.rows[1].value).kind, LineKind::PickCell);
    assert_eq!(next_cell.otherwise.value(), Some(&SheetValue::Chip(result("l5"))));
    let card_index = sheet.document().line("l7").expect("line 7").0;
    let items = sheet
        .places()
        .line(card_index, "l7")
        .and_then(|places| places.table("row"))
        .and_then(|row| row.table("condition"))
        .map(|condition| condition.tables("items"))
        .expect("the items of line 7 have places");
    assert_eq!(items.len(), 3);
    assert_header(&items[1], text, "[[card.line.l7.row.condition.items]]");
    assert_eq!(items[1].key("part").map(|span| &text[span]), Some("\"open\""));
}

#[test]
fn every_table_reads_its_values() {
    let sheet = read(sheet_text("ants.toml"));
    let ants = sheet.document();
    assert_eq!(ants.world.made_of, MadeOf::Agents);
    assert_eq!(ants.world.edges, Edges::Walls);
    assert_eq!(ants.agents[0].properties[1].property_type, PropertyType::Heading);
    assert!(ants.agents[0].properties[1].starts_as.is_none());
    let ground = ants.ground.as_ref().expect("the ants' ground");
    assert_eq!(ground.blocked[0].state, "obstacle");
    let shapes = &ground.properties[0]
        .starts_as
        .as_ref()
        .expect("the ground's start")
        .shapes;
    assert_eq!(shapes.len(), 4);
    assert_eq!(shapes[0].center, Some([0.5, 0.725]));
    assert_eq!(shapes[0].angle, Some(45.0));
    assert_eq!(ants.colors["ground"].shades[1].color, [0x2E, 0x8B, 0xE8, 0xFF]);
    assert_eq!(ants.buttons[0].does[0].rule, ButtonStepRule::StartAgain);
    let way = line(ants, "l5");
    assert_eq!(way.branches.len(), 2);
    assert_eq!(way.branches["carrying_food"], SheetValue::Chip(result("l2")));
    assert_eq!(line(ants, "l10").start_state.as_deref(), Some("any_state"));
    assert_eq!(phrase(&line(ants, "l16").condition).kind, LineKind::Always);

    let life = read(sheet_text("game_of_life.toml")).into_document();
    assert_eq!(life.sheet.next_line, 5);
    assert_eq!(life.world.width, 1024);
    let status = &life.cells.as_ref().expect("the cells")[0];
    assert_eq!(status.states[1].color, [0x00, 0xE6, 0x76, 0xFF]);
    let start = status.starts_as.as_ref().expect("the start of Status");
    assert_eq!(start.rule, StartRule::ByChance);
    assert_eq!(start.key, Some(2_929_187_291));
    assert_eq!(
        start.rows[0].chance.value(),
        Some(&SheetValue::Chip(param("initial_density")))
    );
    assert_eq!(life.cards[1].runs, CardRuns::Once);
    assert_eq!(life.buttons[1].does[0].card.as_deref(), Some("clear"));
    let neighbours = line(&life, "l1");
    assert_eq!(neighbours.around, Some(Around::Cells8));
    assert_eq!(
        phrase(&neighbours.whose).left.value(),
        Some(&SheetValue::Chip(their("status")))
    );
    let survive = phrase(&line(&life, "l3").condition);
    assert_eq!(survive.test, Some(CompareTest::NotOneOf));
    assert_eq!(
        survive.right.value(),
        Some(&SheetValue::List(vec![number(2.0), number(3.0)]))
    );

    let sir = read(sheet_text("sir.toml")).into_document();
    let time = sir.time.expect("the time scale of SIR");
    assert_eq!((time.tick_length, time.tick_unit), (1.0, TimeUnit::Day));
    assert_eq!(sir.parameters[0].unit.as_deref(), Some("per day"));
    let infection = phrase(&line(&sir, "l2").condition);
    assert_eq!(infection.kind, LineKind::Rate);
    assert_eq!(infection.rate_unit, Some(TimeUnit::Day));
    assert!(!infection.rate_unit_from_tick);
    assert_eq!(infection.tries.value(), Some(&SheetValue::Chip(result("l1"))));
    assert_eq!(sir.buttons[0].id, "new_outbreak");

    let boids = read(sheet_text("boids.toml")).into_document();
    let flockmates = line(&boids, "l1");
    assert_eq!(flockmates.summaries[1].summary, Some(SummaryKind::Average));
    assert_eq!(
        flockmates.summaries[1].value.value(),
        Some(&SheetValue::Chip(SheetChip::Relation(ChipRelation::OffsetToThem)))
    );
    assert_eq!(flockmates.include_me, Some(false));
    let randomise = phrase(&line(&boids, "l8").value);
    assert_eq!(
        phrase(&randomise.length).formula.as_deref(),
        Some("length(my.velocity)")
    );
}

#[test]
fn starters_returns_the_grid_sheets_then_the_agent_sheets() {
    let names: Vec<&str> = starters().iter().map(|starter| starter.file_name).collect();
    assert_eq!(
        names,
        [
            "game_of_life.toml",
            "sir.toml",
            "vote.toml",
            "forest_fire.toml",
            "random_walk.toml",
            "boids.toml",
            "bounded_confidence.toml",
            "ants.toml"
        ]
    );
    for (index, starter) in starters().iter().enumerate() {
        let document = read(starter.text).into_document();
        let made_of = if index < 4 { MadeOf::Cells } else { MadeOf::Agents };
        assert_eq!(document.world.made_of, made_of, "{}", starter.file_name);
    }
    assert_eq!(read(sheet_text("vote.toml")).document().sheet.name, "Majority vote");
}

#[test]
fn the_sheets_written_from_round_1_read_their_values() {
    // Majority vote: "otherwise become A" from any state, always.
    let vote = read(sheet_text("vote.toml")).into_document();
    assert_eq!(line(&vote, "l1").around, Some(Around::Block3x3));
    let follow_b = line(&vote, "l2");
    assert_eq!(follow_b.start_state.as_deref(), Some("any_state"));
    assert_eq!(phrase(&follow_b.condition).test, Some(CompareTest::AtLeast));
    let follow_a = line(&vote, "l3");
    assert_eq!(follow_a.start_state.as_deref(), Some("any_state"));
    assert_eq!(phrase(&follow_a.condition).kind, LineKind::Always);

    // Forest fire: two independent chances, each on a line with its own key, and a burn-out that always holds.
    let fire = read(sheet_text("forest_fire.toml")).into_document();
    assert_eq!(line(&fire, "l1").around, Some(Around::Cells4));
    assert_eq!(phrase(&line(&fire, "l3").condition).kind, LineKind::Chance);
    assert_eq!(phrase(&line(&fire, "l4").condition).kind, LineKind::Always);
    assert!(line(&fire, "l3").key.is_some() && line(&fire, "l5").key.is_some());
    assert_eq!(fire.parameters[2].default, 1e-5);
    assert_eq!(fire.buttons[0].does[0].properties, ["cover"]);

    // SIR with chances: "for each of" Infected neighbours, no time scale, and the New outbreak card.
    let sir = read(sheet_text("sir_chance.toml")).into_document();
    assert!(sir.time.is_none());
    let infection = phrase(&line(&sir, "l2").condition);
    assert_eq!(infection.kind, LineKind::Chance);
    assert_eq!(infection.tries.value(), Some(&SheetValue::Chip(result("l1"))));
    assert_eq!(sir.buttons[0].id, "new_outbreak");
    assert_eq!(sir.cards[1].id, "new_outbreak");
    assert_eq!(sir.cards[1].runs, CardRuns::Once);

    // Random walk: a Move by inside a Repeat.
    let walk = read(sheet_text("random_walk.toml")).into_document();
    assert_eq!(walk.world.made_of, MadeOf::Agents);
    let repeat = line(&walk, "l1");
    assert_eq!(repeat.kind, LineKind::Repeat);
    assert_eq!(repeat.lines, ["l2"]);
    assert_eq!(phrase(&line(&walk, "l2").value).kind, LineKind::RandomDirection);

    // Bounded confidence: a filter comparing their value with mine, me included.
    let opinions = read(sheet_text("bounded_confidence.toml")).into_document();
    let like_minded = line(&opinions, "l1");
    assert_eq!(like_minded.include_me, Some(true));
    let only = phrase(&like_minded.only);
    assert_eq!(only.test, Some(CompareTest::Within));
    assert_eq!(
        only.right.value(),
        Some(&SheetValue::Chip(SheetChip::My("opinion".to_owned())))
    );
    assert_eq!(opinions.measures[1].rule, MeasureRule::Spread);
}

/// Returns the depth of the deepest table or list in `text`, the file itself counted as depth 1.
fn nesting_depth(text: &str) -> usize {
    /// Returns the depth of the deepest table or list in `value`, which sits at `depth`.
    fn deepest(value: &toml::Value, depth: usize) -> usize {
        match value {
            toml::Value::Array(items) => items
                .iter()
                .map(|item| deepest(item, depth + 1))
                .fold(depth, usize::max),
            toml::Value::Table(table) => table
                .values()
                .map(|item| deepest(item, depth + 1))
                .fold(depth, usize::max),
            _ => depth - 1,
        }
    }
    let table: toml::Table = toml::from_str(text).expect("the sheet is TOML");
    deepest(&toml::Value::Table(table), 1)
}

/// Returns a result chip of line `line`.
fn result(line: &str) -> SheetChip {
    SheetChip::Result {
        line: line.to_owned(),
        part: None,
        property: None,
    }
}

/// Returns a parameter chip.
fn param(id: &str) -> SheetChip {
    SheetChip::Param(id.to_owned())
}

/// Returns a chip of a neighbour's property.
fn their(id: &str) -> SheetChip {
    SheetChip::Their(id.to_owned())
}

/// Returns a number literal.
fn number(value: f64) -> SheetValue {
    SheetValue::Literal(SheetLiteral::Number(value))
}

#[test]
fn a_sheet_with_an_unknown_key_is_refused_on_the_key() {
    let life = sheet_text("game_of_life.toml");
    let boids = sheet_text("boids.toml");
    for (text, old, new) in [
        (life, "kind = \"sheet\"\n", "kind = \"sheet\"\nmystery = 1\n"),
        (life, "next_line = 5\n", "next_line = 5\nmystery = 1\n"),
        (life, "edges = \"wrap\"\n", "edges = \"wrap\"\nmystery = 1\n"),
        (life, "type = \"states\"\n", "type = \"states\"\nmystery = 1\n"),
        (life, "otherwise = \"dead\"\n", "otherwise = \"dead\"\nmystery = 1\n"),
        (life, "rule = \"run_once\"\n", "rule = \"run_once\"\nmystery = 1\n"),
        (life, "runs = \"each_tick\"\n", "runs = \"each_tick\"\nmystery = 1\n"),
        (life, "around = \"cells_8\"\n", "around = \"cells_8\"\nmystery = 1\n"),
        (life, "test = \"not_one_of\"\n", "test = \"not_one_of\"\nmystery = 1\n"),
        (
            life,
            "left = { their = \"status\" }",
            "left = { their = \"status\", mystery = 1 }",
        ),
        (
            life,
            "{ id = \"dead\", name = \"Dead\", color = \"#151515\" }",
            "{ id = \"dead\", name = \"Dead\", color = \"#151515\", mystery = 1 }",
        ),
        (
            boids,
            "summary = \"total\", value",
            "summary = \"total\", mystery = 1, value",
        ),
        (
            boids,
            "kind = \"random_number\"\n",
            "kind = \"random_number\"\nmystery = 1\n",
        ),
    ] {
        assert!(text.contains(old), "{old}");
        let edited = text.replacen(old, new, 1);
        for edited in [edited.clone(), edited.replace('\n', "\r\n")] {
            let error = refusal(&edited);
            assert!(matches!(error, SheetFileError::Toml(_)), "{error}");
            let diagnostic = error.diagnostic();
            assert_eq!(diagnostic.message, "unknown field `mystery`");
            assert_at(diagnostic, &edited, "mystery");
        }
    }
}

#[test]
fn a_newer_sheet_version_is_refused_on_its_key() {
    let life = sheet_text("game_of_life.toml");
    for (version, message) in [
        (
            "version = 2",
            "the file is sheet version 2, and this build reads sheet versions up to 1",
        ),
        (
            "version = 0",
            "the file names sheet version 0, but sheet versions count from 1",
        ),
    ] {
        // The file also holds a key this build does not know, which would be refused after the header.
        let text = life.replacen("\nversion = 1", &format!("\n{version}"), 1).replacen(
            "next_line = 5\n",
            "next_line = 5\nnew_key = true\n",
            1,
        );
        let error = refusal(&text);
        assert!(matches!(error, SheetFileError::Format(_)), "{error}");
        assert_eq!(error.diagnostic().message, message);
        assert_at(error.diagnostic(), &text, &version["version = ".len()..]);
        assert_eq!(error.diagnostic().line, Some(6));
    }
}

#[test]
fn the_header_check_names_three_kinds_with_or() {
    let text = "format = \"henad-model\"\nformat_version = 1\nkind = \"gpu_agents\"\n";
    let header = ModelHeader::read(text).expect("the header reads");
    assert_eq!(header.kind(), Some("gpu_agents"));
    for (kinds, list) in [
        (&["gpu_grid", "graph", "sheet"][..], "'gpu_grid', 'graph' or 'sheet'"),
        (&["gpu_grid", "graph"][..], "'gpu_grid' or 'graph'"),
        (&["sheet"][..], "'sheet'"),
    ] {
        let problem = header.check(kinds).expect_err("the kind passed");
        assert_eq!(
            problem.message,
            format!("the file contains a model of kind 'gpu_agents', but this reader only reads {list}")
        );
        assert_eq!(&text[problem.span], "\"gpu_agents\"");
    }
    assert!(header.check(&["gpu_agents"]).is_ok());

    // The sheet reader refuses a graph by its kind, and the format and version before it.
    let life = sheet_text("game_of_life.toml");
    for (old, new, message) in [
        (
            "kind = \"sheet\"",
            "kind = \"graph\"",
            "of kind 'graph', but this reader only reads 'sheet'",
        ),
        ("format_version = 1", "format_version = 2", "model file version 2"),
        (
            "format = \"henad-model\"",
            "format = \"other\"",
            "the file's format is 'other'",
        ),
    ] {
        let text = life.replacen(old, new, 1);
        let error = refusal(&text);
        assert!(matches!(error, SheetFileError::Format(_)), "{error}");
        assert!(error.diagnostic().message.contains(message), "{error}");
    }

    // The kind reads from any TOML, a file its reader refuses included, and a header key of the wrong type with it.
    assert_eq!(read_header(life).as_deref(), Some("sheet"));
    for (old, new) in [
        ("width = 1024", "width = -1"),
        ("format_version = 1", "format_version = \"1\""),
        ("format_version = 1", "format_version = 1.0"),
        ("[sheet]\nversion = 1", "[sheet]\nversion = \"one\""),
    ] {
        assert!(life.contains(old), "{old}");
        assert_eq!(
            read_header(&life.replacen(old, new, 1)).as_deref(),
            Some("sheet"),
            "{new}"
        );
    }
    let graph = crate::starters::starters()[0].text;
    assert_eq!(read_header(graph).as_deref(), Some("graph"));
    assert_eq!(read_header("format = \"henad-model\"\n"), None);
    assert_eq!(read_header("[unclosed"), None);
}

#[test]
fn an_unknown_time_unit_is_refused_on_its_key() {
    let sir = sheet_text("sir.toml");
    for (old, new) in [
        ("tick_unit = \"day\"", "tick_unit = \"fortnight\""),
        ("rate_unit = \"day\"", "rate_unit = \"fortnight\""),
    ] {
        let text = sir.replacen(old, new, 1);
        let error = refusal(&text);
        assert!(matches!(error, SheetFileError::Format(_)), "{error}");
        assert_eq!(
            error.diagnostic().message,
            "the time unit 'fortnight' is not one of second, minute, hour, day, week, month or year"
        );
        assert_at(error.diagnostic(), &text, "\"fortnight\"");
    }
    for unit in TimeUnit::ALL {
        let text = sir.replacen("tick_unit = \"day\"", &format!("tick_unit = \"{}\"", unit.as_str()), 1);
        assert_eq!(read(&text).document().time.map(|time| time.tick_unit), Some(unit));
    }
}

/// Returns `text` without the key `key` of the table whose header is `header`, or without the sub-table
/// `[header.key]` when the table writes `key` as one.
fn without_key(text: &str, header: &str, key: &str) -> String {
    let sub_table = format!("[{}.{key}]\n", &header[1..header.len() - 1]);
    if let Some(start) = text.find(&sub_table) {
        let end = text[start..].find("\n\n").map_or(text.len(), |end| start + end + 1);
        return format!("{}{}", &text[..start], &text[end..]);
    }
    let table = text
        .find(&format!("{header}\n"))
        .unwrap_or_else(|| panic!("no {header}"));
    let line = format!("\n{key} = ");
    let at = table
        + text[table..]
            .find(&line)
            .unwrap_or_else(|| panic!("no {key} in {header}"))
        + 1;
    let end = at + text[at..].find('\n').unwrap_or(text.len() - at) + 1;
    format!("{}{}", &text[..at], &text[end..])
}

/// Returns whether the field or slot `key` of `line` is empty.
fn is_empty(line: &SheetLine, key: &str) -> bool {
    match key {
        "name" => line.name.is_none(),
        "key" => line.key.is_none(),
        "around" => line.around.is_none(),
        "property" => line.property.is_none(),
        "new_state" => line.new_state.is_none(),
        "start_state" => line.start_state.is_none(),
        "test" => line.test.is_none(),
        "rate_unit" => line.rate_unit.is_none(),
        "whose" => line.whose.is_blank(),
        "condition" => line.condition.is_blank(),
        "left" => line.left.is_blank(),
        "right" => line.right.is_blank(),
        "chance" => line.chance.is_blank(),
        "rate" => line.rate.is_blank(),
        "tries" => line.tries.is_blank(),
        _ => panic!("no check for {key}"),
    }
}

#[test]
fn a_blank_slot_reads_as_blank() {
    // Each case names the sheet, a line, the slot holding the phrase whose key is deleted, if any, and the key.
    let cases: [(&str, &str, Option<&str>, &str); 33] = [
        ("game_of_life.toml", "l1", None, "name"),
        ("game_of_life.toml", "l1", None, "around"),
        ("game_of_life.toml", "l1", None, "whose"),
        ("game_of_life.toml", "l1", Some("whose"), "test"),
        ("game_of_life.toml", "l1", Some("whose"), "left"),
        ("game_of_life.toml", "l1", Some("whose"), "right"),
        ("game_of_life.toml", "l2", None, "property"),
        ("game_of_life.toml", "l2", None, "new_state"),
        ("game_of_life.toml", "l2", None, "start_state"),
        ("game_of_life.toml", "l2", None, "condition"),
        ("game_of_life.toml", "l2", Some("condition"), "test"),
        ("game_of_life.toml", "l2", Some("condition"), "left"),
        ("game_of_life.toml", "l2", Some("condition"), "right"),
        ("game_of_life.toml", "l3", Some("condition"), "right"),
        ("game_of_life.toml", "l4", None, "start_state"),
        ("game_of_life.toml", "l4", None, "condition"),
        ("sir.toml", "l1", None, "whose"),
        ("sir.toml", "l1", Some("whose"), "left"),
        ("sir.toml", "l2", None, "key"),
        ("sir.toml", "l2", None, "property"),
        ("sir.toml", "l2", None, "new_state"),
        ("sir.toml", "l2", None, "start_state"),
        ("sir.toml", "l2", None, "condition"),
        ("sir.toml", "l2", Some("condition"), "rate"),
        ("sir.toml", "l2", Some("condition"), "rate_unit"),
        ("sir.toml", "l2", Some("condition"), "tries"),
        ("sir.toml", "l3", Some("condition"), "rate"),
        ("sir.toml", "l3", Some("condition"), "rate_unit"),
        ("sir.toml", "l3", None, "condition"),
        ("sir.toml", "l4", None, "key"),
        ("sir.toml", "l4", None, "start_state"),
        ("sir.toml", "l4", Some("condition"), "chance"),
        ("sir.toml", "l4", None, "condition"),
    ];
    for (file_name, line_id, slot, key) in cases {
        let text = sheet_text(file_name);
        let header = match slot {
            Some(slot) => format!("[card.line.{line_id}.{slot}]"),
            None => format!("[card.line.{line_id}]"),
        };
        let original = read(text).into_document();
        let edited = without_key(text, &header, key);
        assert_ne!(edited, text, "{file_name} {header} {key}");
        let document = read(&edited).into_document();
        let (before, after) = match slot {
            Some("whose") => (
                phrase(&line(&original, line_id).whose),
                phrase(&line(&document, line_id).whose),
            ),
            Some(_) => (
                phrase(&line(&original, line_id).condition),
                phrase(&line(&document, line_id).condition),
            ),
            None => (line(&original, line_id), line(&document, line_id)),
        };
        assert!(!is_empty(before, key), "{file_name} {header} {key} was empty");
        assert!(is_empty(after, key), "{file_name} {header} {key} reads as filled");
    }
}

#[test]
fn the_stored_forms_of_the_editor_read() {
    let text = format!(
        "{}\n[card.line.l3.condition]\nkind = \"compare\"\ntest = \"outside\"\nleft = {{ result = \"l1\" }}\n\
         low = 2\n\n[card.line.l4]\nkind = \"become\"\nproperty = \"status\"\nnew_state = \"alive\"\n\
         start_state = \"any_state\"\n\n[card.line.l4.condition]\nkind = \"rate\"\nrate = 0.5\nrate_unit = \"week\"\n\
         rate_unit_from_tick = true\n\n[card.line.l5]\nkind = \"compare\"\ntest = \"one_of\"\n\
         left = {{ my = \"status\" }}\nright = [{{ state = \"dead\" }}, {{ state = \"alive\" }}]\n",
        BLANK_CELLS.replace("lines = [\"l1\"]", "lines = [\"l1\", \"l2\", \"l3\", \"l4\", \"l5\"]"),
    );
    let document = read(&text).into_document();
    let outside = phrase(&line(&document, "l3").condition);
    assert_eq!(outside.test, Some(CompareTest::Outside));
    assert_eq!(outside.low.value(), Some(&number(2.0)));
    assert!(outside.high.is_blank());
    let rate = phrase(&line(&document, "l4").condition);
    assert_eq!(rate.rate_unit, Some(TimeUnit::Week));
    assert!(rate.rate_unit_from_tick);
    assert_eq!(
        line(&document, "l5").right.value(),
        Some(&SheetValue::List(vec![
            SheetValue::Chip(SheetChip::State("dead".to_owned())),
            SheetValue::Chip(SheetChip::State("alive".to_owned())),
        ]))
    );
    // A line with only its kind is a half-built line, every slot blank.
    let half = line(&document, "l2");
    assert_eq!(half.kind, LineKind::Become);
    assert!(half.property.is_none() && half.condition.is_blank());

    // A second kind of agent reads, its properties after its own table.
    let boids = sheet_text("boids.toml");
    let two = boids.replacen(
        "[[agents.property]]",
        "[[agents]]\nid = \"hawk\"\nname = \"Hawk\"\ncount = 10\nmax_count = 100\n\n[[agents.property]]",
        1,
    );
    let document = read(&two).into_document();
    assert_eq!(document.agents.len(), 2);
    assert_eq!(document.agents[0].properties.len(), 0);
    assert_eq!(document.agents[1].properties.len(), 3);
}

/// Returns the literal that `slot` holds.
fn literal(slot: &Slot) -> SheetLiteral {
    match slot.value() {
        Some(SheetValue::Literal(literal)) => *literal,
        other => panic!("{other:?} is no literal"),
    }
}

#[test]
fn a_percent_reads_as_typed_wherever_a_number_can_stand() {
    // A percent in a start's row, in a phrase as an inline table and as a standard table, in a compare slot and in a
    // list.
    let sir = sheet_text("sir.toml")
        .replacen(
            "chance = { param = \"initial_infected\" }",
            "chance = { percent = 1 }",
            1,
        )
        .replacen("rate = { param = \"recovery_rate\" }", "rate = { percent = 12.5 }", 1)
        .replacen(
            "chance = { param = \"initial_infected\" }",
            "\n[card.line.l4.condition.chance]\npercent = 30",
            1,
        );
    let sheet = read(&sir);
    let document = sheet.document();
    let start = document.cells.as_ref().expect("the cells")[0]
        .starts_as
        .as_ref()
        .expect("a start");
    assert_eq!(literal(&start.rows[0].chance), SheetLiteral::Percent(1.0));
    let recovery = literal(&phrase(&line(document, "l3").condition).rate);
    assert_eq!(recovery, SheetLiteral::Percent(12.5));
    assert_eq!(recovery.number(), Some(0.125));
    let outbreak = literal(&phrase(&line(document, "l4").condition).chance);
    assert_eq!(outbreak, SheetLiteral::Percent(30.0));

    // The reader keeps the percent as typed, and it stands for the same number as its decimal.
    assert_ne!(outbreak, SheetLiteral::Number(0.3));
    assert_eq!(outbreak.number(), SheetLiteral::Number(0.3).number());
    assert_eq!(SheetLiteral::Bool(true).number(), None);

    // Both forms have places, the percent's number among them.
    let places_of = |line_id: &str| {
        let card_index = document.line(line_id).expect("the line").0;
        sheet
            .places()
            .line(card_index, line_id)
            .and_then(|places| places.table("condition"))
            .expect("the condition has places")
    };
    let rate = places_of("l3").table("rate").expect("the rate has places");
    assert_eq!(&sir[rate.table.clone()], "{ percent = 12.5 }");
    assert_eq!(rate.key("percent").map(|span| &sir[span]), Some("12.5"));
    let chance = places_of("l4").table("chance").expect("the chance has places");
    assert_header(chance, &sir, "[card.line.l4.condition.chance]");
    assert_eq!(chance.key("percent").map(|span| &sir[span]), Some("30"));

    let life = sheet_text("game_of_life.toml")
        .replacen("right = 3", "right = { percent = 300 }", 1)
        .replacen("right = [2, 3]", "right = [2, { percent = 300 }]", 1);
    let document = read(&life).into_document();
    let born = phrase(&line(&document, "l2").condition);
    assert_eq!(literal(&born.right).number(), Some(3.0));
    let survives = phrase(&line(&document, "l3").condition);
    assert_eq!(
        survives.right.value(),
        Some(&SheetValue::List(vec![
            number(2.0),
            SheetValue::Literal(SheetLiteral::Percent(300.0))
        ]))
    );
}

/// A sheet of cells with one property and one card whose line 1 is complete and line 2 holds only its kind.
const BLANK_CELLS: &str = "format = \"henad-model\"\nformat_version = 1\nkind = \"sheet\"\n\n[sheet]\nversion = 1\n\
     id = \"blank\"\nname = \"Untitled model\"\nnext_line = 6\n\n[world]\nmade_of = \"cells\"\nwidth = 100\n\
     height = 100\nedges = \"wrap\"\n\n[[cells.property]]\nid = \"status\"\nname = \"Status\"\ntype = \"states\"\n\
     states = [\n  { id = \"dead\", name = \"Dead\", color = \"#151515\" },\n  \
     { id = \"alive\", name = \"Alive\", color = \"#00e676\" },\n]\n\n[[card]]\nid = \"cell_tick\"\n\
     every = \"cells\"\nruns = \"each_tick\"\nlines = [\"l1\"]\n\n[card.line.l1]\nkind = \"count\"\n\
     name = \"Alive neighbours\"\naround = \"cells_8\"\n\n[card.line.l2]\nkind = \"become\"\n\n\
     [card.line.l3]\nkind = \"become\"\nproperty = \"status\"\nnew_state = \"alive\"\n";

/// Checks that `text` with `old` replaced by `new` is refused with a message holding `message`, placed on `at`.
fn assert_refused(text: &str, old: &str, new: &str, at: &str, message: &str) {
    assert!(text.contains(old), "{old}");
    let text = text.replacen(old, new, 1);
    let error = refusal(&text);
    let diagnostic = error.diagnostic();
    assert!(diagnostic.message.contains(message), "{diagnostic}");
    assert_at(diagnostic, &text, at);
}

#[test]
fn malformed_values_are_refused_on_their_bytes() {
    let life = sheet_text("game_of_life.toml");
    for (old, new, at, message) in [
        (
            "left = { their = \"status\" }",
            "left = { their = \"status\", my = \"status\" }",
            "\"status\"",
            "a chip names one thing, and this one holds both `my` and `their`",
        ),
        (
            "left = { their = \"status\" }",
            "left = { their = \"status\", part = \"count\" }",
            "\"count\"",
            "`part` belongs to a chip that names a `result`",
        ),
        (
            "left = { their = \"status\" }",
            "left = { test = \"is\" }",
            "\"is\"",
            "`test` is a key of a line or phrase, and a phrase names its `kind`",
        ),
        (
            "left = { their = \"status\" }",
            "left = {}",
            "{}",
            "a chip names one thing by",
        ),
        (
            "left = { their = \"status\" }",
            "left = \"status\"",
            "\"status\"",
            "a number, a percent, true or false, a list, a chip or a phrase",
        ),
        (
            "right = [2, 3]",
            "right = [2, { kind = \"always\" }]",
            "{ kind = \"always\" }",
            "a list holds numbers, percents, true or false, and chips",
        ),
        (
            "right = [2, 3]",
            "right = [2, nan]",
            "nan",
            "the number NaN is not finite",
        ),
        ("default = 0.3", "default = inf", "inf", "the number inf is not finite"),
        (
            "color = \"#00E676\" }",
            "color = \"green\" }",
            "\"green\"",
            "the colour 'green' is not written #RRGGBB or #RRGGBBAA",
        ),
        (
            "around = \"cells_8\"",
            "around = \"cells_6\"",
            "\"cells_6\"",
            "the neighbourhood 'cells_6' is not one of cells_8, cells_4, block_3x3 or agents_within",
        ),
        (
            "kind = \"count\"",
            "kind = \"counting\"",
            "\"counting\"",
            "the line kind 'counting' is not one of count,",
        ),
        (
            "[card.line.l3]\nkind = \"become\"",
            "[card.line.l3]\nparam = \"x\"\nkind = \"become\"",
            "\"x\"",
            "`param` is a key of a chip, and a line or phrase cannot hold it",
        ),
        (
            "left = { their = \"status\" }",
            "left = 1979-05-27",
            "1979-05-27",
            "a sheet holds no dates or times",
        ),
        (
            "[card.line.l3]\nkind = \"become\"\n",
            "[card.line.l3]\n",
            "[card.line.l3]",
            "line l3 names no `kind`",
        ),
    ] {
        assert_refused(life, old, new, at, message);
    }
}

#[test]
fn a_malformed_percent_is_refused_on_its_key() {
    let life = sheet_text("game_of_life.toml");
    for (old, new, at, message) in [
        (
            "right = 3",
            "right = { percent = \"30\" }",
            "\"30\"",
            "expected a number, as in `{ percent = 30 }`",
        ),
        (
            "right = [2, 3]",
            "right = [2, { percent = true }]",
            "true",
            "expected a number, as in `{ percent = 30 }`",
        ),
        (
            "right = 3",
            "right = { percent = nan }",
            "nan",
            "the percent NaN is not finite",
        ),
        (
            "right = 3",
            "right = { percent = 30, my = \"status\" }",
            "\"status\"",
            "a percent holds `percent` alone, and this one also holds `my`",
        ),
        (
            "right = 3",
            "right = { kind = \"always\", percent = 30 }",
            "\"always\"",
            "a percent holds `percent` alone, and this one also holds `kind`",
        ),
        (
            "[card.line.l3]\nkind = \"become\"",
            "[card.line.l3]\npercent = 30\nkind = \"become\"",
            "30",
            "`percent` is the key of a percent, and a line or phrase cannot hold it",
        ),
    ] {
        assert_refused(life, old, new, at, message);
    }
}

#[test]
fn colours_and_sizes_follow_their_rules() {
    let life = sheet_text("game_of_life.toml");
    // A colour reads in either case, with alpha when it has eight digits.
    let text = life.replacen("#00E676", "#00e67680", 1);
    let document = read(&text).into_document();
    assert_eq!(
        document.cells.expect("the cells")[0].states[1].color,
        [0x00, 0xE6, 0x76, 0x80]
    );

    // A card past the limit is refused at its table, and a file past the size limit before parsing.
    let mut text = BLANK_CELLS.to_owned();
    for index in 6..=MAX_LINES_PER_CARD + 3 {
        text.push_str(&format!("\n[card.line.l{index}]\nkind = \"become\"\n"));
    }
    let error = refusal(&text);
    assert!(
        error
            .diagnostic()
            .message
            .contains(&format!("at most {MAX_LINES_PER_CARD}")),
        "{error}"
    );
    assert_at(error.diagnostic(), &text, "[[card]]");
    let error = refusal(&"[".repeat(MAX_SHEET_BYTES + 1));
    assert!(error.diagnostic().message.contains("at most"), "{error}");
    assert!(error.diagnostic().span.is_none());
}

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

/// Returns a number below `bound` from `rng`.
pub(super) fn below(rng: &mut u64, bound: usize) -> usize {
    *rng = xorshift64(*rng);
    (*rng >> 33) as usize % bound.max(1)
}

/// Returns `text` with one random edit: a line removed, copied or swapped, a value replaced, a line inserted, or
/// the text cut short.
pub(super) fn mutate(text: &str, rng: &mut u64) -> String {
    let mut lines: Vec<String> = text.lines().map(str::to_owned).collect();
    if lines.is_empty() {
        return String::new();
    }
    let at = below(rng, lines.len());
    match below(rng, 7) {
        0 => {
            lines.remove(at);
        }
        1 => {
            let copy = lines[at].clone();
            lines.insert(below(rng, lines.len()), copy);
        }
        2 => {
            let other = below(rng, lines.len());
            lines.swap(at, other);
        }
        3 | 4 => {
            if let Some((key, _)) = lines[at].split_once(" = ") {
                let value = HOSTILE_VALUES[below(rng, HOSTILE_VALUES.len())];
                lines[at] = format!("{key} = {value}");
            }
        }
        5 => lines.insert(at, HOSTILE_LINES[below(rng, HOSTILE_LINES.len())].to_owned()),
        _ => {
            let joined = lines.join("\n");
            let mut cut = below(rng, joined.len());
            while !joined.is_char_boundary(cut) {
                cut -= 1;
            }
            return joined[..cut].to_owned();
        }
    }
    lines.join("\n")
}

/// Reads `text` and walks what it read. A panic fails the test with the text.
fn exercise(text: &str) -> bool {
    let outcome = catch_unwind(AssertUnwindSafe(|| match ParsedSheet::parse(text) {
        Ok(sheet) => {
            for (card_index, card) in sheet.document().cards.iter().enumerate() {
                for line_id in card.line_tables.keys() {
                    let places = sheet.places().line(card_index, line_id);
                    assert!(places.is_some(), "line {line_id} has no place");
                    let placed = sheet.place(CodeDiagnostic::error("here"), places.map_or(0..0, |p| p.table.clone()));
                    assert!(placed.line.is_some());
                }
            }
            assert!(!format!("{sheet:?}").is_empty());
            true
        }
        Err(error) => {
            assert!(!error.diagnostic().message.is_empty());
            false
        }
    }));
    outcome.unwrap_or_else(|_| panic!("the sheet reader panicked on:\n{text}"))
}

#[test]
fn the_sheet_reader_never_panics_on_the_mutation_corpus() {
    let mut rng = 0x7368_6565_7400_0001u64;
    let (mut cases, mut read) = (0usize, 0usize);
    for (_, text) in sheets() {
        for edit in 0..400 {
            let mut edited = mutate(text, &mut rng);
            if edit % 3 == 0 {
                edited = mutate(&edited, &mut rng);
            }
            cases += 1;
            read += usize::from(exercise(&edited));
        }
        // Every key of every table deleted in turn.
        for (index, line) in text.lines().enumerate() {
            if line.contains(" = ") {
                let edited: Vec<&str> = text
                    .lines()
                    .enumerate()
                    .filter(|(other, _)| *other != index)
                    .map(|(_, line)| line)
                    .collect();
                cases += 1;
                read += usize::from(exercise(&edited.join("\n")));
            }
        }
    }
    assert!(
        read * 4 > cases,
        "only {read} of {cases} edits read, so the corpus tests the TOML parser alone"
    );
}

#[test]
fn nesting_past_the_limit_is_refused_without_a_panic() {
    // Phrases nested to the limit, past it, and past what a header can name, in standard and inline tables. Line 3's
    // table sits at depth 5, so the deepest phrase sits at depth 5 plus `depth`.
    for depth in [8usize, MAX_NESTING - 5, MAX_NESTING - 4, 40, 200, 4000] {
        let mut header = String::from("[card.line.l2.condition");
        let mut inline = String::new();
        for _ in 0..depth {
            header.push_str(".left");
            inline.push_str("{ kind = \"compare\", left = ");
        }
        inline.push('1');
        inline.push_str(&" }".repeat(depth));
        let standard = format!("{BLANK_CELLS}\n{header}]\nkind = \"always\"\n");
        let nested = format!("{BLANK_CELLS}condition = {inline}\n");
        exercise(&standard);
        assert_eq!(exercise(&nested), depth + 5 <= MAX_NESTING, "{depth}");
        if depth == 40 {
            let error = refusal(&nested);
            assert!(
                error
                    .diagnostic()
                    .message
                    .contains(&format!("at most {MAX_NESTING} deep")),
                "{error}"
            );
        }
    }
}
