//! Rates on the host: the known chances per tick, the lowered rates and competing groups, the uniform values the host
//! computes for the device, the tick length as a parameter, the notes under a rate line, the commands that turn a
//! chance into a rate and back, and the warning on a rate read in another unit than its parameter's.

use std::collections::BTreeMap;

use henad_core::authoring::primitives::space::Boundary;
use henad_core::metadata::TimeUnit;
use henad_core::params::ParamValue;

use super::sheet_document::sheet_text;
use crate::document::PassStage;
use crate::host::{HostEvaluator, HostLanes, HostValue, HostWorld, RunContext};
use crate::sheet::document::{SheetLine, SheetLiteral, SheetTime, SheetValue, Slot};
use crate::sheet::edit::{SheetBuilder, SheetEdit};
use crate::sheet::rates::{
    LONG_TICK_CHANCE, chance_from_rate, chance_per_tick, chance_words, largest_count_chance, mean_stay,
    parameter_defaults, rate_from_chance, rate_note, tick_in,
};
use crate::sheet::relations::CardRelations;
use crate::sheet::{ParsedSheet, SheetFile, sentence};
use crate::value::{ConfigValue, InputValue, Literal};
use crate::wgsl::GridProgram;
use crate::wgsl::uniform::{MemberShape, MemberSource};

/// Returns `text` with `old` replaced by `new` once, where `text` must hold `old` exactly once.
fn edited(text: &str, old: &str, new: &str) -> String {
    assert_eq!(text.matches(old).count(), 1, "the text holds {old:?} other than once");
    text.replacen(old, new, 1)
}

/// Lowers `text`, which must lower.
fn lowered(text: &str) -> SheetFile {
    SheetFile::parse(text).unwrap_or_else(|error| panic!("the sheet does not lower: {error}\n{text}"))
}

/// Returns `text` with a world of `side` by `side` cells in place of 1024 by 1024.
fn sized(text: &str, side: u32) -> String {
    let text = edited(text, "width = 1024", &format!("width = {side}"));
    edited(&text, "height = 1024", &format!("height = {side}"))
}

/// Returns the wrapping grid of `side` by `side` cells.
fn grid(side: u32) -> HostWorld {
    HostWorld::Grid {
        width: side,
        height: side,
        boundary: Boundary::Torus,
    }
}

/// Returns whether `a` and `b` are at most one `f32` step apart.
fn within_one_ulp(a: f32, b: f32) -> bool {
    (a.to_bits() as i64 - b.to_bits() as i64).abs() <= 1
}

/// Returns whether the uniform word `word`, read as a `u32`, is the `rng.threshold` of the chance `chance` within 2^10
/// of the 2^32 units of a threshold.
fn holds_threshold(word: f32, chance: f64) -> bool {
    (f64::from(word.to_bits()) - chance * 4_294_967_295.0).abs() <= 1024.0
}

#[test]
fn rates_give_their_known_chances_per_tick() {
    let day = SheetTime::new(1.0, TimeUnit::Day);
    assert!((chance_from_rate(0.3, 1.0) - 0.2592).abs() < 5e-5);
    assert!((chance_per_tick(0.3, TimeUnit::Day, day, 8.0) - 0.9093).abs() < 5e-5);
    // A rate per week in a model of days is divided by 7.
    assert!((tick_in(day, TimeUnit::Week) - 1.0 / 7.0).abs() < 1e-15);
    assert!((chance_per_tick(0.7, TimeUnit::Week, day, 1.0) - chance_from_rate(0.1, 1.0)).abs() < 1e-15);
    // Gamma = mu = 0.05 per day at one tick of 7 days.
    let t4 = chance_from_rate(0.05 + 0.05, 7.0);
    assert!((t4 - 0.5034).abs() < 5e-5, "{t4}");
    assert!((t4 * 0.05 / (0.05 + 0.05) - t4 / 2.0).abs() < 1e-15);
    assert!((rate_from_chance(chance_from_rate(0.1, 1.0), 1.0) - 0.1).abs() < 1e-15);
    assert!((mean_stay(0.1, 1.0) - 10.508).abs() < 1e-3);
    // The number rule: three figures, and more while the chance prints as its rate times the tick.
    assert_eq!(chance_words(chance_from_rate(0.3, 1.0), 0.3), "0.259");
    assert_eq!(chance_words(chance_from_rate(0.1, 1.0), 0.1), "0.0952");
    assert_eq!(chance_words(chance_from_rate(0.01, 1.0), 0.01), "0.00995");
    assert_eq!(chance_words(chance_from_rate(0.001, 1.0), 0.001), "0.0009995");
}

#[test]
fn the_rate_uniforms_equal_1_minus_exp() {
    let file = lowered(sheet_text("sir.toml"));
    let program = GridProgram::compile(file.lowered()).unwrap_or_else(|problems| panic!("{problems:#?}"));
    let evaluator = HostEvaluator::new(file.lowered()).expect("the host runs the lowered sheet");
    let tick = program
        .shaders()
        .iter()
        .find(|shader| shader.label() == "tick_cell_tick")
        .expect("the tick shader");
    assert!(!tick.text().contains("exp("), "{}", tick.text());
    let params = evaluator.param_values(&[]);
    let bytes = tick
        .uniform_bytes(&evaluator, grid(1024), &params, &RunContext::tick(Some(1), 0))
        .expect("the uniform block");
    let word = |offset: usize| {
        f32::from_le_bytes(
            bytes[offset..offset + 4]
                .try_into()
                .unwrap_or_else(|_| unreachable!("four bytes")),
        )
    };
    let mut values = Vec::new();
    let mut tables = Vec::new();
    for member in &tick.uniform().members {
        match (&member.source, member.shape) {
            (MemberSource::Slot { .. }, _) => values.push(word(member.offset)),
            (MemberSource::Table { .. }, MemberShape::Table { len }) => {
                tables.push(
                    (0..len as usize)
                        .map(|k| word(member.offset + 4 * k))
                        .collect::<Vec<f32>>(),
                );
            }
            _ => {}
        }
    }
    // The recovery rate is the same for every cell, so the host writes its chance 1 - e^(-0.1) as a threshold of
    // the draw's 32 bits. The chance keeps the one step of e^(-0.1) in f32, about 2^8 units of the threshold.
    assert!(
        values
            .iter()
            .any(|&value| holds_threshold(value, 1.0 - (-0.1f64).exp())),
        "{values:?} holds no threshold of 1 - e^(-0.1)"
    );
    assert_eq!(tables.len(), 1, "{tables:?}");
    assert_eq!(tables[0].len(), 9);
    // The base e^(-0.3) is one f32 step from the true value at most. Each power is the f32 `powi` of the base, as the
    // host evaluator computes it, so a power carries a rounding per multiply and stays within 3 steps.
    assert!(within_one_ulp(tables[0][1], (-0.3f64).exp() as f32), "{:?}", tables[0]);
    for (k, &power) in tables[0].iter().enumerate() {
        let known = (-0.3 * k as f64).exp() as f32;
        let steps = (power.to_bits() as i64 - known.to_bits() as i64).abs();
        assert!(
            steps <= 3,
            "e^(-0.3 x {k}) is {known}, the table holds {power}, {steps} steps away"
        );
    }
}

