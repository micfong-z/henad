# Generating the Team Assembly consistency fixture

> [!note]
> This is a distributional test of the Team Assembly model.
> For deterministic self-consistency checks, see `crates/henad-models/tests/consistency_team_assembly.rs`.
> The comparison itself is `scripts/compare_network.py`.

As with SIR, there is no fixture file and no assertion in `cargo test`.
The model is stochastic, the two engines draw from different generators, and a single run carries no information.
The output is a report over many replicates.

## Reference model

The reference is Team Assembly from the NetLogo Models Library (_Sample Models > Networks > Team Assembly_, Bakshy and Wilensky, 2007), after Guimerà, Uzzi, Spiro and Amaral (2005).
It is not redistributed here.
Its rules are used unchanged, and [§Procedure](#procedure) only adds two procedures that write a replicate to CSV.

## Rules

At setup, one team of `team_size` nodes is created and every pair of them is linked.
All of them count as incumbents, so their links are incumbent-incumbent.

Each tick, in this order:

1. Every node alive at the start of the tick counts as an incumbent.
2. A team of `team_size` members is assembled one member at a time.
   With probability `1 - p` the member is a newcomer, a new node.
   Otherwise a second draw succeeds with probability `q`.
   If it succeeds and some member already has a neighbour outside the team, the member is picked uniformly from the non-members linked to a member.
   If not, it is picked uniformly from all non-members.
3. Every pair of members is linked.
   A new link is incumbent-incumbent, newcomer-incumbent or newcomer-newcomer by its two ends.
   A link that already existed becomes a previous collaborator link.
4. Every node that has gone more than `max_downtime` ticks without joining a team retires, and its links go with it.

A node that joined a team at tick `k` is alive through tick `k + max_downtime` and retires at the end of tick `k + max_downtime + 1`.
Setup nodes retire as if they had joined at tick 1, in both engines.
NetLogo first counts their downtime on the first tick.

### Not compared

- Henad's prepended node count is the size of the starting population, grouped into teams.
  At 4, equal to `team_size`, it is NetLogo's setup, and the procedure uses that value.
- NetLogo errors when a pick has no non-member to choose from, where Henad adds a newcomer instead.
  Neither happens in a run that starts from one team, since at least `team_size` nodes are alive at the start of every tick and a pick sees at most `team_size - 1` members already chosen.
- Node positions, the layout and node sizes.
  The rules never read them.
  NetLogo places newcomers at the origin, and Henad places them near the team.

## Parameters

| NetLogo        | value | Henad `--set`     |
| -------------- | ----- | ----------------- |
| `team-size`    | 4     | `team_size=4`     |
| `max-downtime` | 40    | `max_downtime=40` |
| `p`            | 40    | `p=0.4`           |
| `q`            | 65    | `q=0.65`          |
| `layout?`      | off   |                   |

Henad also takes `num_agents=4`, the setup team from [§Not compared](#not-compared).
The CLI never lays out, so `layout?` has no Henad counterpart.
`p` and `q` are percentages in NetLogo, and fractions shown as percentages in Henad.

Apart from `layout?`, these are the model's defaults in both engines.
At these values the population settles at around 130 nodes, small enough for NetLogo's recursive component search.
`layout?` only moves nodes, and turning it off saves NetLogo twelve `layout-spring` iterations a tick.

## Procedure

Open Team Assembly from the Models Library in NetLogo 7.0.4.
Save it with File > Save As into an empty folder of its own, such as `netlogo/team/`.
The CSVs land beside the saved model, and the saved copy keeps the edits.
Left in the library, the model would write into the NetLogo installation, in the folder it shares with Virus on a Network.

Then append these two procedures at the end of the Code tab.

```netlogo
to write-row [ tick-number ]
  file-print (word tick-number ","
    count links with [ color = blue ] ","
    count links with [ color = turquoise ] ","
    count links with [ color = yellow ] ","
    count links with [ color = red ] ","
    (giant-component-size / count turtles) ","
    (mean components))
end

to run-replicate [ filename rng-seed total-ticks ]
  set team-size 4
  set max-downtime 40
  set p 40
  set q 65
  set layout? false
  random-seed rng-seed
  setup
  if file-exists? filename [ file-delete filename ]
  file-open filename
  file-print (word "# engine: NetLogo " netlogo-version)
  file-print "# model: Team Assembly (Models Library)"
  file-print (word "# seed: " rng-seed)
  file-print "tick,Newcomer-Newcomer Links,Newcomer-Incumbent Links,Incumbent-Incumbent Links,Previous Collaborator Links,Giant Component Share,Mean Component Size"
  write-row 0
  repeat total-ticks [
    go
    write-row ticks
  ]
  file-close
end
```

The four colours are the ones `color-collaborations` gives each kind of link.
The last two columns are the values the model's two component plots draw.
`go` runs `find-all-components` before its `tick`, so each row describes the graph at the end of that tick.

Uncheck View Updates in the Interface tab first, or drag the Model Speed slider to its far right.
At the default speed NetLogo runs this model no faster than its frame rate, 30 ticks a second.
At that rate, 50 runs of 1000 ticks take almost half an hour.
Then run the following in the Command Center.
It writes fifty files into the folder holding the saved model.

```netlogo
foreach (range 1 51) [ s -> run-replicate (word "team_netlogo_" s ".csv") s 1000 ]
```

One matching Henad run:

```bash
cargo run --release -p henad-cli -- team_assembly \
  --set num_agents=4 --set team_size=4 --set max_downtime=40 --set p=0.4 --set q=0.65 \
  --steps 1000 --seed 1 --export-stats team_henad_001.csv
```

Henad's `--export-stats` labels the components before each sample, as the app does before it draws.
Both engines write the same seven columns, with a row for tick 0.

## Margins

Seven summary statistics per run, each a mean over ticks 201 to 1000: **giant component share**, **mean component size**, **link count**, and the share of the links that are **newcomer-newcomer**, **newcomer-incumbent**, **incumbent-incumbent** and **previous collaborator** links.

A single tick is too noisy to compare.
The giant component share at tick 1000 alone varies by 40% between runs, since one node retiring can split the giant component in two.
The link count settles by about tick 100, once the setup team and the first cohorts have retired, and the window starts at 201 to leave room.

We compute the difference in means, its 95% confidence interval, and require that the whole interval lies inside the margin.
The difference is Henad's mean minus the reference's.

Margins come from Henad's own measured run-to-run spread, 30 seeds per run length, over two run lengths:

| statistic                   | 1000 ticks (window 201–1000) | 2000 ticks (window 201–2000) |
| --------------------------- | ---------------------------- | ---------------------------- |
| giant component share       | 0.5117 ± 0.0322 (6.3%)       | 0.5086 ± 0.0250 (4.9%)       |
| mean component size         | 14.69 ± 0.898 (6.1%)         | 14.71 ± 0.736 (5.0%)         |
| link count                  | 246.97 ± 1.58 (0.6%)         | 246.95 ± 0.979 (0.4%)        |
| newcomer-newcomer share     | 0.3508 ± 0.0089 (2.5%)       | 0.3508 ± 0.0056 (1.6%)       |
| newcomer-incumbent share    | 0.4827 ± 0.0071 (1.5%)       | 0.4846 ± 0.0044 (0.9%)       |
| incumbent-incumbent share   | 0.0726 ± 0.0051 (7.0%)       | 0.0716 ± 0.0029 (4.0%)       |
| previous collaborator share | 0.0939 ± 0.0044 (4.7%)       | 0.0930 ± 0.0032 (3.5%)       |
| giant share at the last tick | 0.4552 ± 0.1824 (40.1%)     | 0.4819 ± 0.2197 (45.6%)      |

We choose 1000 ticks.
The longer run narrows the spread of all seven statistics, but the component statistics least.
The giant component changes slowly, and neighbouring ticks are far from independent.

**Margins for 1000 ticks, 50 seeds per engine**, with the spread taken from 100 Henad runs:

| statistic                   | margin | 95% CI half-width | headroom |
| --------------------------- | ------ | ----------------- | -------- |
| giant component share       | ±0.045 | 0.0152            | 3.0x     |
| mean component size         | ±1.3   | 0.44              | 3.0x     |
| link count                  | ±1.5   | 0.49              | 3.0x     |
| newcomer-newcomer share     | ±0.013 | 0.0043            | 3.0x     |
| newcomer-incumbent share    | ±0.009 | 0.0029            | 3.1x     |
| incumbent-incumbent share   | ±0.006 | 0.0020            | 3.1x     |
| previous collaborator share | ±0.007 | 0.0022            | 3.2x     |

CI half-width is $\frac{1.96 \sigma \cdot \sqrt{2}}{\sqrt{50}}$, where $\sqrt{2}$ arises because the difference of two independent means has twice the variance of one.

### Checking the margins

Henad against itself checks the script and the margins.
It says nothing about agreement with NetLogo.
Seeds 1 to 50 against seeds 51 to 100 read equivalent on every statistic:

| statistic                   | seeds 1–50      | seeds 51–100    | difference (95% CI) | margin | verdict    |
| --------------------------- | --------------- | --------------- | ------------------- | ------ | ---------- |
| giant component share       | 0.4964 ± 0.0409 | 0.4979 ± 0.0369 | -0.0015 ± 0.0155    | ±0.045 | EQUIVALENT |
| mean component size         | 14.275 ± 1.114  | 14.392 ± 1.135  | -0.117 ± 0.446      | ±1.3   | EQUIVALENT |
| link count                  | 246.73 ± 1.43   | 246.85 ± 1.07   | -0.12 ± 0.50        | ±1.5   | EQUIVALENT |
| newcomer-newcomer share     | 0.3543 ± 0.0122 | 0.3551 ± 0.0096 | -0.0008 ± 0.0044    | ±0.013 | EQUIVALENT |
| newcomer-incumbent share    | 0.4806 ± 0.0079 | 0.4823 ± 0.0069 | -0.0017 ± 0.0030    | ±0.009 | EQUIVALENT |
| incumbent-incumbent share   | 0.0713 ± 0.0054 | 0.0704 ± 0.0046 | 0.0009 ± 0.0020     | ±0.006 | EQUIVALENT |
| previous collaborator share | 0.0937 ± 0.0057 | 0.0922 ± 0.0055 | 0.0015 ± 0.0022     | ±0.007 | EQUIVALENT |

The link count catches a retirement off by one tick.
Against seeds 1 to 50, a reference generated by Henad with `max_downtime` 41 from seeds 51 to 100 reads different on the link count (-6.12 ± 0.53 against ±1.5), and the other six stay equivalent.
Swapping the two seed ranges gives -6.16 ± 0.40, different again, with the other six still equivalent.

## Running the comparison

`scripts/compare_network.py` reads a directory of reference CSVs, generates the Henad side with the settings above, and prints one verdict per statistic.

```bash
cargo build --release -p henad-cli
uv run --project scripts scripts/compare_network.py team_assembly --reference netlogo/team --generate 50
```

`--reference` is the folder holding the saved model, and the script reads only the `team_*.csv` files in it.
The Henad side goes to `henad_team_assembly` beside that folder, unless `--henad` names another.
`--generate` refuses a folder holding any `team_*.csv` file other than the ones it writes.
Otherwise those files would count as replicates.

The verdicts and exit codes are those of `compare_sir.py`: `EQUIVALENT` (0), `DIFFERENT` (1), or `INCONCLUSIVE` (2) when the interval is wider than the margin and more replicates are needed.
A missing or malformed input, or a failed `henad-cli` run, exits 3.
The two-sample KS test is printed as a diagnostic only, for the reason given in `sir_fixture.md`.

## Result

50 replicates per engine, with the parameters in [§Parameters](#parameters), 1000 ticks, and each statistic a mean over ticks 201 to 1000:

| statistic                   | Henad           | NetLogo 7.0.4   | difference (95% CI) | margin | verdict    |
| --------------------------- | --------------- | --------------- | ------------------- | ------ | ---------- |
| giant component share       | 0.4964 ± 0.0409 | 0.4866 ± 0.0434 | 0.0098 ± 0.0167     | ±0.045 | EQUIVALENT |
| mean component size         | 14.275 ± 1.114  | 14.201 ± 1.282  | 0.074 ± 0.477       | ±1.3   | EQUIVALENT |
| link count                  | 246.73 ± 1.43   | 246.93 ± 1.01   | -0.19 ± 0.49        | ±1.5   | EQUIVALENT |
| newcomer-newcomer share     | 0.3543 ± 0.0122 | 0.3587 ± 0.0122 | -0.0044 ± 0.0048    | ±0.013 | EQUIVALENT |
| newcomer-incumbent share    | 0.4806 ± 0.0079 | 0.4815 ± 0.0077 | -0.0009 ± 0.0031    | ±0.009 | EQUIVALENT |
| incumbent-incumbent share   | 0.0713 ± 0.0054 | 0.0690 ± 0.0057 | 0.0023 ± 0.0022     | ±0.006 | EQUIVALENT |
| previous collaborator share | 0.0937 ± 0.0057 | 0.0908 ± 0.0055 | 0.0029 ± 0.0022     | ±0.007 | EQUIVALENT |

The NetLogo side ran in the NetLogo 7.0.4 app, from a copy of the library model carrying the two procedures in [§Procedure](#procedure), with View Updates unchecked.
The 50 runs took about twenty seconds.

Every interval lies inside its margin, the widest reaching three quarters of it, but two of them exclude zero, on the incumbent-incumbent and previous collaborator shares.
The KS test rejects two statistics at the 5% level, the newcomer-newcomer share (`D` 0.32, `p` 0.012) and the previous collaborator share (`D` 0.30, `p` 0.022).
A difference between the engines would persist on seeds these runs did not use, and a draw of seeds would not.
The same procedure over seeds 51 to 2000, 1950 per engine, gives:

| statistic                   | difference (95% CI) | KS `D` | KS `p` |
| --------------------------- | ------------------- | ------ | ------ |
| giant component share       | 0.0009 ± 0.0028     | 0.028  | 0.42   |
| mean component size         | 0.022 ± 0.086       | 0.018  | 0.89   |
| link count                  | -0.001 ± 0.072      | 0.022  | 0.73   |
| newcomer-newcomer share     | -0.00019 ± 0.00072  | 0.022  | 0.73   |
| newcomer-incumbent share    | -0.00006 ± 0.00049  | 0.021  | 0.81   |
| incumbent-incumbent share   | 0.00029 ± 0.00033   | 0.030  | 0.35   |
| previous collaborator share | -0.00004 ± 0.00033  | 0.025  | 0.60   |

No interval excludes zero, and no KS test rejects.
Seeds 1 to 500 flagged the newcomer-newcomer share instead (-0.00176 ± 0.00145, KS `p` 0.002).
Seeds 501 to 2000 alone put it at 0.00019 ± 0.00081.
Neither gap persists on fresh seeds.
