//! Buttons, measures and colours of grid sheets: Set everywhere and Start again on some properties, random starts, the
//! average, total and spread measures, and colours by a second States property and by shading.

use std::collections::BTreeSet;

use henad_core::action::action_seed;

use super::sheet_support::{
    LIFE_PARAMS, alive, edited, host_of, levels, life, life_with, lowered, only_refusal, placed_at, refusals, sized,
    started,
};
use super::support::torus;
use crate::document::{GraphDocument, Palette, PassStage};
use crate::host::{HostLanes, HostValue, RunContext};
use crate::node::Node;
use crate::sheet::SheetFile;
use crate::sheet::document::ShadeHue;
use crate::value::{ConfigValue, InputValue, Literal};
use crate::wgsl::GridProgram;

#[test]
fn a_set_everywhere_button_writes_its_value_on_every_cell() {
    let wipe = "[[button]]\nid = \"wipe\"\nname = \"Wipe\"\n\n[[button.does]]\nrule = \"set_everywhere\"\nproperty = \
                \"status\"\nvalue = { state = \"dead\" }\n\n[[button.does]]\nrule = \"set_everywhere\"\nproperty = \
                \"level\"\nvalue = 0.5\n\n";
    let file = lowered(&life_with(&sized(life(), 16), wipe));
    assert!(file.warnings().is_empty(), "{:?}", file.warnings());
    let document = file.lowered().document();
    let pass = document
        .passes
        .iter()
        .find(|pass| pass.id == "button_wipe")
        .expect("the action pass");
    assert_eq!(pass.stage, PassStage::Action);
    assert_eq!(pass.action.as_deref(), Some("wipe"));
    let action = document
        .actions
        .iter()
        .position(|action| action.id == "wipe")
        .expect("the action");
    assert!(document.actions[action].runs.is_empty());

    let evaluator = host_of(&file);
    let mut lanes = started(&evaluator, 16, 7);
    assert!(!alive(&lanes, 16).is_empty(), "the start leaves some cells alive");
    let mut stream = action_seed(Some(7));
    evaluator
        .run_pass(
            "button_wipe",
            &mut lanes,
            &LIFE_PARAMS,
            &RunContext::action(Some(7), action, 1, 0),
            &mut stream,
        )
        .expect("the press runs");
    assert!(alive(&lanes, 16).is_empty());
    assert!(levels(&lanes).iter().all(|&level| level == 0.5));
}

#[test]
fn a_property_set_twice_by_one_button_is_refused() {
    let twice = "[[button]]\nid = \"wipe\"\nname = \"Wipe\"\n\n[[button.does]]\nrule = \"set_everywhere\"\nproperty = \
                 \"status\"\nvalue = { state = \"dead\" }\n\n[[button.does]]\nrule = \"set_everywhere\"\nproperty = \
                 \"status\"\nvalue = { state = \"alive\" }\n\n";
    let text = life_with(life(), twice);
    let error = only_refusal(&text);
    assert!(
        error.message.contains("'status' is changed twice by one button"),
        "{}",
        error.message
    );
    assert_eq!(placed_at(&error, &text), "[[button]]");
    assert_eq!(
        error.span.as_ref().map(|span| span.start),
        text.find("[[button]]\nid = \"wipe\""),
        "the error sits at the button"
    );
}

