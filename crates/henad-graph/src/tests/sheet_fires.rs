//! Fire counts of sheets: the probes of each change line, compiled beside the lowered graph and counted on the host.

use std::collections::BTreeMap;

use henad_core::authoring::primitives::space::Boundary;
use henad_core::params::ParamValue;

use super::sheet_document::{sheet_text, sheets};
use crate::document::{PassStage, World};
use crate::host::{HostEvaluator, HostLanes, HostValue, HostWorld, RunContext};
use crate::sheet::SheetFile;
use crate::sheet::document::{CardRuns, LineKind};
use crate::sheet::relations::CardRelations;
use crate::sheet::sentence::{fire_count, fire_count_description};
use crate::wgsl::{AgentProgram, GridProgram, Probes};

/// Returns the sheet of `text`, read, checked and lowered.
fn sheet(text: &str) -> SheetFile {
    SheetFile::parse(text).unwrap_or_else(|error| panic!("{error}"))
}

/// Returns the wrapping grid `side` cells wide that the host runs.
fn grid(side: u32) -> HostWorld {
    HostWorld::Grid {
        width: side,
        height: side,
        boundary: Boundary::Torus,
    }
}

/// Runs every tick pass of `evaluator` once over `lanes` as tick `tick`, and returns the count of each probe of
/// `probes`.
fn counted_tick(
    evaluator: &HostEvaluator,
    lanes: &mut HostLanes,
    params: &[ParamValue],
    seed: Option<u64>,
    tick: u32,
    probes: &Probes,
) -> Vec<u32> {
    let mut counts = vec![0; probes.len()];
    let mut stream = 0;
    let ticks: Vec<String> = evaluator
        .document()
        .passes
        .iter()
        .filter(|pass| pass.stage == PassStage::Tick)
        .map(|pass| pass.id.clone())
        .collect();
    for pass in ticks {
        evaluator
            .run_pass_counting(
                &pass,
                lanes,
                params,
                &RunContext::tick(seed, tick),
                &mut stream,
                probes,
                &mut counts,
            )
            .unwrap_or_else(|error| panic!("{pass}: {error}"));
    }
    counts
}

/// Every sheet compiles with the probes of its change lines, and each probe counts for a change line of the card that
/// runs each tick.
#[test]
fn every_sheet_compiles_with_the_probes_of_its_change_lines() {
    for (name, text) in sheets() {
        let file = sheet(text);
        let fires = file.fire_lines();
        let compiled = match file.lowered().document().world {
            World::Agents(_) => {
                AgentProgram::compile_with_probes_and_fault_names(file.lowered(), fires.probes(), file.fault_names())
                    .map(|_| ())
            }
            _ => GridProgram::compile_with_probes(file.lowered(), fires.probes()).map(|_| ()),
        };
        assert!(compiled.is_ok(), "{name}: {compiled:?}");
        assert_eq!(fires.probes().len(), fires.lines().len(), "{name}");
        for line in fires.lines() {
            let (card, found) = file
                .document()
                .line(line)
                .unwrap_or_else(|| panic!("{name}: no line {line}"));
            assert_eq!(file.document().cards[card].runs, CardRuns::EachTick, "{name}: {line}");
            assert!(
                matches!(
                    found.kind,
                    LineKind::Become
                        | LineKind::Set
                        | LineKind::Steer
                        | LineKind::KeepBetween
                        | LineKind::Move
                        | LineKind::Leave
                        | LineKind::Record
                ),
                "{name}: {line} is a {:?}",
                found.kind
            );
        }
    }
}

