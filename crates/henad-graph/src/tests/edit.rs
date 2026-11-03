//! Edit commands and the builder: ids and keys are minted, copies paste with fresh ones, renames rewrite every
//! reference, deletes with references are refused, and a refused command changes nothing.

use henad_core::authoring::primitives::space::Boundary;
use henad_core::params::{ParamDescriptor, ParamKind};

use crate::GraphFile;
use crate::document::{Category, Domain, GridWorld, Lane, Pass, PassStage, Stat, Variant, World};
use crate::edit::{EditCommand, EditError, GraphBuilder, Scope};
use crate::host::{HostEvaluator, HostValue, HostWorld};
use crate::ids::NodeId;
use crate::types::ValueType;
use crate::value::{ConfigValue, InputValue, Literal, MemberName, PortRef};

/// Returns a builder of the starter `name`, with keys drawn from a fixed seed.
fn starter(name: &str) -> GraphBuilder {
    let text = crate::starters::starter(name).expect("a starter").text;
    GraphBuilder::from_file(&GraphFile::parse(text).expect("the starter reads")).with_key_seed(1)
}

/// Returns node `number`'s id.
fn n(number: u32) -> NodeId {
    NodeId::new(number).expect("an id")
}

/// Returns a reference to output `port` of node `number`.
fn link(number: u32, port: Option<&str>) -> InputValue {
    InputValue::Link(PortRef {
        node: n(number),
        port: port.map(str::to_owned),
    })
}

/// Returns a category literal.
fn variant(category: &str, member: &str) -> InputValue {
    InputValue::Literal(Literal::Category(MemberName {
        owner: category.to_owned(),
        member: member.to_owned(),
    }))
}

/// Asserts that the builder's graph reads back and checks without an error.
#[track_caller]
fn assert_checks(builder: &GraphBuilder) -> GraphFile {
    let file = builder.to_file().expect("the graph reads back");
    let errors: Vec<String> = file.check().errors().map(|error| error.message.clone()).collect();
    assert!(errors.is_empty(), "{errors:#?}\n{}", builder.to_toml());
    file
}