/// Returns the uniform values of the tick shader of `file` at the parameter values `params`, in document order.
fn tick_uniforms(file: &SheetFile, params: &[ParamValue]) -> Vec<f32> {
    let program = GridProgram::compile(file.lowered()).unwrap_or_else(|problems| panic!("{problems:#?}"));
    let evaluator = HostEvaluator::new(file.lowered()).expect("the host runs the lowered sheet");
    let tick = program
        .shaders()
        .iter()
        .find(|shader| shader.label() == "tick_cell_tick")
        .expect("the tick shader");
    let bytes = tick
        .uniform_bytes(
            &evaluator,
            grid(64),
            &evaluator.param_values(params),
            &RunContext::tick(Some(1), 0),
        )
        .expect("the uniform block");
    tick.uniform()
        .members
        .iter()
        .filter(|member| matches!(member.source, MemberSource::Slot { .. }))
        .map(|member| {
            f32::from_le_bytes(
                bytes[member.offset..member.offset + 4]
                    .try_into()
                    .unwrap_or_else(|_| unreachable!("four bytes")),
            )
        })
        .collect()
}

#[test]
fn the_competing_thresholds_are_the_known_shares() {
    let file = lowered(sheet_text("sird.toml"));
    let document = file.lowered().document();
    assert_eq!(document.params[0].id(), "tick_length");
    let draws: Vec<&str> = document
        .passes
        .iter()
        .filter(|pass| pass.stage == PassStage::Tick)
        .flat_map(|pass| &pass.body.nodes)
        .filter(|node| node.op.starts_with("rand."))
        .map(|node| node.op.as_str())
        .collect();
    assert_eq!(
        draws,
        ["rand.chance", "rand.float"],
        "line 2 draws a chance, the group one number"
    );
    let params = [
        ParamValue::F32(7.0),
        ParamValue::F32(0.3),
        ParamValue::F32(0.05),
        ParamValue::F32(0.05),
        ParamValue::F32(0.01),
    ];
    let values = tick_uniforms(&file, &params);
    let t4 = (1.0 - (-0.7f64).exp()) as f32;
    for threshold in [t4, t4 / 2.0] {
        assert!(
            values
                .iter()
                .any(|value| (value - threshold).abs() <= 2.0 * f32::EPSILON),
            "{values:?} holds no {threshold}"
        );
    }
}

#[test]
fn a_competing_group_with_a_worked_out_rate_splits_by_its_hazards_on_the_host() {
    // Every cell starts Infected, and line 3's rate is a result, so each share is worked out per cell.
    let text = sized(sheet_text("sird.toml"), 128);
    let text = edited(&text, "default = 0.01\nstep = 0.001", "default = 1\nstep = 0.001");
    let text = edited(&text, "next_line = 6", "next_line = 7");
    let text = edited(
        &text,
        "lines = [\"l1\", \"l2\", \"l3\", \"l4\"]",
        "lines = [\"l1\", \"l6\", \"l2\", \"l3\", \"l4\"]",
    );
    let text = edited(
        &text,
        "# Become Infected from Susceptible, at rate",
        "[card.line.l6]\nkind = \"calculate\"\nname = \"Recovery pressure\"\nformula = \"recovery_rate * 3\"\n\n\
         # Become Infected from Susceptible, at rate",
    );
    let text = edited(
        &text,
        "rate = { param = \"recovery_rate\" }",
        "rate = { result = \"l6\" }",
    );
    let file = lowered(&text);
    let evaluator = HostEvaluator::new(file.lowered()).expect("the host runs the lowered sheet");
    // One tick of 7 days: the cells leave with chance 1 - e^(-(0.15 + 0.05) x 7), three in four of them recovered.
    let params = [
        ParamValue::F32(7.0),
        ParamValue::F32(0.3),
        ParamValue::F32(0.05),
        ParamValue::F32(0.05),
        ParamValue::F32(1.0),
    ];
    let mut lanes = evaluator.init(grid(128), &params, Some(9)).expect("the start runs");
    let mut stream = 0;
    evaluator
        .run_pass(
            "tick_cell_tick",
            &mut lanes,
            &params,
            &RunContext::tick(Some(9), 0),
            &mut stream,
        )
        .expect("a tick runs");
    let counts = health_counts(&lanes, 4);
    let cells = (128 * 128) as f64;
    let left = (counts[2] + counts[3]) as f64 / cells;
    let known = chance_from_rate(0.2, 7.0);
    assert!((left - known).abs() < 0.015, "{left} left, {known} known: {counts:?}");
    let recovered = counts[2] as f64 / (counts[2] + counts[3]) as f64;
    assert!(
        (recovered - 0.75).abs() < 0.015,
        "{recovered} of the removed recovered: {counts:?}"
    );
}

#[test]
fn a_competing_group_with_no_exposure_never_leaves() {
    // Every cell starts Infected, both exits are for each dead neighbour, and no cell is Dead yet.
    let text = sized(sheet_text("sird.toml"), 64);
    let text = edited(&text, "default = 0.01\nstep = 0.001", "default = 1\nstep = 0.001");
    let text = edited(
        &text,
        "right = { state = \"infected\" }",
        "right = { state = \"dead\" }",
    );
    let text = edited(
        &text,
        "rate = { param = \"recovery_rate\" }\nrate_unit = \"day\"\n",
        "rate = { param = \"recovery_rate\" }\nrate_unit = \"day\"\ntries = { result = \"l1\" }\n",
    );
    let text = edited(
        &text,
        "rate = { param = \"death_rate\" }\nrate_unit = \"day\"\n",
        "rate = { param = \"death_rate\" }\nrate_unit = \"day\"\ntries = { result = \"l1\" }\n",
    );
    let file = lowered(&text);
    assert!(GridProgram::compile(file.lowered()).is_ok(), "{text}");
    // The group's chance takes the guard of a rate for each: 1 - select(S <= 0, 1, e^(-S dt)).
    let ops: Vec<&str> = file
        .lowered()
        .document()
        .passes
        .iter()
        .flat_map(|pass| &pass.body.nodes)
        .filter(|node| matches!(file.line_map().line_of(node.id), Some("l3" | "l4")))
        .map(|node| node.op.as_str())
        .collect();
    assert!(ops.contains(&"cmp.le") && ops.contains(&"logic.select"), "{ops:?}");
    let evaluator = HostEvaluator::new(file.lowered()).expect("the host runs the lowered sheet");
    let params = [
        ParamValue::F32(1.0),
        ParamValue::F32(0.3),
        ParamValue::F32(5.0),
        ParamValue::F32(5.0),
        ParamValue::F32(1.0),
    ];
    let mut lanes = evaluator.init(grid(64), &params, Some(3)).expect("the start runs");
    let mut stream = 0;
    for tick in 0..4 {
        evaluator
            .run_pass(
                "tick_cell_tick",
                &mut lanes,
                &params,
                &RunContext::tick(Some(3), tick),
                &mut stream,
            )
            .expect("a tick runs");
    }
    assert_eq!(health_counts(&lanes, 4), [0, 64 * 64, 0, 0]);
}

