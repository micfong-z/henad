# Generating the ants consistency fixture

Produces `crates/henad-models/tests/fixtures/ants/*.txt`, the reference state that `consistency_ants.rs::matches_every_reference_fixture` compares against.

## Reference model

No engine ships this model.
MASON's `sim.app.antsforage` and krABMaga's `antsforaging` are where Henad's ants comes from, but neither is the same simulation: Henad combines deposits with `max` rather than last-writer-wins, reads the whole field before writing any of it, and seeds its generator per chunk per tick.
So every engine in the comparison implements the rules below, and the ports live under `benchmarks/<engine>/`.

## Rules

The world is a bounded integer lattice, `width` by `height`, with no wrapping.
Every ant occupies one cell.
Two scalar layers cover the lattice, `to_food` and `to_home`, and a static site map marks the nest, the food source and the obstacles.

A tick runs three passes in order.

### 1. Deposit

Every ant computes one value from the layer it is currently laying, `to_food` when carrying and `to_home` when searching.
Writing `f` for that layer and `(x, y)` for the ant's cell:

```
best = max(f[x, y], f[x, y] * cutdown + reward)
for each of the eight neighbours (nx, ny) inside the lattice:
    cut  = cutdown ** sqrt(2)   if the neighbour is diagonal
    cut  = cutdown              otherwise
    best = max(best, f[nx, ny] * cut + reward)
```

The centre counts as an orthogonal neighbour.
Obstacles are not skipped here; only the lattice edge stops the scan.
Nothing is written yet.

### 2. Advect

Every ant then moves, reading the layer for the trip it is *not* making: carrying reads `to_home`, searching reads `to_food`.
It scans the eight neighbours in **column-major order**, `dx` outer from `-1` to `1` and `dy` inner from `-1` to `1`, skipping any that leave the lattice or hold an obstacle, and keeps the largest value.

```
best = -1, count = 2
for each passable neighbour (nx, ny) in column-major order:
    m = trail[nx, ny]
    if m > best:                       count = 2
    if m > best or (m == best and draw() < 1 / count):
        best = m; target = (nx, ny)
    count += 1
```

`count` starting at 2 rather than 1 is an off-by-one inherited from the reference.
It gives the first neighbour visited `2/(k+1)` against `1/(k+1)` for the rest, which drifts ants up and left.
It is reproduced rather than corrected, so the ports stay the same simulation.

Then one further branch:

- if `best == 0` and the ant has moved before, then with probability `momentum` it continues in its previous direction, provided that cell is passable;
- otherwise, with probability `random_action` it takes a uniform step in `{-1, 0, 1}²`, provided the step is non-zero and the cell is passable.

The ant's recorded last step becomes the direction actually taken, encoded `(dx + 1) * 3 + (dy + 1)`, with `255` meaning it has not moved yet.
Its reward drops to zero, since the deposit pass spent it.
Landing on the nest while carrying sets `reward = reward_param`, clears `has_food` and counts one delivery; landing on the food source while empty sets `reward = reward_param` and sets `has_food`.

### 3. Field update

Each layer takes the largest of its current value and every deposit landing in that cell, then decays:

```
v = max(v, deposits into this cell)
v = v * evaporation
v = 0 if v < 1e-14
```

Reading the old field everywhere and writing the new one afterwards is what makes the tick order-independent.

## Site layout

Placed proportionally, so the lattice size stays a parameter.
At 200 by 200 this is where the reference hard-codes them.

- nest at `(floor(0.875 w), floor(0.875 h))`
- food at `(floor(0.125 w), floor(0.125 h))`
- obstacles: with `s = 0.407 * 200 / w`, a cell `(x, y)` is an obstacle when either blob centred at `(0.500 w, 0.725 h)` or `(0.450 w, 0.275 h)` contains it, where a blob at `(cx, cy)` contains `(x, y)` when

  ```
  a = ((x - cx) + (y - cy)) * s
  b = ((x - cx) - (y - cy)) * s
  a² / 36 + b² / 1024 <= 1
  ```

