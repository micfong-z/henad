//! Chances, looks, calculations and choices of grid sheets on the host: the kinds and values test sheets against a
//! Rust reference, the chance SIR conserving its cells, the power rule of a chance once for each, the order and keys
//! of chances, the refusal of a formula of arrows, a division by 0 and the range of a Random number line.

use henad_core::explore::fingerprint::fnv1a64;

use super::sheet_support::{
    EXTRA_PROPERTIES, KINDS, VALUES, alive, edited, epidemic, host_of, levels, life, life_with_line, lowered,
    placed_at, refusals, sheet_text, sized,
};
use super::support::torus;
use crate::host::{HostLanes, HostValue, RunContext};
use crate::sheet::SheetFile;
use crate::value::ConfigValue;
use crate::wgsl::GridProgram;

#[test]
fn the_kinds_sheet_runs_its_lines_on_the_host() {
    let parsed = crate::sheet::ParsedSheet::parse(KINDS).expect("the kinds sheet reads");
    assert_eq!(
        parsed.document().to_text().as_deref(),
        Ok(KINDS),
        "the kinds sheet is canonical"
    );
    let file = lowered(KINDS);
    assert!(file.warnings().is_empty(), "{:?}", file.warnings());
    let report = file.lowered().check();
    assert!(
        !report.has_errors(),
        "{:#?}\n{}",
        report.diagnostics,
        file.lowered().to_toml()
    );
    assert!(GridProgram::compile(file.lowered()).is_ok());
    let tick = &file.lowered().document().passes[1];
    assert!(tick.body.nodes.iter().any(|node| node.op == "grid.count"));
    assert!(tick.body.nodes.iter().any(|node| node.op == "grid.fold"));

    let side = 8u32;
    let evaluator = host_of(&file);
    let world = torus(side, side);
    let mut lanes = HostLanes::new(evaluator.document(), world).expect("lanes");
    let cells = (side * side) as usize;
    let mut rng = 0x6b69_6e64_7300_0001u64;
    let mut status = vec![false; cells];
    let mut flag = vec![false; cells];
    for cell in 0..cells {
        rng = henad_core::authoring::primitives::rng::xorshift64(rng);
        status[cell] = rng.is_multiple_of(3);
        flag[cell] = (rng >> 8).is_multiple_of(2);
        assert!(
            lanes
                .column_mut("status")
                .expect("status")
                .set(cell, HostValue::Category(u32::from(status[cell])))
        );
        assert!(
            lanes
                .column_mut("flag")
                .expect("flag")
                .set(cell, HostValue::Bool(flag[cell]))
        );
        assert!(lanes.column_mut("level").expect("level").set(cell, HostValue::F32(7.0)));
    }
    let mut stream = 0;
    evaluator
        .run_pass(
            "tick_cell_tick",
            &mut lanes,
            &[],
            &RunContext::tick(Some(1), 0),
            &mut stream,
        )
        .expect("a tick runs");

    let at = |x: i64, y: i64| ((y.rem_euclid(side as i64)) * side as i64 + x.rem_euclid(side as i64)) as usize;
    for cell in 0..cells {
        let (x, y) = ((cell as u32 % side) as i64, (cell as u32 / side) as i64);
        let mut alive_around = 0u32;
        for (dx, dy) in [(-1, -1), (0, -1), (1, -1), (-1, 0), (1, 0), (-1, 1), (0, 1), (1, 1)] {
            alive_around += u32::from(status[at(x + dx, y + dy)]);
        }
        let mut flagged_around = 0u32;
        for (dx, dy) in [(0, -1), (-1, 0), (1, 0), (0, 1)] {
            flagged_around += u32::from(flag[at(x + dx, y + dy)]);
        }
        let crowded = !(1..=3).contains(&alive_around);
        let level = if crowded { 5.0 } else { alive_around as f32 };
        let lone_flag = flagged_around >= 1 && !flag[cell];
        let new_flag = status[cell] || lone_flag || flag[cell];
        let new_status = if status[cell] {
            alive_around > 1
        } else {
            alive_around == 3 || level >= 5.0
        };
        assert_eq!(
            lanes.column("level").and_then(|lane| lane.get(cell)),
            Some(HostValue::F32(level)),
            "level at {cell}"
        );
        assert_eq!(
            lanes.column("flag").and_then(|lane| lane.get(cell)),
            Some(HostValue::Bool(new_flag)),
            "flag at {cell}"
        );
        assert_eq!(
            lanes.column("status").and_then(|lane| lane.get(cell)),
            Some(HostValue::Category(u32::from(new_status))),
            "status at {cell}"
        );
    }
}