#[test]
fn a_rate_change_inside_a_repeat_competes_with_no_other_line() {
    let text = edited(sheet_text("sird.toml"), "next_line = 6", "next_line = 7");
    let text = edited(
        &text,
        "lines = [\"l1\", \"l2\", \"l3\", \"l4\"]",
        "lines = [\"l1\", \"l2\", \"l6\"]",
    );
    let text = edited(
        &text,
        "# Become Recovered from Infected",
        "[card.line.l6]\nkind = \"repeat\"\ntimes = 2\nlines = [\"l3\", \"l4\"]\n\n# Become Recovered from Infected",
    );
    let document = ParsedSheet::parse(&text).expect("the sheet reads").into_document();
    let relations = CardRelations::new(&document, &document.cards[0]);
    assert_eq!(relations.order(), ["l1", "l2", "l6", "l3", "l4"]);
    assert!(relations.competing_groups().is_empty());
    assert_eq!(relations.otherwise_of("l4"), ["l3"]);
    assert_eq!(
        rate_note(&document, "l3", &parameter_defaults(&document)).map(|note| note.mean_stay),
        Some(None)
    );
}

/// Returns the number of cells of the `health` lane of `lanes` in each of its first `states` states.
fn health_counts(lanes: &HostLanes, states: usize) -> Vec<usize> {
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

#[test]
fn the_rates_sir_sheet_conserves_its_cells_every_tick() {
    let file = lowered(&sized(sheet_text("sir.toml"), 64));
    let evaluator = HostEvaluator::new(file.lowered()).expect("the host runs the lowered sheet");
    // Half a day per tick.
    let params = [ParamValue::F32(0.5)];
    let mut lanes = evaluator.init(grid(64), &params, Some(5)).expect("the start runs");
    let mut stream = 0;
    evaluator
        .run_pass(
            "button_new_outbreak",
            &mut lanes,
            &params,
            &RunContext::action(Some(5), 0, 1, 0),
            &mut stream,
        )
        .expect("the press runs");
    let start = health_counts(&lanes, 3);
    assert!(start[1] > 0, "{start:?}");
    let mut peak = 0;
    for tick in 0..200 {
        evaluator
            .run_pass(
                "tick_cell_tick",
                &mut lanes,
                &params,
                &RunContext::tick(Some(5), tick),
                &mut stream,
            )
            .expect("a tick runs");
        let counts = health_counts(&lanes, 3);
        assert_eq!(counts.iter().sum::<usize>(), 64 * 64, "tick {tick}: {counts:?}");
        peak = peak.max(counts[1]);
    }
    let end = health_counts(&lanes, 3);
    assert!(
        peak > start[1] && end[2] > 0,
        "the epidemic spreads and recovers: {peak}, {end:?}"
    );
}

/// Returns the key of every draw of `file`, with its line, in node order.
fn draw_keys(file: &SheetFile) -> Vec<(Option<String>, ConfigValue)> {
    file.lowered()
        .document()
        .passes
        .iter()
        .flat_map(|pass| &pass.body.nodes)
        .filter(|node| node.op.starts_with("rand."))
        .map(|node| {
            (
                file.line_map().line_of(node.id).map(str::to_owned),
                node.config["key"].clone(),
            )
        })
        .collect()
}

#[test]
fn a_tick_length_override_keeps_the_code_and_the_keys() {
    let file = lowered(sheet_text("sir.toml"));
    let program = GridProgram::compile(file.lowered()).unwrap_or_else(|problems| panic!("{problems:#?}"));
    let entry = program.entry();
    let tick_length = entry
        .param_descriptors()
        .iter()
        .find(|param| param.id() == "tick_length")
        .expect("the tick length is a parameter");
    assert!(tick_length.is_hidden());
    assert_eq!(
        entry.metadata().time.as_ref().map(|time| time.unit),
        Some(TimeUnit::Day)
    );
    // A run's value is a configuration of the one program: the code is the lowered graph's whatever the run sets.
    assert_eq!(entry.source().code(), Some(&file.code()));

    // A new length on the sheet is a new default: the code changes and every key stays.
    let halved = lowered(&edited(
        sheet_text("sir.toml"),
        "tick_length = 1\n",
        "tick_length = 0.5\n",
    ));
    assert_ne!(halved.code(), file.code());
    assert_eq!(draw_keys(&halved), draw_keys(&file));
    let default = |file: &SheetFile| file.lowered().document().params[0].kind().default_value();
    assert_eq!(default(&halved), ParamValue::F32(0.5));
    // So does another unit, with a factor where a rate's unit is not the tick's.
    let hours = lowered(&edited(
        sheet_text("sir.toml"),
        "tick_unit = \"day\"",
        "tick_unit = \"hour\"",
    ));
    assert_eq!(draw_keys(&hours), draw_keys(&file));
    let factors: Vec<f32> = hours
        .lowered()
        .document()
        .passes
        .iter()
        .flat_map(|pass| &pass.body.nodes)
        .filter(|node| node.op == "math.mul")
        .filter_map(|node| match node.inputs.get("b") {
            Some(InputValue::Literal(Literal::F32(factor))) => Some(*factor),
            _ => None,
        })
        .collect();
    assert_eq!(factors, [1.0 / 24.0, 1.0 / 24.0], "a rate per day in a model of hours");
}

#[test]
fn a_rate_per_week_in_a_model_of_days_is_divided_by_7() {
    let text = edited(
        sheet_text("sir.toml"),
        "rate_unit = \"day\"\n\n# Lines",
        "rate_unit = \"week\"\n\n# Lines",
    );
    let file = lowered(&text);
    let params = [ParamValue::F32(1.0), ParamValue::F32(0.3), ParamValue::F32(0.7)];
    let values = tick_uniforms(&file, &params);
    assert!(
        values
            .iter()
            .any(|&value| holds_threshold(value, 1.0 - (-0.1f64).exp())),
        "{values:?} holds no threshold of 1 - e^(-0.1)"
    );
}

/// A rate the same for every cell keeps its chance to the 32 bits of a draw. A recovery rate of 2e-8 per day, whose
/// `1 - e^(-h)` in `f32` is 0, gives a threshold of 2e-8 of the 2^32 units of a draw.
#[test]
fn a_small_rate_keeps_its_chance_in_the_threshold() {
    let file = lowered(sheet_text("sir.toml"));
    let params = [ParamValue::F32(1.0), ParamValue::F32(0.3), ParamValue::F32(2e-8)];
    assert_eq!(1.0 - (-2e-8f32).exp(), 0.0, "the direct form loses the chance");
    let values = tick_uniforms(&file, &params);
    assert!(
        values.iter().any(|value| (85..=86).contains(&value.to_bits())),
        "{values:?} holds no threshold of 2e-8"
    );
}

/// Each other form of rate on the SIR sheet lowers, checks and compiles: a rate line whose result a Become reads, a
/// rate in an all of with a comparison, and a rate worked out for each cell.
#[test]
fn every_form_of_rate_lowers_checks_and_compiles() {
    let sir = sheet_text("sir.toml");
    let rate_line = edited(sir, "next_line = 5", "next_line = 6");
    let rate_line = edited(
        &rate_line,
        "lines = [\"l1\", \"l2\", \"l3\"]",
        "lines = [\"l1\", \"l2\", \"l5\", \"l3\"]",
    );
    let rate_line = edited(
        &rate_line,
        "# Become Recovered from Infected",
        "[card.line.l5]\nkind = \"rate\"\nname = \"Recovery happens\"\nkey = 77\nrate = { param = \"recovery_rate\" \
         }\nrate_unit = \"day\"\n\n# Become Recovered from Infected",
    );
    let rate_line = edited(
        &rate_line,
        "\n\n[card.line.l3.condition]\nkind = \"rate\"\nrate = { param = \"recovery_rate\" }\nrate_unit = \"day\"\n",
        "\ncondition = { result = \"l5\" }\n",
    );
    let forms = [
        rate_line,
        // A rate in an all of with a comparison.
        edited(
            sir,
            "[card.line.l3.condition]\nkind = \"rate\"\nrate = { param = \"recovery_rate\" }\nrate_unit = \"day\"\n",
            "[card.line.l3.condition]\nkind = \"all_of\"\n\n[[card.line.l3.condition.items]]\nkind = \"compare\"\ntest \
             = \"less_than\"\nleft = { result = \"l1\" }\nright = 8\n\n[[card.line.l3.condition.items]]\nkind = \
             \"rate\"\nrate = { param = \"recovery_rate\" }\nrate_unit = \"day\"\n",
        ),
        // A rate worked out for each cell, for each of a count.
        edited(
            sir,
            "rate = { param = \"infection_rate\" }",
            "rate = { result = \"l1\" }",
        ),
    ];
    for text in forms {
        let file = lowered(&text);
        assert!(file.warnings().is_empty(), "{:?}\n{text}", file.warnings());
        let report = file.lowered().check();
        assert!(!report.has_errors(), "{:#?}", report.diagnostics);
        assert!(GridProgram::compile(file.lowered()).is_ok(), "{text}");
    }
    // A rate for each of a count of agents is one exp per agent, guarded at a count of 0.
    let mobile = edited(
        sheet_text("mobile_sir.toml"),
        "edges = \"wrap\"\n",
        "edges = \"wrap\"\n\n[time]\ntick_length = 1\ntick_unit = \"day\"\n",
    );
    let mobile = edited(
        &mobile,
        "kind = \"chance\"\nchance = { param = \"infection_chance\" }",
        "kind = \"rate\"\nrate = { param = \"infection_chance\" }\nrate_unit = \"day\"",
    );
    let file = lowered(&mobile);
    let ops: Vec<&str> = file
        .lowered()
        .document()
        .passes
        .iter()
        .flat_map(|pass| &pass.body.nodes)
        .filter(|node| file.line_map().line_of(node.id) == Some("l2"))
        .map(|node| node.op.as_str())
        .collect();
    assert!(ops.contains(&"math.exp") && ops.contains(&"logic.select"), "{ops:?}");
    assert!(!ops.contains(&"math.powi"), "{ops:?}");
    let report = file.lowered().check();
    assert!(!report.has_errors(), "{:#?}", report.diagnostics);
}

#[test]
fn a_rate_in_an_any_of_keeps_otherwise_and_draws_alone() {
    let sird = sheet_text("sird.toml");
    let text = edited(
        sird,
        "[card.line.l4.condition]\nkind = \"rate\"\nrate = { param = \"death_rate\" }\nrate_unit = \"day\"\n",
        "[card.line.l4.condition]\nkind = \"any_of\"\n\n[[card.line.l4.condition.items]]\nkind = \"rate\"\nrate = { \
         param = \"death_rate\" }\nrate_unit = \"day\"\n\n[[card.line.l4.condition.items]]\nkind = \"compare\"\ntest = \
         \"more_than\"\nleft = { result = \"l1\" }\nright = 7\n",
    );
    let document = ParsedSheet::parse(&text).expect("the sheet reads").into_document();
    let relations = CardRelations::new(&document, &document.cards[0]);
    assert!(relations.competing_groups().is_empty());
    assert_eq!(relations.otherwise_of("l4"), ["l3"]);
    let file = lowered(&text);
    let draws = file
        .lowered()
        .document()
        .passes
        .iter()
        .filter(|pass| pass.stage == PassStage::Tick)
        .flat_map(|pass| &pass.body.nodes)
        .filter(|node| matches!(&*node.op, "rand.chance" | "rand.below"))
        .count();
    assert_eq!(draws, 3, "lines 2, 3 and 4 draw alone");
    // The committed SIRD groups its two exits.
    let committed = ParsedSheet::parse(sird).expect("the sheet reads").into_document();
    assert_eq!(
        CardRelations::new(&committed, &committed.cards[0]).competing_groups(),
        [vec!["l3", "l4"]]
    );
}

/// Returns the note under line `line_id` of `text` at the parameters' defaults.
fn note(text: &str, line_id: &str) -> Option<crate::sheet::rates::RateNote> {
    let document = ParsedSheet::parse(text).expect("the sheet reads").into_document();
    rate_note(&document, line_id, &parameter_defaults(&document))
}

#[test]
fn rate_notes_match_the_spec_examples() {
    let sir = sheet_text("sir.toml");
    let recovery = note(sir, "l3").expect("line 3 has a note");
    assert_eq!(
        recovery.text,
        "Each tick (1 day), an Infected cell becomes Recovered with chance 0.0952. A rate is not a chance. Henad turns \
         it into one as 1 − e^(−0.1 × 1 day)."
    );
    assert_eq!(
        recovery.mean_stay.as_deref(),
        Some(
            "On average a cell stays Infected 10.5 days at this tick length. It approaches 1/0.1 = 10 days as the tick shrinks."
        )
    );
    assert_eq!(recovery.long_tick, None);
    let infection = note(sir, "l2").expect("line 2 has a note");
    assert_eq!(
        infection.text,
        "Each tick (1 day), a Susceptible cell becomes Infected with chance 0.259 with 1 infected neighbour, and 0.451 \
         with 2. A rate is not a chance. Henad turns it into one as 1 − e^(−0.3 × n × 1 day), where n is Infected \
         neighbours."
    );
    assert_eq!(infection.mean_stay, None);
    // The long tick: 1 - e^(-0.3 x 8) is above one half at a tick of 1 day, and below it at a quarter day.
    assert_eq!(infection.long_tick.as_deref(), Some("up to 0.909 with 8"));
    let quarter = edited(sir, "tick_length = 1\n", "tick_length = 0.25\n");
    assert_eq!(note(&quarter, "l2").and_then(|note| note.long_tick), None);
    let document = ParsedSheet::parse(&quarter).expect("the sheet reads").into_document();
    let largest = largest_count_chance(&document, "l2", &parameter_defaults(&document)).expect("a largest count");
    assert_eq!(largest.count, 8);
    assert!(largest.chance < LONG_TICK_CHANCE, "{largest:?}");
    assert_eq!(largest.words(), "up to 0.451 with 8");
    // At 4 days, 1 - e^(-0.3 x 8 x 4) is below 1 and rounds to 1. At 20 days it is 1 in a double.
    let four = edited(sir, "tick_length = 1\n", "tick_length = 4\n");
    assert_eq!(
        note(&four, "l2").and_then(|note| note.long_tick).as_deref(),
        Some("almost 1 with 8")
    );
    let twenty = edited(sir, "tick_length = 1\n", "tick_length = 20\n");
    assert_eq!(
        note(&twenty, "l2").and_then(|note| note.long_tick).as_deref(),
        Some("up to 1 with 8")
    );

    // A tick of 6 hours against a rate per day.
    let six_hours = edited(
        sir,
        "tick_length = 1\ntick_unit = \"day\"",
        "tick_length = 6\ntick_unit = \"hour\"",
    );
    assert_eq!(
        note(&six_hours, "l3").expect("line 3 has a note").text,
        "Each tick (0.25 days), an Infected cell becomes Recovered with chance 0.0247. A rate is not a chance. Henad \
         turns it into one as 1 − e^(−0.1 × 0.25 days)."
    );

    // A competing group: its highest line takes the group's note, and its other line none.
    let sird = sheet_text("sird.toml");
    let group = note(sird, "l3").expect("the group has a note");
    assert_eq!(
        group.text,
        "Each tick (1 day), an Infected cell leaves Infected with chance 0.0952: Recovered 0.0476, Dead 0.0476. A rate \
         is not a chance. Henad turns it into one as 1 − e^(−(0.05 + 0.05) × 1 day)."
    );
    assert_eq!(
        group.mean_stay.as_deref(),
        Some(
            "On average a cell stays Infected 10.5 days at this tick length. It approaches 1/(0.05 + 0.05) = 10 days \
             as the tick shrinks."
        )
    );
    assert_eq!(note(sird, "l4"), None);

    // No note holds a forbidden word.
    for text in [recovery.text, infection.text, group.text] {
        for word in sentence::FORBIDDEN_WORDS {
            assert!(!text.to_lowercase().contains(word), "{word} in {text}");
        }
    }
}

#[test]
fn a_rate_with_more_to_its_condition_has_no_mean_stay() {
    let sir = sheet_text("sir.toml");
    let sird = sheet_text("sird.toml");
    let recovery = note(sir, "l3").expect("line 3 has a note");
    // A rate with more to its condition keeps its note and loses the mean stay, and a group of such lines its note.
    let rate_and_compare = |text: &str, line: &str, param: &str| {
        edited(
            text,
            &format!(
                "[card.line.{line}.condition]\nkind = \"rate\"\nrate = {{ param = \"{param}\" }}\nrate_unit = \"day\"\n"
            ),
            &format!(
                "[card.line.{line}.condition]\nkind = \"all_of\"\n\n[[card.line.{line}.condition.items]]\nkind = \
                 \"compare\"\ntest = \"less_than\"\nleft = {{ result = \"l1\" }}\nright = 8\n\n\
                 [[card.line.{line}.condition.items]]\nkind = \"rate\"\nrate = {{ param = \"{param}\" }}\nrate_unit = \
                 \"day\"\n"
            ),
        )
    };
    let joined = note(&rate_and_compare(sir, "l3", "recovery_rate"), "l3").expect("line 3 has a note");
    assert_eq!(joined.text, recovery.text);
    assert_eq!(joined.mean_stay, None);
    let joined_group = rate_and_compare(sird, "l4", "death_rate");
    let document = ParsedSheet::parse(&joined_group)
        .expect("the sheet reads")
        .into_document();
    assert_eq!(
        CardRelations::new(&document, &document.cards[0]).competing_groups(),
        [vec!["l3", "l4"]]
    );
    assert_eq!(note(&joined_group, "l3"), None);
    // So does a rate line inside a When.
    let inside_when = edited(sir, "next_line = 5", "next_line = 6");
    let inside_when = edited(
        &inside_when,
        "lines = [\"l1\", \"l2\", \"l3\"]",
        "lines = [\"l1\", \"l2\", \"l5\"]",
    );
    let inside_when = edited(
        &inside_when,
        "# Become Recovered from Infected",
        "[card.line.l5]\nkind = \"when\"\nlines = [\"l3\"]\n\n[card.line.l5.condition]\nkind = \"compare\"\ntest = \
         \"less_than\"\nleft = { result = \"l1\" }\nright = 8\n\n# Become Recovered from Infected",
    );
    let when_note = note(&inside_when, "l3").expect("line 3 has a note");
    assert_eq!(when_note.text, recovery.text);
    assert_eq!(when_note.mean_stay, None);
}

#[test]
fn worked_out_rate_notes_match_the_spec_examples() {
    let sir = sheet_text("sir.toml");
    // A worked-out rate of a fixed number times a count (eco-6's), and any other worked out with its words first.
    let worked = |formula: &str| {
        let text = edited(sir, "next_line = 5", "next_line = 6");
        let text = edited(
            &text,
            "lines = [\"l1\", \"l2\", \"l3\"]",
            "lines = [\"l1\", \"l5\", \"l2\", \"l3\"]",
        );
        let text = edited(
            &text,
            "# Become Infected from Susceptible, at rate",
            &format!(
                "[card.line.l5]\nkind = \"calculate\"\nname = \"Pressure\"\nformula = \"{formula}\"\n\n# Become \
                 Infected from Susceptible, at rate"
            ),
        );
        let text = edited(
            &text,
            "rate = { param = \"infection_rate\" }",
            "rate = { result = \"l5\" }",
        );
        edited(
            &text,
            "rate_unit = \"day\"\ntries = { result = \"l1\" }\n",
            "rate_unit = \"day\"\n",
        )
    };
    assert_eq!(
        note(&worked("0.3 * l1"), "l2").expect("a note").text,
        "Each tick (1 day), a Susceptible cell becomes Infected with chance 0.259 with 1 infected neighbour, and 0.451 \
         with 2. A rate is not a chance. Henad turns it into one as 1 − e^(−r × 1 day), where r is 0.3 times Infected \
         neighbours."
    );
    let pressure = worked("infection_rate * l1");
    let parameter = note(&pressure, "l2").expect("a note");
    assert!(
        parameter
            .text
            .contains("chance 0.259 with 1 infected neighbour, and 0.451 with 2"),
        "{}",
        parameter.text
    );
    assert_eq!(parameter.long_tick.as_deref(), Some("up to 0.909 with 8"));
    assert_eq!(
        note(&worked("l1 + 0.3"), "l2").expect("a note").text,
        "Each tick (1 day), a Susceptible cell becomes Infected with a chance that grows with r, where r is Infected \
         neighbours plus 0.3. A rate is not a chance. Henad turns it into one as 1 − e^(−r × 1 day)."
    );

    // A Count with no name takes its automatic name in the note.
    let unnamed = edited(&worked("0.3 * l1"), "name = \"Infected neighbours\"\n", "");
    let document = ParsedSheet::parse(&unnamed).expect("the sheet reads").into_document();
    let (_, count) = document.line("l1").expect("line 1");
    assert_eq!(count.name, None);
    assert_eq!(
        crate::sheet::edit::automatic_name(&document, count).as_deref(),
        Some("Infected neighbours")
    );
    let unnamed_note = note(&unnamed, "l2").expect("a note").text;
    assert!(
        unnamed_note.contains("chance 0.259 with 1 infected neighbour, and 0.451 with 2")
            && unnamed_note.contains("where r is 0.3 times Infected neighbours."),
        "{unnamed_note}"
    );
    assert_eq!(
        crate::sheet::sentence::Sentences::new(&document).line("l5", &[]),
        "Pressure = 0.3 times Infected neighbours",
        "a formula names an unnamed Count by its automatic name"
    );

    // A sheet with no length of one tick, a chance line, and a values map read in place of the defaults.
    let untimed = edited(sir, "[time]\ntick_length = 1\ntick_unit = \"day\"\n\n", "");
    assert_eq!(note(&untimed, "l3"), None);
    assert_eq!(note(sheet_text("sir_chance.toml"), "l3"), None);
    let document = ParsedSheet::parse(sir).expect("the sheet reads").into_document();
    let values = BTreeMap::from([("recovery_rate".to_owned(), 0.2)]);
    let live = rate_note(&document, "l3", &values).expect("a note");
    assert!(
        live.text.contains("chance 0.181") && live.text.contains("e^(−0.2 × 1 day)"),
        "{}",
        live.text
    );
}

/// Returns the chance or rate phrase that the condition of line `line_id` of `builder`'s sheet holds.
fn condition_phrase(builder: &SheetBuilder, line_id: &str) -> SheetLine {
    let (_, line) = builder.document().line(line_id).expect("the line");
    match line.condition.value() {
        Some(SheetValue::Phrase(phrase)) => phrase.as_ref().clone(),
        other => panic!("line {line_id} holds {other:?}"),
    }
}

#[test]
fn change_to_a_rate_keeps_the_chance_per_tick() {
    let mut builder = SheetBuilder::new(
        ParsedSheet::parse(sheet_text("sir.toml"))
            .expect("the sheet reads")
            .into_document(),
    );
    let typed: f64 = ((1.0 - (-0.1f64).exp()) as f32).to_string().parse().expect("a number");
    builder
        .apply(SheetEdit::ChangeToChance { line: "l3".to_owned() })
        .expect_err("line 3's rate is a parameter, no typed number");
    // Line 3 with a typed rate of 0.1 per day turns into the chance 1 - e^(-0.1) and back.
    builder
        .apply(SheetEdit::SetSlot {
            line: "l3".to_owned(),
            path: "condition.rate".to_owned(),
            value: Slot::Filled(SheetValue::Literal(SheetLiteral::Number(0.1))),
        })
        .expect("a typed rate");
    builder
        .apply(SheetEdit::ChangeToChance { line: "l3".to_owned() })
        .expect("line 3 changes to a chance");
    let as_chance = condition_phrase(&builder, "l3");
    assert_eq!(as_chance.kind, crate::sheet::document::LineKind::Chance);
    assert_eq!(
        as_chance.chance,
        Slot::Filled(SheetValue::Literal(SheetLiteral::Number(typed)))
    );
    assert_eq!(as_chance.rate_unit, None);
    builder
        .apply(SheetEdit::ChangeToRate { line: "l3".to_owned() })
        .expect("line 3 changes to a rate");
    let as_rate = condition_phrase(&builder, "l3");
    assert_eq!(
        as_rate.rate,
        Slot::Filled(SheetValue::Literal(SheetLiteral::Number(0.1)))
    );
    assert_eq!(as_rate.rate_unit, Some(TimeUnit::Day));
    assert!(as_rate.rate_unit_from_tick);
    // The key stays with the line, and "for each" with the phrase, across both commands.
    let key = builder.document().line("l2").expect("line 2").1.key;
    let tries = condition_phrase(&builder, "l2").tries;
    assert!(!tries.is_blank());
    builder
        .apply(SheetEdit::ChangeToChance { line: "l2".to_owned() })
        .expect_err("line 2's rate is a parameter");
    builder
        .apply(SheetEdit::SetSlot {
            line: "l2".to_owned(),
            path: "condition.rate".to_owned(),
            value: Slot::Filled(SheetValue::Literal(SheetLiteral::Number(0.3))),
        })
        .expect("a typed rate");
    builder
        .apply(SheetEdit::ChangeToChance { line: "l2".to_owned() })
        .expect("line 2 changes to a chance");
    let typed_infection: f64 = ((1.0 - (-0.3f64).exp()) as f32).to_string().parse().expect("a number");
    let as_chance = condition_phrase(&builder, "l2");
    assert_eq!(as_chance.kind, crate::sheet::document::LineKind::Chance);
    assert_eq!(
        as_chance.chance,
        Slot::Filled(SheetValue::Literal(SheetLiteral::Number(typed_infection)))
    );
    assert_eq!(as_chance.tries, tries);
    assert_eq!(builder.document().line("l2").expect("line 2").1.key, key);
    builder
        .apply(SheetEdit::ChangeToRate { line: "l2".to_owned() })
        .expect("line 2 changes back to a rate");
    // The rate reads back as the shortest number whose chance per tick is the typed chance.
    let as_rate = condition_phrase(&builder, "l2");
    let Slot::Filled(SheetValue::Literal(SheetLiteral::Number(rate))) = as_rate.rate else {
        panic!("line 2 holds {:?}", as_rate.rate);
    };
    assert!((rate - 0.3).abs() < 1e-6, "{rate}");
    assert!(
        within_one_ulp(chance_from_rate(rate, 1.0) as f32, typed_infection as f32),
        "{rate} gives no chance {typed_infection}"
    );
    assert_eq!(as_rate.tries, tries);
    assert_eq!(builder.document().line("l2").expect("line 2").1.key, key);
    let text = builder.document().to_text().expect("the sheet writes");
    let reread = ParsedSheet::parse(&text).expect("the sheet reads back").into_document();
    assert_eq!(reread.line("l2").expect("line 2").1.key, key);
    assert!(SheetFile::parse(&text).is_ok(), "the edited sheet lowers");
}

#[test]
fn a_chance_with_no_tick_or_no_typed_number_does_not_change_to_a_rate() {
    let chance_sheet = ParsedSheet::parse(sheet_text("sir_chance.toml"))
        .expect("the sheet reads")
        .into_document();
    let mut builder = SheetBuilder::new(chance_sheet);
    builder
        .apply(SheetEdit::ChangeToRate { line: "l3".to_owned() })
        .expect_err("the sheet sets no length of one tick");
    let mut builder = SheetBuilder::new(
        ParsedSheet::parse(sheet_text("sir.toml"))
            .expect("reads")
            .into_document(),
    );
    builder
        .apply(SheetEdit::ChangeToRate { line: "l3".to_owned() })
        .expect_err("line 3 holds a rate, not a chance");
    builder
        .apply(SheetEdit::ChangeToRate { line: "l4".to_owned() })
        .expect_err("line 4's chance is a parameter");
    builder
        .apply(SheetEdit::SetSlot {
            line: "l4".to_owned(),
            path: "condition.chance".to_owned(),
            value: Slot::Filled(SheetValue::Literal(SheetLiteral::Number(1.0))),
        })
        .expect("a typed chance");
    builder
        .apply(SheetEdit::ChangeToRate { line: "l4".to_owned() })
        .expect_err("a chance of 1 gives no rate");
}

#[test]
fn a_rate_read_in_another_unit_than_its_parameter_warns() {
    let text = edited(
        sheet_text("sir.toml"),
        "rate = { param = \"recovery_rate\" }\nrate_unit = \"day\"",
        "rate = { param = \"recovery_rate\" }\nrate_unit = \"week\"",
    );
    let file = lowered(&text);
    let warnings: Vec<&str> = file.warnings().iter().map(|warning| warning.message.as_str()).collect();
    assert_eq!(
        warnings,
        ["Recovery rate is per day on the sheet, but this line reads it per week."]
    );
    // The line's unit governs the run.
    let factors = file
        .lowered()
        .document()
        .passes
        .iter()
        .flat_map(|pass| &pass.body.nodes)
        .filter(|node| matches!(node.inputs.get("b"), Some(InputValue::Literal(Literal::F32(factor))) if *factor == 1.0 / 7.0))
        .count();
    assert_eq!(factors, 1);
    assert!(lowered(sheet_text("sir.toml")).warnings().is_empty());
}

#[test]
fn a_chance_for_each_has_its_note_and_a_count_as_a_chance_is_named() {
    use crate::sheet::rates::{chance_note, count_as_draw};

    let document = ParsedSheet::parse(sheet_text("sir_chance.toml"))
        .expect("the sheet reads")
        .into_document();
    let values = parameter_defaults(&document);
    assert_eq!(
        chance_note(&document, "l2", &values).as_deref(),
        Some(
            "Each tick, a Susceptible cell becomes Infected with chance 0.3 with 1 infected neighbour, and 0.51 with 2. \
             Each one counted is a separate try: 1 − (1 − 0.3)^n, where n is Infected neighbours."
        )
    );
    assert_eq!(
        chance_note(&document, "l3", &values),
        None,
        "a chance without for each has no note"
    );
    // A chance that is a result leads with words.
    let mut builder = SheetBuilder::new(document.clone());
    builder
        .apply(SheetEdit::SetSlot {
            line: "l2".to_owned(),
            path: "condition.chance".to_owned(),
            value: Slot::Filled(SheetValue::Chip(crate::sheet::document::SheetChip::Result {
                line: "l1".to_owned(),
                part: None,
                property: None,
            })),
        })
        .expect("the chance takes the count");
    let edited = builder.document();
    assert_eq!(
        chance_note(edited, "l2", &values).as_deref(),
        Some(
            "Each tick, a Susceptible cell becomes Infected with a chance that grows with n, where p is Infected \
             neighbours and n is Infected neighbours. Each one counted is a separate try: 1 − (1 − p)^n."
        )
    );
    assert_eq!(
        count_as_draw(edited, "l2"),
        Some(("Infected neighbours".to_owned(), sentence::DrawKind::Chance))
    );
    assert_eq!(count_as_draw(&document, "l2"), None);
}

#[test]
fn the_card_offers_the_conversion_a_typed_draw_allows() {
    use crate::sheet::rates::conversion;
    use crate::sheet::sentence::DrawKind;

    let document = |text: &str| ParsedSheet::parse(text).expect("the sheet reads").into_document();
    let sir = sheet_text("sir.toml");
    let typed_recovery = edited(sir, "rate = { param = \"recovery_rate\" }", "rate = 0.1");
    assert_eq!(conversion(&document(sir), "l3"), None, "a parameter is no typed number");
    assert_eq!(conversion(&document(&typed_recovery), "l3"), Some(DrawKind::Chance));
    // A chance on the card of a button has no duration and changes into no rate.
    let typed_outbreak = edited(
        sir,
        "kind = \"chance\"\nchance = { param = \"initial_infected\" }\n",
        "kind = \"chance\"\nchance = 0.01\n",
    );
    assert_eq!(conversion(&document(&typed_outbreak), "l4"), None);
    let chance = edited(
        sheet_text("sir_chance.toml"),
        "chance = { param = \"recovery_chance\" }",
        "chance = 0.05",
    );
    assert_eq!(conversion(&document(&chance), "l3"), None, "one tick has no length");
    let timed = edited(
        &chance,
        "edges = \"wrap\"\n",
        "edges = \"wrap\"\n\n[time]\ntick_length = 1\ntick_unit = \"day\"\n",
    );
    assert_eq!(conversion(&document(&timed), "l3"), Some(DrawKind::Rate));
    assert_eq!(conversion(&document(&timed), "l1"), None, "a count draws nothing");
}

#[test]
fn the_time_notice_names_the_chance_lines_of_cards_that_run_each_tick() {
    use crate::sheet::rates::chance_line_numbers;

    let document = |text: &str| ParsedSheet::parse(text).expect("the sheet reads").into_document();
    assert_eq!(chance_line_numbers(&document(sheet_text("sir_chance.toml"))), [2, 3]);
    assert!(
        chance_line_numbers(&document(sheet_text("sir.toml"))).is_empty(),
        "the rates SIR draws at rates, and the chance of New outbreak runs once"
    );
}

#[test]
fn a_fixed_rate_below_a_count_of_its_new_state_asks_to_grow_with_it() {
    use crate::sheet::rates::grow_count;

    let document = |text: &str| ParsedSheet::parse(text).expect("the sheet reads").into_document();
    let sir = sheet_text("sir.toml");
    assert_eq!(grow_count(&document(sir), "l2"), None, "line 2 already grows for each");
    assert_eq!(grow_count(&document(sir), "l3"), None, "no line counts Recovered");
    let alone = edited(
        sir,
        "rate_unit = \"day\"\ntries = { result = \"l1\" }\n",
        "rate_unit = \"day\"\n",
    );
    let found = grow_count(&document(&alone), "l2").expect("line 2 asks");
    assert_eq!(
        (found.line.as_str(), found.name.as_str()),
        ("l1", "Infected neighbours")
    );
    assert_eq!(
        sentence::rate_grow_prompt(&found.name),
        "Should this rate grow with the number of Infected neighbours?"
    );
    // An empty rate has no note and asks nothing.
    let empty = edited(&alone, "rate = { param = \"infection_rate\" }\n", "");
    assert_eq!(note(&empty, "l2"), None);
    assert_eq!(grow_count(&document(&empty), "l2"), None);
    // A rate worked out from the count asks nothing.
    let worked_out = edited(
        &alone,
        "rate = { param = \"infection_rate\" }",
        "rate = { result = \"l1\" }",
    );
    assert_eq!(grow_count(&document(&worked_out), "l2"), None);
    // A count below the line is not open to it.
    let mut builder = SheetBuilder::new(document(&alone));
    builder
        .apply(SheetEdit::MoveLine {
            line: "l1".to_owned(),
            direction: crate::sheet::edit::MoveDirection::Down,
        })
        .expect("line 1 moves down");
    assert_eq!(grow_count(builder.document(), "l2"), None);
}

#[test]
fn a_lower_line_of_a_competing_group_reads_or() {
    let document = ParsedSheet::parse(sheet_text("sird.toml"))
        .expect("the sheet reads")
        .into_document();
    let sentences = crate::sheet::sentence::Sentences::new(&document);
    let parts = sentence::competing_parts(sentences.placed_parts("l4", &[]), vec![3, 4]);
    let parts: Vec<_> = parts.into_iter().map(|placed| placed.part).collect();
    assert_eq!(parts.first(), Some(&sentence::SentencePart::Competing(vec![3, 4])));
    assert_eq!(
        sentence::sentence_text(&parts),
        "or become Dead from Infected, at rate Death rate per day"
    );
    let units: Vec<Option<TimeUnit>> = document
        .parameters
        .iter()
        .map(crate::sheet::rates::parameter_rate_unit)
        .collect();
    assert_eq!(
        units,
        [Some(TimeUnit::Day), Some(TimeUnit::Day), Some(TimeUnit::Day), None]
    );
    assert_eq!(
        sentence::long_tick_note("up to 0.909 with 8"),
        "Up to 0.909 with 8. This tick is long for this rate. A shorter tick follows the rate more closely."
    );
    assert_eq!(
        sentence::long_tick_note(&sentence::long_tick_almost_one(8)),
        "Almost 1 with 8. This tick is long for this rate. A shorter tick follows the rate more closely."
    );
}

#[test]
fn change_to_a_rate_is_refused_on_a_card_that_runs_once_as_the_card_offers_it() {
    use crate::sheet::check;
    use crate::sheet::document::LineKind;
    use crate::sheet::edit::SheetEditError;
    use crate::sheet::rates::conversion;
    use crate::sheet::sentence::DrawKind;

    let document = |text: &str| ParsedSheet::parse(text).expect("the sheet reads").into_document();
    // Line 4, on the card of New outbreak, becomes Infected with a typed chance. The card runs once.
    let typed_outbreak = edited(
        sheet_text("sir.toml"),
        "kind = \"chance\"\nchance = { param = \"initial_infected\" }\n",
        "kind = \"chance\"\nchance = 0.01\n",
    );
    let mut builder = SheetBuilder::new(document(&typed_outbreak));
    assert_eq!(conversion(builder.document(), "l4"), None);
    let before = builder.document().to_text().expect("the sheet writes");
    assert_eq!(
        builder.apply(SheetEdit::ChangeToRate { line: "l4".to_owned() }),
        Err(SheetEditError::Invalid(sentence::RATE_IN_BUTTON_CARD.to_owned()))
    );
    assert_eq!(builder.document().to_text().expect("the sheet writes"), before);
    // Line 3 of a card that runs each tick changes its typed chance to a rate, which the checker takes.
    let chance = edited(
        sheet_text("sir_chance.toml"),
        "chance = { param = \"recovery_chance\" }",
        "chance = 0.01",
    );
    let timed = edited(
        &chance,
        "edges = \"wrap\"\n",
        "edges = \"wrap\"\n\n[time]\ntick_length = 1\ntick_unit = \"day\"\n",
    );
    let mut builder = SheetBuilder::new(document(&timed));
    assert_eq!(conversion(builder.document(), "l3"), Some(DrawKind::Rate));
    builder
        .apply(SheetEdit::ChangeToRate { line: "l3".to_owned() })
        .expect("line 3 changes to a rate");
    assert_eq!(condition_phrase(&builder, "l3").kind, LineKind::Rate);
    let report = check::check(builder.document());
    assert!(!report.has_errors(), "{report:?}");
}
