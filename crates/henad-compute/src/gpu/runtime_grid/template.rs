//! Text of the engine template that a grid rule's step is composed with, and of the rule's display and reduce
//! shaders.
//!
//! The template comes first in the composed entry and the rule's text follows it, so a line of the rule keeps its
//! number after the template's lines. The template's own names start with `henad_` or `HENAD_`, apart from the names
//! a rule calls: [`TEMPLATE_NAMES`] and the state constants.

use henad_core::authoring::model::binding::{BindingDecl, BindingKind};
use henad_core::authoring::primitives::space::Boundary;

/// Side of the square workgroups every pass of a rule dispatches.
pub(crate) const WORKGROUP_SIZE: u32 = 16;

/// Labels of the buffers a rule ping-pongs: the cells, and a one-word tick counter.
pub(crate) const BUFFERS: &[&str] = &["state", "tick"];

/// Names the template declares for the rule's text to use, which the text cannot declare again.
pub(crate) const TEMPLATE_NAMES: &[&str] = &[
    "Params",
    "params",
    "state_in",
    "state_out",
    "tick_in",
    "tick_out",
    "NUM_STATES",
    "neighbor",
    "moore_count",
    "von_neumann_count",
    "main",
];

/// Prefix of the names the template keeps to itself, compared without regard to case.
pub(crate) const PRIVATE_PREFIX: &str = "henad_";

/// Fields the template puts at the head of `Params`, in front of the rule's parameters.
pub(crate) const PARAMS_HEAD: &[&str] = &["width", "height", "seed"];

/// Function whose `fn` keyword the composer's spans of the entry are measured from.
pub(crate) const ANCHOR: &str = "neighbor";

/// Bindings of the step shader, in `@binding` order.
pub(crate) const STEP_BINDINGS: &[BindingDecl] = &[
    storage("state_in", true),
    storage("state_out", false),
    storage("tick_in", true),
    storage("tick_out", false),
    BindingDecl {
        name: "params",
        kind: BindingKind::Uniform,
    },
];

/// Bindings of the display shader, in `@binding` order.
pub(crate) const DISPLAY_BINDINGS: &[BindingDecl] = &[
    storage("state", true),
    BindingDecl {
        name: "output",
        kind: BindingKind::StorageTexture,
    },
    BindingDecl {
        name: "dims",
        kind: BindingKind::Uniform,
    },
];

/// Bindings of the reduce shader, in `@binding` order.
pub(crate) const REDUCE_BINDINGS: &[BindingDecl] = &[
    storage("state", true),
    storage("counters", false),
    BindingDecl {
        name: "dims",
        kind: BindingKind::Uniform,
    },
];

const fn storage(name: &'static str, read_only: bool) -> BindingDecl {
    BindingDecl {
        name,
        kind: BindingKind::Storage { read_only },
    }
}

/// Type of one field of the rule's `Params` block.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FieldType {
    /// A float, for an `F32` parameter.
    F32,
    /// An unsigned integer, for a `U32`, `Bool` or `Choice` parameter.
    U32,
}

impl FieldType {
    fn wgsl(self) -> &'static str {
        match self {
            Self::F32 => "f32",
            Self::U32 => "u32",
        }
    }
}

/// The template of one rule, and where its anchor function starts.
#[derive(Debug)]
pub(crate) struct StepTemplate {
    /// Text of the template, ending with a line break.
    pub(crate) text: String,
    /// Byte offset of the `fn` keyword of [`ANCHOR`].
    pub(crate) anchor_offset: usize,
}

