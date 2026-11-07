//! Limits that hold for an untrusted graph file, at node library 1.
//!
//! A model file holds data and generated WGSL, and no compiled code. Every limit belongs to the library version, since
//! lowering one can make an old file invalid.

/// Size in bytes of the largest graph file, checked before the text is parsed.
///
/// The file's canonical text is held to the same size after the text is read, so a file that reads has a canonical
/// text that reads too.
pub const MAX_FILE_BYTES: usize = 1024 * 1024;

// Nodes
/// Number of nodes of one pass or function.
pub const MAX_NODES_PER_BODY: usize = 4096;
/// Number of nodes of a file as written.
pub const MAX_NODES_IN_FILE: usize = 16384;
/// Number of nodes of a file after its function calls are inlined.
pub const MAX_NODES_INLINED: usize = 65536;

// Declarations
/// Number of passes of a file.
pub const MAX_PASSES: usize = 64;
/// Number of functions of a file.
pub const MAX_FUNCTIONS: usize = 128;
/// Depth of the call graph of a file's functions.
pub const MAX_CALL_DEPTH: usize = 8;
/// Number of lanes of one domain.
pub const MAX_LANES_PER_DOMAIN: usize = 64;
/// Number of fields of a file.
pub const MAX_FIELDS: usize = 8;
/// Number of layers of one field.
pub const MAX_LAYERS_PER_FIELD: usize = 16;
/// Number of counters of a file.
pub const MAX_COUNTERS: usize = 16;
/// Number of categories of a file.
pub const MAX_CATEGORIES: usize = 64;
/// Number of variants of one category.
pub const MAX_VARIANTS: usize = 256;
/// Number of palettes of a file.
pub const MAX_PALETTES: usize = 64;
/// Number of colours of one palette.
pub const MAX_PALETTE_COLORS: usize = 256;
/// Number of parameters of a file.
pub const MAX_PARAMS: usize = 64;
/// Number of statistics of a file.
pub const MAX_STATS: usize = 64;
/// Number of actions of a file.
pub const MAX_ACTIONS: usize = 32;

// Nodes with a list in their configuration
/// Number of accumulators of one region.
pub const MAX_ACCUMULATORS: usize = 16;
/// Number of rules of one `cat.transition` node.
pub const MAX_TRANSITION_RULES: usize = 64;
/// Number of values of one `logic.pick` node.
pub const MAX_PICK_VALUES: usize = 256;
/// Largest `max_exponent` of a `math.powi` node.
pub const MAX_POWI_EXPONENT: u32 = 64;
/// Size in bytes of the text of a `math.expr` node.
pub const MAX_EXPRESSION_BYTES: usize = 4096;
/// Number of inputs, and of outputs, of one `math.expr` node.
pub const MAX_EXPRESSION_PORTS: usize = 16;

// Regions and cost
/// Largest `max` of a `region.repeat` node.
pub const MAX_REPEAT_ITERATIONS: u32 = 65536;
/// Depth of nested `region.repeat` and `grid.fold` nodes after inlining.
pub const MAX_REGION_DEPTH: usize = 3;
/// Number of candidates one `agent.radius_fold` invocation visits before it faults.
pub const MAX_FOLD_CANDIDATES: u32 = 65536;
/// Number of cells of the neighbour index one `agent.radius_fold` invocation walks before it faults.
pub const MAX_WALK_CELLS: u32 = 65536;
/// Cost units per element of one pass.
pub const MAX_PASS_COST: u32 = 65536;

// Text
/// Size in bytes of an id.
pub const MAX_ID_BYTES: usize = 64;
/// Size in bytes of a label.
pub const MAX_LABEL_BYTES: usize = 256;
/// Size in bytes of a description.
pub const MAX_DESCRIPTION_BYTES: usize = 4096;

// World
/// Longest side of a grid in cells.
pub const MAX_GRID_SIDE: u32 = 16384;
/// Number of agents of a population.
pub const MAX_AGENTS: u32 = 10_000_000;

// Bindings
/// Number of storage buffers one generated shader binds before the compiler packs lanes, the WebGPU default of
/// `max_storage_buffers_per_shader_stage`.
///
/// Note that the budget never reads the device's limit.
pub const MAX_STORAGE_BINDINGS: u32 = 8;
/// Size in bytes of the largest buffer of packed lanes at the largest count of its domain, the WebGPU default of
/// `max_storage_buffer_binding_size`.
///
/// The largest count of agents is the world's `max_count`, and of cells the cells of the world's default size.
pub const MAX_PACKED_BUFFER_BYTES: u64 = 128 << 20;