#[test]
fn a_start_again_button_on_some_properties_reruns_only_their_starts() {
    let file = lowered(&life_with(&sized(life(), 64), ""));
    assert!(file.warnings().is_empty(), "{:?}", file.warnings());
    let document = file.lowered().document();
    let randomise = document
        .actions
        .iter()
        .find(|action| action.id == "randomise")
        .expect("the action");
    assert_eq!(randomise.runs, ["restart_randomise"]);
    let restart = document
        .passes
        .iter()
        .find(|pass| pass.id == "restart_randomise")
        .expect("the restart pass");
    assert_eq!(restart.stage, PassStage::Init);
    assert_eq!(restart.enabled, crate::document::PassEnabled::Literal(false));
    let paths: BTreeSet<&str> = restart
        .body
        .nodes
        .iter()
        .filter_map(|node| file.line_map().origin(node.id))
        .map(|origin| origin.path.as_str())
        .collect();
    assert_eq!(paths, BTreeSet::from(["cells.status.starts_as"]));

    let evaluator = host_of(&file);
    let press = |presses: u32| {
        let mut lanes = started(&evaluator, 64, 7);
        let mut stream = action_seed(Some(7));
        let mut states = Vec::new();
        for press in 1..=presses {
            evaluator
                .rerun("randomise", press, &mut lanes, &LIFE_PARAMS, Some(7), &mut stream)
                .expect("the press runs");
            states.push((alive(&lanes, 64), levels(&lanes)));
        }
        states
    };
    let start = started(&evaluator, 64, 7);
    let (start_alive, start_levels) = (alive(&start, 64), levels(&start));
    assert!(
        start_levels.iter().any(|&level| level != start_levels[0]),
        "the levels start at random"
    );
    let states = press(2);
    assert_ne!(states[0].0, start_alive);
    assert_ne!(states[1].0, states[0].0);
    assert!(
        states.iter().all(|(_, levels)| *levels == start_levels),
        "the levels stay"
    );
    assert_eq!(press(2), states, "the presses replay");
}

#[test]
fn a_random_number_start_draws_between_its_values() {
    let file = lowered(&life_with(&sized(life(), 64), ""));
    let evaluator = host_of(&file);
    let levels = levels(&started(&evaluator, 64, 7));
    assert!(levels.iter().all(|level| (0.0..1.0).contains(level)), "{levels:?}");
    let mean = levels.iter().map(|&level| f64::from(level)).sum::<f64>() / levels.len() as f64;
    assert!((mean - 0.5).abs() < 0.05, "mean {mean}");
}

#[test]
fn a_random_number_start_of_a_whole_number_is_refused() {
    let text = life_with(life(), "");
    let text = edited(
        &text,
        "name = \"Level\"\ntype = \"number\"",
        "name = \"Level\"\ntype = \"whole_number\"",
    );
    let error = only_refusal(&text);
    assert!(
        error
            .message
            .contains("only a Number property can start as a random number"),
        "{}",
        error.message
    );
    assert_eq!(placed_at(&error, &text), "[[cells.property]]");
    assert_eq!(
        error.span.as_ref().map(|span| span.start),
        text.find("[[cells.property]]\nid = \"level\""),
        "the error sits at the property"
    );
}

/// Returns `value` as `f64`, for a measure's value.
fn number(value: Option<HostValue>) -> f64 {
    match value {
        Some(HostValue::F64(value)) => value,
        Some(HostValue::F32(value)) => f64::from(value),
        other => panic!("{other:?} is no number"),
    }
}

#[test]
fn the_average_total_and_spread_measures_match_a_reference() {
    let measures = "[[measure]]\nid = \"mean_level\"\nname = \"Mean level\"\ncolor = \"#2060E0\"\nrule = \
                    \"average\"\nover = \"cells\"\nvalue = { my = \"level\" }\n\n[[measure]]\nid = \"total_level\"\n\
                    name = \"Total level\"\ncolor = \"#E06020\"\nrule = \"total\"\nover = \"cells\"\nvalue = { my = \
                    \"level\" }\n\n[[measure]]\nid = \"spread_level\"\nname = \"Spread of level\"\ncolor = \
                    \"#20A060\"\nrule = \"spread\"\nover = \"cells\"\nvalue = { my = \"level\" }\n\n";
    let text = edited(
        &life_with(&sized(life(), 8), ""),
        "[color.cells]",
        &format!("{measures}[color.cells]"),
    );
    let file = lowered(&text);
    assert!(file.warnings().is_empty(), "{:?}", file.warnings());
    let labels: Vec<&str> = file
        .lowered()
        .document()
        .stats
        .iter()
        .map(|stat| stat.label.as_str())
        .collect();
    assert_eq!(labels, ["Alive", "Mean level", "Total level", "Spread of level"]);

    let evaluator = host_of(&file);
    let world = torus(8, 8);
    let mut lanes = HostLanes::new(evaluator.document(), world).expect("lanes");
    let planted: Vec<f32> = (0..64u16).map(|cell| f32::from(cell % 7) * 0.25 - 0.5).collect();
    for (cell, &level) in planted.iter().enumerate() {
        assert!(
            lanes
                .column_mut("level")
                .expect("level")
                .set(cell, HostValue::F32(level))
        );
    }
    let mut stream = 0;
    let outcome = evaluator
        .run_pass(
            "measures",
            &mut lanes,
            &LIFE_PARAMS,
            &RunContext::tick(Some(1), 0),
            &mut stream,
        )
        .expect("the measures run");
    let values: Vec<f64> = evaluator.document().stats[1..]
        .iter()
        .map(|stat| number(outcome.value(&stat.from)))
        .collect();

    let total: f64 = planted.iter().map(|&level| f64::from(level)).sum();
    let mean = total / 64.0;
    let mean_square = planted.iter().map(|&level| f64::from(level * level)).sum::<f64>() / 64.0;
    let spread = (mean_square - mean * mean).max(0.0).sqrt();
    for (found, expected) in values.iter().zip([mean, total, spread]) {
        assert!(
            (found - expected).abs() < 1e-9,
            "{values:?} against {mean}, {total}, {spread}"
        );
    }
    assert!(spread > 0.1, "the planted levels spread");
}

