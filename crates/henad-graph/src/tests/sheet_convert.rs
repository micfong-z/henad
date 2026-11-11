//! Conversion of a sheet to a node graph copy: the copy's header, origin record, frames, labels and notes, a run tick
//! for tick like the sheet on the host, the refusals, the changes since conversion, and the frames that follow Make
//! function and Copy. The device run of the copy sits in henad-models.

use henad_core::action::action_seed;
use henad_core::authoring::primitives::space::Boundary;
use henad_core::explore::fingerprint::fnv1a64;
use henad_core::params::ParamValue;

use crate::GraphFile;
use crate::document::PassStage;
use crate::edit::{EditCommand, GraphBuilder, Scope};
use crate::editor::{FrameLayout, ModelOrigin, NoteLayout};
use crate::host::{HostEvaluator, HostLanes, HostWorld, RunContext};
use crate::ids::NodeId;
use crate::node::Node;
use crate::sheet::{
    ChangesError, ConvertError, ConvertOptions, ConvertRefusal, KIND_SHEET, ParsedSheet, SheetFile, SheetFileError,
    changes_since_conversion, convert, sentence, starters,
};
use crate::value::{InputValue, Literal, PortRef};

/// Date every conversion of these tests records.
const DATE: &str = "2026-11-07";

/// Text of the Game of Life starter.
fn life() -> &'static str {
    starters()[0].text
}

/// Returns `text` with `old` replaced by `new` once, where `text` must hold `old`.
fn edited(text: &str, old: &str, new: &str) -> String {
    assert!(text.contains(old), "the text holds no {old:?}");
    text.replacen(old, new, 1)
}

/// Lowers `text`, which must lower.
fn lowered(text: &str) -> SheetFile {
    SheetFile::parse(text).unwrap_or_else(|error| panic!("the sheet does not lower: {error}"))
}

/// Returns the first copy of `sheet`, with no id or name taken.
fn copy_of(sheet: &SheetFile) -> GraphFile {
    let options = ConvertOptions::new(sheet.document(), DATE, |_, _| false);
    convert(sheet, &options).unwrap_or_else(|error| panic!("the sheet does not convert: {error}"))
}

/// Returns node `number`'s id.
fn id(number: u32) -> NodeId {
    NodeId::new(number).expect("node ids count from 1")
}

/// Returns the frame among `frames` titled `title`.
fn frame<'a>(frames: &'a [FrameLayout], title: &str) -> &'a FrameLayout {
    frames
        .iter()
        .find(|frame| frame.title == title)
        .unwrap_or_else(|| panic!("no frame {title:?} among {frames:#?}"))
}

/// Title of the frame of line 2 of Game of Life.
const LINE_2: &str = "Line 2: Become Alive from Dead, when Alive neighbours is 3";

/// Title of the frame of line 3 of Game of Life.
const LINE_3: &str = "Line 3: Become Dead from Alive, when Alive neighbours is none of the numbers 2, 3";

#[test]
fn the_copy_takes_a_new_id_name_and_description() {
    let sheet = lowered(life());
    let copy = copy_of(&sheet);
    let document = copy.document();
    assert_eq!(document.id, "game_of_life_expert");
    assert_eq!(document.name, "Game of Life (expert)");
    assert_eq!(
        document.description,
        "Converted from the author model Game of Life. Edit it as a node graph."
    );
    assert_eq!(document.target.id, document.id);
    assert_eq!(document.target.name, document.name);
    assert_eq!(document.target.description, document.description);
    // Every pass, declaration and node is the lowered graph's.
    let lowered = sheet.lowered().document();
    assert_eq!(document.passes, lowered.passes);
    assert_eq!(document.lanes, lowered.lanes);
    assert_eq!(document.params.len(), lowered.params.len());
    assert_eq!(document.stats, lowered.stats);
    assert_eq!(document.actions, lowered.actions);
}

#[test]
fn a_taken_id_or_name_numbers_the_copy() {
    let sheet = lowered(life());
    let free = ConvertOptions::new(sheet.document(), DATE, |_, _| false);
    assert_eq!(
        (free.id.as_str(), free.name.as_str()),
        ("game_of_life_expert", "Game of Life (expert)")
    );
    assert_eq!(free.file, "game_of_life.toml");
    assert_eq!(free.date, DATE);
    let second = ConvertOptions::new(sheet.document(), DATE, |id, _| id == "game_of_life_expert");
    assert_eq!(
        (second.id.as_str(), second.name.as_str()),
        ("game_of_life_expert_2", "Game of Life (expert 2)")
    );
    let third = ConvertOptions::new(sheet.document(), DATE, |id, name| {
        id == "game_of_life_expert_2" || name == "Game of Life (expert)"
    });
    assert_eq!(
        (third.id.as_str(), third.name.as_str()),
        ("game_of_life_expert_3", "Game of Life (expert 3)")
    );
}

