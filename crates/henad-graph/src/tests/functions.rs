//! Edits of functions: declarations and ports, making a function from nodes and ungrouping a call, importing a
//! function from another file, the report of a function body, and a Repeat inside a function.

use std::collections::BTreeSet;

use henad_core::explore::fingerprint::fnv1a64;

use super::support::{assert_checks, canonical_builder, node_id, sir_words_after};
use crate::GraphFile;
use crate::edit::{EditCommand, EditError, GraphBuilder, PortSide, Scope};
use crate::ids::NodeId;
use crate::node::{OP_FUNCTION_CALL, OP_FUNCTION_INPUT, OP_FUNCTION_OUTPUT};
use crate::types::ValueType;
use crate::value::{ConfigValue, InputValue, PortRef};
use crate::wgsl::{AgentProgram, GridProgram};

/// Returns the builder of the starter `name` read through its canonical text, with keys drawn from a fixed seed.
fn starter(name: &str) -> GraphBuilder {
    canonical_builder(crate::starters::starter(name).expect("a starter").text, 5)
}

/// Returns the links of every input of the pass `pass` that read node `node`.
fn links_to(builder: &GraphBuilder, pass: &str, node: NodeId) -> Vec<PortRef> {
    let pass = builder
        .document()
        .passes
        .iter()
        .find(|known| known.id == pass)
        .expect("the pass");
    pass.body
        .nodes
        .iter()
        .flat_map(|reader| reader.inputs.values())
        .filter_map(|value| match value {
            InputValue::Link(link) if link.node == node => Some(link.clone()),
            _ => None,
        })
        .collect()
}

/// Returns the id of every node of `builder`'s passes and functions.
fn node_ids(builder: &GraphBuilder) -> BTreeSet<NodeId> {
    let document = builder.document();
    let passes = document.passes.iter().map(|pass| &pass.body);
    let functions = document.functions.iter().map(|function| &function.body);
    passes
        .chain(functions)
        .flat_map(|body| body.nodes.iter().map(|node| node.id))
        .collect()
}

fn function_scope(id: &str) -> Scope {
    Scope::Function(id.to_owned())
}

#[test]
fn making_a_function_and_ungrouping_its_call_keep_what_the_pass_computes() {
    let mut builder = starter("sir.toml");
    let before = sir_words_after(&builder.to_file().expect("reads"), 2);
    let moved = [node_id(12), node_id(13), node_id(15), node_id(16), node_id(17)];
    let made = builder
        .apply(EditCommand::MakeFunction {
            scope: Scope::pass("step"),
            nodes: moved.to_vec(),
            id: "infects".to_owned(),
        })
        .expect("the function is made");
    let file = assert_checks(&builder);
    let call = made[0];
    let function = file
        .document()
        .functions
        .iter()
        .find(|function| function.id == "infects")
        .expect("the new function");
    let ports = |ports: &[crate::document::FunctionPort]| -> Vec<(String, ValueType)> {
        ports
            .iter()
            .map(|port| (port.port.clone(), port.value_type.clone()))
            .collect()
    };
    assert_eq!(
        ports(&function.inputs).len(),
        2,
        "n11 and n14 feed the moved nodes: {}",
        builder.to_toml()
    );
    assert_eq!(ports(&function.outputs), vec![("out".to_owned(), ValueType::Bool)]);
    for node in moved {
        assert!(function.body.node(node).is_some(), "{node} moved into the function");
    }
    let pass = &file.document().passes[1];
    assert_eq!(
        pass.body.node(call).map(|node| node.op.as_str()),
        Some(OP_FUNCTION_CALL)
    );
    assert_eq!(
        sir_words_after(&file, 2),
        before,
        "a function of nodes without draws computes the pass as before"
    );
    GridProgram::compile(&file).expect("the pass with a call compiles");

    let copies = builder
        .apply(EditCommand::Ungroup {
            scope: Scope::pass("step"),
            node: call,
        })
        .expect("the call is ungrouped");
    assert_eq!(
        copies.len(),
        moved.len(),
        "one copy per node but the inputs and outputs"
    );
    let file = assert_checks(&builder);
    assert_eq!(
        sir_words_after(&file, 2),
        before,
        "the copies compute the pass as before"
    );
    builder
        .apply(EditCommand::RemoveFunction("infects".to_owned()))
        .expect("a function nothing calls is removed");
    assert_checks(&builder);
}

