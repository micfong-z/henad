# Generating the boids consistency fixture

Produces `crates/henad-models/tests/fixtures/boids/boids_netlogo.txt`, the reference agent state that `consistency_boids.rs::matches_every_reference_fixture` compares against.

## Reference model

Flocking (Wilensky 1998) in NetLogo is a different algorithm to Henad's, so a custom NetLogo model was written to implement the Henad specification. The reference model is in `crates/henad-models/tests/fixtures/boids/boids_netlogo.nlogox`. This model is created under the rules as stated in [§Rules](#rules).

## Rules

For each agent `i`, against every other agent `j`, with all offsets taken the short way round the torus:

1. `d = wrap(pos_j - pos_i)`
2. if `|d|² < protected_range²`, accumulate `close -= d`
3. if `|d|² < visual_range²`, accumulate `avg_v += vel_j`, `sum_d += d`, `n += 1`

Then:

```
new_v = vel_i + close * separation
if n > 0:
    new_v += (avg_v / n - vel_i) * alignment + (sum_d / n) * cohesion
```

Then clamp the magnitude of `new_v` into `[min_speed, max_speed]`, preserving direction; if it is
exactly zero, set it to `(min_speed, 0)`. Finally `pos_i = (pos_i + new_v) mod world`.

## World setup

In _Settings_:

- **Location of origin**: Corner, Bottom Left.
- `max-pxcor` **99**, `max-pycor` **99**. The dialog should read `Torus: 100 x 100`.
- both **wrap** boxes checked

Wrapping is required, not cosmetic: `setxy` errors on out-of-range coordinates when it is off, and
the port relies on the world folding positions for it.

## Coordinates

Positions live in `xcor`/`ycor`, so NetLogo's own spatial primitives apply. NetLogo's continuous coordinates are patch-centred, i.e. `max-pxcor` 99 spans `[-0.5, 99.5)` while Henad's world is `[0, 100)`. Patches are unit squares centred on integer coordinates, so the world reaches half a patch past the extreme centres. Hence, `export-agents` applies `mod world-size` to bring coordinates back into Henad's
range.

`in-radius` is inclusive (`<= r`) where Henad's visual test is strict (`< r`), so it is used as a candidate set that the loop then filters exactly, in order to use the same shape as the spatial hash query it is checking.

## Scenario 1: `boids-8`

8 agents in a 100x100 world chosen so a single step tests every branch of the kernel:

| #   | pos      | vel        | exercises                                                |
| --- | -------- | ---------- | -------------------------------------------------------- |
| 0   | (10, 10) | (1, 0)     | inside `protected_range` of 1 (separation)               |
| 1   | (13, 10) | (1, 0)     | "                                                        |
| 2   | (30, 30) | (0, 2)     | inside `visual_range` of 3 only (alignment and cohesion) |
| 3   | (42, 30) | (2, 0)     | "                                                        |
| 4   | (75, 75) | (0.5, 0.5) | isolated; speed 0.707 clamps **up** to `min_speed`       |
| 5   | (2, 50)  | (-3, 0)    | 4 apart from 6 only across the wrap seam                 |
| 6   | (98, 50) | (3, 0)     | "                                                        |
| 7   | (60, 10) | (6, 6)     | isolated; speed 8.49 clamps **down** to `max_speed`      |

Every value is representable exactly in both precisions. The coefficients are stronger than the model defaults since we need the change to be large enough to see in one step.

Parameters: `visual_range` 20, `protected_range` 5, `separation` 0.5, `alignment` 0.25, `cohesion` 0.125, `max_speed` 8, `min_speed` 2.

## Scenario 2: `sine-42`

42 boids along two sine periods, each seeing 2 to 8 others. This aims to test an non-uniform distribution of agents.

Since `sin` in `f32` and `f64` differ in the last bits, so as to make sure that both engines begin with bit-identical values, values below are the curve already evaluated and snapped to a 1/8 lattice.

Parameters are identical to `boids-8`.

## Procedure

Open the model at `crates/henad-models/tests/fixtures/boids/boids_netlogo.nlogox` in NetLogo, and run the following in the Command Center.

For the first scenario:

```netlogo
setup-boids-8
repeat 1 [ go ]
export-agents "boids_8.txt" "boids-8" 1
```

For the second scenario:

```netlogo
setup-sine-42
repeat 1 [ go ]
export-agents "boids_sine42.txt" "sine-42" 1
```

Move the results to `crates/henad-models/tests/fixtures/boids/`.

Only one tick is run since Henad holds agent state in `f32` and NetLogo in `f64`, so results differ in the last few bits from the first tick onward, and boids is chaotic enough that the gap compounds until the two flocks are simply different.

The test's tolerance is 1e-5 absolute.

---

This document is assisted with Claude Opus 5, with heavy human edits after generation.