#[test]
fn the_copy_has_its_own_code_and_names_the_sheets() {
    let sheet = lowered(life());
    let copy = copy_of(&sheet);
    assert_ne!(
        copy.code(),
        sheet.code(),
        "the id, name and description enter the copy's code"
    );
    let origin = copy
        .editor()
        .and_then(|editor| editor.model_origin())
        .expect("the copy records its origin");
    assert_eq!(origin.code, format!("{:016x}", sheet.code().hash()));
    assert_eq!(origin.copy_code, format!("{:016x}", copy.code().hash()));
    assert_eq!(origin.kind, KIND_SHEET);
    assert_eq!(origin.model, "game_of_life");
    assert_eq!(origin.name, "Game of Life");
    assert_eq!(origin.file, "game_of_life.toml");
    assert_eq!(origin.hash, format!("{:016x}", fnv1a64(sheet.to_toml().as_bytes())));
    assert_eq!(origin.converted, DATE);
}

/// Canvas, title, colour, member numbers and origins of one frame.
type FrameSummary<'a> = (&'a str, &'a str, &'a str, Vec<u32>, Vec<&'a str>);

#[test]
fn the_copy_frames_each_origin_group() {
    let copy = copy_of(&lowered(life()));
    let editor = copy.editor().expect("the copy has an editor region");
    let frames = editor.frames();
    let summary: Vec<FrameSummary<'_>> = frames
        .iter()
        .map(|frame| {
            (
                frame.scope.as_str(),
                frame.title.as_str(),
                frame.color.as_str(),
                frame.members.iter().map(|member| member.number()).collect(),
                frame.lines.iter().map(String::as_str).collect(),
            )
        })
        .collect();
    assert_eq!(
        summary,
        [
            (
                "pass.start_cells",
                "Sheet, Status starts as",
                "gray",
                vec![1, 2, 3],
                vec!["cells.status.starts_as"]
            ),
            (
                "pass.tick_cell_tick",
                "Line 1: Alive neighbours = count of the 8 cells around me whose Status is Alive",
                "blue",
                vec![1001],
                vec!["l1"]
            ),
            ("pass.tick_cell_tick", LINE_2, "blue", vec![2001], vec!["l2"]),
            (
                "pass.tick_cell_tick",
                LINE_3,
                "blue",
                vec![3001, 3002, 3003],
                vec!["l3"]
            ),
            (
                "pass.tick_cell_tick",
                "Lines 2 and 3: the new Status (the first change that applies wins)",
                "purple",
                vec![4, 5, 6],
                vec!["l2", "l3"]
            ),
            (
                "pass.measures",
                "Sheet, measure: Alive",
                "gray",
                vec![7, 8],
                vec!["measure.alive"]
            ),
            (
                "pass.button_clear",
                "Clear line 1: Become Dead from any state, always",
                "blue",
                vec![9, 10, 11],
                vec!["l4"]
            ),
        ]
    );
    // Every node of the copy sits in exactly one frame.
    for pass in &copy.document().passes {
        for node in &pass.body.nodes {
            let holding = frames.iter().filter(|frame| frame.members.contains(&node.id)).count();
            assert_eq!(holding, 1, "node {} of {}", node.id, pass.id);
        }
    }
}

#[test]
fn the_copy_labels_named_results_and_reads_and_places_no_node() {
    let copy = copy_of(&lowered(life()));
    let editor = copy.editor().expect("the copy has an editor region");
    let label = |number: u32| editor.node_layout(id(number))?.label;
    assert_eq!(label(1001).as_deref(), Some("Alive neighbours"));
    for read in [4, 7, 9] {
        assert_eq!(label(read).as_deref(), Some("my Status"), "node n{read}");
    }
    assert_eq!(label(2001), None);
    let text = copy.to_toml();
    assert!(!text.contains("pos = "), "the copy places no node:\n{text}");
}

