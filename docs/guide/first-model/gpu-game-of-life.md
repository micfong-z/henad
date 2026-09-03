---
title: GPU Grid model
description: A step-by-step tutorial that takes Conway's Game of Life to the GPU as a Henad GPU grid model.
icon: material/expansion-card
---

# Writing a GPU grid model

In this tutorial we'll build Conway's Game of Life (Life) on the GPU from scratch, where the grid lives in a storage buffer, every step is a compute dispatch, and the cells never leave the GPU.

This page assumes you have worked through our [CPU Grid Model](game-of-life.md) tutorial first.
We will reuse its palette, and use it as a cross-check for correctness.

You'll also need a machine with a GPU that wgpu can drive with compute support, which it reaches through Vulkan, Metal or DirectX 12.

## What we will write

A [`GpuGridModel`](../../authoring/gpu-grid-models.md) is a trait that describes a grid stepped by compute shaders.

Recall that the CPU trait asked us for one function, `step_cell`.
This one asks for 3 [WGSL](https://www.w3.org/TR/WGSL/) shaders and a `mod.rs` of declarations declaring some metadata and the initial state.

``` mermaid
flowchart LR
    I["<code>fn seed_buffers</code><br>configure initial state"] --> A
    A["state buffer<br>two sides, swapped by the engine"] --> S["<code>step.wgsl</code><br>once per word"]
    S -->|each tick| A
    A -.->|on publish| D["<code>display.wgsl</code><br>once per texel"]
    A -.->|on publish| R["<code>reduce.wgsl</code><br>once per cell"]
```

`step.wgsl`, `display.wgsl`, `reduce.wgsl` and the declarations in `mod.rs` are the pieces we need to write ourselves.
The three shaders differ in how they are dispatched:

| Shader         | Dispatched                       | Runs                             |
| -------------- | -------------------------------- | -------------------------------- |
| `step.wgsl`    | one invocation per unit of state | every step                       |
| `display.wgsl` | one invocation per display texel | on publish, a few times a second |
| `reduce.wgsl`  | one invocation per cell          | on publish, alongside display    |

The Henad engine handles the rest of the simulation, such as allocating both sides of the state buffer and swapping them after every step, building every pipeline and bind group from the shaders, batching steps into submissions, the display texture, and the snapshot the UI draws.

Let's get started by creating `crates/henad-models/src/gpu_life/`, containing `mod.rs`, `step.wgsl`, `display.wgsl` and `reduce.wgsl`.

## Update rule `step.wgsl`

Let's write down the update rule first, as we did on the CPU.
See the [CPU Grid Model](game-of-life.md#update-rule-step_cell) tutorial for details on the rules.

We will need a different representation of the grid, due to limits on storage buffer sizes, i.e. how much data we can easily store on the GPU.

### Cells as bits

The trait's default is one `u32` per cell and one step invocation per cell, and for a first model that default might be fine on smaller scales.
You can accept it and skip ahead to the [Bindings](#bindings), if you do not want to pack the cells into bits (yet).

However, for practical reasons, it is best to have them packed into bits any way.
A 100M-cell grid at one `u32` per cell is 400 MB per side, which blows the 128 MiB a storage binding is guaranteed on a baseline device (see [wgpu default limits](https://docs.rs/wgpu/latest/wgpu/struct.Limits.html#impl-Limits)).
The same grid at one _bit_ per cell is 12.5 MB per side.

So we pack 32 cells into each `u32`, and pad every row (with 0s) up to a whole number of words[^1].
Cell `x` of row `y` is just bit `x % 32` of word `y * words_per_row + x / 32`.

``` text
CELLS   0 ...... 31 │ 32 ..... 63 │ ... │ ... width-1 [.... padding .....]
WORDS   ── word 0 ─ │ ── word 1 ─ │ ... │ ────────── last word ───────────
BITS    0 ...... 31 │ 0 ...... 31 │ ... │ 0 ........................... 31
```

!!! warning "One invocation per word"

    Packing changes data ownership.
    If the step still dispatched one invocation per cell, 32 invocations would share each output word, and every one of them would read-modify-write it, racing with the other 31.
    Our step therefore dispatches one invocation per word, so each word has exactly one owner and is written with one plain store.

### Bindings

The shader binds the two sides of the state buffer and a small uniform:

``` { .wgsl .annotate title="crates/henad-models/src/gpu_life/step.wgsl" }
@group(0) @binding(0) var<storage, read> state_in: array<u32>; // (1)!
@group(0) @binding(1) var<storage, read_write> state_out: array<u32>;
@group(0) @binding(2) var<uniform> params: vec2<u32>; // (2)!
```

1. The binding layout of a grid model is fixed by the trait: an interleaved read and write pair per buffer, then the uniform. With one buffer that comes out as `state_in` at `0`, `state_out` at `1` and the uniform at `2`.
2. The uniform holds the content `mod.rs` decides to send, and Life needs nothing but the grid dimensions.

### SWAR counting

A `u32` consists of 32 independent one-bit lanes, so a bitwise operation on one word is equivalent to operating on 32 cells at once.
This computational style is called SWAR.

Instead of one 4-bit count per cell, we keep the count _bit-sliced_.
Picture the 32 neighbour counts of a word written out in binary, one count per column.
A bit-sliced representation stores that table by rows rather than by columns.
Word `sb0` holds the lowest bit of every count, `sb1` the next bit up, `sb2` the one above that, and bit `j` of each word belongs to cell `j`.
Reading one cell's count back means picking bit `j` out of each word and reading those bits as a number, and no such read ever happens in the shader.

``` text
cell j            0    1    2    3   ...   31
count             3    0    8    2   ...    5
                 ───  ───  ───  ───       ───
sb0  (weight 1)   1    0    0    0   ...    1
sb1  (weight 2)   1    0    0    1   ...    0
sb2  (weight 4)   0    0    0    0   ...    1
sb3  (weight 8)   0    0    1    0   ...    0
```

A count of 8 is `1000` in binary and needs that fourth row, so a complete count would take four words.
We will keep three and drop `sb3` on purpose, because the rule of Life never needs it.
Without its top bit a count of 8 reads as `000`, the same as a count of 0, and both of those kill the cell.
Every other count keeps its exact value in `sb0` to `sb2`.

Summing one-bit inputs into a bit-sliced count is a job for a carry-save adder, and an adder is nothing but XOR and AND:

``` { .wgsl .annotate title="crates/henad-models/src/gpu_life/step.wgsl" }
// One column of the adder tree: `sum` is the weight-w result, `carry` feeds weight 2w.
struct Adder {
    sum: u32,
    carry: u32,
}

fn full_add(a: u32, b: u32, c: u32) -> Adder { // (1)!
    let t = a ^ b;
    return Adder(t ^ c, (a & b) | (c & t));
}

fn half_add(a: u32, b: u32) -> Adder { // (2)!
    return Adder(a ^ b, a & b);
}
```

1. Three one-bit inputs in, a sum bit and a carry bit out, for all 32 lanes at once.
2. The same with two inputs.

Before any adding, each invocation gathers its neighbourhood.
For a word of cells, the west neighbour of every cell is the same word shifted left by one bit, with bit 0 filled in from the previous word, and similarly for the east.
A small struct carries the three words of one row:

``` wgsl title="crates/henad-models/src/gpu_life/step.wgsl"
// Preloaded row window, with west and east being the cells shifted by 1 bit left and right, respectively.
struct Row {
    cells: u32, // bit j = cell (word*32 + j)
    west: u32,  // bit j = its west neighbour
    east: u32,  // bit j = its east neighbour
}
```

### Loading a row

``` { .wgsl .annotate title="crates/henad-models/src/gpu_life/step.wgsl" }
fn load_row(row: u32, word: u32, stride: u32, width: u32) -> Row {
    let base = row * stride;
    let mid = state_in[base + word];
    let left = state_in[base + (word + stride - 1u) % stride]; // (1)!
    let right = state_in[base + (word + 1u) % stride];

    var r: Row;
    r.cells = mid;
    r.west = (mid << 1u) | (left >> 31u);   // bit 0 comes from the previous word's bit 31
    r.east = (mid >> 1u) | (right << 31u);  // bit 31 comes from the next word's bit 0

    // Those two shifts assume the grid's x-wrap lands on a word edge, which holds only when
    // width % 32 == 0. When the last word is ragged, exactly two bits are wrong, and need to be fixed.
    // When it isn't ragged, both patches rewrite the value that's already there.
    let last = width - 1u;
    if word == 0u { // (2)!
        r.west = (r.west & ~1u) | ((left >> (last % 32u)) & 1u);
    }
    if word == last / 32u {
        let b = last % 32u;
        r.east = (r.east & ~(1u << b)) | ((right & 1u) << b);
    }
    return r;
}
```

1. Neighbouring words wrap within the row through `% stride`, so a row's first and last words see each other. That is the x half of the torus.
2. The two `if` patches finish the job. A width that divides by 32 puts the wrap on a word edge, and the shifts above are already right. Any other width leaves a ragged last word, and exactly two bits come out wrong, one at each end of the row. The patches rewrite those two from the true wrap positions, and when nothing was wrong they rewrite the value already there, so there is no branch on raggedness.

### The entry point

``` { .wgsl .annotate title="crates/henad-models/src/gpu_life/step.wgsl" }
@compute
@workgroup_size(16, 16) // (1)!
fn main(@builtin(global_invocation_id) global_id: vec3<u32>) {
    let width = params.x;
    let height = params.y;
    let stride = (width + 31u) / 32u;

    let word = global_id.x;
    let y = global_id.y;
    if word >= stride || y >= height { // (2)!
        return;
    }

    let up = (y + height - 1u) % height; // (3)!
    let down = (y + 1u) % height;
    let r_up = load_row(up, word, stride, width);
    let r_mid = load_row(y, word, stride, width);
    let r_down = load_row(down, word, stride, width);

    // Compress the 8 neighbours into weight-1 sums and weight-2 carries.
    let a = full_add(r_up.west, r_up.cells, r_up.east);
    let b = full_add(r_down.west, r_down.cells, r_down.east);
    let c = half_add(r_mid.west, r_mid.east);

    // Weight 1: three sums left, one bit out.
    let d = full_add(a.sum, b.sum, c.sum);
    let sb0 = d.sum;

    // Weight 2, four terms. The three stage-1 carries, plus d's.
    let e = full_add(a.carry, b.carry, c.carry);
    let f = half_add(e.sum, d.carry);
    let sb1 = f.sum;

    // Weight 4, two terms. The weight-8 carry is dropped, since only n == 8 sets it, and n == 8
    // has sb1 == 0, so the rule below already excludes it.
    let sb2 = e.carry ^ f.carry; // (4)!

    // Survive on 2, born on 3. Bit-sliced, 3 is 011 and 2 is 010, so both need sb2 == 0 and
    // sb1 == 1 and differ only in sb0, which folds into (sb0 | cells).
    let alive = ~sb2 & sb1 & (sb0 | r_mid.cells); // (5)!

    // Trailing bits of a ragged last word hold no cell, and nothing reads them. load_row's patches
    // keep real cells off them, and display/reduce are bounded by width. The layout invariant is
    // still that they stay zero, and there is no `break` to leave them so now.
    let cells_here = min(width - word * 32u, 32u);
    var mask = 0xFFFFFFFFu;
    if cells_here < 32u {
        mask = (1u << cells_here) - 1u;
    }

    state_out[y * stride + word] = alive & mask; // (6)!
}
```

1. Every shader of a grid model declares the same square workgroup, and the trait's `WORKGROUP_SIZE` defaults to 16 to match it.
2. The dispatch is rounded up to whole workgroups, so some invocations hang off the right and bottom edges of the grid. They leave before touching anything.
3. The y half of the torus. Rows wrap with a plain modulo, since a row is a whole number of words.
4. Here `sb3` goes missing. The weight-8 carry out of this column would be the fourth row of the table above, and the tree never computes it. Dropping it is sound because 8 is `1000` in binary, so a count of 8 leaves `sb1` clear and the rule below already rejects it.
5. The rule collapses beautifully in bit-sliced form. A cell survives on a count of 2, binary `010`, and is born on 3, binary `011`. Both need `sb2 == 0` and `sb1 == 1`, and they differ only in `sb0`, where a set bit means born regardless and a clear bit needs the cell already alive. One expression for all 32 cells.
6. One plain store, by the word's one owner. This is the line the ownership rule above protects.

That is the whole rule.
It compiles to a few dozen bitwise instructions per word, with no loop and no branch over the cells, and all 32 lanes are resolved at once.

## Display `display.wgsl`

On the CPU the engine built our display texture for us, indexing `PALETTE` by the cell value.
On the GPU we need to draw our texture instead, because only the model knows how a word of bits maps to colours.

``` { .wgsl .annotate title="crates/henad-models/src/gpu_life/display.wgsl" }
#import shared::dims::{Dims, cell_at} // (1)!
@group(0) @binding(0) var<storage, read> state: array<u32>;
@group(0) @binding(1) var output: texture_storage_2d<rgba8unorm, write>;
@group(0) @binding(2) var<uniform> dims: Dims; // (2)!

// Palette matches `henad_models::game_of_life::PALETTE`: dead = 0x15/0x15/0x15, alive = 0x00/0xE6/0x76.
const DEAD_COLOR: vec4<f32> = vec4<f32>(21.0 / 255.0, 21.0 / 255.0, 21.0 / 255.0, 1.0); // (3)!
const ALIVE_COLOR: vec4<f32> = vec4<f32>(0.0 / 255.0, 230.0 / 255.0, 118.0 / 255.0, 1.0);

@compute
@workgroup_size(16, 16)
fn main(@builtin(global_invocation_id) global_id: vec3<u32>) {
    if (global_id.x >= dims.tex.x || global_id.y >= dims.tex.y) {
        return;
    }

    let width = dims.grid.x;
    let cell_xy = cell_at(global_id.xy, dims); // (4)!
    let x = cell_xy.x;
    let y = cell_xy.y;

    // Read the containing word and extract this cell's bit, unlike the per-word step pass.
    let words_per_row = (width + 31u) / 32u;
    let word = state[y * words_per_row + (x / 32u)];
    let cell = (word >> (x % 32u)) & 1u;
    let color = select(DEAD_COLOR, ALIVE_COLOR, cell == 1u);
    textureStore(output, vec2<i32>(global_id.xy), color);
}
```

1. Shared WGSL lives in `henad-compute/src/gpu/shared/` and can be reached with `#import`, resolved at build time. `shared::dims` holds the `Dims` struct every grid model's display and reduce shader reads.
2. Display and reduce see buffer 0 only, and get a `Dims` uniform of their own, carrying the grid size and the texture size. Our step uniform never reaches them.
3. The shader writes RGBA directly, so it carries its own copy of the two palette colours as WGSL constants. It is recommended to maintain consistency with the CPU palette.
4. The pass dispatches one invocation per _texel_, never per cell. The texture is capped at 4096 a side, so a big grid is sampled, and `cell_at` reads the cell at `texel * grid / tex`. Nothing special happens if this cap is not reached.

!!! note "Why sample?"

    One texel per cell would cap the grid at the device's maximum texture dimension and cost four bytes per cell, which at 16384^2^ is over a gigabyte of RGBA for something drawn into a panel roughly a thousand pixels wide.
    The [GPU grid models](../../authoring/gpu-grid-models.md#display-is-a-sampled-view) page has the details.

## Statistics `reduce.wgsl`

We would like to display a count of cells alive in the statistics.
On the CPU we counted with `reduce_chunks` at publish time, and the GPU equivalent is a reduction pass that runs at the same snapshot cadence:

``` { .wgsl .annotate title="crates/henad-models/src/gpu_life/reduce.wgsl" }
#import shared::dims::Dims

@group(0) @binding(0) var<storage, read> state: array<u32>;
@group(0) @binding(1) var<storage, read_write> counters: atomic<u32>; // (1)!
@group(0) @binding(2) var<uniform> dims: Dims;

var<workgroup> partial: atomic<u32>; // (2)!

@compute
@workgroup_size(16, 16)
fn main(
    @builtin(global_invocation_id) global_id: vec3<u32>,
    @builtin(local_invocation_index) local_index: u32,
) {
    if (local_index == 0u) {
        atomicStore(&partial, 0u);
    }
    workgroupBarrier();

    // Guarded with an `if` rather than an early `return`: the barriers below must be reached by
    // every invocation in the workgroup, and a partial grid tile would otherwise diverge.
    let width = dims.grid.x;
    let height = dims.grid.y;
    if (global_id.x < width && global_id.y < height) { // (3)!
        // One invocation per cell, reading the containing word and extracting this cell's bit.
        // Deliberately not a per-word countOneBits, which would need this pass to dispatch over
        // words, and the padding bits in a row's last word would then have to be masked off.
        // This runs at the display cadence, so the simpler form is worth more than the speed.
        let words_per_row = (width + 31u) / 32u;
        let word = state[global_id.y * words_per_row + (global_id.x / 32u)];
        if (((word >> (global_id.x % 32u)) & 1u) == 1u) {
            atomicAdd(&partial, 1u);
        }
    }
    workgroupBarrier();

    if (local_index == 0u) {
        atomicAdd(&counters, atomicLoad(&partial)); // (4)!
    }
}
```

1. One `u32` counter per series in `STATS`, which we declare in a moment. Life has one series, so this is a single atomic rather than an array.
2. Workgroup memory, shared by the 256 invocations of one workgroup and nobody else.
3. The bounds check is an `if` around the work rather than an early `return`. Every invocation in a workgroup has to reach both barriers, including the ones hanging off the grid's ragged edge, and an early return would leave them stranded.
4. Folding locally first means one global atomic per 256 cells instead of one per cell, which keeps the pass negligible at the grid sizes the engine targets.

## Implementing `GpuGridModel`

With the three shaders written, let's start on `mod.rs`.

``` rust title="crates/henad-models/src/gpu_life/mod.rs"
use henad_core::authoring::model::gpu_grid_model::GpuGridModel;

pub struct GpuLifeModel;

impl GpuGridModel for GpuLifeModel {}
```

The struct is empty, similar to the CPU model.
A GPU model is const metadata with a few pure functions, and every buffer lives with the engine.

This won't compile yet.
Let's run `cargo check` and see what the compiler says is missing:

``` text title="cargo check -p henad-models"
error[E0046]: not all trait items implemented, missing: `NAME`, `ID`, `DESCRIPTION`, `PALETTE`, `STATS`,
              `BUFFERS`, `STEP_BINDINGS`, `DISPLAY_BINDINGS`, `REDUCE_BINDINGS`, `STEP_SHADER`,
              `DISPLAY_SHADER`, `REDUCE_SHADER`, `param_descriptors`, `dims`, `seed_buffers`,
              `step_params_bytes`, `stats`
 --> crates/henad-models/src/gpu_life/mod.rs:5:1
  |
5 | impl GpuGridModel for GpuLifeModel {}
  | ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ missing 17 items in implementation
  |
  = help: implement the missing item: `const NAME: &'static str = "";`
  = help: implement the missing item: `const BUFFERS: &'static [&'static str] = &[];`
  = help: implement the missing item: `fn seed_buffers(_: u32, _: u32, _: &[ParamValue], _: Option<u64>) -> Vec<Vec<u32>> { todo!() }`
```

17 items is more than the CPU trait asked for, but 12 of them are consts of one line each.
We'll work down the list for the rest of this tutorial.

### Identity

The `impl` starts with the `NAME`, `ID` and `DESCRIPTION` of the model, exactly as on the CPU.

``` { .rust .annotate title="crates/henad-models/src/gpu_life/mod.rs" }
impl GpuGridModel for GpuLifeModel {
    const NAME: &'static str = "Game of Life (GPU)";
    const ID: &'static str = "gpu_life"; // (1)!
    const DESCRIPTION: &'static str = "Conway's Game of Life on a toroidal grid, stepped entirely on the GPU";
}
```

1. The shipped port already holds `gpu_game_of_life`, and IDs have to be unique across the registry, so we need a different one while both stay registered.

### Colours

The stats UI still reads `PALETTE`, even though the display shader carries its own colours.
Back on the CPU page we left the palette outside the `impl` block for exactly this moment.
Make it `pub` in `life.rs`,

``` rust title="crates/henad-models/src/life.rs" hl_lines="1"
pub const PALETTE: [[u8; 4]; 2] = [
    [0x15, 0x15, 0x15, 0xFF], // Dead
    [0x00, 0xE6, 0x76, 0xFF], // Alive
];
```

and point the trait at it, so the chart shows the same colours on both backends:

``` rust title="crates/henad-models/src/gpu_life/mod.rs"
    const PALETTE: &'static [[u8; 4]] = &PALETTE;
```

``` rust title="crates/henad-models/src/gpu_life/mod.rs"
use crate::life::PALETTE;
```

### Buffers and shaders

Next come the declarations with no CPU counterpart, the buffers the step ping-pongs and the three shaders we wrote:

``` { .rust .annotate title="crates/henad-models/src/gpu_life/mod.rs" }
    const BUFFERS: &'static [&'static str] = &["state"]; // (1)!

    const STEP_SHADER: &'static str = crate::shader_bindings::gpu_life::step::SHADER_STRING; // (2)!
    const DISPLAY_SHADER: &'static str = crate::shader_bindings::gpu_life::display::SHADER_STRING;
    const REDUCE_SHADER: &'static str = crate::shader_bindings::gpu_life::reduce::SHADER_STRING;

    const STEP_BINDINGS: &'static [BindingDecl] = crate::binding_decls::bindings::GPU_LIFE_STEP; // (3)!
    const DISPLAY_BINDINGS: &'static [BindingDecl] = crate::binding_decls::bindings::GPU_LIFE_DISPLAY;
    const REDUCE_BINDINGS: &'static [BindingDecl] = crate::binding_decls::bindings::GPU_LIFE_REDUCE;
```

1. One label per ping-ponged buffer. Life needs one, and a shader's binding names refer to it, `state_in` and `state_out` above.
2. The WGSL source, embedded as a string at build time.
3. Each shader's `@group(0)` declarations in `@binding` order, read off the source at build time, so the Rust side cannot disagree with the WGSL about what is bound where.

Neither of those modules knows about our shaders yet.
A `build.rs` in this crate runs `wgsl_bindgen` over every shader it is told about and generates both, so a new shader has to be listed there before anything compiles.
Add our three entries:

``` rust title="crates/henad-models/build.rs"
    "gpu_life/step.wgsl",
    "gpu_life/display.wgsl",
    "gpu_life/reduce.wgsl",
```

For context, here is the list our entries join:

``` rust title="crates/henad-models/build.rs"
--8<-- "crates/henad-models/build.rs:entry_points"
```

``` rust title="crates/henad-models/src/gpu_life/mod.rs"
use henad_core::authoring::model::binding::BindingDecl;
```

??? tip "A model with a second buffer"

    Life keeps everything in one buffer.
    A model whose cells carry more than a step can recompute declares more, and all of them swap sides together.
    The shipped GPU SIR keeps a per-cell random number generator in a second buffer, and its step shader binds two interleaved pairs before the uniform:

    ``` wgsl title="crates/henad-models/src/gpu_sir/step.wgsl"
    --8<-- "crates/henad-models/src/gpu_sir/step.wgsl:bindings"
    ```

    Display and reduce see only buffer 0 in either case.

### Sizes

Unlike a CPU grid model, nothing is prepended to a GPU model's parameter list, so width and height are ours to declare:

``` rust title="crates/henad-models/src/gpu_life/mod.rs"
henad_core::params! {
    const GRID_WIDTH = u32_param("grid_width", "Grid Width", 1024, 1, 16_384);
    const GRID_HEIGHT = u32_param("grid_height", "Grid Height", 1024, 1, 16_384);
}
```

`u32_param` takes the ID, the label the UI shows, then the default, the minimum and the maximum.
The range goes up to 16384 a side, well past the CPU model's 10000, because the packed layout makes such a grid affordable.

Three functions then tell the engine how big everything is:

``` { .rust .annotate title="crates/henad-models/src/gpu_life/mod.rs" }
    fn param_descriptors() -> Vec<ParamDescriptor> {
        descriptors()
    }

    fn dims(params: &[ParamValue]) -> (u32, u32) { // (1)!
        (
            extract_u32(params, GRID_WIDTH, 1024),
            extract_u32(params, GRID_HEIGHT, 1024),
        )
    }

    fn buffer_lens(width: u32, height: u32) -> Vec<usize> { // (2)!
        vec![words_per_row(width) * (height as usize)]
    }

    fn step_dims(width: u32, height: u32) -> (u32, u32) { // (3)!
        (words_per_row(width) as u32, height)
    }
```

1. The grid size, read back out of the parameters. The engine clamps both to at least 1.
2. One length per entry of `BUFFERS`, in `u32` elements. The default is one element per cell, and our packed layout measures in words instead.
3. The step's dispatch domain, in invocations. The default is one per cell, and we override it to one per word, for the ownership reason above. Display and reduce are unaffected, because they only ever read.

`words_per_row` is the one helper the layout needs:

``` rust title="crates/henad-models/src/gpu_life/mod.rs"
/// Words per padded row. 32 cells to a `u32`, rounded up.
pub fn words_per_row(width: u32) -> usize {
    (width as usize).div_ceil(32)
}
```

### Seeding

On the CPU the engine handed `init` a grid and a generator.
Here we build the initial buffer contents ourselves, on the CPU, and the engine uploads them once at construction.
For now the density stays hard-coded, as it did on the CPU page:

``` { .rust .annotate title="crates/henad-models/src/gpu_life/mod.rs" }
    fn seed_buffers(width: u32, height: u32, _params: &[ParamValue], seed: Option<u64>) -> Vec<Vec<u32>> {
        let rng = seed.map_or(GRID_INIT_SEED, mix_seed); // (1)!
        vec![seed_random(width, height, 0.3, rng)] // (2)!
    }
```

1. `seed` is `Some` when a caller asks for a particular run, and `None` from the app. Without one we fall back to `GRID_INIT_SEED`, the same constant the CPU engine seeded our `init` with, so the app shows the same opening grid on both backends.
2. One vector per entry of `BUFFERS`, each exactly as long as `buffer_lens` said.

The fill itself is the CPU `init` again, storing bits instead of bytes:

``` { .rust .annotate title="crates/henad-models/src/gpu_life/mod.rs" }
fn seed_random(width: u32, height: u32, density: f32, mut rng: u64) -> Vec<u32> {
    let threshold = (density * u32::MAX as f32) as u32; // (1)!
    let stride = words_per_row(width);
    let mut words = vec![0u32; stride * (height as usize)];
    for y in 0..height as usize {
        for x in 0..width as usize {
            if below(next_bits(&mut rng), threshold) { // (2)!
                words[y * stride + (x / 32)] |= 1u32 << (x % 32); // (3)!
            }
        }
    }
    words
}
```

1. The same threshold trick as the CPU page, for the same reason.
2. The same generator, drawn once per cell in the same row-major order, and the same Bernoulli trial.
3. Only the storage differs. A live cell sets its bit in the containing word, and the padding bits at the end of a ragged row stay zero.

Given identical parameters, the two backends therefore start from a bit-identical grid.
We'll get real value out of that under [Testing](#testing).

### Finishing up

Three items are left: the stat series, the step's uniform and `stats` itself.

``` { .rust .annotate title="crates/henad-models/src/gpu_life/mod.rs" }
    const STATS: &'static [StatDescriptor] = &[StatDescriptor::new("Alive", PALETTE[1])]; // (1)!

    fn step_params_bytes(width: u32, height: u32, _params: &[ParamValue]) -> Vec<u8> { // (2)!
        bytemuck::cast_slice(&[width, height]).to_vec()
    }

    fn stats(counts: &[u32]) -> Vec<StatValue> { // (3)!
        vec![StatValue::Scalar(f64::from(counts[0]))]
    }
```

1. One series, coloured like a live cell. Its length has to match the number of counters `reduce.wgsl` accumulates, and nothing checks that at compile time.
2. The step's uniform block as raw bytes. `params` in `step.wgsl` is a `vec2<u32>`, and two `u32`s laid end to end are exactly that.
3. `counts` holds one entry per series, read back from the reduce pass. It arrives through an asynchronous readback rather than a stall, so a reported stat is a few milliseconds stale, and reads zero until the first readback lands.

Once we add the imports these need, the file compiles:

``` rust title="crates/henad-models/src/gpu_life/mod.rs"
use henad_compute::cpu::grid_engine::GRID_INIT_SEED;
use henad_core::authoring::primitives::rng::{below, mix_seed, next_bits};
use henad_core::helpers::{extract_u32, u32_param};
use henad_core::params::{ParamDescriptor, ParamValue};
use henad_core::view::{StatDescriptor, StatValue};
```

## Running it

The model compiles, but the app can only pick models it finds in the registry, so we have to register it.
First we declare the module,

``` rust title="crates/henad-models/src/lib.rs"
pub mod gpu_life;
```

then we add a line to the GPU half of the registry:

``` rust title="crates/henad-models/src/registry.rs"
entries.push(register_gpu_grid_model::<crate::gpu_life::GpuLifeModel>(&ctx));
```

For context, here is the block our line joins:

``` rust title="crates/henad-models/src/registry.rs"
--8<-- "crates/henad-models/src/registry.rs:gpu_entries"
```

The GPU half is built only when a `GpuContext` exists.
On a machine with no usable adapter the GPU models are left out of the list entirely, rather than shown and left to fail when selected.
A GPU entry also carries a capacity check, so a grid too large for this device disables Build with a readable reason instead of crashing the process.

With the entry in place, we can finally run the model.
Make sure that `--release` is present to reach full performance.

=== "Desktop app"

    ``` bash
    cargo run --release --bin henad-app
    ```

    Our model shows up as the second Game of Life (GPU) in the picker.
    Press Build, then play, and once it runs try a 16384×16384 grid, which is 268 million cells.
    See [App tour](../app.md) for a quick overview of the UI.

=== "Headless"

    ``` bash
    cargo run --release -p henad-cli -- gpu_life --steps 1000 --reps 3
    ```

    Add `--set grid_width=8192 --set grid_height=8192` to see the model at scale, and `--global-warmup 1000` in front of `--steps`.
    The warm-up runs untimed steps first, letting the GPU clocks ramp up and the first-use shader compilation get paid before anything is measured.

=== "Browser"

    ``` bash
    ./scripts/build_web.sh serve --release
    ```

    Then open `http://localhost:8080`.
    The GPU models appear in the browser too, as long as it exposes WebGPU with compute support.
    See [App tour](../app.md) for a quick overview of the UI.

You should see the same gliders, now stepped on the GPU.
We are now good to implement the missing features: a way to change the starting density without editing the source, and a test.

## Parameters

Let's deal with the density first.
As on the CPU page, we hoist the hard-coded `0.3` out of the seeding and declare it as a parameter:

``` rust title="crates/henad-models/src/gpu_life/mod.rs" hl_lines="4"
henad_core::params! {
    const GRID_WIDTH = u32_param("grid_width", "Grid Width", 1024, 1, 16_384);
    const GRID_HEIGHT = u32_param("grid_height", "Grid Height", 1024, 1, 16_384);
    const DENSITY = f32_param("density", "Initial Density", 0.3, 0.0, 1.0, Some(0.01));
}
```

No `.on_reload()` this time.
Every parameter of a GPU model applies on reload whatever we declare, because the GPU state rejects a live edit, and the engine marks the whole list accordingly.

Then we read the value where the grid is seeded:

``` rust title="crates/henad-models/src/gpu_life/mod.rs" hl_lines="1 2 4"
    fn seed_buffers(width: u32, height: u32, params: &[ParamValue], seed: Option<u64>) -> Vec<Vec<u32>> {
        let density = extract_f32(params, DENSITY, 0.3);
        let rng = seed.map_or(GRID_INIT_SEED, mix_seed);
        vec![seed_random(width, height, density, rng)]
    }
```

`f32_param` and `extract_f32` both come from `henad_core::helpers`, next to the two we already use.

An operator sees exactly the three we declared, in the order we declared them.
Here it is for the shipped port, which declares the same list:

``` text title="cargo run -p henad-cli -- gpu_game_of_life --params"
parameters for gpu_game_of_life (Game of Life (GPU)):
  index=0 id=grid_width kind=u32 default=1024 min=1 max=16384 apply=reload label="Grid Width"
  index=1 id=grid_height kind=u32 default=1024 min=1 max=16384 apply=reload label="Grid Height"
  index=2 id=density kind=f32 default=0.3 min=0 max=1 apply=reload label="Initial Density"
```

!!! note "Indexes looking normal!"

    On the CPU page density sat at index 2 in this list while `DENSITY` read 0, because the engine prepended width and height.
    Nothing is prepended here, so `DENSITY` reads 2 and the list says 2.
    Spelling the list out ourselves also lets it match the CPU model's composed list exactly, so a slider in the app means the same thing on either backend.

## Testing

To convince ourselves the shaders are right, let's write a test.
Life draws no random numbers during a step, and we seeded the grid from the same generator as the CPU model, so the two backends should produce the same results.
That makes the CPU model a correctness oracle for this one, and the test is just a comparison:

``` { .rust .annotate title="crates/henad-models/src/gpu_life/mod.rs" }
#[cfg(test)]
mod tests {
    use super::*;
    use henad_compute::cpu::grid_engine::GridModelState;
    use henad_compute::gpu::grid_engine::GpuGridState;
    use henad_compute::gpu::{GpuContext, GpuSimState as _};
    use henad_core::model::SimState as _;

    use crate::life::LifeModel;

    fn alive(stats: &[henad_core::view::StatEntry]) -> u64 {
        match stats.first().map(|s| s.value.clone()) {
            Some(StatValue::Scalar(v)) => v as u64,
            other => panic!("expected a scalar Alive stat, got {other:?}"),
        }
    }

    /// Runs display and reduce, then waits for the count to land, as a one-shot snapshot does.
    fn refresh_stats(ctx: &GpuContext, state: &mut GpuGridState<GpuLifeModel>) { // (1)!
        let mut encoder = ctx
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
        state.encode_snapshot_passes(&mut encoder);
        ctx.queue.submit(Some(encoder.finish()));
        state.begin_stats_readback();
        state.poll_stats_readback(&ctx.device, true);
    }

    #[test]
    fn the_alive_count_matches_the_cpu_model() {
        let Some(ctx) = crate::tests::support::headless_context("gpu_life_test_device", wgpu::Features::empty()) else { // (2)!
            log::warn!("skipping the_alive_count_matches_the_cpu_model: no wgpu adapter available");
            return;
        };

        // 50 is neither a multiple of 32 nor a power of two, so the ragged last word is covered.
        let params = vec![ParamValue::U32(50), ParamValue::U32(30), ParamValue::F32(0.3)]; // (3)!
        let mut gpu = GpuGridState::<GpuLifeModel>::new(&ctx, &params);
        let mut cpu = GridModelState::<LifeModel>::from_params(&params);

        for tick in 0..10 {
            refresh_stats(&ctx, &mut gpu);
            assert_eq!(
                alive(&gpu.stats()),
                alive(&cpu.stats()),
                "the GPU alive count must match the CPU model's at tick {tick}"
            );

            let mut encoder = ctx
                .device
                .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
            gpu.encode_steps(&mut encoder, 1, None); // (4)!
            ctx.queue.submit(Some(encoder.finish()));
            cpu.step();
        }

        assert!(
            alive(&cpu.stats()) > 0,
            "the grid died out, so the comparison proves nothing"
        );
    }
}
```

1. A GPU state reports whatever its last readback delivered, so a test has to drive the snapshot passes itself and wait, exactly as a one-shot snapshot in the app does.
2. `headless_context` opens a device with no window behind it, and returns `None` on a machine with no adapter, where the test skips. Set `HENAD_REQUIRE_GPU=1` to turn that skip into a failure, so a green run actually means the GPU tests ran.
3. Both models take the same parameter vector, thanks to the list we spelled out.
4. One step per submission here, for simplicity. The real runner encodes many steps per submission, capped at 64, because one oversized submission can trip the OS GPU watchdog and silently zero every later readback.

Registering the model also opted us into the registry tests, which build every GPU model on a stock baseline device and check that the capacity check agrees with what actually builds.

## The finished files

Here is everything we wrote on this page, gathered into four files.

??? example "`gpu_life/mod.rs` completed"

    ``` rust
    --8<-- "crates/henad-models/src/tests/tutorial/gpu_life.rs"
    ```

??? example "`gpu_life/step.wgsl` completed"

    ``` wgsl
    --8<-- "crates/henad-models/src/gpu_game_of_life/step.wgsl"
    ```

??? example "`gpu_life/display.wgsl` completed"

    ``` wgsl
    --8<-- "crates/henad-models/src/gpu_game_of_life/display.wgsl"
    ```

??? example "`gpu_life/reduce.wgsl` completed"

    ``` wgsl
    --8<-- "crates/henad-models/src/gpu_game_of_life/reduce.wgsl"
    ```

The Rust listing is stored in the repository at [`crates/henad-models/src/tests/tutorial/gpu_life.rs`](https://github.com/micfong-z/henad/blob/master/crates/henad-models/src/tests/tutorial/gpu_life.rs), where it binds the shipped shaders under the paths its own directory gives them.
The three shaders are the shipped port's own, at [`crates/henad-models/src/gpu_game_of_life/`](https://github.com/micfong-z/henad/tree/master/crates/henad-models/src/gpu_game_of_life), since a shader carries no model ID and what we wrote is the same file line for line.

The actual default model is at [`crates/henad-models/src/gpu_game_of_life/mod.rs`](https://github.com/micfong-z/henad/blob/master/crates/henad-models/src/gpu_game_of_life/mod.rs).
It runs under its own ID, and its tests pin the adder tree and the ragged wrap.

## Next

The [GPU Agent model](gpu-ants.md) tutorial takes the ant colony to the GPU, where a step becomes a _list_ of passes and determinism needs more careful handling.

For the trait from the reference side, read [GPU grid models](../../authoring/gpu-grid-models.md), and for the WGSL tooling, [shaders and bindings](../../authoring/shaders.md).

*[WGSL]: WebGPU Shading Language
*[SWAR]: SIMD Within A Register

[^1]: A word is 32 bits, i.e. a `u32`.
