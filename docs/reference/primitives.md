---
title: Primitives
description: Every primitive a Henad kernel can call, with its Rust and WGSL signatures.
icon: material/function-variant
---

# Authoring primitives

Primitives are the small vocabulary a model kernel calls for world geometry, neighbourhoods and random draws.
Every one of them exists twice, once in Rust for CPU models and once in WGSL for GPU models.

```rust
use henad_core::authoring::primitives::space::{Boundary, dist_sq, offset_cell};
use henad_core::authoring::primitives::rng::{next_bits, random_float};
```

```wgsl
#import shared::space::{TORUS, dist_sq, offset_cell}
#import shared::rng::{next_bits, random_float}
```

Names match across the two languages.
Each entry below gives the Rust signature, and calls out the WGSL one wherever it differs.

## Index

| Group | Primitives |
|---|---|
| [Space](#space) | [`Boundary`](#boundary), [`wrap_index`](#wrap_index), [`wrap_coord`](#wrap_coord), [`cell_index`](#cell_index), [`offset_cell`](#offset_cell), [`axis_delta`](#axis_delta), [`dist_sq`](#dist_sq), [`heading_octant`](#heading_octant) |
| [Neighbourhoods](#neighbourhoods) | [`MOORE_ROW_MAJOR`](#moore_row_major), [`MOORE_COLUMN_MAJOR`](#moore_column_major), [`VON_NEUMANN`](#von_neumann), [`offsets`](#offsets), [`for_each_neighbor`](#for_each_neighbor), [`neighbor_count`](#neighbor_count), [`neighbor_offset`](#neighbor_offset) |
| [Random](#random) | [`xorshift64`](#xorshift64), [`pcg_hash`](#pcg_hash), [`mix_seed`](#mix_seed), [`next_bits`](#next_bits), [`random_float`](#random_float), [`next_float`](#next_float), [`below`](#below), [`choice3`](#choice3), [`reservoir_accept`](#reservoir_accept) |

## Space

`henad_core::authoring::primitives::space` and `shared::space`.

Positions are `f32` in world units, cells are `u32` indices into a grid `w` by `h`.
The y axis points down, matching the display, and `dy` therefore runs south.

### `Boundary`

```rust
enum Boundary { Torus, Bounded }
```

The world's edge behaviour, passed to every primitive that can cross an edge.
Under `Torus` both axes wrap.
Under `Bounded` each edge is a wall.

The two agree everywhere except at an edge, so a model can switch between them without changing its interior behaviour.

WGSL has no enums and takes a `u32` instead.

```wgsl
const TORUS: u32 = 0u;
const BOUNDED: u32 = 1u;
```

See also: [`offset_cell`](#offset_cell), [`axis_delta`](#axis_delta), [`dist_sq`](#dist_sq).

### `wrap_index`

```rust
fn wrap_index(v: i32, m: i32) -> i32
```

Wraps `v` into `0..m`.

```rust
wrap_index(-1, 8)   // 7
wrap_index(8, 8)    // 0
wrap_index(3, 8)    // 3
```

Panics when `m` is 0.
The WGSL twin is undefined there instead.

See also: [`wrap_coord`](#wrap_coord), [`offset_cell`](#offset_cell).

### `wrap_coord`

```rust
fn wrap_coord(v: f32, world: f32) -> f32
```

Wraps `v` into `0.0..world`, the position wrap an agent leaving one edge needs to re-enter at the other.

```rust
wrap_coord(10.5, 10.0)   // 0.5
wrap_coord(-0.5, 10.0)   // 9.5
```

See also: [`wrap_index`](#wrap_index), [`axis_delta`](#axis_delta).

### `cell_index`

```rust
fn cell_index(x: u32, y: u32, w: u32) -> u32
```

Flat index of cell `(x, y)` in a grid `w` wide, as `y * w + x`.
Every grid buffer in the engine is row-major with `y` as the slow axis.

```rust
cell_index(3, 2, 10)   // 23
```

An agent's cell is `cell_index` of its truncated position.

See also: [`offset_cell`](#offset_cell).

### `offset_cell`

```rust
fn offset_cell(x: u32, y: u32, dx: i32, dy: i32, w: u32, h: u32, boundary: Boundary) -> Option<(u32, u32)>
```

The cell `(dx, dy)` away from `(x, y)`.
Under `Torus` the result is always `Some`, since both axes wrap.
Under `Bounded` a step past any edge gives `None`.

```rust
offset_cell(0, 0, -1, 0, 8, 8, Boundary::Torus)     // Some((7, 0))
offset_cell(0, 0, -1, 0, 8, 8, Boundary::Bounded)   // None
offset_cell(3, 3, 1, 1, 8, 8, Boundary::Bounded)    // Some((4, 4))
```

WGSL has no `Option`, so the twin packs the flag into a third component.
`.z` is non-zero when the cell exists, and `.xy` is meaningless when it is not.

```wgsl
fn offset_cell(x: u32, y: u32, dx: i32, dy: i32, w: u32, h: u32, boundary: u32) -> vec3<i32>
```

See also: [`for_each_neighbor`](#for_each_neighbor), [`wrap_index`](#wrap_index), [`MOORE_ROW_MAJOR`](#moore_row_major).

### `axis_delta`

```rust
fn axis_delta(a: f32, b: f32, world: f32, boundary: Boundary) -> f32
```

The shortest signed delta from `a` to `b` along one axis.
Under `Bounded` this is `b - a`.
Under `Torus` the axis is a ring of circumference `world`, and the result is the representative of `b - a` in `[-world / 2, world / 2)`.

```rust
axis_delta(1.0, 9.0, 10.0, Boundary::Bounded)   //  8.0
axis_delta(1.0, 9.0, 10.0, Boundary::Torus)     // -2.0, backwards round the ring is shorter
axis_delta(0.0, 5.0, 10.0, Boundary::Torus)     // -5.0, the half-open range makes antipodes negative
```

Inputs outside `[0, world)` are wrapped, so a caller need not normalise first.
The WGSL twin assumes the two positions are within one world of each other.
Every position the engine produces satisfies that.

Both sides name the arguments `a` and `b`.
`from` and `target` are reserved words in WGSL.

See also: [`dist_sq`](#dist_sq), [`wrap_coord`](#wrap_coord).

### `dist_sq`

```rust
fn dist_sq(ax: f32, ay: f32, bx: f32, by: f32, world_w: f32, world_h: f32, boundary: Boundary) -> f32
```

Squared distance between two points, wrapping each axis through [`axis_delta`](#axis_delta).
Compare it against a squared radius rather than taking a `sqrt`.

```rust
dist_sq(0.0, 0.0, 3.0, 4.0, 100.0, 100.0, Boundary::Bounded)   // 25.0
dist_sq(0.5, 0.5, 9.5, 9.5, 10.0, 10.0, Boundary::Torus)       //  2.0, across the seam
```

The WGSL twin takes vectors instead of scalars, matching how the agent buffers are packed.

```wgsl
fn dist_sq(a: vec2<f32>, b: vec2<f32>, world: vec2<f32>, boundary: u32) -> f32
```

See also: [`axis_delta`](#axis_delta).

### `heading_octant`

```rust
fn heading_octant(vx: f32, vy: f32) -> u8
```

A velocity discretised into one of eight octants, running clockwise from east because the display's y axis points down.
Spans are half-open, and the result comes from comparisons rather than `atan2`.

| Octant | Span | Direction |
|---|---|---|
| 0 | 0° to 45° | East to south-east |
| 1 | 45° to 90° | South-east to south |
| 2 | 90° to 135° | South to south-west |
| 3 | 135° to 180° | South-west to west |
| 4 | 180° to 225° | West to north-west |
| 5 | 225° to 270° | North-west to north |
| 6 | 270° to 315° | North to north-east |
| 7 | 315° to 360° | North-east to east |

```rust
heading_octant(1.0, 0.0)   // 0
heading_octant(0.0, 1.0)   // 1
```

The WGSL twin returns `u32`.

See also: [`axis_delta`](#axis_delta).

## Neighbourhoods

The offset tables and the two ways of walking them.
Rust holds each table as a const slice, WGSL as an id plus an accessor.

### `MOORE_ROW_MAJOR`

```rust
const MOORE_ROW_MAJOR: [(i32, i32); 8]
```

The 8 surrounding cells, `dy` outer and `dx` inner.

```text
(-1, -1)  (0, -1)  (1, -1)  (-1, 0)  (1, 0)  (-1, 1)  (0, 1)  (1, 1)
```

This is the order `GridModel::step_cell` receives its `neighbors` slice in.
A model indexes that slice by position, so the order is published API.

See also: [`MOORE_COLUMN_MAJOR`](#moore_column_major), [`VON_NEUMANN`](#von_neumann), [`offsets`](#offsets).

### `MOORE_COLUMN_MAJOR`

```rust
const MOORE_COLUMN_MAJOR: [(i32, i32); 8]
```

The same 8 cells, `dx` outer and `dy` inner.

```text
(-1, -1)  (-1, 0)  (-1, 1)  (0, -1)  (0, 1)  (1, -1)  (1, 0)  (1, 1)
```

The ants model walks this order.

!!! warning "The two Moore orders are not interchangeable"

    A kernel that breaks ties between equally good neighbours draws from the visit order.
    Swapping one table for the other then changes its results, even though both cover the same 8 cells.

See also: [`MOORE_ROW_MAJOR`](#moore_row_major), [`reservoir_accept`](#reservoir_accept).

### `VON_NEUMANN`

```rust
const VON_NEUMANN: [(i32, i32); 4]
```

The 4 orthogonal cells, in `step_cell` order.

```text
(0, -1)  (-1, 0)  (1, 0)  (0, 1)
```

See also: [`MOORE_ROW_MAJOR`](#moore_row_major), [`offsets`](#offsets).

### `offsets`

```rust
fn offsets(kind: NeighborhoodKind) -> &'static [(i32, i32)]
```

The table for a `NeighborhoodKind`, in `step_cell` order.
`Moore` gives [`MOORE_ROW_MAJOR`](#moore_row_major) and `VonNeumann` gives [`VON_NEUMANN`](#von_neumann).

`NeighborhoodKind` comes from `henad_core::topology`.

See also: [`for_each_neighbor`](#for_each_neighbor).

### `for_each_neighbor`

```rust
fn for_each_neighbor(
    x: u32,
    y: u32,
    w: u32,
    h: u32,
    offsets: &[(i32, i32)],
    boundary: Boundary,
    f: impl FnMut(u32, u32),
)
```

Calls `f` with each neighbour of `(x, y)` that exists, in table order.
Neighbours outside a `Bounded` grid are skipped rather than clamped, so `f` runs fewer than `offsets.len()` times at an edge.

```rust
// A corner of a bounded grid has 3 Moore neighbours. A corner of a torus has 8.
for_each_neighbor(0, 0, 4, 4, &MOORE_ROW_MAJOR, Boundary::Bounded, |nx, ny| { .. });
```

Rust only, since WGSL has no closures.
A shader loops over [`neighbor_offset`](#neighbor_offset) to the same effect.

See also: [`offset_cell`](#offset_cell), [`offsets`](#offsets).

### `neighbor_count`

```wgsl
fn neighbor_count(table: u32) -> u32
```

How many offsets a table holds, 4 for `VON_NEUMANN` and 8 for either Moore table.
WGSL only.

Tables are named by id there.

```wgsl
const MOORE_ROW_MAJOR: u32 = 0u;
const MOORE_COLUMN_MAJOR: u32 = 1u;
const VON_NEUMANN: u32 = 2u;
```

See also: [`neighbor_offset`](#neighbor_offset).

### `neighbor_offset`

```wgsl
fn neighbor_offset(table: u32, n: u32) -> vec2<i32>
```

Offset `n` of a table, in the same order as the Rust const of that name.
WGSL only, and the loop form of [`for_each_neighbor`](#for_each_neighbor).

```wgsl
for (var n = 0u; n < neighbor_count(MOORE_ROW_MAJOR); n++) {
    let d = neighbor_offset(MOORE_ROW_MAJOR, n);
    let cell = offset_cell(x, y, d.x, d.y, w, h, TORUS);
}
```

See also: [`neighbor_count`](#neighbor_count), [`offset_cell`](#offset_cell).

## Random

`henad_core::authoring::primitives::rng` and `shared::rng`.

A draw takes a raw `u32` word and is pure.
A `next_*` form advances a generator and then calls the pure form.

The generators themselves differ between backends.
Rust runs `xorshift64` over `u64` state, WGSL runs `pcg_hash` over `u32`, since WGSL has no 64-bit integers.
The two fill the same role and produce different streams.

!!! warning "Every draw needs its own word"

    Feeding one `next_bits` result to two draws correlates them, and neither the compiler nor any test catches it.

### `xorshift64`

```rust
fn xorshift64(state: u64) -> u64
```

The CPU generator.
Takes a state and returns the next one, never 0.
The state must never be 0 either.
[`mix_seed`](#mix_seed) guarantees that.

See also: [`next_bits`](#next_bits), [`pcg_hash`](#pcg_hash).

### `pcg_hash`

```wgsl
fn pcg_hash(input: u32) -> u32
```

The GPU generator, and the WGSL counterpart of [`xorshift64`](#xorshift64).
A model that seeds a GPU buffer from Rust mirrors this function bit for bit.

See also: [`next_bits`](#next_bits).

### `mix_seed`

```rust
fn mix_seed(seed: u64) -> u64
```

Scrambles a user-supplied seed into a usable `xorshift64` state.
Guards against the absorbing zero state, and decorrelates adjacent seeds such as 1, 2 and 3.

Rust only.

See also: [`xorshift64`](#xorshift64).

### `next_bits`

```rust
fn next_bits(rng: &mut u64) -> u32
```

Advances `rng` and returns 32 fresh bits, taken from the top half of the state.

The WGSL twin advances a pointer to the caller's own `u32`.

```wgsl
fn next_bits(r: ptr<function, u32>) -> u32
```

See also: [`next_float`](#next_float), [`xorshift64`](#xorshift64).

### `random_float`

```rust
fn random_float(bits: u32, max: f32) -> f32
```

A uniform float in `[0, max)`, built from the top 24 bits of `bits`.

```rust
random_float(0, 1.0)          // 0.0
random_float(u32::MAX, 1.0)   // strictly under 1.0
```

24 bits is the f32 mantissa width, so every value the draw can produce is exact and the range really is half-open.
Both backends run the same form, and the two are bit-equal.

See also: [`next_float`](#next_float), [`below`](#below), [`reservoir_accept`](#reservoir_accept).

### `next_float`

```rust
fn next_float(rng: &mut u64, max: f32) -> f32
```

Advances `rng`, then draws with [`random_float`](#random_float).

```wgsl
fn next_float(r: ptr<function, u32>, max: f32) -> f32
```

The two backends draw from different streams, since the generators differ.

See also: [`random_float`](#random_float), [`next_bits`](#next_bits).

### `below`

```rust
fn below(bits: u32, threshold: u32) -> bool
```

A Bernoulli trial, true for `threshold` of the 2^32 possible words.
Pass `(p * u32::MAX as f32) as u32` for probability `p`.
A threshold of 0 never fires.

The comparison is integer rather than float, so a seeded run cannot drift on a machine that rounds differently.

See also: [`random_float`](#random_float), [`next_bits`](#next_bits).

### `choice3`

```rust
fn choice3(bits: u32) -> i32
```

One of `-1`, `0` or `+1`.

```rust
choice3(0)   // -1
choice3(1)   //  0
choice3(2)   //  1
```

See also: [`reservoir_accept`](#reservoir_accept).

### `reservoir_accept`

```rust
fn reservoir_accept(bits: u32, count: u32) -> bool
```

Accepts the `count`-th of a run of equally good candidates, with probability `1 / count`.
`count` is the candidate's 1-based position.
Reservoir sampling over ties, so once `n` equal candidates have been seen each has been picked with probability `1 / n`.

```rust
reservoir_accept(u32::MAX, 1)   // true, the first of a run always wins
reservoir_accept(0, 2)          // true
reservoir_accept(u32::MAX, 2)   // false, the second wins half the time
```

A `count` of 0 accepts.

See also: [`random_float`](#random_float), [`MOORE_COLUMN_MAJOR`](#moore_column_major).

## Parity

Each pair is pinned by a parity test.
`crates/henad-compute/src/gpu/shared/parity.wgsl` runs one invocation per case and one dispatch for the whole set, switching on an op code, and `crates/henad-compute/src/gpu/tests/parity.rs` drives it and compares.
Op codes come from the generated bindings.

Integer results must match exactly, and so must [`random_float`](#random_float).
Other float results are compared against a tolerance, because WGSL evaluates float `%` through a division where Rust implements an exact fmod.

Parity covers the pure functions only.
The generators are excluded, along with anything that has no twin.

On a machine with no adapter the test skips silently.
Set `HENAD_REQUIRE_GPU=1` to turn that skip into a failure.

## Related, elsewhere

Some things a kernel reaches for are not primitives, and live with the engine instead.

| Need | Where |
|---|---|
| Agents within a radius | `SpatialHash::query_radius`, in `henad_core::spatial_hash`. Takes a caller-provided result buffer, so a query does not allocate |
| World size | `henad_core::Extent`. The engine prepends world size to every agent model's params |
| A per-chunk RNG seed | `chunk_seed(base, tick, chunk_index)`, in `henad-compute`'s `cpu/primitives/chunked.rs` |
| Counting cells | `reduce_chunks`, in the same file |
| Arithmetic, trigonometry, `min`, `max`, `clamp` | Rust and WGSL both provide these already |

## Not provided

- **Agentsets.** There is no first-class filtered collection of agents, and no ask-style iteration over one. A model filters inside its own kernel, over flat lanes.
- **Global ordering.** Nothing sorts agents or picks a global maximum across them.
- **A per-cell list of agents.** Nothing keeps a second copy of where each agent stands. `SpatialHash` is rebuilt from positions each tick and answers the same queries.
- **A global RNG seed.** A chunk's RNG comes from `chunk_seed(base, tick, chunk_index)`. A run is then independent of the thread count.
- **Dynamic populations.** Agents are neither created nor removed mid-run.
- **Non-uniform activation.** Every agent steps every tick. A model wanting less carries its own phase counter.
- **Dynamic evaluation.** Both backends compile ahead of time.

A primitive is written when two real models need it.
Anything with one call site stays in that model.

## Where these live

```text
crates/henad-core/src/authoring/primitives/space.rs   <->  crates/henad-compute/src/gpu/shared/space.wgsl
crates/henad-core/src/authoring/primitives/rng.rs     <->  crates/henad-compute/src/gpu/shared/rng.wgsl
```

Adding a primitive means adding both sides, an op in `parity.wgsl` and a case builder in the parity driver.
