//! Quick-add: the node kinds a body can take, filtered by the pass, its domain, a dragged wire's type and a query.
//!
//! A kind is *offered* to a pass when the pass's stage allows it, the backend that runs the pass runs it, and its
//! domain is the pass's own or none. A kind is offered to a function when it has no effect a function cannot hold and
//! its domain is the world's or none, and a function's input and output are offered to a function alone.
//!
//! A *wire end* is the dragged end of a wire dropped on the canvas. A kind fits an output's wire when one of its
//! inputs takes the output's type, and fits an input's wire when one of its outputs makes the input's type.

use henad_graph::catalogue::{Effect, NodeDef, SocketType, definitions, lookup};
use henad_graph::document::{Domain, GraphDocument, Pass, PassStage};
use henad_graph::node::{OP_FUNCTION_INPUT, OP_FUNCTION_OUTPUT};

use super::display::display_name;
use super::passes::topology_domain;
use henad_graph::types::ValueType;

/// Words from other modelling tools, each with the word of the catalogue it stands for.
const SYNONYMS: [(&str, &str); 12] = [
    ("patch", "grid"),
    ("patches", "grid"),
    ("cell", "grid"),
    ("neighbor", "grid"),
    ("neighbour", "grid"),
    ("turtle", "agent"),
    ("turtles", "agent"),
    ("random", "rand"),
    ("chance", "rand"),
    ("if", "select"),
    ("variable", "lane"),
    ("color", "view"),
];

/// Dragged end of a wire dropped on the canvas, with the type of its socket when the checker knows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WireEnd {
    /// The wire leaves an output, and the new node reads it.
    Output(Option<ValueType>),
    /// The wire enters an input, and the new node feeds it.
    Input(Option<ValueType>),
}

/// Returns the kinds `pass` of `document` takes, or a function of it when `pass` is `None`, that fit `wire` and match
/// `query`, in catalogue order, at the top of the body or, when `in_region` holds, in a region's body.
///
/// Every word of `query` has to appear in the kind's id, label, group, display name or category, or stand for a word
/// that does. A kind that only a region body takes, such as `region.put`, is offered in a region alone, and a kind a
/// region body refuses, such as `lane.set`, outside every region alone.
pub fn offered_kinds(
    document: &GraphDocument,
    pass: Option<&Pass>,
    wire: Option<&WireEnd>,
    query: &str,
    in_region: bool,
) -> Vec<&'static NodeDef> {
    let words: Vec<String> = query.split_whitespace().map(str::to_lowercase).collect();
    let domain = topology_domain(document.world.topology());
    definitions()
        .filter(|def| lookup(def.id, document.library).is_some_and(|newest| std::ptr::eq(newest, *def)))
        .filter(|def| match pass {
            Some(pass) => takes(def, pass, in_region) && !is_interface(def),
            None => function_takes(def, domain, in_region),
        })
        .filter(|def| wire.is_none_or(|wire| wire_port(def, wire).is_some()))
        .filter(|def| matches_words(def, &words))
        .collect()
}

/// Returns whether `pass` takes a node of the kind `def` at the top of its body, or in a region's body when
/// `in_region` holds.
pub fn takes(def: &NodeDef, pass: &Pass, in_region: bool) -> bool {
    let stage = def.contexts.allows(pass.stage);
    let backend = if pass.stage == PassStage::Init {
        def.backends.host
    } else {
        def.backends.wgsl
    };
    let domain = def.domain.is_none_or(|domain| Some(domain) == pass.domain);
    let place = if in_region {
        def.contexts.region
    } else {
        !def.has_effect(Effect::RegionSink)
    };
    stage && backend && domain && place
}

/// Returns whether `def` is a function's input or output, which only a function's body holds.
fn is_interface(def: &NodeDef) -> bool {
    def.id == OP_FUNCTION_INPUT || def.id == OP_FUNCTION_OUTPUT
}

/// Returns whether a function of a world whose elements are `domain` takes a node of the kind `def` at the top of its
/// body, or in a region's body when `in_region` holds.
pub fn function_takes(def: &NodeDef, domain: Domain, in_region: bool) -> bool {
    let pure = def.sink().is_none();
    let place = if in_region {
        def.contexts.region && !is_interface(def)
    } else {
        !def.has_effect(Effect::RegionSink)
    };
    pure && place && def.domain.is_none_or(|own| own == domain)
}

/// Returns the name of the first fixed socket of `def` that takes `wire`: an input for a wire that leaves an output,
/// an output for a wire that enters an input.
pub fn wire_port(def: &NodeDef, wire: &WireEnd) -> Option<&'static str> {
    match wire {
        WireEnd::Output(value_type) => def
            .inputs
            .iter()
            .find(|socket| fits(def, socket.value_type, value_type.as_ref()))
            .map(|socket| socket.name),
        WireEnd::Input(value_type) => def
            .outputs
            .iter()
            .find(|socket| fits(def, socket.value_type, value_type.as_ref()))
            .map(|socket| socket.name),
    }
}

/// Returns whether a socket of `def` typed `socket` can carry `value_type`. An unknown type fits every socket, and so
/// does a socket whose type its configuration sets.
fn fits(def: &NodeDef, socket: SocketType, value_type: Option<&ValueType>) -> bool {
    let Some(value_type) = value_type else {
        return true;
    };
    match socket {
        SocketType::Fixed(class) => class.holds(value_type),
        SocketType::Var => def.type_var.iter().any(|class| class.holds(value_type)),
        _ => true,
    }
}

