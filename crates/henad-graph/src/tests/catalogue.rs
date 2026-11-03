//! The catalogue: its definitions are well formed, and every case reads and checks.

use std::collections::BTreeSet;

use super::case_files::case_file;
use crate::GraphFile;
use crate::catalogue::{CaseForm, ConfigPorts, KnownAnswer, NodeDef, definitions, lookup};

/// Kinds of milestone G1, as the plan's table lists them.
const G1_KINDS: [&str; 64] = [
    "ctx.index",
    "ctx.press_seed",
    "ctx.tick",
    "lane.get",
    "lane.set",
    "math.add",
    "math.sub",
    "math.mul",
    "math.div",
    "math.rem",
    "math.min",
    "math.max",
    "math.abs",
    "math.ceil",
    "math.trunc",
    "math.round",
    "math.exp",
    "math.log",
    "math.sin",
    "math.cos",
    "math.powi",
    "math.pow",
    "bits.xor",
    "bits.and",
    "bits.or",
    "bits.not",
    "bits.shl",
    "bits.shr",
    "cmp.eq",
    "cmp.ne",
    "cmp.lt",
    "cmp.le",
    "cmp.gt",
    "cmp.ge",
    "logic.and",
    "logic.or",
    "logic.xor",
    "logic.not",
    "logic.select",
    "logic.pick",
    "cat.switch",
    "cat.transition",
    "conv.to_f32",
    "conv.to_u32",
    "conv.to_i32",
    "conv.u32_to_cat",
    "conv.cat_to_u32",
    "rand.float",
    "rand.below",
    "rand.chance",
    "rng.threshold",
    "rng.hash",
    "rng.to_float",
    "rng.below",
    "rng.lane_seed",
    "stream.bits",
    "region.repeat",
    "region.put",
    "grid.count",
    "grid.life_like",
    "stat.count_where",
    "stat.count_by",
    "view.color",
    "view.ramp",
];

#[test]
fn the_catalogue_holds_every_kind_of_milestone_g1_and_no_other() {
    let listed: BTreeSet<&str> = G1_KINDS.into_iter().collect();
    let defined: BTreeSet<&str> = definitions().map(|def| def.id).collect();
    assert_eq!(listed, defined);
    assert_eq!(definitions().count(), defined.len(), "a kind is defined twice");
}

#[test]
fn every_definition_is_complete() {
    for def in definitions() {
        assert_eq!(def.since, 1, "{}", def.id);
        assert!(
            !def.label.is_empty() && !def.group.is_empty(),
            "{} needs a label and a group",
            def.id
        );
        assert!(def.semantics.ends_with('.'), "{} needs its written semantics", def.id);
        assert!(!def.cases.is_empty(), "{} has no known-answer case", def.id);
        let names: BTreeSet<&str> = def.cases.iter().map(|case| case.name).collect();
        assert_eq!(names.len(), def.cases.len(), "{} names two cases alike", def.id);
        assert_eq!(lookup(def.id, 1).map(|found| found.id), Some(def.id));
        assert!(lookup(def.id, 0).is_none());
    }
}

#[test]
fn no_fixed_port_name_is_a_prefix_of_another_or_of_a_made_port() {
    const MADE_PREFIXES: [&str; 5] = ["init", "case", "rule", "prev", "fired"];
    for def in definitions() {
        let fixed: Vec<&str> = def
            .inputs
            .iter()
            .map(|socket| socket.name)
            .chain(def.outputs.iter().map(|socket| socket.name))
            .chain(def.inner_outputs.iter().map(|socket| socket.name))
            .collect();
        for name in &fixed {
            assert!(!name.contains('.'), "{}: `{name}`", def.id);
            if def.config_ports != ConfigPorts::None {
                assert!(!MADE_PREFIXES.contains(name), "{}: `{name}` is a prefix", def.id);
            }
        }
    }
}

/// Returns each case of the catalogue with its definition.
fn every_case() -> impl Iterator<Item = (&'static NodeDef, &'static KnownAnswer)> {
    definitions().flat_map(|def| def.cases.iter().map(move |case| (def, case)))
}

#[test]
fn every_case_reads_and_checks_without_an_error() {
    for (def, case) in every_case() {
        let text = case_file(def, case);
        let file =
            GraphFile::parse(&text).unwrap_or_else(|error| panic!("{} '{}': {error}\n{text}", def.id, case.name));
        let report = file.check();
        let errors: Vec<String> = report.errors().map(ToString::to_string).collect();
        assert!(
            errors.is_empty(),
            "{} '{}':\n{}\n{text}",
            def.id,
            case.name,
            errors.join("\n")
        );
    }
}

#[test]
fn a_pass_case_runs_a_pass_named_case_and_names_only_its_lanes() {
    for (def, case) in every_case() {
        let CaseForm::Pass(pass) = &case.form else { continue };
        let file = GraphFile::parse(&case_file(def, case)).expect("the case reads");
        let document = file.document();
        assert!(
            document.passes.iter().any(|candidate| candidate.id == "case"),
            "{} '{}'",
            def.id,
            case.name
        );
        let cells = (pass.width * pass.height) as usize;
        for (lane, values) in pass.before.iter().chain(pass.after) {
            assert!(
                document.lanes.iter().any(|known| known.id == *lane),
                "{} '{}': {lane}",
                def.id,
                case.name
            );
            assert_eq!(values.len(), cells, "{} '{}': {lane}", def.id, case.name);
        }
        if !pass.colors.is_empty() {
            assert_eq!(pass.colors.len(), cells, "{} '{}'", def.id, case.name);
        }
    }
}
