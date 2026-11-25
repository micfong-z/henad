//! View in viewport: one node output of a graph model shown in the Viewport through a temporary view pass.
//!
//! The *view file* is the document with every view pass switched off and one view pass added, which computes the
//! output again from the lanes and colours each element by it. The nodes the output reads are copied into the new pass,
//! regions whole, and a number, a flag or a category is mapped to a colour: a category by its own variant colours, and
//! any other value through [`RAMP`]. The view file runs as the preview and never reaches the Editor's document.

use std::collections::BTreeSet;

use henad_graph::GraphFile;
use henad_graph::catalogue::lookup;
use henad_graph::check::CheckReport;
use henad_graph::document::{Pass, PassEnabled, PassStage};
use henad_graph::edit::{EditCommand, EditError, GraphBuilder, Scope};
use henad_graph::ids::NodeId;
use henad_graph::node::NodeBody;
use henad_graph::types::{Rate, ValueType};
use henad_graph::value::{ConfigValue, InputValue, Literal, PortRef};

use super::unused_id;

/// Colours of the ramp a number is shown through, low to high: nine stops of viridis.
pub const RAMP: [[u8; 4]; 9] = [
    [68, 1, 84, 255],
    [71, 45, 123, 255],
    [59, 82, 139, 255],
    [44, 114, 142, 255],
    [33, 145, 140, 255],
    [40, 174, 128, 255],
    [94, 201, 98, 255],
    [173, 220, 48, 255],
    [253, 231, 37, 255],
];

/// Range of a float that [`RAMP`] spans, low to high.
const FLOAT_RANGE: [f32; 2] = [0.0, 1.0];

/// One node output shown in the Viewport.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ViewTarget {
    /// Pass of the node.
    pub pass: String,
    /// The node.
    pub node: NodeId,
    /// Output port.
    pub port: String,
}

impl ViewTarget {
    /// Returns the name of the output as the Editor shows it, `n12.out`.
    pub fn name(&self) -> String {
        format!("{}.{}", self.node, self.port)
    }
}

/// Returns whether an output of `value_type` at `rate` can be shown in the Viewport: an element-rate number, flag or
/// category.
pub fn can_view(value_type: Option<&ValueType>, rate: Rate) -> bool {
    rate == Rate::Element
        && matches!(
            value_type,
            Some(ValueType::F32 | ValueType::U32 | ValueType::I32 | ValueType::Bool | ValueType::Category(_))
        )
}

/// Returns the view file of `target` in the document `builder` holds, which `report` checked.
///
/// # Errors
///
/// Returns the message the Editor shows when the output cannot be shown: a node or a pass the document lacks, an
/// output that is no element-rate number, flag or category, or a view file the checker refuses, as for a value that
/// reads a random draw.
pub fn view_file(builder: &GraphBuilder, report: &CheckReport, target: &ViewTarget) -> Result<GraphFile, String> {
    let document = builder.document();
    let pass = document
        .passes
        .iter()
        .find(|pass| pass.id == target.pass)
        .ok_or_else(|| format!("Model file has no pass {}.", target.pass))?;
    let node = pass
        .body
        .node(target.node)
        .ok_or_else(|| format!("Pass {} has no node {}.", pass.id, target.node))?;
    if node.region.is_some() {
        return Err("A value inside a region cannot be shown in the viewport.".to_owned());
    }
    let port_type = report
        .passes
        .iter()
        .find(|known| known.id == pass.id)
        .and_then(|known| known.outputs.get(&target.node))
        .and_then(|ports| ports.get(&target.port))
        .ok_or_else(|| format!("Node {} has no output {}.", target.node, target.port))?;
    let value_type = port_type.value_type.clone();
    if !can_view(value_type.as_ref(), port_type.rate) {
        return Err("Only per-element numbers, flags and categories can be shown in the viewport.".to_owned());
    }
    let domain = pass.domain;
    let copied = upstream(&pass.body, target.node, document.library);

    let mut draft = builder.clone();
    let failed = |error: EditError| format!("The view pass cannot be made: {error}.");
    let text = draft.copy(&Scope::pass(&pass.id), &copied).map_err(failed)?;
    let views: Vec<String> = document
        .passes
        .iter()
        .filter(|pass| pass.stage == PassStage::View)
        .map(|pass| pass.id.clone())
        .collect();
    for id in views {
        draft
            .apply(EditCommand::SetPassEnabled {
                id,
                enabled: PassEnabled::Literal(false),
            })
            .map_err(failed)?;
    }
    let taken = |id: &str| {
        draft.document().palettes.iter().any(|palette| palette.id == id)
            || draft.document().categories.iter().any(|category| category.id == id)
            || draft.document().passes.iter().any(|pass| pass.id == id)
    };
    let palette = unused_id("view_ramp", taken);
    let view_pass = unused_id("view_in_viewport", taken);
    draft
        .apply(EditCommand::AddPalette(henad_graph::document::Palette::new(
            &palette,
            RAMP.to_vec(),
        )))
        .map_err(failed)?;
    draft
        .apply(EditCommand::AddPass(Pass::new(&view_pass, PassStage::View, domain)))
        .map_err(failed)?;
    let scope = Scope::pass(&view_pass);
    let pasted = draft
        .apply(EditCommand::Paste {
            scope: scope.clone(),
            text,
            offset: [0.0, 0.0],
        })
        .map_err(failed)?;
    // A paste mints ids in the order of the copied ids, which `copy` holds sorted.
    let value = copied
        .iter()
        .zip(&pasted)
        .find(|(copied, _)| **copied == target.node)
        .map(|(_, pasted)| PortRef {
            node: *pasted,
            port: Some(target.port.clone()),
        })
        .ok_or_else(|| "The view pass cannot be made: the node was not copied.".to_owned())?;
    let mut builder = ViewBuilder {
        draft: &mut draft,
        scope,
    };
    builder.color(value, value_type.as_ref(), &palette).map_err(failed)?;
    let file = draft
        .to_file()
        .map_err(|error| format!("The view pass cannot be made: {error}."))?;
    let checked = file.check();
    if let Some(problem) = checked.errors().next() {
        return Err(format!(
            "Only a value the view pass can compute can be shown in the viewport. {}",
            super::sentence(&problem.message)
        ));
    }
    Ok(file)
}