#[test]
fn the_chance_sir_sheet_conserves_its_cells_every_tick() {
    let run = epidemic(&lowered(&sized(sheet_text("sir_chance.toml"), 64)), &[]);
    assert!(run.start[1] > 0, "the start infects some cells: {:?}", run.start);
    // New outbreak runs its card once, and its chance infects more susceptible cells.
    assert!(run.pressed[1] > run.start[1], "{:?} then {:?}", run.start, run.pressed);
    assert!(
        run.peak > run.pressed[1] && run.end[2] > 0,
        "the epidemic spreads and recovers: {}, {:?}",
        run.peak,
        run.end
    );
}

/// A grid sheet `side` cells wide and high whose only line sets Hit to yes with chance `chance` once for each of the
/// cells `around` whose Flag is yes. `chance` is a value as a sheet writes it, and a result `l2` is a Calculate of
/// 0.1 times 2.
fn power_sheet(side: u32, around: &str, chance: &str) -> String {
    format!(
        "format = \"henad-model\"\nformat_version = 1\nkind = \"sheet\"\n\n[sheet]\nversion = 1\nid = \"power\"\nname = \
         \"Power\"\nnext_line = 4\n\n[world]\nmade_of = \"cells\"\nwidth = {side}\nheight = {side}\nedges = \"wrap\"\n\n\
         [[cells.property]]\nid = \"flag\"\nname = \"Flag\"\ntype = \"yes_no\"\n\n[cells.property.starts_as]\nrule = \
         \"value\"\nvalue = false\n\n[[cells.property]]\nid = \"hit\"\nname = \"Hit\"\ntype = \"yes_no\"\n\n\
         [cells.property.starts_as]\nrule = \"value\"\nvalue = false\n\n[[card]]\nid = \"cell_tick\"\nevery = \
         \"cells\"\nruns = \"each_tick\"\nlines = [\"l1\", \"l2\", \"l3\"]\n\n[card.line.l1]\nkind = \"count\"\nname = \
         \"Flagged around\"\naround = \"{around}\"\n\n[card.line.l1.whose]\nkind = \"compare\"\ntest = \"is\"\nleft = \
         {{ their = \"flag\" }}\nright = true\n\n[card.line.l2]\nkind = \"calculate\"\nname = \"Base chance\"\nformula = \
         \"0.1 * 2\"\n\n[card.line.l3]\nkind = \"set\"\nkey = 21\nproperty = \"hit\"\nvalue = true\n\n\
         [card.line.l3.condition]\nkind = \"chance\"\nchance = {chance}\ntries = {{ result = \"l1\" }}\n"
    )
}

/// Runs one tick of the lowered power sheet `file`, `side` cells wide, from the flags `flagged` places, and returns the
/// share of the cells `counted` keeps that Hit holds yes on.
fn hit_share(
    file: &SheetFile,
    side: u32,
    flagged: impl Fn(u32, u32) -> bool,
    counted: impl Fn(u32, u32) -> bool,
) -> f64 {
    let evaluator = host_of(file);
    let world = torus(side, side);
    let mut lanes = HostLanes::new(evaluator.document(), world).expect("lanes");
    for cell in 0..side * side {
        let flag = flagged(cell % side, cell / side);
        assert!(
            lanes
                .column_mut("flag")
                .expect("flag")
                .set(cell as usize, HostValue::Bool(flag))
        );
    }
    let mut stream = 0;
    evaluator
        .run_pass(
            "tick_cell_tick",
            &mut lanes,
            &[],
            &RunContext::tick(Some(9), 0),
            &mut stream,
        )
        .expect("a tick runs");
    let hit = lanes.column("hit").expect("a hit lane");
    let (mut cells, mut hits) = (0u32, 0u32);
    for cell in 0..side * side {
        if counted(cell % side, cell / side) {
            cells += 1;
            hits += u32::from(hit.get(cell as usize) == Some(HostValue::Bool(true)));
        }
    }
    f64::from(hits) / f64::from(cells)
}