Sites are written after the blobs, so neither is buried.

## The scenario

Randomness in this model lives entirely inside the advect pass: a tie between two neighbours draws, and so do the momentum and random-action branches.
Two engines with different generators can therefore agree on the rules and still disagree on every trajectory, which is why there was no cross-engine check here before.

The gate removes all three draws instead of trying to match generators.
`momentum` and `random_action` are zero, so neither branch can fire.
The field is seeded with values that are distinct within every 3 by 3 neighbourhood, so no tie is ever reached and no draw is ever taken.
What is left is the rules, and any correct implementation reaches the same state from any generator.

The tie-break distribution is the one thing this does not check.
That is deliberate: it is the reference's defect, reproduced on purpose, and it is disclosed in the write-up instead.

**World** 32 by 32, `cutdown` 0.9, `reward` 1.0, `momentum` 0, `random_action` 0, `evaporation` 0.999.

**Field** stated as a formula, so no data file is needed:

```
to_food[x, y] = ((7 x + 13 y) mod 97 + 1) / 98
to_home[x, y] = ((11 x + 5 y) mod 89 + 1) / 90
```

Two cells collide only when `7 dx + 13 dy` (or `11 dx + 5 dy`) is a multiple of the modulus, and across a 3 by 3 window those sums stay well inside one period.
Every value lies in `(0, 1]`, which also keeps the best neighbour off zero and the momentum branch out of reach.

**Ants** twelve, covering the four corners, a top edge, both sites, two cells beside an obstacle, one ant carrying a previous direction, and two sharing a cell so the deposit combine has something to merge.

| # | x | y | last step | has food | reward |
|---|---|---|---|---|---|
| 0 | 0 | 0 | 255 | 0 | 1.0 |
| 1 | 31 | 0 | 255 | 1 | 1.0 |
| 2 | 0 | 31 | 255 | 0 | 0.5 |
| 3 | 31 | 31 | 255 | 1 | 0.5 |
| 4 | 16 | 0 | 255 | 0 | 1.0 |
| 5 | 4 | 4 | 255 | 0 | 1.0 |
| 6 | 28 | 28 | 255 | 1 | 1.0 |
| 7 | 8 | 12 | 255 | 0 | 1.0 |
| 8 | 22 | 20 | 255 | 1 | 1.0 |
| 9 | 15 | 15 | 7 | 0 | 1.0 |
| 10 | 15 | 15 | 255 | 0 | 0.25 |
| 11 | 15 | 16 | 2 | 1 | 1.0 |

At this size the nest is `(28, 28)` and the food source `(4, 4)`, so ants 5 and 6 start on them.
Ant 7 has an obstacle at `(9, 13)` and ant 8 one at `(21, 19)`.

**Steps** 5.
Deposits eventually recreate ties in the evolved field, and the run stops being generator-independent at nine ticks, so five leaves headroom.
`consistency_ants.rs::the_gate_scenario_does_not_depend_on_the_random_stream` is what holds that line.

## Procedure

Each engine's harness writes the fixture itself:

```bash
uv run --project scripts scripts/validate_ports.py --engines <engine> --models ants
```

That runs `benchmarks/<engine>/` in validate mode, writes `crates/henad-models/tests/fixtures/ants/ants_lattice_<engine>.txt`, and compares it against Henad.

## Fixture format

A `# key: value` header, then the agent rows, then the two layers, each row of a layer being one lattice row of `width` values ascending in `x`, and the rows ascending in `y`.

```text
# engine: Mesa 3.3.0
# model: Henad ants rule, ported for this comparison
# scenario: ants-lattice
# steps: 5
# width: 32
# height: 32
# agents: 12
# --- agents: x y last_step has_food reward
1 1 5 0 0
...
# --- to_food
9.897959e-1 ...
...
# --- to_home
...
```

## Tolerance

`1e-6` relative.
Henad holds the field in `f32` and most engines hold it in `f64`, so the two agree to about seven digits and no further.
Agent positions, `last_step` and `has_food` are integers and must match exactly.