#[test]
#[expect(clippy::too_many_lines, reason = "the test builds a whole model, node by node")]
fn a_builder_makes_a_graph_that_checks_and_runs() {
    let world = World::Grid(GridWorld {
        width: 8,
        height: 8,
        boundary: Boundary::Torus,
    });
    let mut builder = GraphBuilder::new("node_built", world).with_key_seed(2);
    let declarations = [
        EditCommand::AddCategory(Category::new(
            "life",
            vec![
                Variant::new("Dead", [0, 0, 0, 255]),
                Variant::new("Alive", [255, 255, 255, 255]),
            ],
        )),
        EditCommand::AddLane(Lane::new(
            "state",
            Domain::Cells,
            ValueType::Category("life".to_owned()),
        )),
        EditCommand::AddParam(ParamDescriptor::new(
            "density",
            "Density",
            ParamKind::F32 {
                min: 0.0,
                max: 1.0,
                default: 0.5,
                step: None,
            },
        )),
        EditCommand::AddPass(Pass::new("seed", PassStage::Init, Some(Domain::Cells))),
        EditCommand::AddPass(Pass::new("counts", PassStage::Stats, Some(Domain::Cells))),
        EditCommand::AddPass(Pass::new("step", PassStage::Tick, Some(Domain::Cells))),
    ];
    for command in declarations {
        builder.apply(command).expect("the declaration applies");
    }
    let ids: Vec<&str> = builder.document().passes.iter().map(|pass| pass.id.as_str()).collect();
    assert_eq!(ids, ["seed", "step", "counts"], "passes keep stack order");

    let seed = Scope::pass("seed");
    let lane = |id: &str| ("lane", ConfigValue::String(id.to_owned()));
    let threshold = builder
        .add_node(
            &seed,
            "rng.threshold",
            &[],
            &[("p", InputValue::Param("density".to_owned()))],
        )
        .expect("adds");
    let bits = builder.add_node(&seed, "stream.bits", &[], &[]).expect("adds");
    let below = builder
        .add_node(
            &seed,
            "cmp.lt",
            &[],
            &[("a", link(bits.number(), None)), ("b", link(threshold.number(), None))],
        )
        .expect("adds");
    let chosen = builder
        .add_node(
            &seed,
            "logic.select",
            &[],
            &[
                ("cond", link(below.number(), None)),
                ("if_true", variant("life", "Alive")),
                ("if_false", variant("life", "Dead")),
            ],
        )
        .expect("adds");
    builder
        .add_node(
            &seed,
            "lane.set",
            &[lane("state")],
            &[("value", link(chosen.number(), None))],
        )
        .expect("adds");
    assert_eq!(
        builder.document().passes[0].draw_order,
        vec![bits],
        "a stream draw joins the draw order"
    );

    let step = Scope::pass("step");
    let cell = builder
        .add_node(&step, "lane.get", &[lane("state")], &[])
        .expect("adds");
    let count = builder
        .add_node(
            &step,
            "grid.count",
            &[lane("state")],
            &[("value", variant("life", "Alive"))],
        )
        .expect("adds");
    let config = [
        ("category", ConfigValue::String("life".to_owned())),
        ("birth", ConfigValue::Array(vec![ConfigValue::Integer(3)])),
        (
            "survive",
            ConfigValue::Array(vec![ConfigValue::Integer(2), ConfigValue::Integer(3)]),
        ),
        ("dead", ConfigValue::String("Dead".to_owned())),
        ("alive", ConfigValue::String("Alive".to_owned())),
    ];
    let next = builder
        .add_node(
            &step,
            "grid.life_like",
            &config,
            &[
                ("cell", link(cell.number(), None)),
                ("count", link(count.number(), None)),
            ],
        )
        .expect("adds");
    builder
        .add_node(
            &step,
            "lane.set",
            &[lane("state")],
            &[("value", link(next.number(), None))],
        )
        .expect("adds");

    let counts = Scope::pass("counts");
    let read = builder
        .add_node(&counts, "lane.get", &[lane("state")], &[])
        .expect("adds");
    let counted = builder
        .add_node(
            &counts,
            "stat.count_by",
            &[("category", ConfigValue::String("life".to_owned()))],
            &[("value", link(read.number(), None))],
        )
        .expect("adds");
    let from = PortRef {
        node: counted,
        port: Some("Alive".to_owned()),
    };
    builder
        .apply(EditCommand::AddStat(Stat::new("Alive", from, [255, 255, 255, 255])))
        .expect("adds");

    let file = assert_checks(&builder);
    assert_eq!(builder.editor().next_id(), Some(counted.number() + 1));
    let evaluator = HostEvaluator::new(&file).expect("the graph runs");
    let lanes = evaluator
        .init(HostWorld::of(evaluator.document()), &[], Some(1))
        .expect("the init runs");
    let alive = (0..64)
        .filter(|cell| lanes.column("state").and_then(|column| column.get(*cell)) == Some(HostValue::Category(1)))
        .count();
    assert!((16..48).contains(&alive), "about half the cells live: {alive}");
}

#[test]
fn a_new_node_takes_the_next_id_and_a_fresh_key() {
    let mut builder = starter("sir.toml");
    let step = Scope::pass("step");
    let add = |builder: &mut GraphBuilder| {
        builder
            .apply(EditCommand::AddNode {
                scope: step.clone(),
                op: "rand.float".to_owned(),
                region: None,
                pos: Some([10.0, 20.0]),
            })
            .expect("adds")[0]
    };
    let first = add(&mut builder);
    let second = add(&mut builder);
    assert_eq!((first, second), (n(52), n(53)), "the editor's next id comes first");
    assert_eq!(builder.editor().next_id(), Some(54));
    let key = |builder: &GraphBuilder, id: NodeId| {
        builder.document().passes[1]
            .body
            .node(id)
            .and_then(|node| node.config.get("key"))
            .and_then(ConfigValue::as_integer)
            .expect("a key")
    };
    let keys = [key(&builder, n(14)), key(&builder, first), key(&builder, second)];
    assert!(
        keys[0] != keys[1] && keys[1] != keys[2] && keys[0] != keys[2],
        "{keys:?}"
    );
    let mut again = starter("sir.toml");
    let replayed = add(&mut again);
    assert_eq!(key(&again, replayed), keys[1], "one key seed mints one key");
    assert_eq!(
        builder.editor().node_layout(first).map(|layout| layout.pos),
        Some([10.0, 20.0])
    );
}

