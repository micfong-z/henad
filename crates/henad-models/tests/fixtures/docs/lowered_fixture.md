# Writing the hand-lowered fixtures

Produces `crates/henad-models/tests/fixtures/lowered/`, the graph files a lowered sheet is compared with.
`src/tests/lowered_fixtures.rs::every_lowered_fixture_reads` reads each one as a graph file and checks it.

| Directory | Files | Models |
| --- | --- | --- |
| `lowered/round1/` | `game_of_life.toml`, `vote.toml`, `sir.toml`, `forest_fire.toml`, `random_walk.toml`, `bounded_confidence.toml`, `boids.toml`, `ants.toml` | the eight models of round 1 of the #37 redesign, SIR with chances per tick |
| `lowered/rates/` | `sir_rates.toml`, `sird_competing.toml`, `sird_higher_wins.toml` | SIR with rates per day, and SIRD with two rates out of Infected, as competing risks and under "the higher change wins" |

No script generates these files and no reference engine is involved.
Each was written by hand, from the model's sheet and card, as the graph the sheet lowering would write.
They sit apart from `tests/fixtures/graphs/`, whose corpus loaders hold every file there to a stock model at tick 0.

## The models

Each file opens with a comment that states its model in the card's words, one sentence per line of the card.
The round 1 cards, as the files hold them:

```text
Game of Life
1  Alive neighbours = count of the 8 cells around me whose Status is Alive
2  Become Alive if Dead, when Alive neighbours is 3
3  Become Dead if Alive, when Alive neighbours is not one of 2, 3

Vote
1  B in my block = count of the 3 by 3 block, me included, whose Opinion is B
2  Become B, when B in my block is at least 5
3  otherwise become A

SIR
1  Infected neighbours = count of the 8 cells around me whose Health is Infected
2  Become Infected if Susceptible, with chance Infection chance for each of Infected neighbours
3  Become Recovered if Infected, with chance Recovery chance

Forest fire
1  Burning neighbours = count of the 4 cells around me whose Cover is Burning
2  Become Burning if Tree, when Burning neighbours is at least 1
3  otherwise become Burning if Tree, with chance Lightning chance
4  Become Empty if Burning
5  Become Tree if Empty, with chance Growth chance

Random walk
1  Repeat Steps per tick times:
2      Move by a random direction of length Step size

Bounded confidence
1  Like-minded = look at people within Hearing range, me included, only those whose Opinion is within Confidence of mine: average of their Opinion
2  Steer Opinion towards Like-minded's average opinion, strength Persuasion
3  Move by a random direction of length Wander
```

Boids and ants are round 1's cards of the stock models, and their files list the lines in their opening comments.
The rates files hold SIR's card with "at rate ... per day" in place of "with chance", and SIRD adds "Become Dead if Infected, at rate Death rate per day" as line 4.

## How each author form was written

The ids follow the sheet: a lane, parameter or action takes the id of its property, parameter or button, and a category variant the name of its state.
The model id is `r1_<model>` for round 1 and `r2_<model>` for the rates files, so no fixture replaces an example model.
Node ids and draw keys were numbered by hand, unique in each file.

