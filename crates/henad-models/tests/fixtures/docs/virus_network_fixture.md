# Generating the Virus on a Network consistency fixture

> [!note]
> This is a distributional test of the Virus on a Network model.
> For deterministic self-consistency checks, see `crates/henad-models/tests/consistency_virus_network.rs`.
> The comparison itself is `scripts/compare_network.py`.

As with SIR, there is no fixture file and no assertion in `cargo test`.
The model is stochastic, the two engines draw from different generators, and a single run carries no information.
The output is a report over many replicates.

## Reference model

The reference is Virus on a Network from the NetLogo Models Library (_Sample Models > Networks > Virus on a Network_, Stonedahl and Wilensky, 2008).
It is not redistributed here.
Open it from the library, then make the two edits in [§Procedure](#procedure): one swaps its network generator for the one Henad uses, and one adds two procedures that write a replicate to CSV.

## Rules

Each tick, in this order:

1. Every node advances its check timer by one, and a timer that reaches `virus_check_frequency` goes back to 0.
2. Every node that was infected at the start of the tick tries once to infect each susceptible neighbour, with probability `virus_spread_chance`.
3. Every infected node whose timer is 0 recovers with probability `recovery_chance`.
   A node that recovers becomes resistant with probability `gain_resistance_chance`, and susceptible otherwise.

Step 3 sees the nodes infected in step 2.
A node can be infected and recover in the same tick, in both engines.
A resistant node stays resistant.
Each node's timer starts at a uniform draw from `0..virus_check_frequency`.

Henad writes step 2 in pull form: a susceptible node draws once for each infected neighbour, and stops at the first success.
With the infected set fixed at the start of the tick in both engines, the chance of becoming infected is `1 - (1 - c)^k` for `k` infected neighbours either way.

### Network

The graph is undirected, with `m = n * k / 2` distinct links between `n` nodes and no self-links, chosen uniformly (the Erdős–Rényi G(n, m) model).

NetLogo's own generator is different.
It repeatedly links a random node to its nearest unlinked neighbour until the link count is reached.
The result is a spatially clustered network.
Henad's `Geometric` choice is a random geometric graph, also spatial but built by linking every pair within a radius.
The comparison uses G(n, m) on both sides.

### Not compared

- Henad's `directed` parameter, the "Rewire a link" action and the `keep_rewiring` parameter have no counterpart in the library model, and stay off.
- NetLogo stops `go` once no node is infected.
  Henad keeps stepping, and nothing can change after that point.
  Both write the same rows either way.
- Node positions and the layout.
  The rules never read them.

## Parameters

| NetLogo                  | value | Henad `--set`                      |
| ------------------------ | ----- | ---------------------------------- |
| `number-of-nodes`        | 1000  | `num_agents=1000`                  |
| `average-node-degree`    | 6     | `average_node_degree=6`            |
| `initial-outbreak-size`  | 10    | `initial_outbreak_size=10`         |
| `virus-spread-chance`    | 2.5   | `virus_spread_chance=0.025`        |
| `virus-check-frequency`  | 2     | `virus_check_frequency=2`          |
| `recovery-chance`        | 5     | `recovery_chance=0.05`             |
| `gain-resistance-chance` | 30    | `gain_resistance_chance=0.3`       |

NetLogo's chances are percentages, and Henad's are fractions shown as percentages in the app.
NetLogo draws recovery and resistance as `random 100 < chance`, an integer draw, where Henad draws a real number.
The two agree whenever the chance is a whole number of percent, as both are here.

The `number-of-nodes` slider stops at 300.
A value set from code is not clamped, so the procedure below sets 1000 directly.

> [!note]
> **Model defaults are not used.**
> At NetLogo's defaults the network has 150 nodes, and every statistic is noisy.
> The 3 initial infections let the outbreak die out early in some runs.
> The distribution then has two humps, and a comparison of means handles it badly.
> Henad's defaults have 10,000 nodes and the same small outbreak.
> A check frequency of 2 makes the timer matter, where at 1 every timer is always 0.
> With the other values as in the table and `gain_resistance_chance` at its default of 5%, the infection declines slowly.
> About a third of the nodes are still infected at tick 600, and a fifth at tick 1000.
> The final resistant fraction then depends on when the run stops.
> At 30% the infection had died out by tick 1000 in 794 of 800 runs, and none of the other six had more than three infected nodes left.
> The final counts have all but stopped moving.

## Procedure

Open Virus on a Network from the Models Library in NetLogo 7.0.4.
Save it with File > Save As into an empty folder of its own, such as `netlogo/virus/`.
The CSVs land beside the saved model, and the saved copy keeps the edits.
Left in the library, the model would write into the NetLogo installation, in the folder it shares with Team Assembly.

Then make two edits in the Code tab.

First, replace the body of `setup-spatially-clustered-network` with a G(n, m) loop.
`create-link-with` does nothing when the two nodes are already linked.
The loop keeps drawing pairs until there are `m` distinct links.
Keeping the name means `setup` needs no change.
The `layout-spring` loop goes with the old body.
It only moved nodes.

```netlogo
to setup-spatially-clustered-network
  let num-links (average-node-degree * number-of-nodes) / 2
  while [ count links < num-links ]
  [
    ask one-of turtles [ create-link-with one-of other turtles ]
  ]
end
```

Second, append these two procedures at the end of the Code tab.

```netlogo
to write-counts [ tick-number ]
  file-print (word tick-number ","
    count turtles with [ not infected? and not resistant? ] ","
    count turtles with [ infected? ] ","
    count turtles with [ resistant? ])
end

to run-replicate [ filename rng-seed total-ticks ]
  set number-of-nodes 1000
  set average-node-degree 6
  set initial-outbreak-size 10
  set virus-spread-chance 2.5
  set virus-check-frequency 2
  set recovery-chance 5
  set gain-resistance-chance 30
  random-seed rng-seed
  setup
  if file-exists? filename [ file-delete filename ]
  file-open filename
  file-print (word "# engine: NetLogo " netlogo-version)
  file-print "# model: Virus on a Network (Models Library), G(n, m) generator"
  file-print (word "# seed: " rng-seed)
  file-print (word "# nodes: " count turtles ", links: " count links)
  file-print "tick,Susceptible,Infected,Resistant"
  write-counts 0
  let t 0
  repeat total-ticks [
    go
    set t t + 1
    write-counts t
  ]
  file-close
end
```

The row number comes from `t` rather than from `ticks`.
Once no node is infected, `go` stops before its `tick`, and `ticks` stops advancing.

Uncheck View Updates in the Interface tab first, or drag the Model Speed slider to its far right.
At the default speed NetLogo runs this model no faster than its frame rate, 30 ticks a second.
At that rate, 400 runs of 1000 ticks take almost four hours.
Then run the following in the Command Center.
It writes 400 files into the folder holding the saved model.

```netlogo
foreach (range 1 401) [ s -> run-replicate (word "virus_netlogo_" s ".csv") s 1000 ]
```

One matching Henad run:

```bash
cargo run --release -p henad-cli -- virus_network \
  --set num_agents=1000 --set average_node_degree=6 --set initial_outbreak_size=10 \
  --set virus_spread_chance=0.025 --set virus_check_frequency=2 \
  --set recovery_chance=0.05 --set gain_resistance_chance=0.3 \
  --set directed=false --set network=Random --set keep_rewiring=false \
  --steps 1000 --seed 1 --export-stats virus_henad_001.csv
```

Both write `tick,Susceptible,Infected,Resistant` with a row for tick 0.
One script reads either.

## Margins

Three summary statistics per run: **peak infected fraction**, **tick of that peak**, and **final resistant fraction**.

We compute the difference in means, its 95% confidence interval, and require that the whole interval lies inside the margin.
The difference is Henad's mean minus the reference's.

Margins come from Henad's own measured run-to-run spread, with the other parameters as above and the outbreak at 1% of the nodes:

| nodes | seeds | peak I frac            | tick of peak | final R frac           |
| ----- | ----- | ---------------------- | ------------ | ---------------------- |
| 500   | 30    | 0.6136 ± 0.0289 (4.7%) | 72.6 ± 6.39  | 0.7615 ± 0.0184 (2.4%) |
| 1000  | 30    | 0.6114 ± 0.0176 (2.9%) | 72.2 ± 5.93  | 0.7611 ± 0.0121 (1.6%) |
| 2000  | 30    | 0.6121 ± 0.0156 (2.6%) | 71.6 ± 5.93  | 0.7591 ± 0.0094 (1.2%) |

We choose 1000 nodes.
At 2000 nodes the two fractions spread a little less.
The tick of the peak spreads by about six ticks at every size, since the infected count stays near its peak for tens of ticks and small fluctuations decide which of those ticks is highest.
More seeds narrow all three, and with view updates off they cost little.
Headless NetLogo ran 400 seeds of each wrong engine from [§Checking the margins](#checking-the-margins) in about a minute and a half.

**Margins for 1000 nodes, 400 seeds per engine**, with the spread taken from 400 Henad runs:

| statistic                | margin     | 95% CI half-width | headroom |
| ------------------------ | ---------- | ----------------- | -------- |
| peak infected fraction   | ±0.0075    | 0.0025            | 3.0x     |
| tick of peak             | ±2.5 ticks | 0.84              | 3.0x     |
| final resistant fraction | ±0.0053    | 0.0018            | 3.0x     |

CI half-width is $\frac{1.96 \sigma \cdot \sqrt{2}}{\sqrt{400}}$, where $\sqrt{2}$ arises because the difference of two independent means has twice the variance of one.

The headroom is three, where SIR's is closer to four on its two fractions.
Fifty seeds per engine, as SIR's comparison uses, would give margins of about ±0.021, ±7.1 ticks and ±0.015 at the same headroom.
Both wrong engines in [§Checking the margins](#checking-the-margins) read equivalent under those, at 200 seeds per engine and at 400.
At 50 seeds the second of them reads inconclusive on the peak, and the extra replicates an inconclusive verdict asks for turn it equivalent.

### Checking the margins

Henad against itself checks the script and the margins.
It says nothing about agreement with NetLogo.
Seeds 1 to 400 against seeds 401 to 800 read equivalent on every statistic:

| statistic                | seeds 1–400       | seeds 401–800     | difference (95% CI) | margin  | verdict    |
| ------------------------ | ----------------- | ----------------- | ------------------- | ------- | ---------- |
| peak infected fraction   | 0.61361 ± 0.01826 | 0.61379 ± 0.01769 | -0.00018 ± 0.00250  | ±0.0075 | EQUIVALENT |
| tick of peak             | 73.09 ± 6.06      | 73.30 ± 6.51      | -0.21 ± 0.87        | ±2.5    | EQUIVALENT |
| final resistant fraction | 0.76125 ± 0.01282 | 0.76101 ± 0.01261 | 0.00023 ± 0.00177   | ±0.0053 | EQUIVALENT |

Two wrong engines check that the margins can fail.
Each is a copy of the saved model from [§Procedure](#procedure), in a folder of its own, that breaks one of the orderings in [§Rules](#rules).
Both ran for seeds 1001 to 1400, with `foreach (range 1001 1401)` in place of the procedure's range, and were compared against Henad's seeds 1 to 400.

In the first, `spread-virus` asks every turtle and tests `infected?` inside the block, as `ask turtles [ if infected? [ ... ] ]`.
A node infected earlier in the same `ask` then spreads in the same tick.
The outbreak peaks about three and a half ticks earlier, and the tick of the peak reads different (3.42 ± 0.87 against ±2.5).
The peak reads inconclusive (-0.00795 ± 0.00247 against ±0.0075), and the final resistant fraction equivalent.
Against Henad's seeds 401 to 800 the tick of the peak reads different again (3.63 ± 0.90).
Both times it clears the margin only narrowly, and another draw could read inconclusive.
Reading equivalent would take a difference about four standard errors below the one measured.

In the second, `go` calls `do-virus-checks` before `spread-virus`.
A node infected in a tick then cannot recover in the same tick.
The peak rises by about 0.016 and reads different (-0.01573 ± 0.00249 against ±0.0075), while the other two read equivalent.

## Running the comparison

`scripts/compare_network.py` reads a directory of reference CSVs, generates the Henad side with the settings above, and prints one verdict per statistic.

```bash
cargo build --release -p henad-cli
uv run --project scripts scripts/compare_network.py virus_network --reference netlogo/virus --generate 400
```

`--reference` is the folder holding the saved model, and the script reads only the `virus_*.csv` files in it.
The Henad side goes to `henad_virus_network` beside that folder, unless `--henad` names another.
`--generate` refuses a folder holding any `virus_*.csv` file other than the ones it writes.
Otherwise those files would count as replicates.

The verdicts and exit codes are those of `compare_sir.py`: `EQUIVALENT` (0), `DIFFERENT` (1), or `INCONCLUSIVE` (2) when the interval is wider than the margin and more replicates are needed.
A missing or malformed input, or a failed `henad-cli` run, exits 3.
The two-sample KS test is printed as a diagnostic only, for the reason given in `sir_fixture.md`.

## Result

400 replicates per engine, on 1000 nodes of average degree 6, with an outbreak of 10 and the other parameters as in [§Parameters](#parameters), 1000 ticks:

| statistic                | Henad             | NetLogo 7.0.4     | difference (95% CI) | margin  | verdict    |
| ------------------------ | ----------------- | ----------------- | ------------------- | ------- | ---------- |
| peak infected fraction   | 0.61361 ± 0.01826 | 0.61421 ± 0.01787 | -0.00059 ± 0.00251  | ±0.0075 | EQUIVALENT |
| tick of peak             | 73.09 ± 6.06      | 73.50 ± 6.90      | -0.41 ± 0.90        | ±2.5    | EQUIVALENT |
| final resistant fraction | 0.76125 ± 0.01282 | 0.76136 ± 0.01337 | -0.00012 ± 0.00182  | ±0.0053 | EQUIVALENT |

KS diagnostics were unremarkable (`D` 0.04 to 0.06, `p` 0.47 to 0.91).

The NetLogo side ran in the NetLogo 7.0.4 app, from a copy of the library model carrying the two edits in [§Procedure](#procedure), with View Updates unchecked.
The 400 runs took about two and a half minutes.
Every file reports 1000 nodes and 3000 links.
The slider's limit of 300 did not clamp the node count.