#[test]
fn a_paste_mints_fresh_ids_and_keys_and_keeps_wires_inside_the_copy() {
    let mut builder = starter("sir.toml");
    let step = Scope::pass("step");
    let text = builder.copy(&step, &[n(12), n(13), n(14)]).expect("copies");
    let made = builder
        .apply(EditCommand::Paste {
            scope: step.clone(),
            text: text.clone(),
            offset: [100.0, 0.0],
        })
        .expect("pastes");
    assert_eq!(made, vec![n(52), n(53), n(54)]);
    let body = &builder.document().passes[1].body;
    let powi = body.node(n(53)).expect("the pasted powi");
    assert_eq!(
        powi.inputs.get("base"),
        Some(&link(52, None)),
        "the wire inside the copy follows it"
    );
    assert!(
        !powi.inputs.contains_key("exponent"),
        "the wire from outside the copy is dropped"
    );
    let key = |id: NodeId| body.node(id).and_then(|node| node.config.get("key")).cloned();
    assert_ne!(key(n(54)), key(n(14)), "the pasted draw takes a fresh key");
    assert_eq!(
        builder.editor().node_layout(n(52)).map(|layout| layout.pos),
        builder
            .editor()
            .node_layout(n(12))
            .map(|layout| [layout.pos[0] + 100.0, layout.pos[1]])
    );
    builder.to_file().expect("the graph reads back");

    let mut vote = starter("vote.toml");
    let made = vote
        .apply(EditCommand::Paste {
            scope: Scope::pass("step"),
            text,
            offset: [0.0, 0.0],
        })
        .expect("pastes into another file");
    let sub = vote.document().passes[1].body.node(made[0]).expect("the pasted node");
    assert!(
        !sub.inputs.contains_key("b"),
        "a binding of a parameter the file lacks is dropped"
    );
}

#[test]
fn removing_a_node_drops_its_wires_and_its_placement() {
    let mut builder = starter("sir.toml");
    let step = Scope::pass("step");
    builder
        .apply(EditCommand::RemoveNode {
            scope: step.clone(),
            node: n(11),
        })
        .expect("removes");
    let body = &builder.document().passes[1].body;
    assert!(body.node(n(11)).is_none());
    assert!(!body.node(n(13)).expect("powi").inputs.contains_key("exponent"));
    assert!(builder.editor().node_layout(n(11)).is_none());
    assert!(!builder.to_toml().contains("\"n11\""), "no frame names the node");
    let refused = builder.apply(EditCommand::RemoveNode {
        scope: Scope::pass("counts"),
        node: n(31),
    });
    assert!(
        matches!(&refused, Err(EditError::Referenced { references, .. }) if references.len() == 3),
        "{refused:?}"
    );
}

#[test]
fn renames_rewrite_every_reference() {
    let mut builder = starter("sir.toml");
    let before = HostEvaluator::new(&builder.to_file().expect("reads"))
        .expect("runs")
        .init(HostWorld::of(builder.document()), &[], Some(4))
        .expect("inits");
    let renames = [
        EditCommand::RenameLane {
            from: "health".to_owned(),
            to: "status".to_owned(),
        },
        EditCommand::RenameVariant {
            category: "health".to_owned(),
            from: "I".to_owned(),
            to: "Infected".to_owned(),
        },
        EditCommand::RenameParam {
            from: "infection_rate".to_owned(),
            to: "beta".to_owned(),
        },
        EditCommand::RenamePass {
            from: "step".to_owned(),
            to: "advance".to_owned(),
        },
        EditCommand::RenameAction {
            from: "seed_outbreak".to_owned(),
            to: "outbreak_now".to_owned(),
        },
        EditCommand::RenameStat {
            from: "Infected".to_owned(),
            to: "Sick".to_owned(),
        },
    ];
    for rename in renames {
        builder.apply(rename).expect("renames");
    }
    let file = assert_checks(&builder);
    assert!(file.check().warnings().next().is_none(), "the renames leave no warning");
    let text = builder.to_toml();
    for gone in [
        "config.lane = \"health\"",
        "health.I\"",
        "infection_rate",
        "pass.step",
        "seed_outbreak",
        "n31.I\"",
    ] {
        assert!(!text.contains(gone), "'{gone}' is still written");
    }
    for written in [
        "\"status\"",
        "health.Infected",
        "\"beta\"",
        "pass.advance",
        "n31.Infected",
    ] {
        assert!(text.contains(written), "'{written}' is not written");
    }
    let after = HostEvaluator::new(&file)
        .expect("runs")
        .init(HostWorld::of(file.document()), &[], Some(4))
        .expect("inits");
    assert_eq!(
        after.columns()[0].words(),
        before.columns()[0].words(),
        "the meaning stays"
    );
}