/// Adds the nodes that colour each element by one value to the view pass.
struct ViewBuilder<'a> {
    draft: &'a mut GraphBuilder,
    scope: Scope,
}

impl ViewBuilder<'_> {
    /// Adds a node of kind `op` and returns its id.
    fn add(&mut self, op: &str) -> Result<NodeId, EditError> {
        let made = self.draft.apply(EditCommand::AddNode {
            scope: self.scope.clone(),
            op: op.to_owned(),
            region: None,
            pos: None,
        })?;
        made.first()
            .copied()
            .ok_or_else(|| EditError::Invalid(format!("no {op} node was added")))
    }

    /// Wires `source` to the input `port` of `node`.
    fn connect(&mut self, node: NodeId, port: &str, source: PortRef) -> Result<(), EditError> {
        self.draft
            .apply(EditCommand::Connect {
                scope: self.scope.clone(),
                node,
                port: port.to_owned(),
                source,
            })
            .map(drop)
    }

    /// Sets the input `port` of `node` to `literal`.
    fn literal(&mut self, node: NodeId, port: &str, literal: Literal) -> Result<(), EditError> {
        self.draft
            .apply(EditCommand::SetLiteral {
                scope: self.scope.clone(),
                node,
                port: port.to_owned(),
                literal,
            })
            .map(drop)
    }

    /// Converts `value` with the one-input kind `op` and returns the converted output.
    fn convert(&mut self, op: &str, value: PortRef) -> Result<PortRef, EditError> {
        let node = self.add(op)?;
        self.connect(node, "a", value)?;
        Ok(PortRef { node, port: None })
    }

    /// Maps the float `value` to an entry of `palette` across `range`, and returns the entry.
    fn ramp(&mut self, value: PortRef, range: [f32; 2], palette: &str) -> Result<PortRef, EditError> {
        let node = self.add("view.ramp")?;
        self.set_palette(node, palette)?;
        self.connect(node, "value", value)?;
        self.literal(node, "low", Literal::F32(range[0]))?;
        self.literal(node, "high", Literal::F32(range[1]))?;
        Ok(PortRef { node, port: None })
    }

    fn set_palette(&mut self, node: NodeId, palette: &str) -> Result<(), EditError> {
        self.draft
            .apply(EditCommand::SetConfig {
                scope: self.scope.clone(),
                node,
                key: "palette".to_owned(),
                value: Some(ConfigValue::String(palette.to_owned())),
            })
            .map(drop)
    }

    /// Colours each element by `value` of `value_type`: a category by its variant colours, a `u32` by the entry of
    /// `palette` it indexes, an `i32` as the band of that entry, a flag as the first or the last entry, and a float
    /// through `palette` across [`FLOAT_RANGE`].
    fn color(&mut self, value: PortRef, value_type: Option<&ValueType>, palette: &str) -> Result<(), EditError> {
        let bands = f32::from(u8::try_from(RAMP.len()).unwrap_or(u8::MAX));
        let (index, palette) = match value_type {
            Some(ValueType::Category(category)) => (value, category.as_str()),
            Some(ValueType::U32) => (value, palette),
            Some(ValueType::I32) => {
                let float = self.convert("conv.to_f32", value)?;
                (self.ramp(float, [0.0, bands], palette)?, palette)
            }
            Some(ValueType::Bool) => {
                let flag = self.convert("conv.to_u32", value)?;
                let scaled = self.add("math.mul")?;
                self.connect(scaled, "a", flag)?;
                let last = u32::try_from(RAMP.len() - 1).unwrap_or(0);
                self.literal(scaled, "b", Literal::U32(last))?;
                (
                    PortRef {
                        node: scaled,
                        port: None,
                    },
                    palette,
                )
            }
            _ => (self.ramp(value, FLOAT_RANGE, palette)?, palette),
        };
        let color = self.add("view.color")?;
        self.set_palette(color, palette)?;
        self.connect(color, "index", index)
    }
}

/// Returns `node` and every node of `body` it reads, directly or through others, sorted by id. A region joins whole,
/// its body included, and so does the region around a node that joins.
fn upstream(body: &NodeBody, node: NodeId, library: u32) -> Vec<NodeId> {
    let mut kept = BTreeSet::from([node]);
    let mut queue = vec![node];
    while let Some(current) = queue.pop() {
        let Some(found) = body.node(current) else { continue };
        let mut next: Vec<NodeId> = found
            .inputs
            .values()
            .filter_map(|value| match value {
                InputValue::Link(link) => Some(link.node),
                _ => None,
            })
            .collect();
        next.extend(found.region);
        if lookup(&found.op, library).is_some_and(|def| def.is_region) {
            next.extend(
                body.nodes
                    .iter()
                    .filter(|member| member.region == Some(current))
                    .map(|member| member.id),
            );
        }
        for id in next {
            if kept.insert(id) {
                queue.push(id);
            }
        }
    }
    kept.into_iter().collect()
}