#[test]
fn once_for_each_follows_the_power_rule() {
    let side = 300;
    let everywhere = |_: u32, _: u32| true;
    for chance in ["0.2", "{ result = \"l2\" }"] {
        let power = if chance == "0.2" { "math.powi" } else { "math.pow" };
        for (around, neighbours) in [("cells_8", 8), ("cells_4", 4), ("block_3x3", 9)] {
            let file = lowered(&power_sheet(side, around, chance));
            assert!(file.warnings().is_empty(), "{:?}", file.warnings());
            let tick = &file.lowered().document().passes[1];
            let node = tick
                .body
                .nodes
                .iter()
                .find(|node| node.op == power)
                .unwrap_or_else(|| panic!("{around}, {chance}: no {power}"));
            if power == "math.powi" {
                assert_eq!(
                    node.config.get("max_exponent"),
                    Some(&ConfigValue::Integer(neighbours)),
                    "{around}"
                );
            }
            // Every cell flagged: each cell counts all its neighbours, the bound of the power.
            let expected = 1.0 - 0.8f64.powi(neighbours as i32);
            let share = hit_share(&file, side, everywhere, everywhere);
            assert!(
                (share - expected).abs() < 0.006,
                "{around}, {chance}: {share} for {expected}"
            );
            // No cell flagged: no try, so no cell is hit.
            assert_eq!(
                hit_share(&file, side, |_, _| false, everywhere),
                0.0,
                "{around}, {chance}"
            );
        }
        // Every third row flagged: a cell of another row counts the 3 cells of the flagged row beside it.
        let file = lowered(&power_sheet(side, "cells_8", chance));
        let share = hit_share(&file, side, |_, y| y % 3 == 0, |_, y| y % 3 != 0);
        assert!((share - 0.488).abs() < 0.01, "{chance}: {share} for 0.488");
    }
}

#[test]
fn the_forest_fire_lowers_its_spread_line_before_its_lightning_line() {
    let file = lowered(sheet_text("forest_fire.toml"));
    let tick = &file.lowered().document().passes[1];
    let transition = tick
        .body
        .nodes
        .iter()
        .find(|node| node.op == "cat.transition")
        .expect("the cover's transition");
    let Some(ConfigValue::Array(rules)) = transition.config.get("rules") else {
        panic!("the transition holds no rules");
    };
    let ids: Vec<&str> = rules
        .iter()
        .filter_map(|rule| match rule {
            ConfigValue::Table(table) => table.get("id").and_then(ConfigValue::as_str),
            _ => None,
        })
        .collect();
    let place = |id: &str| {
        ids.iter()
            .position(|rule| *rule == id)
            .unwrap_or_else(|| panic!("no rule {id}"))
    };
    assert!(place("l2") < place("l3"), "{ids:?}");
    // Both chances lower to a draw keyed by their own line, compared with a threshold of all 32 bits.
    let draws = tick.body.nodes.iter().filter(|node| node.op == "rand.below").count();
    assert_eq!(draws, 2);
}

#[test]
fn a_chance_inside_a_when_draws_with_its_lines_composed_key() {
    let text = edited(life(), "next_line = 5", "next_line = 7");
    let text = edited(
        &text,
        "lines = [\"l1\", \"l2\", \"l3\"]",
        "lines = [\"l1\", \"l2\", \"l3\", \"l5\"]",
    );
    let text = edited(
        &text,
        "# Lines of Clear: run once, every cell, when pressed",
        "[card.line.l5]\nkind = \"when\"\nkey = 31\nlines = [\"l6\"]\n\n[card.line.l5.condition]\nkind = \
         \"chance\"\nchance = 0.5\n\n[card.line.l6]\nkind = \"become\"\nkey = 32\nproperty = \"status\"\nnew_state = \
         \"dead\"\nstart_state = \"alive\"\n\n[card.line.l6.condition]\nkind = \"chance\"\nchance = 0.25\n\n\
         # Lines of Clear: run once, every cell, when pressed",
    );
    let file = lowered(&text);
    assert!(file.warnings().is_empty(), "{:?}", file.warnings());
    let path = (fnv1a64(b"condition") & 0xffff_ffff) as u32;
    let mut keys: Vec<i64> = file.lowered().document().passes[1]
        .body
        .nodes
        .iter()
        .filter(|node| node.op == "rand.below")
        .filter_map(|node| match node.config.get("key") {
            Some(ConfigValue::Integer(key)) => Some(*key),
            _ => None,
        })
        .collect();
    keys.sort_unstable();
    let mut expected = vec![
        i64::from(crate::normal::compose_key(31, path)),
        i64::from(crate::normal::compose_key(32, path)),
    ];
    expected.sort_unstable();
    assert_eq!(keys, expected, "one draw per chance, each keyed by its own line");
}