#[test]
fn a_delete_with_references_is_refused_with_their_list() {
    let mut builder = starter("sir.toml");
    let text = builder.to_toml();
    let refusals = [
        (EditCommand::RemoveLane("health".to_owned()), 7),
        (EditCommand::RemoveParam("initial_infected_pct".to_owned()), 2),
        (
            EditCommand::RemoveVariant {
                category: "health".to_owned(),
                name: "S".to_owned(),
            },
            5,
        ),
        (EditCommand::RemoveAction("seed_outbreak".to_owned()), 1),
        (EditCommand::RemovePass("counts".to_owned()), 3),
    ];
    for (command, count) in refusals {
        let refused = builder.apply(command.clone());
        assert!(
            matches!(&refused, Err(EditError::Referenced { references, .. }) if references.len() == count),
            "{command:?}: {refused:?}"
        );
    }
    assert_eq!(builder.to_toml(), text, "a refused command changes nothing");
    builder
        .apply(EditCommand::RemoveStat("Recovered".to_owned()))
        .expect("removes");
    builder
        .apply(EditCommand::RemovePass("outbreak".to_owned()))
        .expect("removes");
    builder
        .apply(EditCommand::RemoveAction("seed_outbreak".to_owned()))
        .expect("removes");
    assert!(
        !builder.to_toml().contains("pass.outbreak"),
        "the canvas goes with its pass"
    );
    assert_checks(&builder);
}

#[test]
fn a_structural_mistake_is_refused_and_changes_nothing() {
    let mut builder = starter("sir.toml");
    let text = builder.to_toml();
    let step = Scope::pass("step");
    let mistakes = [
        EditCommand::SetLiteral {
            scope: step.clone(),
            node: n(14),
            port: "max".to_owned(),
            literal: Literal::F32(f32::NAN),
        },
        EditCommand::BindParam {
            scope: step.clone(),
            node: n(14),
            port: "max".to_owned(),
            param: "nothing".to_owned(),
        },
        EditCommand::SetRegion {
            scope: step.clone(),
            node: n(14),
            region: Some(n(13)),
        },
        EditCommand::Connect {
            scope: step.clone(),
            node: n(15),
            port: "a".to_owned(),
            source: PortRef {
                node: n(42),
                port: None,
            },
        },
        EditCommand::Move {
            node: n(999),
            pos: [0.0, 0.0],
        },
        EditCommand::AddNode {
            scope: step.clone(),
            op: "math.nothing".to_owned(),
            region: None,
            pos: None,
        },
        EditCommand::AddLane(Lane::new("infection_rate", Domain::Cells, ValueType::U32)),
        EditCommand::RenamePass {
            from: "step".to_owned(),
            to: "seed".to_owned(),
        },
    ];
    for mistake in mistakes {
        assert!(builder.apply(mistake.clone()).is_err(), "{mistake:?} applies");
    }
    assert_eq!(builder.to_toml(), text);
}

#[test]
fn a_node_moves_into_a_region_and_out_again() {
    let mut builder = starter("sir.toml");
    let step = Scope::pass("step");
    let repeat = builder
        .add_node(
            &step,
            "region.repeat",
            &[("max", ConfigValue::Integer(4))],
            &[("count", InputValue::Literal(Literal::U32(2)))],
        )
        .expect("adds");
    let inner = builder.add_node(&step, "math.add", &[], &[]).expect("adds");
    let into = |region| EditCommand::SetRegion {
        scope: step.clone(),
        node: inner,
        region,
    };
    builder.apply(into(Some(repeat))).expect("moves in");
    assert_eq!(
        builder.document().passes[1]
            .body
            .node(inner)
            .and_then(|node| node.region),
        Some(repeat)
    );
    assert!(
        builder
            .apply(EditCommand::SetRegion {
                scope: step.clone(),
                node: repeat,
                region: Some(repeat),
            })
            .is_err(),
        "a region cannot hold itself"
    );
    builder.apply(into(None)).expect("moves out");
    builder.apply(into(Some(repeat))).expect("moves in");
    builder
        .apply(EditCommand::RemoveNode {
            scope: step.clone(),
            node: repeat,
        })
        .expect("removes");
    assert_eq!(
        builder.document().passes[1]
            .body
            .node(inner)
            .and_then(|node| node.region),
        None,
        "a removed region's members move out"
    );
}

