# Authoring primitives

This is the index of the primitives Henad provides to model authors.

Every primitive here has two implementations, one for CPU models and one for GPU models.
CPU models use a Rust function in `henad_core::authoring::primitives`, and GPU models use a WGSL function under `shared::` with `#import`.
Each pair has the same semantics and is tested for parity on the outputs.
Integer results must match exactly.
Float results are compared with a tolerance, since WGSL's float `%` goes through a division while Rust's is an exact fmod.

See docstring for each primitive for its full signature and semantics.

## Status

| Status | Meaning |
|---|---|
| `shipped` | Exists in `authoring::primitives` and in `shared::`, with a parity test |
| `planned` | Being built now, under issue #11 |
| `deferred` | Wanted, but no model needs it yet, so it is not written |
| `elsewhere` | Already covered by something Henad has, named in the row |
| `rejected` | Deliberately not provided, for the reason in the row |

`deferred` is the default for anything without two real call sites.

## Reference

Our primary reference is the [NetLogo dictionary](https://docs.netlogo.org/dictionary).

Semantically, we need some alterations to NetLogo's primitives to make them work in a parallel engine.
NetLogo's agentsets, `ask` and `with` are lazily-filtered collections of boxed agents iterated one at a time in a global order, which prevents large models from scaling.
Henad is struct-of-arrays and steps every agent in parallel, so it cannot provide those primitives.

> [!note]
> In NetLogo, wrapping is a property of the world, set once in the model settings, and primitives like `distancexy` consult it implicitly.
> Henad's primitives are pure functions with no world to consult, so wrapping is an explicit `Boundary` argument.

## Pairing conventions

**Names match across languages.**
`axis_delta` in Rust is `axis_delta` in WGSL.
We try to match the names, but this isn't always possible. For example, `from` and `target` are in WGSL's reserved-word list and are parse errors as identifiers, so a delta is called `a` and `b`.

**Fallible primitives return a validity flag in WGSL.**
WGSL has no `Option`, so a Rust `Option<(u32, u32)>` becomes a `vec3<i32>` whose `.z` is non-zero when the value is present.

**Parity covers pure functions only.**
Note that the CPU RNG is `xorshift64` over `u64` and the GPU RNG is `pcg_hash` over `u32`, because WGSL has no 64-bit integers.
Those two are counterparts in role and produce different streams.
Hence, only those derived from a raw word are parity-tested, such as `unit` and `choice3`, which are bit-equal on both sides by construction.

**Some primitives have no WGSL twin.**
For example, `for_each_neighbor` takes a closure, which WGSL has no equivalent for.
The WGSL idiom is a loop over the offset table instead, and the tables are the shared part.

## Space

| NetLogo | Henad | Status | Notes |
|---|---|---|---|
| — | `Boundary` (`Torus`, `Bounded`) | `shipped` | A world setting in NetLogo, an argument here |
| — | `wrap_index(v, m)` | `shipped` | Wraps an index into `0..m`, replacing three spellings of `(v + m - 1) % m` |
| — | `wrap_coord(v, world)` | `shipped` | The `rem_euclid` position wrap |
| `patch-at` | `offset_cell(x, y, dx, dy, w, h, boundary)` | `shipped` | `dy` runs south, matching the display's downward y axis |
| `distancexy` | `axis_delta(a, b, world, boundary)` | `shipped` | One axis, signed, shortest way round on a torus |
| `distancexy` | `dist_sq(ax, ay, bx, by, world_w, world_h, boundary)` | `shipped` | Squared, so the hot paths pay no `sqrt` |
| — | `cell_index(x, y, w)` | `shipped` | Pins the `y * w + x` convention once |
| `towards` | `heading_octant(vx, vy)` | `shipped` | Eight-way discretisation, comparisons rather than `atan2` |
| `distance`, `distancexy` | true distance with `sqrt` | `deferred` | Every caller today wants the square |
| `towards`, `towardsxy` | heading in radians | `deferred` | Boids deliberately avoids per-agent `atan2` |
| `dx`, `dy` | heading components | `deferred` | No caller |
| `patch-ahead`, `patch-left-and-ahead`, `patch-right-and-ahead` | — | `deferred` | No caller; all three compose from `offset_cell` and a heading |
| `patch-at-heading-and-distance` | — | `deferred` | No caller |
| `can-move?` | — | `deferred` | Ants' `passable` is bounded-plus-obstacle, which is model state, not geometry |
| `in-radius` | `SpatialHash::query_radius` | `elsewhere` | `henad_core::spatial_hash`, with a caller-provided result buffer so a query does not allocate |
| `in-cone`, `at-points` | — | `deferred` | No caller; both would build on the same index |
| `patch-here` | — | `elsewhere` | An agent's cell is `cell_index` of its truncated position |
| `world-width`, `world-height`, `min-pxcor`, `max-pxcor`, `min-pycor`, `max-pycor` | `Extent` | `elsewhere` | `henad_core::Extent`; the engine prepends world size to every agent model's params |

## Neighbourhoods

| NetLogo | Henad | Status | Notes |
|---|---|---|---|
| `neighbors` | `MOORE_ROW_MAJOR` | `shipped` | The 8 surrounding cells, `dy` outer. The `step_cell` slice order |
| `neighbors` | `MOORE_COLUMN_MAJOR` | `shipped` | The same 8 cells, `dx` outer. See the warning below |
| `neighbors4` | `VON_NEUMANN` | `shipped` | The 4 orthogonal cells |
| — | `offsets(kind)` | `shipped` | The table for a `NeighborhoodKind`, in `step_cell` order |
| `neighbors`, `neighbors4` | `for_each_neighbor(..)` | `shipped` | Rust only. The WGSL idiom is a loop over the offset table |
| — | `neighbor_offset(table, n)`, `neighbor_count(table)` | `shipped` | WGSL only, the loop form of the tables above |


> [!warning]
> Two different iteration orders are in use, and they are not interchangeable.
> 
> `GridModel::step_cell` receives its `neighbors` slice row-major, `dy` outer and `dx` inner, and that order is published API.
> 
> The ants model walks `dx` outer and `dy` inner, and that order is important because ties between equal pheromone are broken by a reservoir draw, so the visit order changes the result.
> This is for consistency with krABMga.


## Random

Draws take a raw `u32` word so the same function serves both backends.
The `next_*` forms advance a generator and then call the pure form.

| NetLogo | Henad | Status | Notes |
|---|---|---|---|
| `random-float 1` | `unit(word)` | `planned` | `word / u32::MAX`, bit-equal on both backends |
| `random-float 1` | `next_unit(rng)` | `planned` | Advances the generator, then `unit` |
| — | `below(word, threshold)` | `planned` | The Bernoulli draw both grid models' `init` writes out by hand |
| `random 3` | `choice3(word)` | `planned` | One of `-1`, `0`, `+1` |
| `one-of` | `reservoir_accept(word, count)` | `planned` | Uniform pick among equal candidates, the tie-break ants needs |
| `random-normal` | — | `deferred` | No caller |
| `random-exponential`, `random-poisson`, `random-gamma` | — | `deferred` | No caller |
| `n-of`, `up-to-n-of` | — | `deferred` | No caller |
| `random-seed`, `new-seed` | — | `rejected` | A global mutable seed would make results depend on scheduling. A chunk's RNG comes from `chunk_seed(base, tick, chunk_index)`, which is what makes runs independent of the thread count |

## Deliberately not provided

| NetLogo | Why not |
|---|---|
| `ask`, `ask-concurrent` | Serial iteration over an agentset in a global order. This is the direct cause of NetLogo's scaling ceiling, and the engine owns the pass here precisely so it can be parallel |
| `with`, `of`, `all?`, `any?`, `member?` | Agentsets as first-class lazily-filtered values, which needs per-agent indirection. A Henad model filters inside its own kernel, over flat lanes |
| `sort-by`, `max-one-of`, `min-one-of`, `with-max`, `with-min` | Same reason, plus a global ordering that would serialise the step |
| `turtles-here`, `turtles-at`, `turtles-on`, `other` | Turtle-on-patch containment keeps a second copy of every agent's position, coherent only by convention. `SpatialHash` is the replacement, rebuilt from positions each tick |
| `count` | The agentset half is rejected with `with`. Counting cells is `reduce_chunks` in `henad-compute`'s `cpu/primitives/chunked.rs` |
| `create-turtles`, `hatch`, `sprout`, `die` | Dynamic population. Nothing obstructs it today, but there is no helper and no example yet; tracked separately |
| `every`, `wait`, `stop` | Non-uniform activation. A global priority queue is inherently serial, and models can carry phase counters, which covers the common cases |
| `run`, `runresult`, `carefully` | Dynamic evaluation of code strings. No place in a compiled engine |
| `abs`, `sin`, `sqrt`, `floor`, `mod`, `mean`, `sum`, `variance`, and the rest of the maths group | Rust and WGSL both already have these. Nothing to add |

## Where these live

```
crates/henad-core/src/authoring/primitives/space.rs   <->  crates/henad-compute/src/gpu/shared/space.wgsl
crates/henad-core/src/authoring/primitives/rng.rs     <->  crates/henad-compute/src/gpu/shared/rng.wgsl
```

The parity test that pins each pair is `crates/henad-compute/src/gpu/shared/parity.wgsl`, driven from `crates/henad-compute/src/gpu/tests/parity.rs`.
It skips silently on a machine with no adapter, so set `HENAD_REQUIRE_GPU=1` to turn that into a failure.

The shader is one invocation per case and one dispatch for the lot, switching on an op code.
Adding a primitive means adding an op there and a case builder in the driver, and the op codes come from the generated bindings rather than being retyped.