/// Plants random statuses, flags and levels on a world `side` cells wide, runs one tick of the lowered values sheet
/// `file`, and returns the lanes with the planted statuses, flags and levels.
fn values_after_one_tick(file: &SheetFile, side: u32) -> (HostLanes, Vec<bool>, Vec<bool>, Vec<f32>) {
    let cells = (side * side) as usize;
    let evaluator = host_of(file);
    let world = torus(side, side);
    let mut lanes = HostLanes::new(evaluator.document(), world).expect("lanes");
    let mut rng = 0x7661_6c75_6573_0001u64;
    let (mut status, mut flag, mut level) = (vec![false; cells], vec![false; cells], vec![0.0f32; cells]);
    for cell in 0..cells {
        rng = henad_core::authoring::primitives::rng::xorshift64(rng);
        status[cell] = rng.is_multiple_of(3);
        flag[cell] = (rng >> 8).is_multiple_of(2);
        level[cell] = ((rng >> 16) % 1000) as f32 / 1000.0;
        assert!(
            lanes
                .column_mut("status")
                .expect("status")
                .set(cell, HostValue::Category(u32::from(status[cell])))
        );
        assert!(
            lanes
                .column_mut("flag")
                .expect("flag")
                .set(cell, HostValue::Bool(flag[cell]))
        );
        assert!(
            lanes
                .column_mut("level")
                .expect("level")
                .set(cell, HostValue::F32(level[cell]))
        );
    }
    let mut stream = 0;
    evaluator
        .run_pass(
            "tick_cell_tick",
            &mut lanes,
            &[],
            &RunContext::tick(Some(1), 0),
            &mut stream,
        )
        .expect("a tick runs");

    (lanes, status, flag, level)
}

#[test]
fn the_values_sheet_runs_its_lines_on_the_host() {
    let parsed = crate::sheet::ParsedSheet::parse(VALUES).expect("the values sheet reads");
    assert_eq!(
        parsed.document().to_text().as_deref(),
        Ok(VALUES),
        "the values sheet is canonical"
    );
    let file = lowered(VALUES);
    assert!(file.warnings().is_empty(), "{:?}", file.warnings());
    let report = file.lowered().check();
    assert!(!report.has_errors(), "{:#?}", report.diagnostics);
    assert!(GridProgram::compile(file.lowered()).is_ok());
    let tick = &file.lowered().document().passes[1];
    for op in ["grid.fold", "math.expr", "cat.switch", "logic.select"] {
        assert!(tick.body.nodes.iter().any(|node| node.op == op), "no {op}");
    }

    let side = 8u32;
    let cells = (side * side) as usize;
    let (lanes, status, flag, level) = values_after_one_tick(&file, side);

    let at = |x: i64, y: i64| ((y.rem_euclid(side as i64)) * side as i64 + x.rem_euclid(side as i64)) as usize;
    let near = |value: Option<HostValue>, expected: f32| match value {
        Some(HostValue::F32(found)) => (found - expected).abs() < 1e-4,
        _ => false,
    };
    for cell in 0..cells {
        let (x, y) = ((cell as u32 % side) as i64, (cell as u32 / side) as i64);
        let around: Vec<usize> = [(-1, -1), (0, -1), (1, -1), (-1, 0), (1, 0), (-1, 1), (0, 1), (1, 1)]
            .iter()
            .map(|&(dx, dy)| at(x + dx, y + dy))
            .collect();
        let total: f32 = around.iter().map(|&other| level[other]).sum();
        let top = around.iter().map(|&other| level[other]).fold(f32::MIN, f32::max);
        let low = around.iter().map(|&other| level[other]).fold(f32::MAX, f32::min);
        let beside: Vec<usize> = [(0, -1), (-1, 0), (1, 0), (0, 1)]
            .iter()
            .map(|&(dx, dy)| at(x + dx, y + dy))
            .filter(|&other| status[other])
            .collect();
        let alive_near = beside.len() as f32;
        let alive_level = if beside.is_empty() {
            0.0
        } else {
            beside.iter().map(|&other| level[other]).sum::<f32>() / alive_near
        };
        let any_flagged = beside.iter().any(|&other| flag[other]);
        let all_flagged = beside.iter().all(|&other| flag[other]);
        let spread = (top - low) / 2.0 + alive_near.max(1.0).powi(2) - level[cell];
        let band = if alive_level >= 0.5 {
            1
        } else if any_flagged {
            2
        } else {
            3
        };
        let score = if status[cell] { 10.0 } else { spread };
        let new_status = status[cell] || all_flagged;
        assert!(
            near(lanes.column("total").and_then(|lane| lane.get(cell)), total),
            "total at {cell}"
        );
        assert!(
            near(lanes.column("score").and_then(|lane| lane.get(cell)), score),
            "score at {cell}"
        );
        assert_eq!(
            lanes.column("band").and_then(|lane| lane.get(cell)),
            Some(HostValue::I32(band)),
            "band at {cell}"
        );
        assert_eq!(
            lanes.column("status").and_then(|lane| lane.get(cell)),
            Some(HostValue::Category(u32::from(new_status))),
            "status at {cell}"
        );
    }
}