#[test]
fn the_copy_keeps_the_purpose_and_each_meaning_as_notes() {
    let copy = copy_of(&lowered(life()));
    let notes = copy.editor().expect("the copy has an editor region").notes();
    assert_eq!(
        notes,
        [
            NoteLayout::about_model(
                "Purpose",
                "Conway's Game of Life. Pattern to look for: gliders and other shapes that persist and travel."
            ),
            NoteLayout::about_model("Initial density", "Share of cells alive at the start"),
        ]
    );
}

/// First segments of the paths of the parts of a sheet that a frame of a converted copy can name.
const PART_ROOTS: [&str; 7] = ["cells", "agents", "ground", "card", "measure", "button", "color"];

#[test]
fn every_sheet_starter_converts_and_checks_clean() {
    assert!(!starters().is_empty());
    for starter in starters() {
        let sheet = lowered(starter.text);
        let copy = copy_of(&sheet);
        let report = copy.check();
        assert!(
            report.diagnostics.is_empty(),
            "{}: {:#?}",
            starter.file_name,
            report.diagnostics
        );
        let frames = copy.editor().expect("an editor region").frames();
        for pass in &copy.document().passes {
            for node in &pass.body.nodes {
                let holding = frames.iter().filter(|frame| frame.members.contains(&node.id)).count();
                assert_eq!(holding, 1, "{}: node {} of {}", starter.file_name, node.id, pass.id);
            }
        }
        let document = sheet.document();
        let is_line = |id: &str| document.cards.iter().any(|card| card.line_tables.contains_key(id));
        for frame in &frames {
            for origin in &frame.lines {
                let part = origin.split('.').next().unwrap_or_default();
                assert!(
                    is_line(origin) || PART_ROOTS.contains(&part),
                    "{}: frame {:?} names {origin:?}, which is no line and no part of the sheet",
                    starter.file_name,
                    frame.title
                );
            }
            let raw = PART_ROOTS.iter().any(|root| frame.title.contains(&format!("{root}.")));
            assert!(
                !raw && !frame.title.ends_with(": ") && !frame.title.contains(sentence::SHEET_PART_WORDS),
                "{}: frame title {:?} names no part in words",
                starter.file_name,
                frame.title
            );
        }
        assert_eq!(
            changes_since_conversion(&copy, &sheet).expect("the sheet is the original"),
            [],
            "{}: an unedited copy has no change",
            starter.file_name
        );
    }
}

#[test]
fn the_origin_record_survives_a_round_trip() {
    let copy = copy_of(&lowered(life()));
    let text = copy.to_toml();
    let read = GraphFile::parse(&text).expect("the copy reads");
    assert_eq!(read.to_toml(), text, "the copy's text is a fixed point");
    assert_eq!(read.code(), copy.code());
    let editor = read.editor().expect("an editor region");
    assert_eq!(
        editor.model_origin(),
        copy.editor().and_then(|editor| editor.model_origin())
    );
    assert_eq!(editor.frames(), copy.editor().expect("an editor region").frames());
    assert!(text.contains("\nline = \"l2\"\n"), "{text}");
    assert!(text.contains("\nlines = [\"l2\", \"l3\"]\n"), "{text}");

    // A record set by hand reads back, and a record missing a key reads as none.
    let mut region = crate::editor::EditorRegion::default();
    let origin = ModelOrigin {
        kind: "sheet".to_owned(),
        model: "m".to_owned(),
        name: "M".to_owned(),
        file: "m.toml".to_owned(),
        hash: "0123456789abcdef".to_owned(),
        code: "1".to_owned(),
        copy_code: "2".to_owned(),
        converted: DATE.to_owned(),
    };
    region.set_model_origin(&origin);
    assert_eq!(region.model_origin(), Some(origin));
    region
        .table_mut()
        .get_mut("origin")
        .and_then(toml::Value::as_table_mut)
        .expect("the record")
        .remove("hash");
    assert_eq!(region.model_origin(), None);
}

#[test]
fn a_sheet_with_an_error_or_a_skipped_line_does_not_convert() {
    // An unknown parameter is an error, so the sheet lowers to nothing.
    let broken = edited(
        life(),
        "chance = { param = \"initial_density\" }",
        "chance = { param = \"missing\" }",
    );
    assert!(matches!(SheetFile::parse(&broken), Err(SheetFileError::Invalid(_))));
    let parsed = ParsedSheet::parse(&broken).expect("the sheet reads");
    let refusal = ConvertRefusal::of(&parsed.check());
    assert_eq!(refusal, Some(ConvertRefusal::Errors));
    assert_eq!(
        refusal.map(ConvertRefusal::tooltip),
        Some("Fix the problems on the card to convert.")
    );

    // Line 3 loses its numbers, so it is skipped.
    let skipped = lowered(&edited(life(), "right = [2, 3]\n", ""));
    let options = ConvertOptions::new(skipped.document(), DATE, |_, _| false);
    assert_eq!(
        convert(&skipped, &options).map(|_| ()),
        Err(ConvertError::Refused(ConvertRefusal::SkippedLines))
    );
    assert_eq!(
        ConvertRefusal::SkippedLines.tooltip(),
        "Some lines do not run yet. Finish or delete them to convert."
    );
    assert_eq!(ConvertRefusal::of(&lowered(life()).parsed().check()), None);
}