#[test]
fn an_ungrouped_call_draws_what_the_call_drew() {
    let mut builder = starter("sir.toml");
    let made = builder
        .apply(EditCommand::MakeFunction {
            scope: Scope::pass("step"),
            nodes: vec![node_id(14), node_id(15), node_id(18)],
            id: "draws".to_owned(),
        })
        .expect("the function is made");
    let called = sir_words_after(&assert_checks(&builder), 2);
    builder
        .apply(EditCommand::Ungroup {
            scope: Scope::pass("step"),
            node: made[0],
        })
        .expect("the call is ungrouped");
    assert_eq!(
        sir_words_after(&assert_checks(&builder), 2),
        called,
        "the copy of the draw takes the key it had inside the call"
    );
}

#[test]
fn a_function_is_not_made_of_nodes_it_cannot_hold() {
    let mut builder = starter("sir.toml");
    let text = builder.to_toml();
    let cases = [
        (vec![node_id(19), node_id(20)], "a lane write"),
        (vec![node_id(16)], "no id"),
        (Vec::new(), "no node"),
    ];
    for (nodes, reason) in cases {
        let id = if reason == "no id" { "Infects" } else { "infects" };
        let refused = builder.apply(EditCommand::MakeFunction {
            scope: Scope::pass("step"),
            nodes,
            id: id.to_owned(),
        });
        assert!(matches!(refused, Err(EditError::Invalid(_))), "{reason}: {refused:?}");
    }
    let mut boids = starter("boids.toml");
    let refused = boids.apply(EditCommand::MakeFunction {
        scope: Scope::pass("flock"),
        nodes: vec![node_id(28)],
        id: "push_close".to_owned(),
    });
    assert!(
        matches!(&refused, Err(EditError::Invalid(message)) if message.contains("region outside")),
        "a put leaves its region: {refused:?}"
    );
    assert_eq!(builder.to_toml(), text, "a refused edit changes nothing");
}

#[test]
fn nodes_inside_a_radius_fold_make_a_function_called_inside_it() {
    let mut builder = starter("boids.toml");
    let made = builder
        .apply(EditCommand::MakeFunction {
            scope: Scope::pass("flock"),
            nodes: vec![node_id(27)],
            id: "away".to_owned(),
        })
        .expect("the function is made");
    let file = assert_checks(&builder);
    let pass = file
        .document()
        .passes
        .iter()
        .find(|pass| pass.id == "flock")
        .expect("flock");
    assert_eq!(pass.body.node(made[0]).and_then(|call| call.region), Some(node_id(25)));
    let call = pass.body.node(made[0]).expect("the call");
    assert_eq!(
        call.inputs.values().next(),
        Some(&InputValue::Link(PortRef {
            node: node_id(25),
            port: Some("delta".to_owned())
        })),
        "the inner output enters through the call"
    );
    AgentProgram::compile(&file).expect("the pass compiles");
}

#[test]
fn a_function_that_gains_a_second_output_names_the_bare_links_to_its_calls_first() {
    let mut builder = starter("boids.toml");
    let bare = links_to(&builder, "flock", node_id(49));
    assert!(
        !bare.is_empty() && bare.iter().all(|link| link.port.is_none()),
        "the canonical text links the only output bare: {bare:?}"
    );
    builder
        .apply(EditCommand::AddFunctionPort {
            function: "clamp_speed".to_owned(),
            side: PortSide::Output,
            port: "speed".to_owned(),
            value_type: ValueType::F32,
        })
        .expect("the output is added");
    let named = links_to(&builder, "flock", node_id(49));
    assert_eq!(named.len(), bare.len());
    assert!(
        named.iter().all(|link| link.port.as_deref() == Some("out")),
        "{named:?}"
    );
}

