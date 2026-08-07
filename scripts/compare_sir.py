#!/usr/bin/env -S uv run --script
"""Cross-engine SIR comparison: Henad against a reference engine, distributionally.

SIR is stochastic and the two engines draw from different generators, so we need to compare the
distribution of a few summary statistics over many replicates.

Margins are derived in `crates/henad-models/tests/fixtures/docs/sir_fixture.md` from Henad's own
measured run-to-run spread.

Usage:

    uv run scripts/compare_sir.py --netlogo DIR --generate 50
"""

from __future__ import annotations

import argparse
import csv
import statistics as st
import subprocess
import sys
from dataclasses import dataclass
from pathlib import Path
from enum import Enum

import numpy as np
from scipy import stats

REPO_ROOT = Path(__file__).resolve().parent.parent

# Parms matching sir_fixture.md.
DEFAULT_GRID = 256
DEFAULT_BETA = 0.08
DEFAULT_GAMMA = 0.3
DEFAULT_INITIAL = 0.01
DEFAULT_STEPS = 300

CONFIDENCE = 0.95


@dataclass(frozen=True)
class Statistic:
    """One summary number per run, and how far apart the two engines may be on it."""

    key: str
    label: str
    margin: float
    fmt: str

    def of(self, run: Run) -> float:
        return getattr(run, self.key)


@dataclass(frozen=True)
class Run:
    peak_infected: float
    tick_of_peak: float
    final_recovered: float


STATISTICS = (
    Statistic("peak_infected", "peak infected fraction", 0.004, "{:.5f}"),
    Statistic("tick_of_peak", "tick of peak", 1.5, "{:.2f}"),
    Statistic("final_recovered", "final recovered fraction", 0.03, "{:.5f}"),
)

class Verdict(Enum):
    EQUIVALENT = "EQUIVALENT"
    DIFFERENT = "DIFFERENT"
    INCONCLUSIVE = "INCONCLUSIVE"


def read_run(path: Path) -> Run:
    """Summarise one replicate.

    Both engines write `tick,Susceptible,Infected,Recovered`; NetLogo prefixes `#` provenance lines
    which are skipped. The cell count is taken from the first row rather than a flag, so a
    mismatched `--grid` cannot silently rescale one side's fractions.
    """
    with path.open() as handle:
        rows = list(csv.DictReader(line for line in handle if not line.startswith("#")))
    if not rows:
        raise SystemExit(f"{path}: no data rows")

    try:
        counts = [(int(r["Susceptible"]), int(r["Infected"]), int(r["Recovered"])) for r in rows]
    except KeyError as exc:
        raise SystemExit(f"{path}: missing column {exc}") from exc

    total = sum(counts[0])
    if total == 0:
        raise SystemExit(f"{path}: first row sums to zero cells")

    infected = [c[1] for c in counts]
    peak = max(infected)
    return Run(
        peak_infected=peak / total,
        tick_of_peak=float(infected.index(peak)),
        final_recovered=counts[-1][2] / total,
    )


def read_dir(directory: Path) -> list[Run]:
    paths = sorted(directory.glob("*.csv"))
    if len(paths) < 2:
        raise SystemExit(f"{directory}: need >= 2 .csv replicates to compute variance")
    return [read_run(p) for p in paths]


def generate_henad(out_dir: Path, count: int, args: argparse.Namespace) -> None:
    """Run `henad-cli` once per seed. Seeds are 1..count, matching the reference procedure."""
    out_dir.mkdir(parents=True, exist_ok=True)
    binary = REPO_ROOT / "target" / "release" / "henad-cli"
    if not binary.exists():
        raise SystemExit(f"{binary} not found; run `cargo build --release -p henad-cli` first")

    for seed in range(1, count + 1):
        dest = out_dir / f"henad_{seed:03d}.csv"
        subprocess.run(
            [
                str(binary), "sir",
                "--set", f"grid_width={args.grid}",
                "--set", f"grid_height={args.grid}",
                "--set", f"infection_rate={args.beta}",
                "--set", f"recovery_rate={args.gamma}",
                "--set", f"initial_infected_pct={args.initial}",
                "--steps", str(args.steps),
                "--seed", str(seed),
                "--export-stats", str(dest),
            ],
            capture_output=True,
            check=True,
        )
        print(f"\r  generated {seed}/{count}", end="", file=sys.stderr, flush=True)
    print(file=sys.stderr)


