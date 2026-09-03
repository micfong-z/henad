---
title: Fields
description: The grid slot underneath an agent model, and the three field implementations Henad ships.
icon: material/layers-outline
---

# Fields

A field is the grid slot an [agent model](agent-models.md) sits over.
It owns the cells, updates them once per tick and draws them.
An `AgentModel` names one as its `Field` associated type, and Henad ships three implementations.

| Type | Contents | Used by |
|---|---|---|
| `NoField` | No grid at all | `boids` |
| `ScalarField<S>` | `f32` layers written by agent deposits and decayed each tick | `ants` |
| `CaField<M>` | A whole [`GridModel`](grid-models.md) underneath the population | — |

Pick `ScalarField` for agents that leave deposits behind them and let those deposits fade.
`CaField` puts a live cellular automaton under the population instead.

`NoField` is the default shape and costs nothing: it reports no grid view, allocates no deposit lanes, and has an `update` that does nothing.

## `ScalarField`

`ScalarField<S>` holds `S::FIELDS` double-buffered `f32` grids over one shared scatter scratch.
Alongside them sit a `u8` layer of static terrain and a `u8` layer of quantised palette indices.
Ants runs two layers, a route-to-food trail and a route-to-home trail.

The mechanics belong to the engine, and your model fills in the rules it cannot know through `ScalarFieldSpec`.

| Item | Role |
|---|---|
| `FIELDS` | How many `f32` layers share the grid and the scratch |
| `COMBINE` | How deposits landing in the same cell combine |
| `PALETTE` | Colours for the quantised display layer |
| `build_sites` | Static terrain, written once at construction |
| `decay` | One cell's value, one tick on |
| `quantize` | One cell's palette index, from the terrain and every layer's value |

A tick over the field runs in a fixed order: scatter the deposits, then decay, then swap.
Decay comes after the merge, and a fresh deposit is therefore already one step old by the time anything reads it.

### Deposits

The agent side fills two lanes per agent, one cell index and one value per layer.

```rust
pub struct Deposits {
    pub cell: Vec<u32>,
    pub values: Vec<Vec<f32>>,
}
```

An agent that writes one layer leaves the others at the combine's identity, which keeps every lane dense and saves each layer from needing its own agent list.
The lanes are allocated once for the population and reused every tick, and ants fills them from `run_deposit_pass`.

### Combining

```rust
pub enum Combine {
    Max,
    SumFixed { scale: f32 },
}
```

Both variants are commutative and associative, and the scatter relies on that property to run in parallel at all.

`Max` takes the largest deposit and needs non-negative values, with `0.0` as its identity.
`SumFixed` totals in fixed point at `scale` steps per unit.
Float addition is not associative, and a float sum would come out differently depending on how the values were grouped.

Ants uses `Max`.
Its `deposit_value` floors at the value the cell already holds, so a maximum reproduces the reference model's plain overwrite without needing a write order.

### The scatter

`ScatterGrid` covers the one write pattern the rest of the engine cannot express, many agents depositing into the same cell, and it has two arms behind one API.

**Shadow.** Every rayon worker fills its own private grid without contention, and the grids are then reduced across workers per cell.
The scratch cost is `n_cells * workers`.

**Sorted.** A counting sort by cell permutes the values, so the reduce reads each cell's run contiguously.
This arm costs a pass over the agents and needs no per-worker grid.

The arm is picked at construction, from whether the shadow scratch fits a 256 MiB budget.
That judgement depends on the worker count, which puts a hard requirement on the two arms: **both must produce identical bits**.
Otherwise a model's results would depend on the machine it ran on.
A test pins each arm explicitly and checks that the two agree.

!!! warning "Atomics scale badly here"

    `fetch_max` scales negatively under contention, at 7.1 ms on one thread and 99.2 ms on four.
    The strategy choice rests on measurements from `benches/scatter.rs`, so re-run the benchmark rather than arguing from first principles.

    ```bash
    cargo bench -p henad-compute --bench scatter
    ```

### Drawing it

`GridView::cells` is `&[u8]` while a field layer is `f32`, and the layer therefore owns the quantisation.
`quantize` turns the terrain marker and every layer's current value into one palette index, and it runs from `prepare_view`, on publish rather than every tick.
See [palettes and views](views.md).

## `CaField`

`CaField<M>` puts a whole `GridModel` underneath a population.
The grid steps by `M`'s neighbourhood rule, the agent kernel reads it as a plain `&[u8]`, and the field takes no deposits.

`M::init` and `M::step_cell` behave exactly as they do for a standalone grid model.
A `GridModel` you have already written and tested drops in underneath a population unchanged.

## Parameters

A field declares its own parameters, which are appended after the model's own.
Both `from_params` calls receive their own 0-based slice, worked out from the descriptor lengths.
A model or a layer that gains a parameter therefore cannot shift the other's indices.

For ants, the composed list looks like this:

```text
0  num_agents        engine
1  world_width       engine
2  world_height      engine
3  update_cutdown    model
4  reward            model
5  momentum          model
6  random_action     model
7  evaporation       field
```

## Next

- [Agent models](agent-models.md) covers the trait a field sits under.
- [Writing an agent model](../guide/first-model/ants.md) builds a `ScalarField` end to end.
- [Writing fast models](performance.md) explains what the scatter costs at scale.