#[test]
fn renaming_a_function_and_its_ports_rewrites_its_calls() {
    let mut builder = starter("boids.toml");
    let commands = [
        EditCommand::RenameFunction {
            from: "clamp_speed".to_owned(),
            to: "limit_speed".to_owned(),
        },
        EditCommand::RenameFunctionPort {
            function: "limit_speed".to_owned(),
            side: PortSide::Input,
            from: "v".to_owned(),
            to: "velocity".to_owned(),
        },
        EditCommand::RenameFunctionPort {
            function: "limit_speed".to_owned(),
            side: PortSide::Output,
            from: "out".to_owned(),
            to: "clamped".to_owned(),
        },
        EditCommand::SetFunctionPortType {
            function: "limit_speed".to_owned(),
            side: PortSide::Input,
            port: "min".to_owned(),
            value_type: ValueType::F32,
        },
    ];
    for command in commands {
        builder.apply(command).expect("the edit runs");
    }
    let file = assert_checks(&builder);
    let text = builder.to_toml();
    assert!(text.contains("config.function = \"limit_speed\""), "{text}");
    assert!(text.contains("in.velocity = \"n35.out\""), "{text}");
    assert!(!text.contains("clamp_speed"), "{text}");
    AgentProgram::compile(&file).expect("the renamed function compiles");
    let referenced = [
        EditCommand::RemoveFunction("limit_speed".to_owned()),
        EditCommand::RemoveFunctionPort {
            function: "limit_speed".to_owned(),
            side: PortSide::Output,
            port: "clamped".to_owned(),
        },
        EditCommand::RemoveFunctionPort {
            function: "limit_speed".to_owned(),
            side: PortSide::Input,
            port: "velocity".to_owned(),
        },
    ];
    for command in referenced {
        let refused = builder.apply(command);
        assert!(matches!(refused, Err(EditError::Referenced { .. })), "{refused:?}");
    }
    builder
        .apply(EditCommand::AddFunctionPort {
            function: "limit_speed".to_owned(),
            side: PortSide::Input,
            port: "spare".to_owned(),
            value_type: ValueType::U32,
        })
        .expect("an input is added");
    builder
        .apply(EditCommand::RemoveFunctionPort {
            function: "limit_speed".to_owned(),
            side: PortSide::Input,
            port: "spare".to_owned(),
        })
        .expect("an input nothing names is removed");
    let duplicate = builder.apply(EditCommand::AddFunction("limit_speed".to_owned()));
    assert!(matches!(duplicate, Err(EditError::Invalid(_))), "{duplicate:?}");
}

#[test]
fn an_imported_function_comes_with_its_callees_fresh_ids_and_an_origin() {
    let ants_text = crate::starters::starter("ants.toml").expect("ants").text;
    let ants = GraphFile::parse(ants_text).expect("ants reads");
    let mut builder = starter("boids.toml");
    let before = node_ids(&builder);
    let made = builder
        .apply(EditCommand::ImportFunction {
            text: ants_text.to_owned(),
            source: "ants.toml".to_owned(),
            function: "move_if_passable".to_owned(),
            id: None,
        })
        .expect("the function is imported");
    assert!(
        made.iter().all(|node| !before.contains(node)),
        "every copy takes a fresh id"
    );
    let ids: Vec<&str> = builder
        .document()
        .functions
        .iter()
        .map(|function| function.id.as_str())
        .collect();
    for id in ["move_if_passable", "passable"] {
        assert!(ids.contains(&id), "{id} is imported: {ids:?}");
    }
    let errors: Vec<String> = builder
        .to_file()
        .expect("reads")
        .check()
        .errors()
        .map(|error| error.message.clone())
        .collect();
    assert!(
        !errors.is_empty()
            && errors
                .iter()
                .all(|error| error.contains("'sites'") || error.contains("'site'")),
        "an import copies no declaration, and the boids declare no field 'sites' and no category 'site': {errors:#?}"
    );
    let original = ants
        .document()
        .functions
        .iter()
        .find(|function| function.id == "move_if_passable")
        .expect("the original");
    let hash = format!(
        "{:016x}",
        fnv1a64(crate::writer::function_text(ants.document(), original).as_bytes())
    );
    let origin = builder.editor().function_origin("move_if_passable").expect("an origin");
    assert_eq!(
        (origin.name.as_str(), origin.hash.as_str()),
        ("ants.toml", hash.as_str())
    );
    let file = GraphFile::parse(&builder.to_toml()).expect("the text reads");
    assert_eq!(
        file.code(),
        GraphFile::from_document(builder.document(), None)
            .expect("reads")
            .code(),
        "the origin is outside the code identity"
    );
    assert_eq!(
        file.editor()
            .and_then(|editor| editor.function_origin("move_if_passable")),
        Some(origin)
    );

    let again = builder.apply(EditCommand::ImportFunction {
        text: ants_text.to_owned(),
        source: "ants.toml".to_owned(),
        function: "passable".to_owned(),
        id: None,
    });
    assert!(
        matches!(&again, Err(EditError::Invalid(message)) if message.contains("another name")),
        "{again:?}"
    );
    builder
        .apply(EditCommand::ImportFunction {
            text: ants_text.to_owned(),
            source: "ants.toml".to_owned(),
            function: "passable".to_owned(),
            id: Some("passable_2".to_owned()),
        })
        .expect("an import under another name");
    let broken = builder.apply(EditCommand::ImportFunction {
        text: "format = 1".to_owned(),
        source: "broken.toml".to_owned(),
        function: "site_xy".to_owned(),
        id: None,
    });
    assert!(matches!(broken, Err(EditError::Import(_))), "{broken:?}");
}

