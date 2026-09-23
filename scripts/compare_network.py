#!/usr/bin/env -S uv run --script
"""Cross-engine comparison of the two network models: Henad against a reference engine, distributionally.

Virus on a Network and Team Assembly are stochastic, and the engines draw from different generators, so we compare
the distribution of a few summary statistics over many replicates, the same way `compare_sir.py` compares SIR.

The settings, the statistics and their margins are derived in
`crates/henad-models/tests/fixtures/docs/virus_network_fixture.md` and `team_assembly_fixture.md`, which also give
the procedure for the NetLogo side.

Usage:

    uv run --project scripts scripts/compare_network.py virus_network --reference DIR --generate 400
    uv run --project scripts scripts/compare_network.py team_assembly --reference DIR --generate 50

Each side is read from the model's own files in its directory, `virus_*.csv` or `team_*.csv`. The Henad side
defaults to `henad_<model>` beside the reference directory, and `--generate` refuses a directory holding any other
file of the model.

The exit status is that of `compare_sir.py`: 0 when every statistic is equivalent, 1 when one differs, and 2 when
one is inconclusive and none differs. A missing or malformed input, or a failed `henad-cli` run, exits 3.
"""

from __future__ import annotations

import argparse
import csv
import statistics as st
import subprocess
import sys
from collections.abc import Callable
from dataclasses import dataclass
from pathlib import Path

import numpy as np
from scipy import stats

from compare_sir import ArgumentParser, Verdict, difference_interval, fail, verdict

REPO_ROOT = Path(__file__).resolve().parent.parent

Row = dict[str, str]


@dataclass(frozen=True)
class Statistic:
    """One summary number per run, and how far apart the two engines may be on it."""

    key: str
    label: str
    margin: float
    fmt: str


@dataclass(frozen=True)
class Model:
    """A fixture: the Henad settings, the columns both engines write, and the statistics read from them."""

    # File-name prefix of the model's CSVs on both sides, as in `virus_netlogo_1.csv` and `virus_henad_001.csv`.
    prefix: str
    settings: tuple[str, ...]
    steps: int
    columns: tuple[str, ...]
    statistics: tuple[Statistic, ...]
    summarise: Callable[[list[Row]], dict[str, float]]
    description: str


def virus_run(rows: list[Row]) -> dict[str, float]:
    """Peak infected fraction, the tick it was reached, and the resistant fraction at the end."""
    counts = [(int(r["Susceptible"]), int(r["Infected"]), int(r["Resistant"])) for r in rows]
    total = sum(counts[0])
    if total == 0:
        raise ValueError("first row sums to zero nodes")
    infected = [c[1] for c in counts]
    peak = max(infected)
    return {
        "peak_infected": peak / total,
        "tick_of_peak": float(rows[infected.index(peak)]["tick"]),
        "final_resistant": counts[-1][2] / total,
    }


# Team Assembly's statistics are averaged over the ticks from here on. The first ones are the approach to the
# steady state, and a single tick's components swing too far from one tick to the next to compare.
TEAM_WINDOW_START = 201

TEAM_LINKS = (
    "Newcomer-Newcomer Links",
    "Newcomer-Incumbent Links",
    "Incumbent-Incumbent Links",
    "Previous Collaborator Links",
)


def team_run(rows: list[Row]) -> dict[str, float]:
    """Means over the window of the component statistics, the link count, and each link type's share."""
    window = [r for r in rows if int(r["tick"]) >= TEAM_WINDOW_START]
    if not window:
        raise ValueError(f"no rows from tick {TEAM_WINDOW_START} on")
    totals = [sum(int(r[c]) for c in TEAM_LINKS) for r in window]
    if 0 in totals:
        raise ValueError("a row in the window has no links")
    run = {
        "giant_share": st.mean(float(r["Giant Component Share"]) for r in window),
        "mean_component_size": st.mean(float(r["Mean Component Size"]) for r in window),
        "links": st.mean(totals),
    }
    for column, key in zip(TEAM_LINKS, ("newcomer_newcomer", "newcomer_incumbent", "incumbent_incumbent", "repeat")):
        run[key] = st.mean(int(r[column]) / total for r, total in zip(window, totals))
    return run