#[test]
fn colouring_by_a_second_states_property_writes_a_view_pass() {
    let text = edited(
        &life_with(life(), ""),
        "[color.cells]\nrule = \"by_state\"\nproperty = \"status\"",
        "[color.cells]\nrule = \"by_state\"\nproperty = \"mood\"",
    );
    let file = lowered(&text);
    assert!(file.warnings().is_empty(), "{:?}", file.warnings());
    let document = file.lowered().document();
    let view = document
        .passes
        .iter()
        .find(|pass| pass.id == "colours")
        .expect("the view pass");
    assert_eq!(view.stage, PassStage::View);
    let color = view
        .body
        .nodes
        .iter()
        .find(|node| node.op == "view.color")
        .expect("a view.color");
    assert_eq!(color.config["palette"], ConfigValue::String("mood".to_owned()));
    assert!(view.body.nodes.iter().all(|node| node.op != "view.ramp"));
    assert!(GridProgram::compile(file.lowered()).is_ok());

    // The first States property takes its colours without a view pass.
    let first = lowered(&life_with(life(), ""));
    assert!(
        first
            .lowered()
            .document()
            .passes
            .iter()
            .all(|pass| pass.id != "colours")
    );
}

#[test]
fn a_shaded_number_ramps_into_its_palette() {
    let text = edited(
        &life_with(life(), ""),
        "[color.cells]\nrule = \"by_state\"\nproperty = \"status\"",
        "[color.cells]\nrule = \"shaded\"\nproperty = \"level\"",
    );
    let file = lowered(&text);
    assert!(file.warnings().is_empty(), "{:?}", file.warnings());
    let document = file.lowered().document();
    let palette = shaded_palette(document);
    assert_eq!(palette.colors.len(), 8);
    let view = document
        .passes
        .iter()
        .find(|pass| pass.id == "colours")
        .expect("the view pass");
    let ramp = view
        .body
        .nodes
        .iter()
        .find(|node| node.op == "view.ramp")
        .expect("a view.ramp");
    assert_eq!(ramp.config["palette"], ConfigValue::String("shaded_level".to_owned()));
    assert!(view.body.nodes.iter().any(|node| node.op == "view.color"));
    let report = file.lowered().check();
    assert!(!report.has_errors(), "{:#?}", report.diagnostics);
    assert!(GridProgram::compile(file.lowered()).is_ok());
}

/// Returns the palette `shaded_level` that a shaded colour rule over Level writes into `document`.
fn shaded_palette(document: &GraphDocument) -> &Palette {
    document
        .palettes
        .iter()
        .find(|palette| palette.id == "shaded_level")
        .expect("the palette")
}

/// Returns the first `view.ramp` node of `document`.
fn ramp_node(document: &GraphDocument) -> &Node {
    document
        .passes
        .iter()
        .flat_map(|pass| &pass.body.nodes)
        .find(|node| node.op == "view.ramp")
        .expect("a view.ramp")
}