/// Parameters of Game of Life on the host: the initial density.
const LIFE_PARAMS: [ParamValue; 1] = [ParamValue::F32(0.3)];

/// Runs `file`, Game of Life or a copy of it, for 200 ticks at seed 7 on a wrapping world 64 cells wide, pressing
/// Randomise at tick 60, Clear at tick 150 and Randomise at tick 160, and returns the status lane's words at tick 0 and
/// after each tick.
fn life_run(file: &GraphFile) -> Vec<Vec<u32>> {
    let side = 64;
    let seed = Some(7);
    let evaluator = HostEvaluator::new(file).unwrap_or_else(|error| panic!("{error}"));
    let world = HostWorld::Grid {
        width: side,
        height: side,
        boundary: Boundary::Torus,
    };
    let mut lanes = evaluator.init(world, &LIFE_PARAMS, seed).expect("the start runs");
    let words = |lanes: &HostLanes| lanes.column("status").expect("a status lane").words().to_vec();
    let clear = file
        .document()
        .actions
        .iter()
        .position(|action| action.id == "clear")
        .expect("a Clear action");
    let mut action_stream = action_seed(seed);
    let mut randomise_presses = 0;
    let mut run = vec![words(&lanes)];
    for tick in 0..200u32 {
        match tick {
            60 | 160 => {
                randomise_presses += 1;
                evaluator
                    .rerun(
                        "randomise",
                        randomise_presses,
                        &mut lanes,
                        &LIFE_PARAMS,
                        seed,
                        &mut action_stream,
                    )
                    .expect("Randomise runs");
            }
            150 => {
                evaluator
                    .run_pass(
                        "button_clear",
                        &mut lanes,
                        &LIFE_PARAMS,
                        &RunContext::action(seed, clear, 1, tick),
                        &mut action_stream,
                    )
                    .expect("Clear runs");
            }
            _ => {}
        }
        let mut stream = 0;
        for pass in file
            .document()
            .passes
            .iter()
            .filter(|pass| pass.stage == PassStage::Tick)
        {
            evaluator
                .run_pass(
                    &pass.id,
                    &mut lanes,
                    &LIFE_PARAMS,
                    &RunContext::tick(seed, tick),
                    &mut stream,
                )
                .expect("the tick runs");
        }
        run.push(words(&lanes));
    }
    run
}

#[test]
fn the_converted_game_of_life_runs_tick_for_tick_like_the_sheet_on_the_host() {
    let sheet = lowered(life());
    let copy = copy_of(&sheet);
    assert!(copy.check().diagnostics.is_empty());
    let expected = life_run(sheet.lowered());
    let alive = |words: &[u32]| words.iter().filter(|&&word| word == 1).count();
    assert!(alive(&expected[150]) > 0, "cells live before Clear");
    assert_eq!(alive(&expected[151]), 0, "Clear leaves no cell alive");
    assert!(alive(&expected[200]) > 0, "Randomise brings cells back");
    let actual = life_run(&copy);
    for (tick, (copy_words, sheet_words)) in actual.iter().zip(&expected).enumerate() {
        assert!(
            copy_words == sheet_words,
            "the copy departs from the sheet at tick {tick}"
        );
    }
}