MODELS = {
    "virus_network": Model(
        prefix="virus",
        settings=(
            "num_agents=1000",
            "average_node_degree=6",
            "initial_outbreak_size=10",
            "virus_spread_chance=0.025",
            "virus_check_frequency=2",
            "recovery_chance=0.05",
            "gain_resistance_chance=0.3",
            "directed=false",
            "network=Random",
            "keep_rewiring=false",
        ),
        steps=1000,
        columns=("tick", "Susceptible", "Infected", "Resistant"),
        statistics=(
            Statistic("peak_infected", "peak infected fraction", 0.0075, "{:.5f}"),
            Statistic("tick_of_peak", "tick of peak", 2.5, "{:.2f}"),
            Statistic("final_resistant", "final resistant fraction", 0.0053, "{:.5f}"),
        ),
        summarise=virus_run,
        description="1000 nodes, degree 6, outbreak 10, spread 2.5%, check every 2, recovery 5%, resistance 30%",
    ),
    "team_assembly": Model(
        prefix="team",
        settings=("num_agents=4", "team_size=4", "max_downtime=40", "p=0.4", "q=0.65"),
        steps=1000,
        columns=("tick", *TEAM_LINKS, "Giant Component Share", "Mean Component Size"),
        statistics=(
            Statistic("giant_share", "giant component share", 0.045, "{:.4f}"),
            Statistic("mean_component_size", "mean component size", 1.3, "{:.3f}"),
            Statistic("links", "link count", 1.5, "{:.2f}"),
            Statistic("newcomer_newcomer", "newcomer-newcomer share", 0.013, "{:.4f}"),
            Statistic("newcomer_incumbent", "newcomer-incumbent share", 0.009, "{:.4f}"),
            Statistic("incumbent_incumbent", "incumbent-incumbent share", 0.006, "{:.4f}"),
            Statistic("repeat", "previous collaborator share", 0.007, "{:.4f}"),
        ),
        summarise=team_run,
        description=f"team size 4, max downtime 40, p 40%, q 65%, means over ticks {TEAM_WINDOW_START} on",
    ),
}


def read_run(path: Path, model: Model, steps: int) -> dict[str, float]:
    """Summarise one replicate.

    Both engines write the model's stat columns after a `tick` column, and NetLogo prefixes `#` provenance lines,
    which are skipped. A run of the wrong length is refused, so a mismatched `--steps` cannot pass silently.
    """
    with path.open() as handle:
        reader = csv.DictReader(line for line in handle if not line.startswith("#"))
        missing = [c for c in model.columns if c not in (reader.fieldnames or [])]
        if missing:
            fail(f"{path}: missing columns {missing}")
        rows = list(reader)
    if not rows:
        fail(f"{path}: no data rows")
    try:
        last = int(rows[-1]["tick"])
        if last != steps:
            fail(f"{path}: ends at tick {last}, expected {steps}")
        return model.summarise(rows)
    except (TypeError, ValueError) as exc:
        fail(f"{path}: {exc}")


def read_dir(directory: Path, model: Model, steps: int) -> list[dict[str, float]]:
    """Summarise every replicate in `directory` whose name starts with the model's prefix."""
    pattern = f"{model.prefix}_*.csv"
    paths = sorted(directory.glob(pattern))
    if len(paths) < 2:
        fail(f"{directory}: need >= 2 replicates matching {pattern} to compute variance")
    return [read_run(p, model, steps) for p in paths]


