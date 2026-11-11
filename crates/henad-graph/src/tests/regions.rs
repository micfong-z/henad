//! Edits of regions: accumulators, the output half's placement, wrapping nodes in a Repeat, and the bare links an
//! edit names before it can give them another meaning.

use super::support::{assert_checks, canonical_builder, link, node_id, sir_words_after, starter_builder};
use crate::GraphFile;
use crate::edit::{EditCommand, EditError, GraphBuilder, Scope};
use crate::ids::NodeId;
use crate::node::{Accumulator, Combine};
use crate::types::ValueType;
use crate::value::{ConfigValue, InputValue, Literal};
use crate::wgsl::GridProgram;

/// A grid with one Repeat `n1` of one `or` accumulator `hit`, which `n4` reads outside it.
const ONE_ACCUMULATOR: &str = r#"format = "henad-model"
format_version = 1
kind = "graph"
id = "node_regions"

[graph]
topology = "grid"
library = 1

[[target]]
backend = "gpu"

[grid]
width = 8
height = 8

[[lane]]
id = "hit"
domain = "cells"
type = "bool"

[[pass]]
id = "step"
stage = "tick"
kind = "map"
domain = "cells"

[[pass.node]]
id = "n1"
op = "region.repeat"
config.max = 4
config.acc = [{ name = "hit", type = "bool", combine = "or" }]
in.count = { u32 = 3 }

[[pass.node]]
id = "n2"
op = "cmp.eq"
in.a = "n1.k"
in.b = { u32 = 2 }
region = "n1"

[[pass.node]]
id = "n3"
op = "region.put"
config.acc = "hit"
in.value = "n2"
region = "n1"

[[pass.node]]
id = "n4"
op = "logic.not"
in.a = "n1.hit"

[[pass.node]]
id = "n5"
op = "lane.set"
config.lane = "hit"
in.value = "n4"
"#;

fn step() -> Scope {
    Scope::pass("step")
}

/// Returns the input `port` of node `node` of `builder`'s pass `pass`.
fn input(builder: &GraphBuilder, pass: &str, node: NodeId, port: &str) -> Option<InputValue> {
    let pass = builder.document().passes.iter().find(|known| known.id == pass)?;
    pass.body.node(node)?.inputs.get(port).cloned()
}

#[test]
fn a_second_accumulator_names_the_bare_links_to_the_region_first() {
    let mut builder = canonical_builder(ONE_ACCUMULATOR, 3);
    assert_eq!(
        input(&builder, "step", node_id(4), "a"),
        Some(link(node_id(1), None)),
        "the canonical text reads bare"
    );
    builder
        .apply(EditCommand::AddAccumulator {
            scope: step(),
            node: node_id(1),
            accumulator: Accumulator::new("total_hits", ValueType::U32, Combine::Count),
        })
        .expect("the accumulator is added");
    assert_eq!(
        input(&builder, "step", node_id(4), "a"),
        Some(link(node_id(1), Some("hit")))
    );
    assert!(builder.to_toml().contains("in.a = \"n1.hit\""), "{}", builder.to_toml());
    assert_checks(&builder);
}

#[test]
fn a_configuration_edit_names_the_bare_links_to_the_node_first() {
    let mut builder = canonical_builder(ONE_ACCUMULATOR, 3);
    let two = ConfigValue::Array(vec![
        Accumulator::new("hit", ValueType::Bool, Combine::Or).to_config(),
        Accumulator::new("miss", ValueType::Bool, Combine::Or).to_config(),
    ]);
    builder
        .apply(EditCommand::SetConfig {
            scope: step(),
            node: node_id(1),
            key: "acc".to_owned(),
            value: Some(two),
        })
        .expect("the configuration is set");
    assert_eq!(
        input(&builder, "step", node_id(4), "a"),
        Some(link(node_id(1), Some("hit")))
    );
    assert_checks(&builder);
}

#[test]
fn an_accumulator_rename_rewrites_its_input_its_put_and_its_readers() {
    let mut builder = canonical_builder(ONE_ACCUMULATOR, 3);
    let carry = |name: &str| EditCommand::AddAccumulator {
        scope: step(),
        node: node_id(1),
        accumulator: Accumulator::new(name, ValueType::U32, Combine::Carry),
    };
    builder.apply(carry("steps")).expect("the accumulator is added");
    builder
        .apply(EditCommand::SetLiteral {
            scope: step(),
            node: node_id(1),
            port: "init.steps".to_owned(),
            literal: Literal::U32(0),
        })
        .expect("the start is set");
    let next = builder
        .add_node(
            &step(),
            "math.add",
            &[],
            &[
                ("a", link(node_id(1), Some("prev.steps"))),
                ("b", InputValue::Literal(Literal::U32(1))),
            ],
        )
        .expect("the body node is added");
    let put = builder
        .add_node(
            &step(),
            "region.put",
            &[("acc", ConfigValue::String("steps".to_owned()))],
            &[("value", link(next, None))],
        )
        .expect("the put is added");
    for node in [next, put] {
        builder
            .apply(EditCommand::SetRegion {
                scope: step(),
                node,
                region: Some(node_id(1)),
            })
            .expect("the node moves in");
    }
    assert_checks(&builder);

    builder
        .apply(EditCommand::RenameAccumulator {
            scope: step(),
            node: node_id(1),
            from: "steps".to_owned(),
            to: "walked".to_owned(),
        })
        .expect("the accumulator is renamed");
    let text = builder.to_toml();
    assert!(!text.contains("steps"), "{text}");
    assert_eq!(
        input(&builder, "step", next, "a"),
        Some(link(node_id(1), Some("prev.walked")))
    );
    assert_eq!(
        input(&builder, "step", node_id(1), "init.walked"),
        Some(InputValue::Literal(Literal::U32(0)))
    );
    assert_checks(&builder);
    let refused = builder.apply(carry("walked"));
    assert!(matches!(refused, Err(EditError::Invalid(_))), "{refused:?}");
    let refused = builder.apply(carry("k"));
    assert!(
        matches!(refused, Err(EditError::Invalid(_))),
        "an inner output keeps its name: {refused:?}"
    );
}