/// The starters count each change line with a probe, a Become from any state through a row of each state, and leave
/// out the button's lines, the lines that always apply with no row and a Set that reads "otherwise".
#[test]
fn the_starters_count_their_change_lines() {
    let lines = |name: &str| sheet(sheet_text(name)).fire_lines().lines().to_vec();
    assert_eq!(lines("game_of_life.toml"), ["l2", "l3"]);
    assert_eq!(lines("sir.toml"), ["l2", "l3"]);
    assert_eq!(lines("vote.toml"), ["l2", "l2", "l3", "l3"]);
    assert_eq!(lines("forest_fire.toml"), ["l2", "l3", "l4", "l5"]);
    assert_eq!(lines("sird.toml"), ["l2", "l3", "l4"]);
    assert!(lines("boids.toml").is_empty(), "every boids change always applies");
    let ants = sheet(sheet_text("ants.toml"));
    let ants_lines = ants.fire_lines().lines();
    for (id, kind) in [
        ("l10", LineKind::Become),
        ("l13", LineKind::Become),
        ("l3", LineKind::Leave),
    ] {
        assert!(ants_lines.iter().any(|line| line == id), "ants {id}: {ants_lines:?}");
        assert_eq!(ants.document().line(id).map(|(_, line)| line.kind), Some(kind));
    }
    // "otherwise (when lines 11 and 14 do not apply) set Reward held to 0, always" applies where both of its higher
    // Sets do not, which no output of the graph holds.
    let mut overridden: Vec<&str> = Vec::new();
    for card in &ants.document().cards {
        let relations = CardRelations::new(ants.document(), card);
        overridden.extend(relations.order().iter().copied().filter(|id| {
            relations.line(id).is_some_and(|line| line.kind == LineKind::Set) && !relations.otherwise_of(id).is_empty()
        }));
    }
    assert!(!overridden.is_empty(), "the ants hold an otherwise Set");
    for id in overridden {
        assert!(
            !ants_lines.iter().any(|line| line == id),
            "ants {id} reads otherwise: {ants_lines:?}"
        );
    }
}

/// A change inside a Repeat has no count, even when its condition is a node outside the Repeat's region: a Move by
/// with a blank condition inside a Repeat inside a When takes the When's condition.
#[test]
fn a_change_inside_a_repeat_has_no_count() {
    let text = sheet_text("random_walk.toml");
    let wrapped = [
        ("next_line = 3\n", "next_line = 4\n"),
        ("lines = [\"l1\"]\n", "lines = [\"l3\"]\n"),
        (
            "# Repeat Steps per tick times:\n",
            "[card.line.l3]\nkind = \"when\"\nlines = [\"l1\"]\n\n[card.line.l3.condition]\nkind = \"compare\"\n\
             test = \"at_least\"\nleft = { param = \"step_size\" }\nright = 1\n\n",
        ),
    ]
    .iter()
    .fold(text.to_owned(), |text, (old, new)| {
        assert!(text.contains(old), "the walk holds {old:?}");
        text.replacen(old, new, 1)
    });
    let file = sheet(&wrapped);
    assert_eq!(
        file.document().line("l2").map(|(_, line)| line.kind),
        Some(LineKind::Move)
    );
    assert!(
        file.fire_lines().is_empty(),
        "the Move by inside the Repeat: {:?}",
        file.fire_lines().lines()
    );
}

/// A blinker planted in the tick-0 lanes gives 2 births and 2 deaths each tick: line 2 counts 2 and line 3 counts 2.
#[test]
fn the_life_sheet_blinker_fires_lines_2_and_3_twice_each_tick() {
    let file = sheet(sheet_text("game_of_life.toml"));
    let evaluator = HostEvaluator::new(file.lowered()).expect("the host runs the lowered graph");
    let side = 32;
    let mut lanes = HostLanes::new(evaluator.document(), grid(side)).expect("lanes");
    let status = lanes.column_mut("status").expect("a status lane");
    for (x, y) in [(15, 14), (15, 15), (15, 16)] {
        assert!(status.set((y * side + x) as usize, HostValue::Category(1)));
    }
    let fires = file.fire_lines();
    let params = [ParamValue::F32(0.3)];
    for tick in 0..6 {
        let counts = counted_tick(&evaluator, &mut lanes, &params, Some(1), tick, fires.probes());
        assert_eq!(
            fires.counts_by_line(&counts),
            BTreeMap::from([("l2".to_owned(), 2), ("l3".to_owned(), 2)]),
            "tick {}",
            tick + 1
        );
    }
}

