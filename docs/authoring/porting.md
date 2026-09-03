---
title: Porting a model
description: Taking a working CPU model into compute shaders, and the parts that stay comparable between the two.
icon: material/swap-horizontal
---

# Porting a model to the GPU

Every GPU model in the repository was written after its CPU counterpart worked, and a new port should follow the same order to keep the pair comparable.
A port that seeds itself through the CPU model's `init` starts from the same tick 0, and any divergence after that point belongs to the port alone.

Pick the GPU trait matching the topology you already have.

| You have | You want |
|---|---|
| [`GridModel`](grid-models.md) | [`GpuGridModel`](gpu-grid-models.md) |
| [`AgentModel`](agent-models.md) | [`GpuAgentModel`](gpu-agent-models.md) |

## Reuse the parameter list

Neither GPU trait prepends anything to your parameter list.
A GPU model spells out its whole descriptor list itself, which leaves a port free to hand back its counterpart's list verbatim.

```rust
fn param_descriptors() -> Vec<ParamDescriptor> {
    agent_model_param_descriptors::<BoidsModel>()
}
```

Both backends then take the same vector in the same order, and moving a slider on one gives you the same simulation on the other.
Read the engine's own three parameters back out by the names `cpu::agent_engine` gives them, and route the rest through the CPU model's `from_params`.

## Seed through the CPU `init`

```rust
fn seed_buffers(geom: &Geometry, params: &[ParamValue], seed: Option<u64>) -> Vec<Vec<u8>> {
    let mut lanes = BoidLanes::alloc(geom.num_agents as usize);
    let mut rng = seed.map_or(AGENT_INIT_SEED, mix_seed);
    BoidsModel::init(&mut lanes, geom.extent, split_params::<BoidsModel>(params).0, &mut rng);
    // ... pack the lanes into the buffer layout the shaders read
}
```

This call belongs in `seed_buffers` and nowhere else in a port.
Confining it there means the initial state is imported rather than reimplemented, and the port has no room to drift from its counterpart.

The part that genuinely changes is the packing.
Boids interleaves `pos_x`/`pos_y` into the `vec2<f32>` layout its shaders read, and turns the CPU's palette-index colour lane into packed RGBA, since a GPU model draws its colours directly.

## Tick 0 and after

**Tick 0 is bit-identical** across all four ports, because it comes straight from the CPU `init`.

**After tick 0 it depends on the model.**

`gpu_game_of_life`

:   Fully deterministic and bit-identical throughout, so its CPU counterpart serves as the correctness oracle and comparing the two is a plain equality check.

`gpu_sir`

:   Diverges immediately.
    The CPU RNG is `xorshift64` over `u64`, while the GPU RNG is `pcg_hash` over `u32`, because WGSL has no 64-bit integers.
    The two are counterparts in role but produce different streams.

`gpu_ants`

:   Diverges for the same reason, but a run still replays, because deposits combine with `max` and max is order-independent.

`gpu_boids`

:   Diverges even though nothing draws random numbers during the step.
    The neighbour index never fixes the order within a cell, and float addition is not associative, so the neighbour sums differ in their last bits and the trajectories walk apart from there.

A port whose stream differs cannot be checked by equality, and it needs a different kind of oracle.
Compare against a distribution or an invariant instead: population conservation, a rate matching its closed form, or a quantity that must never decrease.
See [determinism and testing](determinism.md).

## Reuse the palette

The colours end up living in two places, because the stats UI reads `PALETTE` while a display shader writes RGBA directly.
Take them from the CPU model's `const` and pack them for the uniform instead of retyping them, the way `gpu_boids` builds its `packed_heading_palette`.

A grid model's display shader bakes them into WGSL constants instead, and keeping those in agreement remains your responsibility.

## Ask before you allocate

A model that ran fine at 4096² on the CPU might not fit the device at all.
Before anything is created, `gpu/capacity.rs` computes buffer sizes, texture dimensions and per-pass storage-binding counts from what the model declares, and checks them against the device.
The app disables Build with a readable reason, and both engines assert with the same message.

Two limits shape ports in practice.

- One logical buffer has to fit inside one storage binding.
  The baseline caps that at 128 MiB, and the engine raises it to whatever the adapter reports.
- Storage bindings per shader stage sit at 8 in the WebGPU baseline, and the engine asks for exactly what the widest model needs rather than for headroom.
  If a pass needs nine storage buffers, restructure the pass rather than asking for a higher limit.

A registry test builds every GPU model on a stock baseline device and asserts at the same time that the capacity check agrees, so an over-reported pass count fails there.

## Check it against the counterpart

Register the port, then run both sides.

```bash
cargo run --release -p henad-cli -- boids --steps 1000 --reps 3
cargo run --release -p henad-cli -- gpu_boids --steps 1000 --reps 3
```

A CPU run against a GPU run measures throughput, and it says nothing about correctness.
For correctness, compare like with like: the same backend, the same seed, and the invariants holding on both sides.

## Next

- [Shaders and bindings](shaders.md) covers the WGSL side.
- [Determinism and testing](determinism.md) covers the oracles a diverging port needs.
- [Registering a model](registering.md) describes the last step for either backend.
