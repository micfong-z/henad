//! Bodies of a file as the checker reads them: positions by id, the region tree and each node's definition.

use std::collections::BTreeMap;
use std::ops::Range;

use henad_core::diagnostic::CodeDiagnostic;

use super::Checker;
use crate::catalogue::{NodeDef, lookup};
use crate::document::{Function, GraphDocument, Pass};
use crate::file::{BodyPlaces, NodePlaces};
use crate::ids::NodeId;
use crate::limits::MAX_REGION_DEPTH;
use crate::node::Node;
use crate::normal::Origin;

/// Every body of a file.
pub(super) struct BodyViews<'a> {
    /// One view per pass, in stack order.
    pub passes: Vec<BodyView<'a>>,
    /// One view per function, in file order.
    pub functions: Vec<BodyView<'a>>,
}

impl<'a> BodyViews<'a> {
    /// Returns the views of the checker's file, reporting a broken region tree or an unknown kind once per node.
    pub fn new(checker: &mut Checker<'a>) -> Self {
        let document = checker.file.document();
        let places = checker.file.places();
        let passes = document
            .passes
            .iter()
            .zip(&places.passes)
            .map(|(pass, places)| BodyView::new(checker, document, places, Owner::Pass(pass)))
            .collect();
        let functions = document
            .functions
            .iter()
            .zip(&places.functions)
            .map(|(function, places)| BodyView::new(checker, document, places, Owner::Function(function)))
            .collect();
        Self { passes, functions }
    }
}

/// Table a body belongs to.
#[derive(Clone, Copy)]
pub(super) enum Owner<'a> {
    Pass(&'a Pass),
    Function(&'a Function),
}

/// One body of a file.
pub(super) struct BodyView<'a> {
    pub document: &'a GraphDocument,
    pub owner: Owner<'a>,
    pub nodes: &'a [Node],
    pub places: &'a BodyPlaces,
    /// Position of each id in [`Self::nodes`], the first node for a duplicate id.
    pub positions: BTreeMap<NodeId, usize>,
    /// Definition of each node, `None` for an unknown kind.
    pub defs: Vec<Option<&'static NodeDef>>,
    /// Position of each node's innermost region, `None` outside every region or when the node's `region` is broken.
    pub parents: Vec<Option<usize>>,
    /// Number of regions around each node.
    pub depths: Vec<usize>,
    /// Origin of each node normalisation added, for a pass checked after normalisation.
    pub origins: Option<&'a BTreeMap<NodeId, Origin>>,
}

impl<'a> BodyView<'a> {
    fn new(checker: &mut Checker<'a>, document: &'a GraphDocument, places: &'a BodyPlaces, owner: Owner<'a>) -> Self {
        let nodes = match owner {
            Owner::Pass(pass) => pass.body.nodes.as_slice(),
            Owner::Function(function) => function.body.nodes.as_slice(),
        };
        let mut view = Self::unchecked(document, owner, nodes, places, None);
        view.report_unknown_kinds(checker);
        view.build_region_tree(checker);
        view
    }

    /// Returns the view of a pass after normalisation: `nodes` with `origins` for the nodes normalisation added, placed
    /// at `places`.
    ///
    /// The pass as written was viewed already, which reported its unknown kinds and its broken region tree, so this
    /// reports a region tree problem only at a node normalisation added.
    pub fn normalised(
        checker: &mut Checker<'a>,
        document: &'a GraphDocument,
        pass: &'a Pass,
        nodes: &'a [Node],
        places: &'a BodyPlaces,
        origins: &'a BTreeMap<NodeId, Origin>,
    ) -> Self {
        let mut view = Self::unchecked(document, Owner::Pass(pass), nodes, places, Some(origins));
        view.build_region_tree(checker);
        view
    }