/// The SIRD sheet's competing lines 3 and 4 count separately: each counts the cells that went from Infected to its
/// own state in the tick, every tick for 30 ticks.
#[test]
fn the_sird_competing_lines_count_separately() {
    let file = sheet(sheet_text("sird.toml"));
    let evaluator = HostEvaluator::new(file.lowered()).expect("the host runs the lowered graph");
    let params = [
        ParamValue::F32(1.0),
        ParamValue::F32(0.3),
        ParamValue::F32(0.2),
        ParamValue::F32(0.1),
        ParamValue::F32(0.2),
    ];
    let seed = Some(4);
    let mut lanes = evaluator.init(grid(64), &params, seed).expect("the start runs");
    let fires = file.fire_lines();
    let health = |lanes: &HostLanes| lanes.column("health").expect("a health lane").words().to_vec();
    let (mut recovered, mut died) = (0, 0);
    for tick in 0..30 {
        let before = health(&lanes);
        let counts = fires.counts_by_line(&counted_tick(
            &evaluator,
            &mut lanes,
            &params,
            seed,
            tick,
            fires.probes(),
        ));
        let after = health(&lanes);
        let went = |from: u32, to: u32| {
            before
                .iter()
                .zip(&after)
                .filter(|&(&old, &new)| old == from && new == to)
                .count() as u64
        };
        assert_eq!(counts.get("l2").copied(), Some(went(0, 1)), "tick {tick}: infected");
        assert_eq!(counts.get("l3").copied(), Some(went(1, 2)), "tick {tick}: recovered");
        assert_eq!(counts.get("l4").copied(), Some(went(1, 3)), "tick {tick}: died");
        recovered += went(1, 2);
        died += went(1, 3);
    }
    assert!(recovered > 0 && died > 0, "{recovered} recovered, {died} died");
    assert!(
        recovered > died,
        "recovery runs at twice the death rate: {recovered}, {died}"
    );
}

/// The probes leave the sheet's text and the lowered graph's code alone, and change only the shaders.
#[test]
fn probes_leave_the_sheet_code_and_the_lowered_text_alone() {
    for name in ["game_of_life.toml", "forest_fire.toml", "sird.toml"] {
        let text = sheet_text(name);
        let file = sheet(text);
        let plain = GridProgram::compile(file.lowered()).unwrap_or_else(|problems| panic!("{problems:?}"));
        let probed = GridProgram::compile_with_probes(file.lowered(), file.fire_lines().probes())
            .unwrap_or_else(|problems| panic!("{problems:?}"));
        assert_eq!(probed.entry().source(), plain.entry().source(), "{name}");
        assert_eq!(probed.entry().source().code(), Some(&file.code()), "{name}");
        let again = sheet(text);
        assert_eq!(again.lowered().to_toml(), file.lowered().to_toml(), "{name}");
        assert_eq!(again.to_toml(), file.to_toml(), "{name}");
        let sources = |program: &GridProgram| -> Vec<String> {
            program
                .shaders()
                .iter()
                .map(|shader| shader.text().to_owned())
                .collect()
        };
        assert_ne!(sources(&probed), sources(&plain), "{name}: the step counts its probes");
    }
}

/// The counts of a line's rows add up, and counts of another length than the probes give no count.
#[test]
fn counts_by_line_adds_the_rows_of_a_line_and_drops_counts_of_another_length() {
    let file = sheet(sheet_text("vote.toml"));
    let fires = file.fire_lines();
    assert_eq!(
        fires.counts_by_line(&[1, 2, 3, 4]),
        BTreeMap::from([("l2".to_owned(), 3), ("l3".to_owned(), 7)])
    );
    assert!(fires.counts_by_line(&[1, 2, 3]).is_empty());
    assert!(fires.counts_by_line(&[]).is_empty());
}

/// The count beside a line groups its digits, and its description names the line, the things and the tick.
#[test]
fn a_fire_count_reads_as_its_number_and_its_description() {
    assert_eq!(fire_count(30_877), "30,877");
    assert_eq!(fire_count(2), "2");
    assert_eq!(
        fire_count_description("Line 2", 2, "cell", 4),
        "Line 2 applied to 2 cells in tick 4."
    );
    assert_eq!(
        fire_count_description("Line 3", 1, "cell", 12),
        "Line 3 applied to 1 cell in tick 12."
    );
    assert_eq!(
        fire_count_description("Line 10", 1_200, "ant", 7),
        "Line 10 applied to 1,200 ants in tick 7."
    );
}