/// Returns the template of a rule with `params`, each a field name and type, `constants` named in state order, and
/// `boundary` at the grid's edge.
pub(crate) fn step_template(params: &[(&str, FieldType)], constants: &[String], boundary: Boundary) -> StepTemplate {
    let boundary = match boundary {
        Boundary::Torus => "HENAD_TORUS",
        Boundary::Bounded => "HENAD_BOUNDED",
    };
    let mut text = String::from(
        "// Engine template of a grid rule. The rule's own text follows it.\n\
         #import henad::rng::{pcg_hash as henad_pcg_hash}\n\
         #import henad::space::{offset_cell as henad_offset_cell, TORUS as HENAD_TORUS, BOUNDED as HENAD_BOUNDED}\n\n\
         struct Params {\n",
    );
    for head in PARAMS_HEAD {
        text.push_str(&format!("    {head}: u32,\n"));
    }
    for (id, field) in params {
        text.push_str(&format!("    {id}: {},\n", field.wgsl()));
    }
    text.push_str(
        "}\n\n\
         @group(0) @binding(0) var<storage, read> state_in: array<u32>;\n\
         @group(0) @binding(1) var<storage, read_write> state_out: array<u32>;\n\
         @group(0) @binding(2) var<storage, read> tick_in: array<u32>;\n\
         @group(0) @binding(3) var<storage, read_write> tick_out: array<u32>;\n\
         @group(0) @binding(4) var<uniform> params: Params;\n\n",
    );
    text.push_str(&format!("const NUM_STATES: u32 = {}u;\n", constants.len()));
    for (index, constant) in constants.iter().enumerate() {
        text.push_str(&format!("const {constant}: u32 = {index}u;\n"));
    }
    text.push_str("\nvar<private> henad_cell: vec2<u32>;\n\n");
    let anchor_offset = text.len();
    text.push_str(&format!(
        "fn neighbor(dx: i32, dy: i32) -> u32 {{\n    \
         let cell = henad_offset_cell(henad_cell.x, henad_cell.y, dx, dy, params.width, params.height, {boundary});\n    \
         if (cell.z == 0) {{\n        \
         return 0u;\n    \
         }}\n    \
         return state_in[u32(cell.y) * params.width + u32(cell.x)];\n\
         }}\n\n"
    ));
    text.push_str(
        "fn moore_count(state: u32) -> u32 {\n    \
         return u32(neighbor(-1, -1) == state) + u32(neighbor(0, -1) == state) + u32(neighbor(1, -1) == state)\n        \
         + u32(neighbor(-1, 0) == state) + u32(neighbor(1, 0) == state)\n        \
         + u32(neighbor(-1, 1) == state) + u32(neighbor(0, 1) == state) + u32(neighbor(1, 1) == state);\n\
         }\n\n\
         fn von_neumann_count(state: u32) -> u32 {\n    \
         return u32(neighbor(0, -1) == state) + u32(neighbor(-1, 0) == state) + u32(neighbor(1, 0) == state)\n        \
         + u32(neighbor(0, 1) == state);\n\
         }\n\n",
    );
    text.push_str(&format!(
        "@compute @workgroup_size({WORKGROUP_SIZE}, {WORKGROUP_SIZE})\n\
         fn main(@builtin(global_invocation_id) henad_id: vec3<u32>) {{\n    \
         let henad_tick = tick_in[0];\n    \
         if (henad_id.x == 0u && henad_id.y == 0u) {{\n        \
         tick_out[0] = henad_tick + 1u;\n    \
         }}\n    \
         if (henad_id.x >= params.width || henad_id.y >= params.height) {{\n        \
         return;\n    \
         }}\n    \
         henad_cell = henad_id.xy;\n    \
         let henad_index = henad_id.y * params.width + henad_id.x;\n    \
         var henad_rng = henad_pcg_hash(params.seed ^ henad_pcg_hash(henad_index ^ henad_pcg_hash(henad_tick)));\n    \
         state_out[henad_index] = min(step_cell(state_in[henad_index], &henad_rng), NUM_STATES - 1u);\n\
         }}\n\n\
         // The rule's own text starts on the next line.\n"
    ));
    StepTemplate { text, anchor_offset }
}