    /// Returns the view of `nodes` with an empty region tree, reporting nothing.
    fn unchecked(
        document: &'a GraphDocument,
        owner: Owner<'a>,
        nodes: &'a [Node],
        places: &'a BodyPlaces,
        origins: Option<&'a BTreeMap<NodeId, Origin>>,
    ) -> Self {
        let mut positions = BTreeMap::new();
        for (position, node) in nodes.iter().enumerate() {
            positions.entry(node.id).or_insert(position);
        }
        Self {
            document,
            owner,
            nodes,
            places,
            positions,
            defs: nodes.iter().map(|node| lookup(&node.op, document.library)).collect(),
            parents: vec![None; nodes.len()],
            depths: vec![0; nodes.len()],
            origins,
        }
    }

    /// Returns the origin of the node at `position` when normalisation added it.
    pub fn origin(&self, position: usize) -> Option<&'a Origin> {
        self.origins?.get(&self.nodes[position].id)
    }

    /// Reports each node whose kind does not resolve at the file's library.
    fn report_unknown_kinds(&self, checker: &mut Checker<'_>) {
        for (position, node) in self.nodes.iter().enumerate() {
            if self.defs[position].is_none() {
                let message = match crate::catalogue::definitions().find(|def| def.id == node.op) {
                    Some(def) => format!(
                        "node {} uses '{}' from node library {}, but the file declares library {}",
                        node.id, node.op, def.since, self.document.library
                    ),
                    None => format!(
                        "node {} uses '{}', which is not defined in this build",
                        node.id, node.op
                    ),
                };
                self.error(checker, position, message, self.op_place(position));
            }
        }
    }

    /// Sets [`Self::parents`] and [`Self::depths`], and reports a `region` that names no region, a cycle of regions and
    /// regions nested too deep.
    fn build_region_tree(&mut self, checker: &mut Checker<'_>) {
        for (position, node) in self.nodes.iter().enumerate() {
            let Some(region) = node.region else { continue };
            let place = self.region_place(position);
            match self.positions.get(&region) {
                None => self.region_error(
                    checker,
                    position,
                    format!("node {} names the region {region}, which is not in this body", node.id),
                    place,
                ),
                Some(&parent) if self.defs[parent].is_some_and(|def| !def.is_region) => self.region_error(
                    checker,
                    position,
                    format!("node {} names the region {region}, which is not a region", node.id),
                    place,
                ),
                // A region of an unknown kind still has a body, and a wrong kind has its own diagnostic.
                Some(&parent) => self.parents[position] = Some(parent),
            }
        }
        // Each walk stops at a node it finished, so the cost over the body stays linear.
        const UNVISITED: u8 = 0;
        const WALKING: u8 = 1;
        const DONE: u8 = 2;
        let mut states = vec![UNVISITED; self.nodes.len()];
        let mut walk = Vec::new();
        for start in 0..self.nodes.len() {
            let mut current = Some(start);
            while let Some(position) = current {
                match states[position] {
                    UNVISITED => {
                        states[position] = WALKING;
                        walk.push(position);
                        current = self.parents[position];
                    }
                    WALKING => {
                        self.region_error(
                            checker,
                            position,
                            format!("the regions around node {} form a cycle", self.nodes[position].id),
                            self.region_place(position),
                        );
                        self.parents[position] = None;
                        current = None;
                    }
                    _ => current = None,
                }
            }
            while let Some(position) = walk.pop() {
                states[position] = DONE;
                self.depths[position] =
                    self.parents[position].map_or(0, |parent| self.depths[parent].saturating_add(1));
            }
        }
        for position in 0..self.nodes.len() {
            let is_region = self.defs[position].is_some_and(|def| def.is_region);
            if is_region && self.depths[position] >= MAX_REGION_DEPTH {
                self.region_error(
                    checker,
                    position,
                    format!(
                        "{} nests {} regions deep, expected at most {MAX_REGION_DEPTH}",
                        self.name(position),
                        self.depths[position] + 1,
                    ),
                    self.table_place(position),
                );
            }
        }
    }

    /// Adds a region tree error about the node at `position`, skipped at a node as written of a normalised pass.
    ///
    /// The pass as written reported its own region tree problems already.
    fn region_error(&self, checker: &mut Checker<'_>, position: usize, message: String, place: Option<Range<usize>>) {
        if self.origins.is_none() || self.origin(position).is_some() {
            self.error(checker, position, message, place);
        }
    }

    /// Returns the name a message gives the node at `position`, such as `Add n3`.
    pub fn name(&self, position: usize) -> String {
        let id = self
            .origin(position)
            .map_or(self.nodes[position].id, |origin| origin.node);
        let mut name = match self.defs[position] {
            Some(def) => format!("{} {id}", def.label),
            None => format!("node {id}"),
        };
        if let Some(function) = self.origin(position).and_then(|origin| origin.function.as_ref()) {
            name.push_str(&format!(" of the function '{function}'"));
        }
        name
    }

    /// Returns the stage's pass, `None` in a function body.
    pub fn pass(&self) -> Option<&'a Pass> {
        match self.owner {
            Owner::Pass(pass) => Some(pass),
            Owner::Function(_) => None,
        }
    }

    /// Returns whether the region at `region` holds the node at `position`, at any depth.
    pub fn is_inside(&self, position: usize, region: usize) -> bool {
        let mut current = self.parents[position];
        while let Some(parent) = current {
            if parent == region {
                return true;
            }
            current = self.parents[parent];
        }
        false
    }

    fn node_places(&self, position: usize) -> Option<&NodePlaces> {
        self.places.nodes.get(position)
    }

    /// Returns the place of the node's table.
    pub fn table_place(&self, position: usize) -> Option<Range<usize>> {
        self.node_places(position).map(|places| places.table.clone())
    }

    /// Returns the place of the node's `op` value.
    pub fn op_place(&self, position: usize) -> Option<Range<usize>> {
        self.node_places(position).map(|places| places.op.clone())
    }

    /// Returns the place of the node's `region` value, or of its table.
    pub fn region_place(&self, position: usize) -> Option<Range<usize>> {
        self.node_places(position)
            .and_then(|places| places.region.clone())
            .or_else(|| self.table_place(position))
    }

    /// Returns the place of the configuration value `key`, or of the node's table.
    pub fn config_place(&self, position: usize, key: &str) -> Option<Range<usize>> {
        self.node_places(position)
            .and_then(|places| places.config.get(key).cloned())
            .or_else(|| self.table_place(position))
    }

    /// Returns the place of the input value `port`, or of the node's table.
    pub fn input_place(&self, position: usize, port: &str) -> Option<Range<usize>> {
        self.node_places(position)
            .and_then(|places| places.inputs.get(port).cloned())
            .or_else(|| self.table_place(position))
    }

    /// Adds an error about the node at `position`, with its pass and id.
    pub fn error(&self, checker: &mut Checker<'_>, position: usize, message: String, place: Option<Range<usize>>) {
        self.add(checker, position, CodeDiagnostic::error(message), place);
    }

    /// Adds a warning about the node at `position`, with its pass and id.
    pub fn warning(&self, checker: &mut Checker<'_>, position: usize, message: String, place: Option<Range<usize>>) {
        self.add(checker, position, CodeDiagnostic::warning(message), place);
    }

    fn add(&self, checker: &mut Checker<'_>, position: usize, diagnostic: CodeDiagnostic, place: Option<Range<usize>>) {
        let mut diagnostic = match self.origin(position) {
            Some(origin) => {
                let written = origin.written();
                let note = match &origin.function {
                    Some(function) => format!("in node {} of the function '{function}'", origin.node),
                    None => format!("in node {} as expanded", origin.node),
                };
                diagnostic.with_node(written.to_string()).with_note(note)
            }
            None => diagnostic.with_node(self.nodes[position].id.to_string()),
        };
        if let Owner::Pass(pass) = self.owner {
            diagnostic = diagnostic.with_pass(pass.id.clone());
        }
        checker.add(diagnostic, place);
    }
}