#[test]
fn a_hued_shading_ramps_from_its_darkest_to_its_lightest_value() {
    let shaded = edited(
        &life_with(life(), ""),
        "[color.cells]\nrule = \"by_state\"\nproperty = \"status\"",
        "[color.cells]\nrule = \"shaded\"\nproperty = \"level\"\nhue = \"blue\"\ndarkest = 2\nlightest = 10",
    );
    let parsed = crate::sheet::ParsedSheet::parse(&shaded).expect("the sheet reads");
    let rule = &parsed.document().colors["cells"];
    assert_eq!(
        (rule.hue, rule.darkest, rule.lightest),
        (Some(ShadeHue::Blue), Some(2.0), Some(10.0))
    );
    assert_eq!(
        parsed.document().to_text().expect("the sheet writes"),
        shaded,
        "the keys write back as read"
    );
    let file = lowered(&shaded);
    let document = file.lowered().document();
    let palette = shaded_palette(document);
    let [dark, light] = ShadeHue::Blue.ends();
    assert_eq!(palette.colors.len(), 8);
    assert_eq!(palette.colors.first(), Some(&dark), "the low end is the hue's step 900");
    assert_eq!(
        palette.colors.last(),
        Some(&light),
        "the high end is the hue's step 200"
    );
    let ramp = ramp_node(document);
    let input = |port: &str| ramp.inputs.get(port).cloned();
    assert_eq!(input("low"), Some(InputValue::Literal(Literal::F32(2.0))));
    assert_eq!(input("high"), Some(InputValue::Literal(Literal::F32(10.0))));
    assert!(GridProgram::compile(file.lowered()).is_ok());

    // A rule darkest at the higher value ramps over the reversed palette, from the lower value up.
    let reversed = edited(&shaded, "darkest = 2\nlightest = 10", "darkest = 10\nlightest = 2");
    let file = lowered(&reversed);
    let document = file.lowered().document();
    let palette = shaded_palette(document);
    assert_eq!(
        palette.colors.first(),
        Some(&light),
        "the low end is the hue's step 200"
    );
    assert_eq!(palette.colors.last(), Some(&dark), "the high end is the hue's step 900");
    let ramp = ramp_node(document);
    let input = |port: &str| ramp.inputs.get(port).cloned();
    assert_eq!(input("low"), Some(InputValue::Literal(Literal::F32(2.0))));
    assert_eq!(input("high"), Some(InputValue::Literal(Literal::F32(10.0))));
    assert!(GridProgram::compile(file.lowered()).is_ok());

    // Two equal ends leave no range to shade over.
    let flat = edited(&shaded, "lightest = 10", "lightest = 2");
    let error = only_refusal(&flat);
    assert!(error.message.contains("got 2 for both"), "{}", error.message);
    assert!(placed_at(&error, &flat).starts_with("[color.cells]"));
    assert!(crate::sheet::ParsedSheet::parse(&edited(&shaded, "hue = \"blue\"", "hue = \"teal\"")).is_err());
}

#[test]
fn a_shading_on_a_log_scale_is_refused() {
    let shaded = edited(
        &life_with(life(), ""),
        "[color.cells]\nrule = \"by_state\"\nproperty = \"status\"",
        "[color.cells]\nrule = \"shaded\"\nproperty = \"level\"",
    );
    for keys in ["scale = \"log\"\ndecades = 3", "scale = \"log\"", "decades = 3"] {
        let text = edited(
            &shaded,
            "property = \"level\"\n",
            &format!("property = \"level\"\n{keys}\n"),
        );
        let errors = refusals(&text);
        assert_eq!(errors.len(), 1, "{keys}: {errors:?}");
        assert!(errors[0].message.contains("log scale"), "{}", errors[0].message);
        assert!(placed_at(&errors[0], &text).starts_with("[color.cells]"), "{keys}");
    }
    let linear = edited(
        &shaded,
        "property = \"level\"\n",
        "property = \"level\"\nscale = \"linear\"\n",
    );
    assert!(SheetFile::parse(&linear).is_ok());
}