/// Returns the display shader of a rule whose states take the colours of `palette`, in state order.
pub(crate) fn display_shader(palette: &[[u8; 4]]) -> String {
    let mut text = format!(
        "#import henad::dims::{{Dims, cell_at}}\n\n\
         @group(0) @binding(0) var<storage, read> state: array<u32>;\n\
         @group(0) @binding(1) var output: texture_storage_2d<rgba8unorm, write>;\n\
         @group(0) @binding(2) var<uniform> dims: Dims;\n\n\
         const PALETTE = array<vec4<f32>, {}>(\n",
        palette.len()
    );
    for [red, green, blue, alpha] in palette {
        text.push_str(&format!(
            "    vec4<f32>({red}.0 / 255.0, {green}.0 / 255.0, {blue}.0 / 255.0, {alpha}.0 / 255.0),\n"
        ));
    }
    text.push_str(&format!(
        ");\n\n\
         @compute @workgroup_size({WORKGROUP_SIZE}, {WORKGROUP_SIZE})\n\
         fn main(@builtin(global_invocation_id) global_id: vec3<u32>) {{\n    \
         if (global_id.x >= dims.tex.x || global_id.y >= dims.tex.y) {{\n        \
         return;\n    \
         }}\n    \
         let cell_xy = cell_at(global_id.xy, dims);\n    \
         let cell = state[cell_xy.y * dims.grid.x + cell_xy.x];\n    \
         var palette = PALETTE;\n    \
         textureStore(output, vec2<i32>(global_id.xy), palette[min(cell, {last}u)]);\n\
         }}\n",
        last = palette.len().saturating_sub(1)
    ));
    text
}

/// Returns the reduce shader of a rule with `states` states, which counts the cells in each state.
pub(crate) fn reduce_shader(states: usize) -> String {
    format!(
        "#import henad::dims::Dims\n\n\
         @group(0) @binding(0) var<storage, read> state: array<u32>;\n\
         @group(0) @binding(1) var<storage, read_write> counters: array<atomic<u32>, {states}>;\n\
         @group(0) @binding(2) var<uniform> dims: Dims;\n\n\
         var<workgroup> partial: array<atomic<u32>, {states}>;\n\n\
         @compute @workgroup_size({WORKGROUP_SIZE}, {WORKGROUP_SIZE})\n\
         fn main(\n    \
         @builtin(global_invocation_id) global_id: vec3<u32>,\n    \
         @builtin(local_invocation_index) local_index: u32,\n\
         ) {{\n    \
         if (local_index < {states}u) {{\n        \
         atomicStore(&partial[local_index], 0u);\n    \
         }}\n    \
         workgroupBarrier();\n    \
         if (global_id.x < dims.grid.x && global_id.y < dims.grid.y) {{\n        \
         let cell = state[global_id.y * dims.grid.x + global_id.x];\n        \
         atomicAdd(&partial[min(cell, {last}u)], 1u);\n    \
         }}\n    \
         workgroupBarrier();\n    \
         if (local_index < {states}u) {{\n        \
         let count = atomicLoad(&partial[local_index]);\n        \
         if (count > 0u) {{\n            \
         atomicAdd(&counters[local_index], count);\n        \
         }}\n    \
         }}\n\
         }}\n",
        last = states.saturating_sub(1)
    )
}

/// Returns the constant name of a state labelled `label`: its ASCII letters and digits in upper case, with every run
/// of other characters made one underscore. `None` when the label holds no ASCII letter or digit.
pub(crate) fn constant_name(label: &str) -> Option<String> {
    let mut name = String::new();
    let mut gap = false;
    for c in label.chars() {
        if c.is_ascii_alphanumeric() {
            if gap && !name.is_empty() {
                name.push('_');
            }
            gap = false;
            name.push(c.to_ascii_uppercase());
        } else {
            gap = true;
        }
    }
    (!name.is_empty()).then_some(name)
}

#[cfg(test)]
mod tests {
    use super::constant_name;

    #[test]
    fn a_label_becomes_an_upper_snake_constant() {
        assert_eq!(constant_name("Dead").as_deref(), Some("DEAD"));
        assert_eq!(
            constant_name("  infected cells (I) ").as_deref(),
            Some("INFECTED_CELLS_I")
        );
        assert_eq!(constant_name("état 2").as_deref(), Some("TAT_2"));
        assert_eq!(constant_name("—"), None);
    }
}