#[test]
fn the_values_sheet_honors_each_only_and_gives_0_for_a_summary_of_no_cell() {
    let file = lowered(VALUES);
    let side = 8u32;
    let cells = (side * side) as usize;
    let (lanes, status, _, level) = values_after_one_tick(&file, side);
    let at = |x: i64, y: i64| ((y.rem_euclid(side as i64)) * side as i64 + x.rem_euclid(side as i64)) as usize;
    // Cells whose Count with an `only` differs from the same Count without it, and cells with and without a
    // neighbour of level at least 0.8, so each `only` and each empty summary is met at least once.
    let (mut trimmed, mut empty, mut filled) = (0, 0, 0);
    for cell in 0..cells {
        let (x, y) = ((cell as u32 % side) as i64, (cell as u32 / side) as i64);
        let around: Vec<usize> = [(-1, -1), (0, -1), (1, -1), (-1, 0), (1, 0), (-1, 1), (0, 1), (1, 1)]
            .iter()
            .map(|&(dx, dy)| at(x + dx, y + dy))
            .collect();
        let alive_around = around.iter().filter(|&&other| status[other]).count();
        let bright = around
            .iter()
            .filter(|&&other| status[other] && level[other] >= 0.5)
            .count();
        let high: Vec<f32> = around
            .iter()
            .map(|&other| level[other])
            .filter(|&other| other >= 0.8)
            .collect();
        let (high_mean, high_top, high_low) = if high.is_empty() {
            (0.0, 0.0, 0.0)
        } else {
            (
                high.iter().sum::<f32>() / high.len() as f32,
                high.iter().copied().fold(f32::MIN, f32::max),
                high.iter().copied().fold(f32::MAX, f32::min),
            )
        };
        trimmed += usize::from(bright != alive_around);
        empty += usize::from(high.is_empty());
        filled += usize::from(!high.is_empty());
        assert_eq!(
            lanes.column("bright").and_then(|lane| lane.get(cell)),
            Some(HostValue::I32(bright as i32)),
            "bright at {cell}"
        );
        for (lane, expected) in [("high_mean", high_mean), ("high_top", high_top), ("high_low", high_low)] {
            let found = lanes.column(lane).and_then(|column| column.get(cell));
            assert!(
                matches!(found, Some(HostValue::F32(value)) if (value - expected).abs() < 1e-4),
                "{lane} at {cell}: {found:?}, not {expected}"
            );
        }
    }
    assert!(trimmed > 0, "the Count's `only` never leaves a cell out");
    assert!(
        empty > 0 && filled > 0,
        "{empty} cells see no high level, {filled} see one"
    );
}

#[test]
fn a_formula_of_arrows_or_past_the_depth_limit_is_refused_at_its_line() {
    for formula in ["length(l1)", &vec!["l1"; 20].join(" + ")] {
        let text = life_with_line(&format!(
            "[card.line.l5]\nkind = \"calculate\"\nname = \"Worked out\"\nformula = \"{formula}\""
        ));
        let errors = refusals(&text);
        assert_eq!(errors.len(), 1, "{formula}: {errors:?}");
        assert!(placed_at(&errors[0], &text).starts_with("[card.line.l5]"), "{formula}");
    }
}