/// Returns `copy` with Game of Life's birth rule made the one of `HighLife`: line 2's comparison becomes "is 3 or is
/// 6".
fn highlife(copy: &GraphFile) -> GraphFile {
    let mut document = copy.document().clone();
    let tick = document
        .passes
        .iter_mut()
        .find(|pass| pass.id == "tick_cell_tick")
        .expect("the tick pass");
    let count = PortRef {
        node: id(1001),
        port: None,
    };
    let equals = |number: u32, value: u32| {
        let mut node = Node::new(id(number), "cmp.eq");
        node.inputs.insert("a".to_owned(), InputValue::Link(count.clone()));
        node.inputs
            .insert("b".to_owned(), InputValue::Literal(Literal::U32(value)));
        node
    };
    tick.body.nodes.push(equals(9001, 3));
    tick.body.nodes.push(equals(9002, 6));
    let birth = tick
        .body
        .nodes
        .iter_mut()
        .find(|node| node.id == id(2001))
        .expect("line 2's comparison");
    *birth = Node::new(id(2001), "logic.or");
    for (port, source) in [("a", 9001), ("b", 9002)] {
        birth.inputs.insert(
            port.to_owned(),
            InputValue::Link(PortRef {
                node: id(source),
                port: None,
            }),
        );
    }
    GraphFile::from_document(&document, copy.editor()).expect("the edited copy reads")
}

#[test]
fn a_highlife_edit_is_found_by_changes_since_conversion() {
    let sheet = lowered(life());
    let copy = copy_of(&sheet);
    let edited_copy = highlife(&copy);
    assert!(
        edited_copy.check().diagnostics.is_empty(),
        "{:#?}",
        edited_copy.check().diagnostics
    );
    assert_ne!(edited_copy.code(), copy.code());
    let changes = changes_since_conversion(&edited_copy, &sheet).expect("the sheet is the original");
    let changed: Vec<(&str, Vec<u32>)> = changes
        .iter()
        .filter(|group| !group.changed.is_empty())
        .map(|group| {
            (
                group.title.as_deref().unwrap_or_default(),
                group.changed.iter().map(|node| node.number()).collect(),
            )
        })
        .collect();
    assert_eq!(
        changed,
        [(LINE_2, vec![2001])],
        "one changed node under the frame of line 2"
    );
    let added: Vec<(Option<&str>, Vec<u32>)> = changes
        .iter()
        .filter(|group| !group.added.is_empty())
        .map(|group| {
            (
                group.title.as_deref(),
                group.added.iter().map(|node| node.number()).collect(),
            )
        })
        .collect();
    assert_eq!(added, [(None, vec![9001, 9002])], "the new nodes join no frame");
    assert!(changes.iter().all(|group| group.removed.is_empty()));

    // A removed node is listed under the frame it sat in at conversion.
    let mut builder = GraphBuilder::from_file(&copy);
    builder
        .apply(EditCommand::RemoveNode {
            scope: Scope::pass("tick_cell_tick"),
            node: id(3002),
        })
        .expect("the node is removed");
    let removed = builder.to_file().expect("the copy reads");
    let changes = changes_since_conversion(&removed, &sheet).expect("the sheet is the original");
    let listed: Vec<(Option<&str>, Vec<u32>)> = changes
        .iter()
        .filter(|group| !group.removed.is_empty())
        .map(|group| {
            (
                group.title.as_deref(),
                group.removed.iter().map(|node| node.number()).collect(),
            )
        })
        .collect();
    assert_eq!(listed, [(Some(LINE_3), vec![3002])]);
}

#[test]
fn changes_need_the_sheet_the_copy_was_converted_from() {
    let sheet = lowered(life());
    let copy = copy_of(&sheet);
    let changed = lowered(&edited(life(), "default = 0.3", "default = 0.4"));
    let error = changes_since_conversion(&copy, &changed).expect_err("the sheet changed");
    assert_eq!(
        error,
        ChangesError::OriginalChanged {
            name: "Game of Life".to_owned()
        }
    );
    assert_eq!(error.to_string(), "Game of Life has changed since this conversion.");
    assert_eq!(
        changes_since_conversion(sheet.lowered(), &sheet),
        Err(ChangesError::NoOrigin)
    );

    // A new purpose and a note under a line change the sheet's text and leave its code, so the changes still list.
    let noted = lowered(&edited(
        &edited(
            life(),
            "Conway's Game of Life.",
            "Conway's Game of Life, as he wrote it.",
        ),
        "[card.line.l1]\n",
        "[card.line.l1]\nnote = \"Counts the eight cells around.\"\n",
    ));
    assert_ne!(noted.to_toml(), sheet.to_toml());
    assert_eq!(noted.code(), sheet.code());
    assert_eq!(changes_since_conversion(&copy, &noted), Ok(Vec::new()));
}