| Author form | Graph |
| --- | --- |
| World, kinds, properties, states, parameters | the header, one GPU `[[target]]`, `[grid]` or `[agents]`, `[[category]]`, `[[lane]]` (Position with `role = "position"`, Heading as a `vec2f` lane), `[[field]]` per ground property, `[[param]]` |
| Starts as | one init pass per kind of thing, of `rand.chance`, `rand.float`, `vec.from_angle`, constants and shape tests over `ctx.cell_xy` |
| Measures | `[[stat]]` over generated stats passes: `stat.count_by`, `stat.mean`, `stat.sum`, `stat.counter`, a spread as two means and `f64` arithmetic after them |
| Colours | nothing for "by state", otherwise a view pass and a palette |
| Buttons | Start again as `runs` naming the init pass, Run once and Set everywhere as an action pass |
| The card | one tick pass per kind of thing |
| Count | `grid.count`, or a fold with a `count` accumulator |
| Look around | one region per source, one accumulator and `region.put` per summary, an average as a sum, a count and a guarded divide |
| Pick a cell | `grid.fold` carrying the best value, its cell and the tie count, with `rng.reservoir` over `rand.bits` for uniform ties, the ants' three folds fused into one |
| With chance, for each of n | `rand.chance` of 1 - (1 - p)^n, through `math.sub` and `math.powi` |
| Compare, All of, Choose, Depending on, Calculate | `cmp.*`, `logic.*`, `logic.select` chains, `cat.switch`, `math.expr` |
| Become | one `cat.transition` per property, one row per line in line order |
| Set, Move to | a `logic.select` chain from the bottom line up |
| Steer, Keep between, my new X | `math.add` of scaled terms onto the start value, then a clamp |
| Move by | `math.add` and `space.wrap`, and the Heading lane |
| Leave | `field.deposit` |
| Record | `counter.add` |
| When | `logic.and` of its condition into each change inside |
| Repeat | `region.repeat` with `sum`, `or` and `carry` accumulators, its `max` the parameter's maximum |

The rates files add the time scale.

- The tick length is a parameter, `tick_length`, default 1, in the rates' unit, here days.
  Its table sets `hidden = true`, which keeps it off the Parameters panel, and `tick_unit = "day"`, which names the unit.
  The entry then carries the time scale, and `--export-stats` and a sweep's `series.csv` write a `time` column, the tick times the tick length.
- A rate r on a line with n tries becomes the chance 1 - exp(-r × Δt × n) per tick, Δt being the tick length.
  The value exp(-r × Δt) reads only parameters, so the host computes it before the run, and `math.powi` raises it to the count.
- `sird_competing.toml` lowers lines 3 and 4 as one group with one draw u in [0, 1).
  The chance to leave Infected is t4 = 1 - exp(-(γ + μ) × Δt), line 3's threshold is t3 = t4 × γ / max(γ + μ, 10^-30), and `cat.transition` takes line 3 when u < t3 and line 4 when u < t4.
- `sird_higher_wins.toml` draws lines 3 and 4 independently, line 3 first, for contrast.
  A cell then dies with chance exp(-γ × Δt) × (1 - exp(-μ × Δt)) per tick.
  The split is biased towards recovery.

## Checking a file

Every file was run before it was committed, with a build of the workspace at the commit that adds it:

```bash
cargo build -p henad-cli
for file in crates/henad-models/tests/fixtures/lowered/*/*.toml; do
  ./target/debug/henad-cli --model-file "$file" --steps 20 --reps 1 \
    --export-stats "/tmp/$(basename "$(dirname "$file")")-$(basename "$file" .toml).csv"
done
```

A file with buttons takes one `--act <id>@10` per action id as well (`randomise` and `clear` for Game of Life, `seed_outbreak` for SIR, `replant` for the forest fire, `mix_opinions`, `randomise_headings` and `reset_colony` for bounded confidence, boids and ants).
Each run must read, check, compile and write 21 rows.
Then read the CSV and check the model's behaviour by eye:

- Game of Life: the Alive count at tick 0 is near Initial density times the cells, and it is 0 after Clear.
- Vote: Holding B starts near Initial share of B times the cells and changes less from tick to tick as the blocks form.
- SIR, SIR with rates and SIRD: Infected rises from about 1% of the cells, the three or four counts sum to the cells at every tick, and Recovered never falls.
- Forest fire: the counts sum to the cells, Burning is 0 at tick 0 and becomes positive within a few ticks, and Replant draws the cover again.
- Random walk: the average position stays near the centre of the world.
- Bounded confidence: the spread of opinion falls, apart from the press of Mix opinions, which draws the opinions again.
- Boids: the averages stay finite, and Randomise headings changes the average velocity at its tick.
- Ants: the trails grow from 0, and Reset colony clears them at its tick.

A file edited later is checked again the same way before it is committed.
`every_lowered_fixture_reads` then holds its header to `kind = "graph"` and the graph checker to no error.
