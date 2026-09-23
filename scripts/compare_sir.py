#!/usr/bin/env -S uv run --script
"""Cross-engine SIR comparison: Henad against a reference engine, distributionally.

SIR is stochastic and the two engines draw from different generators, so we need to compare the
distribution of a few summary statistics over many replicates.

Margins are derived in `crates/henad-models/tests/fixtures/docs/sir_fixture.md` from Henad's own
measured run-to-run spread.

Usage:

    uv run scripts/compare_sir.py --reference DIR --generate 50

The exit status is 0 when every statistic is equivalent, 1 when one differs, and 2 when one is
inconclusive and none differs. A missing or malformed input, or a failed `henad-cli` run, exits 3.
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
from typing import NoReturn

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


def fail(message: str) -> NoReturn:
    """Prints `message` and exits with status 3. No verdict uses that status.

    A bare `SystemExit` carrying a message exits 1, the status of `DIFFERENT`.
    """
    print(message, file=sys.stderr)
    raise SystemExit(3)


class ArgumentParser(argparse.ArgumentParser):
    """Argument parser whose usage errors go through `fail`.

    argparse's own status for them is 2, the status of `INCONCLUSIVE`.
    """

    def error(self, message: str) -> NoReturn:
        self.print_usage(sys.stderr)
        fail(f"{self.prog}: error: {message}")


def check_ticks(path: Path, ticks: list[int], steps: int) -> None:
    """Exits with status 3 unless `ticks` is exactly 0, 1, ..., `steps`.

    `ticks` holds each data row's tick in file order. Both engines write a row for tick 0.
    """
    for expected, tick in enumerate(ticks):
        if tick != expected:
            if expected == 0:
                fail(f"{path}: starts at tick {tick}, expected 0")
            fail(f"{path}: tick {tick} follows tick {ticks[expected - 1]}, expected {expected}")
    if len(ticks) != steps + 1:
        fail(f"{path}: ends at tick {len(ticks) - 1}, expected {steps}")


def read_run(path: Path, steps: int) -> Run:
    """Summarise one replicate.

    Both engines write `tick,Susceptible,Infected,Recovered`; NetLogo prefixes `#` provenance lines
    which are skipped. The cell count is taken from the first row rather than a flag, so a
    mismatched `--grid` cannot silently rescale one side's fractions.

    The ticks must run 0, 1, ..., `steps` with one row each. The tick of the peak is the peak's
    row index, and a dropped or repeated row would shift it. A mismatched `--steps` is refused too.
    """
    with path.open() as handle:
        rows = list(csv.DictReader(line for line in handle if not line.startswith("#")))
    if not rows:
        fail(f"{path}: no data rows")

    try:
        ticks = [int(r["tick"]) for r in rows]
        counts = [(int(r["Susceptible"]), int(r["Infected"]), int(r["Recovered"])) for r in rows]
    except KeyError as exc:
        fail(f"{path}: missing column {exc}")
    except (TypeError, ValueError) as exc:
        fail(f"{path}: {exc}")
    check_ticks(path, ticks, steps)

    total = sum(counts[0])
    if total == 0:
        fail(f"{path}: first row sums to zero cells")

    infected = [c[1] for c in counts]
    peak = max(infected)
    return Run(
        peak_infected=peak / total,
        tick_of_peak=float(infected.index(peak)),
        final_recovered=counts[-1][2] / total,
    )


def read_dir(directory: Path, steps: int) -> list[Run]:
    paths = sorted(directory.glob("*.csv"))
    if len(paths) < 2:
        fail(f"{directory}: need >= 2 .csv replicates to compute variance")
    return [read_run(p, steps) for p in paths]


def generate_henad(out_dir: Path, count: int, args: argparse.Namespace) -> None:
    """Run `henad-cli` once per seed. Seeds are 1..count, matching the reference procedure."""
    out_dir.mkdir(parents=True, exist_ok=True)
    binary = args.binary
    if not binary.exists():
        fail(f"{binary} not found; run `cargo build --release -p henad-cli` first")

    for seed in range(1, count + 1):
        dest = out_dir / f"henad_{seed:03d}.csv"
        try:
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
        except subprocess.CalledProcessError as exc:
            fail(f"\nhenad-cli failed on seed {seed}:\n{exc.stderr.decode(errors='replace').strip()}")
        except OSError as exc:
            fail(f"\ncannot run henad-cli on seed {seed}: {exc}")
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
    parser = ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    # `--netlogo` is the old name, kept because the fixture docs give it.
    parser.add_argument(
        "--reference", "--netlogo", dest="reference", type=Path, required=True, help="directory of reference CSVs"
    )
    parser.add_argument("--henad", type=Path, help="directory of Henad CSVs (default: <reference>/../henad)")
    parser.add_argument("--generate", type=int, metavar="N", help="produce N Henad replicates first")
    parser.add_argument("--binary", type=Path, default=REPO_ROOT / "target" / "release" / "henad-cli")
    parser.add_argument("--grid", type=int, default=DEFAULT_GRID)
    parser.add_argument("--beta", type=float, default=DEFAULT_BETA)
    parser.add_argument("--gamma", type=float, default=DEFAULT_GAMMA)
    parser.add_argument("--initial", type=float, default=DEFAULT_INITIAL)
    parser.add_argument(
        "--steps", type=int, default=DEFAULT_STEPS, help="ticks per run, and the tick every CSV must end on"
    )
    args = parser.parse_args()

    henad_dir = args.henad or args.reference.parent / "henad"
    if args.generate:
        print(f"generating {args.generate} Henad replicates into {henad_dir}", file=sys.stderr)
        generate_henad(henad_dir, args.generate, args)

    reference = read_dir(args.reference, args.steps)
    henad = read_dir(henad_dir, args.steps)

    print(f"\nHenad n={len(henad)}   reference n={len(reference)}")
    print(f"grid {args.grid}x{args.grid}, beta {args.beta}, gamma {args.gamma}, "
          f"initial {args.initial}, {args.steps} steps\n")

    header = f"{'statistic':>24} | {'Henad':>18} | {'reference':>18} | {'difference':>20} | {'margin':>7} | verdict"
    print(header)
    print("-" * len(header))

    verdicts = []
    for spec in STATISTICS:
        h = [spec.of(r) for r in henad]
        n = [spec.of(r) for r in reference]
        diff, half = difference_interval(h, n)
        result = verdict(diff, half, spec.margin)
        verdicts.append(result)
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
    if all(v == Verdict.EQUIVALENT for v in verdicts):
        print("All statistics equivalent within margin.")
        return 0
    if any(v == Verdict.DIFFERENT for v in verdicts):
        print("At least one statistic differs beyond its margin - see the table above.")
        return 1
    # Undecided, not failed. More replicates is the answer, and the caller needs to tell the two
    # apart rather than record a correct engine as a failure.
    print("At least one statistic is inconclusive; run more replicates.")
    return 2


if __name__ == "__main__":
    sys.exit(main())