#[test]
fn make_function_keeps_frame_members() {
    let copy = copy_of(&lowered(life()));
    let mut builder = GraphBuilder::from_file(&copy).with_key_seed(1);
    let made = builder
        .apply(EditCommand::MakeFunction {
            scope: Scope::pass("tick_cell_tick"),
            nodes: vec![id(3001), id(3002), id(3003)],
            id: "survives".to_owned(),
        })
        .expect("line 3's nodes make a function");
    let call = made[0];
    let frames = builder.editor().frames();
    let on_pass: Vec<&FrameLayout> = frames
        .iter()
        .filter(|frame| frame.title == LINE_3 && frame.scope == "pass.tick_cell_tick")
        .collect();
    assert_eq!(on_pass.len(), 1);
    assert_eq!(on_pass[0].members, [call], "the call takes the moved nodes' place");
    let in_function: Vec<&FrameLayout> = frames
        .iter()
        .filter(|frame| frame.title == LINE_3 && frame.scope == "function.survives")
        .collect();
    assert_eq!(in_function.len(), 1);
    assert_eq!(in_function[0].members, [id(3001), id(3002), id(3003)]);
    assert_eq!(in_function[0].lines, ["l3"]);
    assert_eq!(in_function[0].color, "blue");
    // Frames of other lines stay as they were.
    assert_eq!(frame(&frames, LINE_2).members, [id(2001)]);
    let file = builder.to_file().expect("the edited copy reads");
    assert!(file.check().diagnostics.is_empty(), "{:#?}", file.check().diagnostics);
}

#[test]
fn copy_carries_frame_membership() {
    let copy = copy_of(&lowered(life()));
    let mut builder = GraphBuilder::from_file(&copy).with_key_seed(1);
    let scope = Scope::pass("tick_cell_tick");
    let text = builder.copy(&scope, &[id(3001), id(3002)]).expect("the nodes copy");
    let copied = GraphFile::parse(&text).expect("the copied text reads");
    let copied_frames = copied.editor().expect("an editor region").frames();
    assert_eq!(copied_frames.len(), 1, "{copied_frames:#?}");
    assert_eq!(copied_frames[0].title, LINE_3);
    assert_eq!(copied_frames[0].members, [id(3001), id(3002)]);
    assert_eq!(copied_frames[0].lines, ["l3"]);

    let pasted = builder
        .apply(EditCommand::Paste {
            scope: scope.clone(),
            text,
            offset: [40.0, 40.0],
        })
        .expect("the nodes paste");
    let frames = builder.editor().frames();
    let line_3: Vec<&FrameLayout> = frames.iter().filter(|frame| frame.title == LINE_3).collect();
    assert_eq!(line_3.len(), 2, "the paste adds a frame of its own: {line_3:#?}");
    assert_eq!(
        line_3[0].members,
        [id(3001), id(3002), id(3003)],
        "the original frame is unchanged"
    );
    assert_eq!(line_3[1].members, pasted);
    assert_eq!(line_3[1].scope, "pass.tick_cell_tick");
    assert_eq!(line_3[1].lines, ["l3"]);
}

#[test]
fn set_variant_color_round_trips_and_changes_the_code() {
    let copy = copy_of(&lowered(life()));
    let mut builder = GraphBuilder::from_file(&copy);
    builder
        .apply(EditCommand::SetVariantColor {
            category: "status".to_owned(),
            name: "alive".to_owned(),
            color: [255, 160, 0, 255],
        })
        .expect("the colour is set");
    let file = builder.to_file().expect("the edited copy reads");
    let alive = &file.document().categories[0].variants[1];
    assert_eq!((alive.name.as_str(), alive.color), ("alive", [255, 160, 0, 255]));
    assert_ne!(file.code(), copy.code(), "a variant's colour enters the code");
    let read = GraphFile::parse(&file.to_toml()).expect("the text reads");
    assert_eq!(read.document().categories, file.document().categories);
    for (category, name) in [("status", "zombie"), ("mood", "alive")] {
        let refused = builder.apply(EditCommand::SetVariantColor {
            category: category.to_owned(),
            name: name.to_owned(),
            color: [0, 0, 0, 255],
        });
        assert!(refused.is_err(), "{category}.{name}");
    }
}

#[test]
fn the_conversion_words_follow_the_wording_table() {
    assert_eq!(
        sentence::expert_copy_number_name("Game of Life", 1),
        "Game of Life (expert)"
    );
    assert_eq!(
        sentence::expert_copy_number_name("Game of Life", 2),
        "Game of Life (expert 2)"
    );
    assert_eq!(sentence::lines_prefix(Some("Clear"), &[1, 2]), "Clear lines 1 and 2");
    assert_eq!(sentence::lines_prefix(None, &[2, 3, 4]), "Lines 2, 3 and 4");
}