/// Returns whether every one of `words` appears in the id, label or group of `def` or in its display name or category,
/// directly or through a synonym.
fn matches_words(def: &NodeDef, words: &[String]) -> bool {
    let (name, category) = display_name(def.id).map_or(("", ""), |display| (display.name, display.category));
    let haystack = format!("{} {} {} {name} {category}", def.id, def.label, def.group).to_lowercase();
    words.iter().all(|word| {
        haystack.contains(word.as_str())
            || SYNONYMS
                .iter()
                .any(|(synonym, meaning)| synonym == word && haystack.contains(meaning))
    })
}

#[cfg(test)]
mod tests {
    use henad_graph::GraphFile;
    use henad_graph::document::Pass;
    use henad_graph::types::ValueType;

    use super::{WireEnd, offered_kinds, wire_port};

    fn sir() -> GraphFile {
        let text = henad_graph::starters::starter("sir.toml").expect("a starter").text;
        GraphFile::parse(text).expect("the starter reads")
    }

    fn pass<'a>(file: &'a GraphFile, id: &str) -> &'a Pass {
        file.document()
            .passes
            .iter()
            .find(|pass| pass.id == id)
            .expect("the pass")
    }

    fn ids(file: &GraphFile, pass_id: &str, wire: Option<&WireEnd>, query: &str) -> Vec<&'static str> {
        offered_kinds(file.document(), Some(pass(file, pass_id)), wire, query, false)
            .into_iter()
            .map(|def| def.id)
            .collect()
    }

    #[test]
    fn each_stage_offers_the_kinds_it_allows() {
        let file = sir();
        let step = ids(&file, "step", None, "");
        let counts = ids(&file, "counts", None, "");
        let seed = ids(&file, "seed", None, "");
        assert!(step.contains(&"lane.set") && step.contains(&"rand.float"));
        assert!(!counts.contains(&"lane.set"), "a stats pass writes no lane");
        assert!(!counts.contains(&"rand.float"), "a stats pass draws nothing");
        assert!(counts.contains(&"stat.count_by"));
        assert!(!step.contains(&"stat.count_by"), "a tick pass reduces nothing");
        assert!(seed.contains(&"stream.bits"), "an init pass draws from its stream");
        assert!(!step.contains(&"stream.bits"), "a tick pass has no stream");
        assert!(!step.contains(&"region.put"), "the top of a body takes no put");
        assert!(step.contains(&"region.repeat"));
        let body = offered_kinds(file.document(), Some(pass(&file, "step")), None, "", true);
        let body: Vec<&str> = body.into_iter().map(|def| def.id).collect();
        assert!(body.contains(&"region.put"), "a region's body takes a put");
        assert!(!body.contains(&"lane.set"), "a region's body writes no lane");
        assert!(body.contains(&"region.repeat"), "a Repeat nests");
    }

    #[test]
    fn a_function_offers_pure_kinds_and_its_own_input_and_output() {
        let text = henad_graph::starters::starter("boids.toml").expect("a starter").text;
        let file = GraphFile::parse(text).expect("the starter reads");
        let offered: Vec<&str> = offered_kinds(file.document(), None, None, "", false)
            .into_iter()
            .map(|def| def.id)
            .collect();
        for kept in [
            "fn.input",
            "fn.output",
            "math.add",
            "rand.float",
            "region.repeat",
            "fn.call",
        ] {
            assert!(offered.contains(&kept), "a function takes {kept}");
        }
        for refused in [
            "lane.set",
            "stream.float",
            "counter.add",
            "stat.sum",
            "view.color",
            "region.put",
        ] {
            assert!(!offered.contains(&refused), "a function holds no {refused}");
        }
        let in_pass = ids(&file, "flock", None, "");
        assert!(!in_pass.contains(&"fn.input"), "a pass holds no function input");
        assert!(
            !offered.contains(&"grid.count"),
            "a function of agents takes no grid kind"
        );
    }

    #[test]
    fn a_wire_offers_the_kinds_whose_sockets_take_its_type() {
        let file = sir();
        let bools = ids(&file, "step", Some(&WireEnd::Output(Some(ValueType::Bool))), "");
        assert!(bools.contains(&"logic.and"));
        assert!(!bools.contains(&"math.sin"), "a sine reads no truth value");
        let floats = ids(&file, "step", Some(&WireEnd::Input(Some(ValueType::F32))), "");
        assert!(floats.contains(&"rand.float"));
        assert!(!floats.contains(&"cmp.lt"), "a comparison makes no float");
        let def = henad_graph::catalogue::lookup("cmp.lt", 1).expect("a kind");
        assert_eq!(wire_port(def, &WireEnd::Output(Some(ValueType::F32))), Some("a"));
    }

    #[test]
    fn a_query_matches_ids_labels_and_synonyms() {
        let file = sir();
        assert_eq!(ids(&file, "step", None, "life_like"), ["grid.life_like"]);
        let patches = ids(&file, "step", None, "patch count");
        assert!(patches.contains(&"grid.count"), "{patches:?}");
        assert!(ids(&file, "step", None, "no such kind").is_empty());
    }
}