#[test]
fn a_division_by_0_gives_0() {
    // Level is set to 1 / Alive neighbours, which is 0 for a cell with no live neighbour.
    let text = life_with_line("[card.line.l5]\nkind = \"calculate\"\nname = \"Inverse\"\nformula = \"1 / l1\"");
    let text = edited(&text, "[[parameter]]", &format!("{EXTRA_PROPERTIES}[[parameter]]"));
    let text = edited(
        &text,
        "[card.line.l5]",
        "[card.line.l6]\nkind = \"set\"\nproperty = \"level\"\nvalue = { result = \"l5\" }\n\n\
         [card.line.l6.condition]\nkind = \"always\"\n\n[card.line.l5]",
    );
    let text = edited(
        &text,
        "lines = [\"l1\", \"l2\", \"l3\", \"l5\"]",
        "lines = [\"l1\", \"l2\", \"l3\", \"l5\", \"l6\"]",
    );
    let text = edited(&text, "next_line = 6", "next_line = 7");
    let file = lowered(&text);
    assert!(GridProgram::compile(file.lowered()).is_ok(), "{text}");
    let evaluator = host_of(&file);
    let world = torus(32, 32);
    let mut lanes = evaluator.init(world, &[], Some(5)).expect("the start runs");
    let before = alive(&lanes, 32);
    let mut stream = 0;
    evaluator
        .run_pass(
            "tick_cell_tick",
            &mut lanes,
            &[],
            &RunContext::tick(Some(5), 0),
            &mut stream,
        )
        .expect("a tick runs");
    let mut lonely = 0;
    for (cell, level) in levels(&lanes).into_iter().enumerate() {
        let (x, y) = ((cell % 32) as i64, (cell / 32) as i64);
        let around = (-1..=1i64)
            .flat_map(|dy| (-1..=1i64).map(move |dx| (dx, dy)))
            .filter(|&(dx, dy)| (dx, dy) != (0, 0))
            .filter(|&(dx, dy)| before.contains(&((x + dx).rem_euclid(32) as u32, (y + dy).rem_euclid(32) as u32)))
            .count();
        let known = if around == 0 { 0.0 } else { 1.0 / around as f32 };
        lonely += usize::from(around == 0);
        assert!(
            (level - known).abs() < 1e-6,
            "cell {cell}: {level} for {around} live neighbours"
        );
    }
    assert!(lonely > 0, "no cell has 0 live neighbours");
}

/// A chain of divisions at the deepest formula a sheet allows lowers and passes the graph check, each divisor bound
/// apart from the quotient.
#[test]
fn a_chain_of_divisions_at_the_depth_limit_lowers() {
    // A reference is one level deep, and each division adds one.
    let divisions = crate::sheet::limits::MAX_FORMULA_DEPTH - 1;
    for formula in [
        vec!["l1"; divisions + 1].join(" / "),
        format!("{}l1{}", "l1 / (".repeat(divisions), ")".repeat(divisions)),
    ] {
        let text = life_with_line(&format!(
            "[card.line.l5]\nkind = \"calculate\"\nname = \"Worked out\"\nformula = \"{formula}\""
        ));
        let file = lowered(&text);
        let report = file.lowered().check();
        assert!(!report.has_errors(), "{formula}: {:#?}", report.diagnostics);
        assert!(GridProgram::compile(file.lowered()).is_ok(), "{formula}");
    }
}

#[test]
fn a_random_number_line_draws_between_its_values() {
    let text =
        life_with_line("[card.line.l5]\nkind = \"random_number\"\nname = \"Noise\"\nkey = 13\nlow = 2\nhigh = 3");
    let text = edited(&text, "[[parameter]]", &format!("{EXTRA_PROPERTIES}[[parameter]]"));
    let text = edited(
        &text,
        "[card.line.l5]",
        "[card.line.l6]\nkind = \"set\"\nproperty = \"level\"\nvalue = { result = \"l5\" }\n\n\
         [card.line.l6.condition]\nkind = \"always\"\n\n[card.line.l5]",
    );
    let text = edited(
        &text,
        "lines = [\"l1\", \"l2\", \"l3\", \"l5\"]",
        "lines = [\"l1\", \"l2\", \"l3\", \"l5\", \"l6\"]",
    );
    let text = edited(&text, "next_line = 6", "next_line = 7");
    let file = lowered(&text);
    let evaluator = host_of(&file);
    let world = torus(32, 32);
    let mut lanes = evaluator.init(world, &[], Some(3)).expect("the start runs");
    let mut stream = 0;
    evaluator
        .run_pass(
            "tick_cell_tick",
            &mut lanes,
            &[],
            &RunContext::tick(Some(3), 0),
            &mut stream,
        )
        .expect("a tick runs");
    let drawn = levels(&lanes);
    assert!(drawn.iter().all(|level| (2.0..3.0).contains(level)), "{drawn:?}");
    let mean = drawn.iter().sum::<f32>() / drawn.len() as f32;
    assert!((mean - 2.5).abs() < 0.05, "{mean}");
}