#[test]
fn an_accumulator_that_something_names_is_not_removed() {
    let mut builder = canonical_builder(ONE_ACCUMULATOR, 3);
    let remove = |name: &str| EditCommand::RemoveAccumulator {
        scope: step(),
        node: node_id(1),
        name: name.to_owned(),
    };
    let refused = builder.apply(remove("hit"));
    let Err(EditError::Referenced { references, .. }) = refused else {
        panic!("{refused:?}");
    };
    assert_eq!(references, ["the put node n3", "node n4"]);
    builder
        .apply(EditCommand::AddAccumulator {
            scope: step(),
            node: node_id(1),
            accumulator: Accumulator::new("spare", ValueType::F32, Combine::Max),
        })
        .expect("the accumulator is added");
    builder
        .apply(EditCommand::SetAccumulator {
            scope: step(),
            node: node_id(1),
            accumulator: Accumulator::new("spare", ValueType::U32, Combine::Min),
        })
        .expect("the accumulator changes");
    assert!(
        builder
            .to_toml()
            .contains("{ name = \"spare\", type = \"u32\", combine = \"min\" }")
    );
    builder
        .apply(remove("spare"))
        .expect("an unnamed accumulator is removed");
    assert!(!builder.to_toml().contains("spare"));
    assert!(builder.apply(remove("spare")).is_err(), "it is gone");
}

#[test]
fn a_node_moved_into_a_region_it_reads_bare_names_the_port_first() {
    let mut builder = canonical_builder(ONE_ACCUMULATOR, 3);
    builder
        .apply(EditCommand::SetRegion {
            scope: step(),
            node: node_id(4),
            region: Some(node_id(1)),
        })
        .expect("the node moves in");
    assert_eq!(
        input(&builder, "step", node_id(4), "a"),
        Some(link(node_id(1), Some("hit"))),
        "inside the body a bare link to the region names nothing"
    );
}

#[test]
fn the_output_half_of_a_region_is_placed_in_the_editor_region() {
    let mut builder = canonical_builder(ONE_ACCUMULATOR, 3);
    let before = builder.to_file().expect("reads").code();
    builder
        .apply(EditCommand::MoveRegionOutput {
            node: node_id(1),
            pos: [300.4, 20.6],
        })
        .expect("the half moves");
    let text = builder.to_toml();
    assert!(text.contains("node.n1 = { output_pos = [300.0, 21.0] }"), "{text}");
    let file = GraphFile::parse(&text).expect("the text reads");
    assert_eq!(file.code(), before, "the placement is outside the code identity");
    assert_eq!(
        file.editor()
            .and_then(|editor| editor.node_layout(node_id(1)))
            .and_then(|layout| layout.output_pos),
        Some([300.0, 21.0])
    );
    assert_eq!(file.to_toml(), text, "the text is canonical");
    let refused = builder.apply(EditCommand::MoveRegionOutput {
        node: node_id(4),
        pos: [0.0, 0.0],
    });
    assert!(matches!(refused, Err(EditError::Invalid(_))), "{refused:?}");
}

#[test]
fn wrapping_nodes_in_a_repeat_keeps_what_the_pass_computes() {
    let mut builder = starter_builder("sir.toml", 1);
    let before = sir_words_after(&builder.to_file().expect("reads"), 1);
    let wrapped: Vec<NodeId> = (12..=18).map(node_id).collect();
    let made = builder
        .apply(EditCommand::WrapInRepeat {
            scope: step(),
            nodes: wrapped.clone(),
            bounds: None,
        })
        .expect("the nodes are wrapped");
    let file = assert_checks(&builder);
    let [repeat, puts @ ..] = made.as_slice() else {
        panic!("no Repeat: {made:?}")
    };
    assert_eq!(
        puts.len(),
        2,
        "n19 reads n17 and n18 from inside: {}",
        builder.to_toml()
    );
    let pass = &file.document().passes[1];
    for node in &wrapped {
        assert_eq!(pass.body.node(*node).and_then(|known| known.region), Some(*repeat));
    }
    let text = builder.to_toml();
    assert!(text.contains("combine = \"carry\""), "{text}");
    assert!(
        text.contains("in.count = { u32 = 1 }") && text.contains("config.max = 1"),
        "{text}"
    );
    assert!(
        builder
            .editor()
            .node_layout(*repeat)
            .is_some_and(|layout| layout.output_pos.is_some()),
        "the output half is placed"
    );
    let after = sir_words_after(&file, 1);
    assert_eq!(before, after, "a Repeat that runs once computes the pass as before");
    GridProgram::compile(&file).expect("the wrapped pass compiles");
}