def difference_interval(a: list[float], b: list[float]) -> tuple[float, float]:
    """Difference in means and the half-width of its confidence interval.

    Welch rather than pooled: the two engines have no reason to share a variance, and the reference
    side may well have a different replicate count.
    """
    na, nb = len(a), len(b)
    diff = st.mean(a) - st.mean(b)
    var = st.variance(a) / na + st.variance(b) / nb
    if var == 0:
        return diff, 0.0
    # Welch-Satterthwaite degrees of freedom.
    df = var**2 / (
        (st.variance(a) / na) ** 2 / (na - 1) + (st.variance(b) / nb) ** 2 / (nb - 1)
    )
    return diff, float(stats.t.ppf(1 - (1 - CONFIDENCE) / 2, df)) * var**0.5


def verdict(diff: float, half_width: float, margin: float) -> Verdict:
    """Where the confidence interval sits relative to the margin.

    `INCONCLUSIVE` is a real outcome, not a failure: it means the replicate count is too low to
    decide, and the answer is more runs rather than a wider margin.
    """
    if abs(diff) + half_width <= margin:
        return Verdict.EQUIVALENT
    if abs(diff) - half_width > margin:
        return Verdict.DIFFERENT
    return Verdict.INCONCLUSIVE


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--netlogo", type=Path, required=True, help="directory of reference CSVs")
    parser.add_argument("--henad", type=Path, help="directory of Henad CSVs (default: <netlogo>/../henad)")
    parser.add_argument("--generate", type=int, metavar="N", help="produce N Henad replicates first")
    parser.add_argument("--grid", type=int, default=DEFAULT_GRID)
    parser.add_argument("--beta", type=float, default=DEFAULT_BETA)
    parser.add_argument("--gamma", type=float, default=DEFAULT_GAMMA)
    parser.add_argument("--initial", type=float, default=DEFAULT_INITIAL)
    parser.add_argument("--steps", type=int, default=DEFAULT_STEPS)
    args = parser.parse_args()

    henad_dir = args.henad or args.netlogo.parent / "henad"
    if args.generate:
        print(f"generating {args.generate} Henad replicates into {henad_dir}", file=sys.stderr)
        generate_henad(henad_dir, args.generate, args)

    reference = read_dir(args.netlogo)
    henad = read_dir(henad_dir)

    print(f"\nHenad n={len(henad)}   reference n={len(reference)}")
    print(f"grid {args.grid}x{args.grid}, beta {args.beta}, gamma {args.gamma}, "
          f"initial {args.initial}, {args.steps} steps\n")

    header = f"{'statistic':>24} | {'Henad':>18} | {'reference':>18} | {'difference':>20} | {'margin':>7} | verdict"
    print(header)
    print("-" * len(header))

    all_equivalent = True
    for spec in STATISTICS:
        h = [spec.of(r) for r in henad]
        n = [spec.of(r) for r in reference]
        diff, half = difference_interval(h, n)
        result = verdict(diff, half, spec.margin)
        all_equivalent &= result == Verdict.EQUIVALENT
        print(
            f"{spec.label:>24} | "
            f"{spec.fmt.format(st.mean(h)):>8} ±{spec.fmt.format(st.stdev(h)):>9} | "
            f"{spec.fmt.format(st.mean(n)):>8} ±{spec.fmt.format(st.stdev(n)):>9} | "
            f"{spec.fmt.format(diff):>9} ±{spec.fmt.format(half):>9} | "
            f"{spec.margin:>7} | {result.value}"
        )

    # Diagnostic only. KS compares distribution shape, which two matching means can hide, but it
    # is a difference test and so must never gate: at any alpha it rejects a correct pair at rate
    # alpha, and with enough replicates it rejects differences far too small to matter.
    print("\nKolmogorov-Smirnov (diagnostic):")
    for spec in STATISTICS:
        h = np.array([spec.of(r) for r in henad])
        n = np.array([spec.of(r) for r in reference])
        result = stats.ks_2samp(h, n)
        print(f"{spec.label:>24} | D = {result.statistic:.4f}  p = {result.pvalue:.4f}") # type: ignore

    print()
    if all_equivalent:
        print("All statistics equivalent within margin.")
        return 0
    print("Not all statistics equivalent - see the table above.")
    return 1


if __name__ == "__main__":
    sys.exit(main())