def generate_henad(out_dir: Path, count: int, name: str, model: Model, args: argparse.Namespace) -> None:
    """Run `henad-cli` once per seed. Seeds are 1..count, matching the reference procedure.

    A directory already holding other files of this model is refused. `read_dir` would count them as replicates.
    """
    if not args.binary.exists():
        fail(f"{args.binary} not found; run `cargo build --release -p henad-cli` first")
    out_dir.mkdir(parents=True, exist_ok=True)
    wanted = {f"{model.prefix}_henad_{seed:03d}.csv" for seed in range(1, count + 1)}
    extra = sorted(p.name for p in out_dir.glob(f"{model.prefix}_*.csv") if p.name not in wanted)
    if extra:
        fail(
            f"{out_dir}: holds {len(extra)} {model.prefix}_*.csv file(s) that --generate {count} does not write, "
            f"such as {extra[0]}. Clear them, or pass another --henad."
        )

    settings = [arg for setting in model.settings for arg in ("--set", setting)]
    for seed in range(1, count + 1):
        dest = out_dir / f"{model.prefix}_henad_{seed:03d}.csv"
        try:
            subprocess.run(
                [
                    str(args.binary), name, *settings,
                    "--steps", str(args.steps),
                    "--seed", str(seed),
                    "--export-stats", str(dest),
                ],
                capture_output=True,
                check=True,
            )
        except subprocess.CalledProcessError as exc:
            fail(f"\nhenad-cli failed on seed {seed}:\n{exc.stderr.decode(errors='replace').strip()}")
        print(f"\r  generated {seed}/{count}", end="", file=sys.stderr, flush=True)
    print(file=sys.stderr)


def main() -> int:
    parser = ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("model", choices=sorted(MODELS), help="model id")
    parser.add_argument("--reference", type=Path, required=True, help="directory of reference CSVs")
    parser.add_argument("--henad", type=Path, help="directory of Henad CSVs (default: <reference>/../henad_<model>)")
    parser.add_argument("--generate", type=int, metavar="N", help="produce N Henad replicates first")
    parser.add_argument("--binary", type=Path, default=REPO_ROOT / "target" / "release" / "henad-cli")
    parser.add_argument("--steps", type=int, help="ticks per run (default: the fixture's)")
    args = parser.parse_args()

    model = MODELS[args.model]
    args.steps = args.steps or model.steps
    henad_dir = args.henad or args.reference.parent / f"henad_{args.model}"
    if args.generate:
        print(f"generating {args.generate} Henad replicates into {henad_dir}", file=sys.stderr)
        generate_henad(henad_dir, args.generate, args.model, model, args)

    reference = read_dir(args.reference, model, args.steps)
    henad = read_dir(henad_dir, model, args.steps)

    print(f"\nHenad n={len(henad)}   reference n={len(reference)}")
    print(f"{args.model}: {model.description}, {args.steps} steps\n")

    width = max(len(s.label) for s in model.statistics)
    header = (
        f"{'statistic':>{width}} | {'Henad':>18} | {'reference':>18} | {'difference':>20} | {'margin':>7} | verdict"
    )
    print(header)
    print("-" * len(header))

    verdicts = []
    for spec in model.statistics:
        h = [run[spec.key] for run in henad]
        n = [run[spec.key] for run in reference]
        diff, half = difference_interval(h, n)
        result = verdict(diff, half, spec.margin)
        verdicts.append(result)
        print(
            f"{spec.label:>{width}} | "
            f"{spec.fmt.format(st.mean(h)):>8} ±{spec.fmt.format(st.stdev(h)):>9} | "
            f"{spec.fmt.format(st.mean(n)):>8} ±{spec.fmt.format(st.stdev(n)):>9} | "
            f"{spec.fmt.format(diff):>9} ±{spec.fmt.format(half):>9} | "
            f"{spec.margin:>7} | {result.value}"
        )

    # Diagnostic only, for the reason given in `compare_sir.py`.
    print("\nKolmogorov-Smirnov (diagnostic):")
    for spec in model.statistics:
        h = np.array([run[spec.key] for run in henad])
        n = np.array([run[spec.key] for run in reference])
        result = stats.ks_2samp(h, n)
        print(f"{spec.label:>{width}} | D = {result.statistic:.4f}  p = {result.pvalue:.4f}")  # type: ignore

    print()
    if all(v == Verdict.EQUIVALENT for v in verdicts):
        print("All statistics equivalent within margin.")
        return 0
    if any(v == Verdict.DIFFERENT for v in verdicts):
        print("At least one statistic differs beyond its margin - see the table above.")
        return 1
    print("At least one statistic is inconclusive; run more replicates.")
    return 2


if __name__ == "__main__":
    sys.exit(main())