#[test]
fn a_wrap_of_nodes_in_two_regions_or_of_no_node_is_refused() {
    let mut builder = canonical_builder(ONE_ACCUMULATOR, 3);
    let text = builder.to_toml();
    for nodes in [vec![node_id(2), node_id(4)], Vec::new()] {
        let refused = builder.apply(EditCommand::WrapInRepeat {
            scope: step(),
            nodes,
            bounds: None,
        });
        assert!(matches!(refused, Err(EditError::Invalid(_))), "{refused:?}");
    }
    assert_eq!(builder.to_toml(), text, "a refused wrap changes nothing");
}

#[test]
fn a_wrap_of_a_put_without_its_region_or_of_a_lane_write_is_refused() {
    let mut builder = canonical_builder(ONE_ACCUMULATOR, 3);
    let text = builder.to_toml();
    for (nodes, reason) in [
        (vec![node_id(2), node_id(3)], "feeds a region outside the selection"),
        (vec![node_id(4), node_id(5)], "does not belong in a region body"),
    ] {
        let refused = builder.apply(EditCommand::WrapInRepeat {
            scope: step(),
            nodes,
            bounds: None,
        });
        assert!(
            matches!(&refused, Err(EditError::Invalid(message)) if message.contains(reason)),
            "{refused:?}"
        );
    }
    assert_eq!(builder.to_toml(), text, "a refused wrap changes nothing");
}

#[test]
fn a_wrapped_region_nests_and_its_members_follow_it() {
    let mut builder = canonical_builder(ONE_ACCUMULATOR, 3);
    let made = builder
        .apply(EditCommand::WrapInRepeat {
            scope: step(),
            nodes: vec![node_id(1), node_id(3)],
            bounds: Some([[0.0, 0.0], [400.0, 200.0]]),
        })
        .expect("the region is wrapped");
    let outer = made[0];
    let body = &builder.document().passes[0].body;
    assert_eq!(
        body.region_path(node_id(3)),
        [node_id(1), outer],
        "a member of a wrapped region stays in it"
    );
    assert_eq!(
        input(&builder, "step", node_id(4), "a"),
        Some(link(outer, Some("hit"))),
        "the reader outside reads the new accumulator"
    );
    let layout = builder.editor().node_layout(outer).expect("placed");
    assert_eq!(layout.pos, [-280.0, 0.0]);
    assert_eq!(layout.output_pos, Some([740.0, 0.0]));
    assert_checks(&builder);
}

#[test]
fn a_wrap_that_would_nest_regions_past_the_depth_limit_is_refused() {
    let mut builder = canonical_builder(ONE_ACCUMULATOR, 3);
    let mut outer = node_id(1);
    for _ in 0..2 {
        let made = builder
            .apply(EditCommand::WrapInRepeat {
                scope: step(),
                nodes: vec![outer],
                bounds: None,
            })
            .expect("the region is wrapped");
        outer = made[0];
        assert_checks(&builder);
    }
    assert_eq!(builder.document().passes[0].body.region_path(node_id(1)).len(), 2);
    let text = builder.to_toml();
    let refused = builder.apply(EditCommand::WrapInRepeat {
        scope: step(),
        nodes: vec![outer],
        bounds: None,
    });
    assert!(
        matches!(&refused, Err(EditError::Invalid(message)) if message.contains("4 deep")),
        "{refused:?}"
    );
    let refused = builder.apply(EditCommand::WrapInRepeat {
        scope: step(),
        nodes: vec![node_id(2)],
        bounds: None,
    });
    assert!(
        matches!(&refused, Err(EditError::Invalid(message)) if message.contains("4 deep")),
        "a Repeat inside the deepest region: {refused:?}"
    );
    assert_eq!(builder.to_toml(), text, "a refused wrap changes nothing");
}

#[test]
fn a_wrap_inside_a_radius_fold_is_refused() {
    let mut builder = starter_builder("boids.toml", 1);
    let text = builder.to_toml();
    let refused = builder.apply(EditCommand::WrapInRepeat {
        scope: Scope::pass("flock"),
        nodes: vec![node_id(26)],
        bounds: None,
    });
    assert!(
        matches!(&refused, Err(EditError::Invalid(message)) if message.contains("cannot sit in the region n25")),
        "{refused:?}"
    );
    assert_eq!(builder.to_toml(), text, "a refused wrap changes nothing");
}