#[test]
fn an_import_is_refused_under_the_name_of_one_of_its_callees() {
    let ants_text = crate::starters::starter("ants.toml").expect("ants").text;
    let mut builder = starter("boids.toml");
    let clash = builder.apply(EditCommand::ImportFunction {
        text: ants_text.to_owned(),
        source: "ants.toml".to_owned(),
        function: "move_if_passable".to_owned(),
        id: Some("passable".to_owned()),
    });
    assert!(
        matches!(&clash, Err(EditError::Invalid(message)) if message.contains("another name")),
        "{clash:?}"
    );
    let ids: Vec<&str> = builder
        .document()
        .functions
        .iter()
        .map(|function| function.id.as_str())
        .collect();
    assert_eq!(ids, ["clamp_speed"], "a refused import adds no function");
}

#[test]
fn a_function_of_pure_arithmetic_imports_into_another_model_and_checks() {
    let boids_text = crate::starters::starter("boids.toml").expect("boids").text;
    let mut ants = starter("ants.toml");
    ants.apply(EditCommand::ImportFunction {
        text: boids_text.to_owned(),
        source: "boids.toml".to_owned(),
        function: "clamp_speed".to_owned(),
        id: None,
    })
    .expect("a function of pure arithmetic is imported");
    assert_checks(&ants);
}

#[test]
fn a_function_body_has_a_report_with_its_types_and_cost() {
    let builder = starter("boids.toml");
    let report = builder.to_file().expect("reads").check();
    let clamp = report
        .functions
        .iter()
        .find(|function| function.id == "clamp_speed")
        .expect("a report of clamp_speed");
    assert!(clamp.cost > 0);
    let sqrt = clamp.outputs.get(&node_id(105)).and_then(|ports| ports.get("out"));
    assert_eq!(sqrt.and_then(|port| port.value_type.clone()), Some(ValueType::F32));
    let input = clamp.outputs.get(&node_id(101)).and_then(|ports| ports.get("value"));
    assert_eq!(input.and_then(|port| port.value_type.clone()), Some(ValueType::Vec2F));
}

#[test]
fn nodes_of_a_function_wrap_in_a_repeat() {
    let mut builder = starter("boids.toml");
    let made = builder
        .apply(EditCommand::WrapInRepeat {
            scope: function_scope("clamp_speed"),
            nodes: vec![node_id(104), node_id(105)],
            bounds: None,
        })
        .expect("the nodes are wrapped");
    let file = assert_checks(&builder);
    let function = &file.document().functions[0];
    assert_eq!(
        function.body.node(node_id(105)).and_then(|node| node.region),
        Some(made[0])
    );
    AgentProgram::compile(&file).expect("the function with a Repeat compiles");
}

#[test]
fn an_empty_function_takes_ports_and_nodes() {
    let mut builder = starter("sir.toml");
    builder
        .apply(EditCommand::AddFunction("double".to_owned()))
        .expect("the function is added");
    for (side, port) in [(PortSide::Input, "a"), (PortSide::Output, "twice")] {
        builder
            .apply(EditCommand::AddFunctionPort {
                function: "double".to_owned(),
                side,
                port: port.to_owned(),
                value_type: ValueType::F32,
            })
            .expect("the port is added");
    }
    let scope = function_scope("double");
    let input = builder
        .add_node(
            &scope,
            OP_FUNCTION_INPUT,
            &[("port", ConfigValue::String("a".to_owned()))],
            &[],
        )
        .expect("the input");
    let sum = builder
        .add_node(
            &scope,
            "math.add",
            &[],
            &[
                (
                    "a",
                    InputValue::Link(PortRef {
                        node: input,
                        port: None,
                    }),
                ),
                (
                    "b",
                    InputValue::Link(PortRef {
                        node: input,
                        port: None,
                    }),
                ),
            ],
        )
        .expect("the sum");
    builder
        .add_node(
            &scope,
            OP_FUNCTION_OUTPUT,
            &[("port", ConfigValue::String("twice".to_owned()))],
            &[("value", InputValue::Link(PortRef { node: sum, port: None }))],
        )
        .expect("the output");
    assert_checks(&builder);
}