#[test]
fn passes_move_within_their_stage() {
    let mut builder = starter("sir.toml");
    builder
        .apply(EditCommand::AddPass(Pass::new(
            "later",
            PassStage::Init,
            Some(Domain::Cells),
        )))
        .expect("adds");
    let order = |builder: &GraphBuilder| -> Vec<String> {
        builder.document().passes.iter().map(|pass| pass.id.clone()).collect()
    };
    assert_eq!(order(&builder), ["seed", "later", "step", "counts", "outbreak"]);
    builder
        .apply(EditCommand::MovePass {
            id: "later".to_owned(),
            index: 0,
        })
        .expect("moves");
    assert_eq!(order(&builder), ["later", "seed", "step", "counts", "outbreak"]);
    builder
        .apply(EditCommand::MovePass {
            id: "later".to_owned(),
            index: 9,
        })
        .expect("moves");
    assert_eq!(order(&builder), ["seed", "later", "step", "counts", "outbreak"]);
}

#[test]
fn a_frame_and_a_move_change_the_editor_region_alone() {
    let mut builder = starter("vote.toml");
    let code = builder.to_file().expect("reads").code();
    builder
        .apply(EditCommand::Frame {
            scope: Scope::pass("step"),
            title: "Count".to_owned(),
            color: "blue".to_owned(),
            members: vec![n(10)],
        })
        .expect("frames");
    builder
        .apply(EditCommand::Move {
            node: n(10),
            pos: [5.0, 6.0],
        })
        .expect("moves");
    let file = builder.to_file().expect("reads");
    assert_eq!(file.code(), code, "the code identity stays");
    assert!(file.to_toml().contains("title = \"Count\""));
}

/// A graph whose category `kind` holds one variant, counted by `n21`, which a stat reads.
const ONE_VARIANT: &str = r#"format = "henad-model"
format_version = 1
kind = "graph"
id = "node_one_variant"

[graph]
topology = "grid"
library = 1

[[target]]
backend = "gpu"

[grid]
width = 4
height = 4

[[category]]
id = "kind"
variants = [{ name = "Only", color = [1, 2, 3] }]

[[lane]]
id = "state"
domain = "cells"
type = "cat:kind"

[[stat]]
label = "Only"
from = "n21.Only"
color = [1, 2, 3]

[[pass]]
id = "step"
stage = "tick"
kind = "map"
domain = "cells"

[[pass]]
id = "counts"
stage = "stats"
kind = "map"
domain = "cells"

[[pass.node]]
id = "n20"
op = "lane.get"
config.lane = "state"

[[pass.node]]
id = "n21"
op = "stat.count_by"
config.category = "kind"
in.value = "n20"
"#;

#[test]
fn a_bare_link_to_a_counter_of_one_variant_is_named_before_a_variant_edit() {
    let canonical = GraphFile::parse(ONE_VARIANT).expect("the text reads").to_toml();
    assert!(canonical.contains("from = \"n21\"\n"), "{canonical}");
    let file = GraphFile::parse(&canonical).expect("the canonical text reads");
    let mut builder = GraphBuilder::from_file(&file);
    let refused = builder.apply(EditCommand::RemoveVariant {
        category: "kind".to_owned(),
        name: "Only".to_owned(),
    });
    assert!(matches!(refused, Err(EditError::Referenced { .. })), "{refused:?}");
    builder
        .apply(EditCommand::AddVariant {
            category: "kind".to_owned(),
            variant: Variant::new("Other", [4, 5, 6, 255]),
        })
        .expect("the variant is added");
    let file = assert_checks(&builder);
    assert_eq!(
        file.document().stats[0].from,
        PortRef {
            node: n(21),
            port: Some("Only".to_owned()),
        }
    );
}
