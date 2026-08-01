#!/usr/bin/env python3
# /// script
# requires-python = ">=3.10"
# dependencies = ["matplotlib>=3.8"]
# ///
"""Plot how the benchmark matrix moved across commits.

Reads every ``results/<n>_bench_matrix_<host>_<sha>.csv`` produced by
``bench_matrix.py`` and renders one figure per question:

  01  absolute throughput scaling, one panel per model, one line per commit
  02  throughput relative to the oldest commit
  03  how sensitive the numbers are to the benchmark config (GPU)
  04  the same scaling question for the agent models, whose axis is agent count not grid size

Run with uv (no venv needed):  uv run scripts/plot_bench_history.py
"""

from __future__ import annotations

import argparse
import csv
import math
import re
import subprocess
import sys
from dataclasses import dataclass
from pathlib import Path

import matplotlib

matplotlib.use("Agg")
import matplotlib.pyplot as plt
from matplotlib.ticker import FuncFormatter

REPO_ROOT = Path(__file__).resolve().parent.parent
DEFAULT_RESULTS = REPO_ROOT / "results"

_FILENAME_RE = re.compile(r"^(?P<order>\d+)_bench_matrix_(?P<host>.+)_(?P<sha>[0-9a-f]{7,40})\.csv$")

# Categorical slots 1-3, validated all-pairs for CVD separation. Slot 4 (purple) was added when a
# fourth commit made the list wrap and repeat slot 1 in both colour and marker; it has NOT been
# through the same all-pairs check, so re-validate before relying on it in a figure for the paper.
COMMIT_COLORS = ["#2a78d6", "#eb6834", "#1baf7a", "#8b5cc7"]
COMMIT_MARKERS = ["o", "s", "^", "D"]

# Kept next to the figures it captions; see select_representative_run for the rationale.
POLICY_NOTE = "each point is that model's longest, most-warmed config (CPU 1k steps, GPU 100k)"

TEXT_PRIMARY = "#0b0b0b"
TEXT_SECONDARY = "#52514e"
SURFACE = "#fcfcfb"

# Panel order for the per-model figures: CPU row first, then GPU row.
GRID_MODELS = ["game_of_life", "sir", "gpu_game_of_life", "gpu_sir"]
# Agent models scale along agent count, not grid size, so they get their own figure.
AGENT_MODELS = ["boids", "ants"]
MODEL_TITLES = {
    "game_of_life": "game_of_life (CPU)",
    "sir": "sir (CPU)",
    "gpu_game_of_life": "gpu_game_of_life (GPU)",
    "gpu_sir": "gpu_sir (GPU)",
    "boids": "boids (CPU, continuous space)",
    "ants": "ants (CPU, agents over a scalar field)",
}


@dataclass(frozen=True)
class Commit:
    """One benchmark CSV, identified by the commit it was measured at."""

    order: int
    sha: str
    host: str
    path: Path
    subject: str

    @property
    def short(self) -> str:
        return self.sha[:7]

    @property
    def label(self) -> str:
        if not self.subject:
            return self.short
        # Commit subjects here run to several lines; a legend entry gets one.
        subject = self.subject if len(self.subject) <= 32 else self.subject[:31] + "…"
        return f"{self.short} — {subject}"


@dataclass(frozen=True)
class Run:
    """One ``ok`` row: a single (model, grid, config) measurement."""

    model: str
    is_gpu: bool
    cells: int | None
    grid_w: int | None
    agents: int | None
    steps: int
    warmup: int
    global_warmup: int
    mean_s: float
    min_s: float
    max_s: float
    updates_per_sec: float
    steps_per_sec: float

    @property
    def scale(self) -> int | None:
        """Where this run sits on its model's scaling axis: cells for a grid, agents otherwise.

        One accessor so the scaling figures work for both, since `updates_per_sec` already means
        "cell updates" or "agent updates" to match.
        """
        return self.cells if self.cells is not None else self.agents

    @property
    def config(self) -> tuple[int, int, int]:
        """The benchmark knobs, as a hashable key: (steps, global_warmup, warmup)."""
        return (self.steps, self.global_warmup, self.warmup)

    @property
    def throughput_band(self) -> tuple[float, float]:
        """(slowest, fastest) updates/sec over the reps, from the min/max rep times."""
        scale = self.updates_per_sec * self.mean_s
        return (scale / self.max_s, scale / self.min_s)


def git_subject(sha: str) -> str:
    """First line of the commit message, or "" when the object is unknown here."""
    try:
        out = subprocess.run(
            ["git", "-C", str(REPO_ROOT), "log", "-1", "--format=%s", sha],
            capture_output=True,
            text=True,
            timeout=10,
        )
    except (OSError, subprocess.SubprocessError):
        return ""
    return out.stdout.strip().split("\n")[0] if out.returncode == 0 else ""


def _int_or_none(text: str) -> int | None:
    return int(text) if text.strip() else None


def _agents(row: dict[str, str]) -> int | None:
    """Population for an agent-model row, recovered for CSVs written before the agent axis.

    Those ran every agent model once at its default population and recorded no count. It is
    exactly recoverable anyway: ``updates_per_sec`` is population × ``steps_per_sec``, so the
    ratio is the population the run actually used — no assumption about what the default was at
    that commit. Recovering it puts those runs on the same axis as the swept ones, which is what
    makes figure 04 a comparison across commits rather than one line.
    """
    recorded = _int_or_none(row.get("num_agents", ""))
    if recorded is not None:
        return recorded
    if row["grid_w"].strip():
        return None  # a grid model; population there means cells, already read as `cells`
    try:
        steps_per_sec = float(row["steps_per_sec"])
        agents = float(row["updates_per_sec"]) / steps_per_sec
    except (KeyError, ValueError, ZeroDivisionError):
        return None
    # Both rates are reported rounded, so the ratio lands near the count rather than on it —
    # 50k reads as 50000.5 to 50001.0 across four rows, which would scatter one population over
    # several x positions. The residual is ~1e-5 relative, so 4 significant figures absorbs it
    # while staying far finer than the gap between any two points on the ladder.
    return _round_sig(agents, 4) if agents >= 1 else None


def _round_sig(value: float, digits: int) -> int:
    exponent = math.floor(math.log10(value))
    factor = 10 ** (exponent - digits + 1)
    return int(round(value / factor) * factor) if factor > 1 else int(round(value))


def load_results(results_dir: Path) -> tuple[list[Commit], dict[str, list[Run]]]:
    """Load every benchmark CSV, oldest commit first."""
    commits: list[Commit] = []
    for path in sorted(results_dir.glob("*.csv")):
        match = _FILENAME_RE.match(path.name)
        if not match:
            print(f"skipping unrecognised file: {path.name}", file=sys.stderr)
            continue
        sha = match.group("sha")
        commits.append(
            Commit(
                order=int(match.group("order")),
                sha=sha,
                host=match.group("host"),
                path=path,
                subject=git_subject(sha),
            )
        )
    commits.sort(key=lambda c: c.order)

    runs: dict[str, list[Run]] = {}
    for commit in commits:
        rows: list[Run] = []
        with commit.path.open(newline="", encoding="utf-8") as handle:
            for row in csv.DictReader(handle):
                if row["status"] != "ok":
                    continue
                rows.append(
                    Run(
                        model=row["model"],
                        is_gpu=row["is_gpu"].strip().lower() == "true",
                        cells=_int_or_none(row["requested_cells"]),
                        grid_w=_int_or_none(row["grid_w"]),
                        agents=_agents(row),
                        steps=int(row["steps"]),
                        warmup=int(row["warmup"]),
                        global_warmup=int(row["global_warmup"]),
                        mean_s=float(row["mean_s"]),
                        min_s=float(row["min_s"]),
                        max_s=float(row["max_s"]),
                        updates_per_sec=float(row["updates_per_sec"]),
                        steps_per_sec=float(row["steps_per_sec"]),
                    )
                )
        runs[commit.sha] = rows
    return commits, runs


def select_representative_run(candidates: list[Run]) -> Run | None:
    """Pick the run representing a (commit, model, grid size) in figures 01 and 02.

    Policy: **longest run, then most warmed** — max ``steps``, then max ``global_warmup``,
    then max ``warmup``. That is the closest thing the matrix has to steady-state
    throughput: it charges one-time init to the warmup rather than to the measurement,
    which otherwise dominates small grids (CPU ``game_of_life`` at 1 cell reads 1.5M
    updates/sec cold vs 33M warmed).

    The config matrix is fixed across commits, so every commit resolves to the *same*
    config for a given cell — which is what makes the commit-over-commit comparison
    paired. It does mean CPU panels are measured at 1k steps and GPU panels at 100k
    (only GPU models have the long configs); panels are per-model and never compared
    across devices, and figure 03 quantifies exactly what that choice is worth.
    """
    if not candidates:
        return None
    return max(candidates, key=lambda r: (r.steps, r.global_warmup, r.warmup))


def error_bars(centers: list[float], bands: list[tuple[float, float]]) -> list[list[float]]:
    """(below, above) offsets for matplotlib, from mean-centred slowest/fastest-rep bands."""
    below = [max(c - lo, 0.0) for c, (lo, _) in zip(centers, bands)]
    above = [max(hi - c, 0.0) for c, (_, hi) in zip(centers, bands)]
    return [below, above]


ERROR_KW = {"elinewidth": 0.9, "capsize": 2.5, "ecolor": TEXT_SECONDARY}


def by_model(runs: list[Run], model: str) -> list[Run]:
    return [r for r in runs if r.model == model]


def scale_points(runs_by_commit: dict[str, list[Run]], model: str) -> list[int]:
    """Every point on this model's scaling axis that was successfully measured, ascending."""
    sizes = {
        r.scale
        for rows in runs_by_commit.values()
        for r in by_model(rows, model)
        if r.scale is not None and r.scale >= 1
    }
    return sorted(sizes)


def series_for_model(
    commits: list[Commit],
    runs_by_commit: dict[str, list[Run]],
    model: str,
) -> tuple[list[int], dict[str, dict[int, Run]]]:
    """Representative run per (commit, scale point), plus the shared x axis."""
    sizes = scale_points(runs_by_commit, model)
    picked: dict[str, dict[int, Run]] = {}
    for commit in commits:
        rows = by_model(runs_by_commit[commit.sha], model)
        per_size: dict[int, Run] = {}
        for size in sizes:
            chosen = select_representative_run([r for r in rows if r.scale == size])
            if chosen is not None:
                per_size[size] = chosen
        picked[commit.sha] = per_size
    return sizes, picked


def si(value: float, _pos: float = 0) -> str:
    """Compact SI-ish label for axis ticks: 3.4e9 -> "3.4G"."""
    for limit, suffix in ((1e12, "T"), (1e9, "G"), (1e6, "M"), (1e3, "k")):
        if abs(value) >= limit:
            scaled = value / limit
            return f"{scaled:.0f}{suffix}" if scaled >= 10 else f"{scaled:.1f}{suffix}"
    return f"{value:.0f}"


def grid_label(cells: int) -> str:
    side = round(cells**0.5)
    return f"{side}²" if side * side == cells else si(cells)


def apply_style() -> None:
    plt.rcParams.update(
        {
            "figure.facecolor": SURFACE,
            "axes.facecolor": SURFACE,
            "savefig.facecolor": SURFACE,
            "axes.edgecolor": "#d5d4cf",
            "axes.labelcolor": TEXT_SECONDARY,
            "axes.titlecolor": TEXT_PRIMARY,
            "axes.spines.top": False,
            "axes.spines.right": False,
            "axes.grid": True,
            "grid.color": "#e6e5e0",
            "grid.linewidth": 0.8,
            "xtick.color": TEXT_SECONDARY,
            "ytick.color": TEXT_SECONDARY,
            "font.size": 9,
            "legend.frameon": False,
            "axes.axisbelow": True,
            "figure.dpi": 140,
        }
    )


def commit_legend(fig: plt.Figure, commits: list[Commit], ncol: int = 3) -> None:
    handles = [
        plt.Line2D(
            [],
            [],
            color=COMMIT_COLORS[i % len(COMMIT_COLORS)],
            marker=COMMIT_MARKERS[i % len(COMMIT_MARKERS)],
            markersize=5,
            linewidth=2,
            label=f"{c.order}. {c.label}",
        )
        for i, c in enumerate(commits)
    ]
    fig.legend(handles=handles, loc="lower center", ncol=ncol, bbox_to_anchor=(0.5, 0.0), fontsize=7.5)


def fig_scaling(commits, runs_by_commit, out: Path) -> None:
    """01 — absolute throughput vs grid size, one panel per model."""
    # No shared x: the GPU panels reach one grid size further than the CPU ones.
    fig, axes = plt.subplots(2, 2, figsize=(10, 7.2))
    for ax, model in zip(axes.flat, GRID_MODELS):
        sizes, picked = series_for_model(commits, runs_by_commit, model)
        for i, commit in enumerate(commits):
            per_size = picked[commit.sha]
            xs = [s for s in sizes if s in per_size]
            if not xs:
                continue
            ys = [per_size[s].updates_per_sec for s in xs]
            bands = [per_size[s].throughput_band for s in xs]
            ax.errorbar(
                xs,
                ys,
                yerr=error_bars(ys, bands),
                color=COMMIT_COLORS[i % len(COMMIT_COLORS)],
                marker=COMMIT_MARKERS[i % len(COMMIT_MARKERS)],
                markersize=4.5,
                linewidth=1.8,
                **ERROR_KW,
            )
        ax.set(xscale="log", yscale="log", title=MODEL_TITLES[model])
        ax.set_xticks(sizes, minor=False)
        ax.set_xticks([], minor=True)
        ax.set_xticklabels([grid_label(s) for s in sizes], rotation=45, ha="right")
        ax.yaxis.set_major_formatter(FuncFormatter(si))
        ax.set_ylabel("cell updates / sec")

    axes.flat[3].annotate(
        "8192² fails on all three commits\n(wgpu validation error)",
        xy=(0.03, 0.9),
        xycoords="axes fraction",
        fontsize=7.5,
        color=TEXT_SECONDARY,
    )
    fig.suptitle("Throughput vs grid size, per model", fontsize=13, color=TEXT_PRIMARY)
    fig.text(
        0.5,
        0.935,
        "marker = mean of 3 reps, error bars = slowest-to-fastest rep · " + POLICY_NOTE,
        ha="center",
        fontsize=8,
        color=TEXT_SECONDARY,
    )
    commit_legend(fig, commits)
    fig.tight_layout(rect=(0, 0.07, 1, 0.93))
    fig.savefig(out)
    plt.close(fig)


def fig_change_vs_baseline(commits, runs_by_commit, out: Path) -> None:
    """02 — throughput relative to the oldest commit."""
    baseline, later = commits[0], commits[1:]
    fig, axes = plt.subplots(2, 2, figsize=(10, 7))
    width = 0.8 / max(len(later), 1)

    for ax, model in zip(axes.flat, GRID_MODELS):
        sizes, picked = series_for_model(commits, runs_by_commit, model)
        base = picked[baseline.sha]
        xs = [s for s in sizes if s in base]
        positions = range(len(xs))
        for i, commit in enumerate(later, start=1):
            per_size = picked[commit.sha]
            ratios: list[float] = []
            bands: list[tuple[float, float]] = []
            for size in xs:
                run = per_size.get(size)
                if run is None:
                    ratios.append(0.0)
                    bands.append((0.0, 0.0))
                    continue
                # Rep spread is carried through the division; the baseline stays its mean.
                reference = base[size].updates_per_sec
                low, high = run.throughput_band
                ratios.append(run.updates_per_sec / reference)
                bands.append((low / reference, high / reference))
            offsets = [p - 0.4 + width * (i - 0.5) for p in positions]
            color = COMMIT_COLORS[i % len(COMMIT_COLORS)]
            ax.bar(
                offsets,
                ratios,
                width=width * 0.9,
                color=color,
                linewidth=0,
                yerr=error_bars(ratios, bands),
                error_kw=ERROR_KW,
            )
            for x, ratio, (_, high) in zip(offsets, ratios, bands):
                if ratio:
                    ax.text(
                        x,
                        high,
                        f" {ratio:.2f}×",
                        ha="center",
                        va="bottom",
                        rotation=90,
                        fontsize=6.5,
                        color=TEXT_SECONDARY,
                    )
        ax.axhline(1.0, color=COMMIT_COLORS[0], linewidth=1.4, linestyle="--")
        ax.set(title=MODEL_TITLES[model], ylabel=f"× {baseline.short}")
        ax.set_xticks(list(positions))
        ax.set_xticklabels([grid_label(s) for s in xs], rotation=45, ha="right")
        ax.set_ylim(0, max(1.6, ax.get_ylim()[1] * 1.3))

    fig.suptitle(f"Throughput relative to {baseline.short}", fontsize=13, color=TEXT_PRIMARY)
    fig.text(
        0.5,
        0.935,
        f"dashed line = {baseline.short} baseline (1.00×), above it is faster · error bars = rep spread · {POLICY_NOTE}",
        ha="center",
        fontsize=8,
        color=TEXT_SECONDARY,
    )
    commit_legend(fig, commits)
    fig.tight_layout(rect=(0, 0.07, 1, 0.93))
    fig.savefig(out)
    plt.close(fig)


def fig_config_sensitivity(commits, runs_by_commit, out: Path, cells: int) -> None:
    """03 — the same GPU model under every benchmark config, at one grid size."""
    models = ["gpu_game_of_life", "gpu_sir"]
    configs = sorted(
        {
            r.config
            for rows in runs_by_commit.values()
            for m in models
            for r in by_model(rows, m)
            if r.cells == cells
        }
    )
    fig, axes = plt.subplots(len(models), 1, figsize=(10, 6.4), sharex=True)
    width = 0.8 / len(commits)

    for ax, model in zip(axes, models):
        for i, commit in enumerate(commits):
            lookup = {r.config: r for r in by_model(runs_by_commit[commit.sha], model) if r.cells == cells}
            values = [lookup[c].updates_per_sec if c in lookup else 0.0 for c in configs]
            bands = [lookup[c].throughput_band if c in lookup else (0.0, 0.0) for c in configs]
            offsets = [p - 0.4 + width * (i + 0.5) for p in range(len(configs))]
            ax.bar(
                offsets,
                values,
                width=width * 0.9,
                color=COMMIT_COLORS[i % len(COMMIT_COLORS)],
                linewidth=0,
                yerr=error_bars(values, bands),
                error_kw=ERROR_KW,
            )
        ax.set(title=MODEL_TITLES[model], ylabel="cell updates / sec")
        ax.yaxis.set_major_formatter(FuncFormatter(si))
        ax.set_xticks(range(len(configs)))
        ax.set_xticklabels(
            [f"{s//1000}k steps\nglobal {gw}\nwarmup {w}" for s, gw, w in configs],
            fontsize=7,
        )

    fig.suptitle(f"Benchmark-config sensitivity at {grid_label(cells)}", fontsize=13, color=TEXT_PRIMARY)
    fig.text(
        0.5,
        0.935,
        "short 1k-step runs charge fixed GPU submission cost to every step and understate steady-state throughput"
        " · error bars = rep spread",
        ha="center",
        fontsize=8,
        color=TEXT_SECONDARY,
    )
    commit_legend(fig, commits)
    fig.tight_layout(rect=(0, 0.07, 1, 0.93))
    fig.savefig(out)
    plt.close(fig)


def fig_agents(commits, runs_by_commit, out: Path) -> None:
    """04 — agent models: throughput vs agent count. Skipped when no commit measured any.

    The counterpart of figure 01 for the agent axis. The population is swept at the model's own
    default density (`bench_matrix.world_for_agents`), so the world grows with the count and a
    flat line means per-agent cost held constant as the population scaled.
    """
    present = [m for m in AGENT_MODELS if scale_points(runs_by_commit, m)]
    if not present:
        print("no agent-model rows in any CSV — skipping figure 04", file=sys.stderr)
        return

    fig, axes = plt.subplots(1, len(present), figsize=(5.2 * len(present), 4.4), squeeze=False)
    missing: list[str] = []
    for ax, model in zip(axes.flat, present):
        sizes, picked = series_for_model(commits, runs_by_commit, model)
        for i, commit in enumerate(commits):
            per_size = picked[commit.sha]
            xs = [s for s in sizes if s in per_size]
            if not xs:
                label = f"{commit.order}. {commit.short}"
                if label not in missing:
                    missing.append(label)
                continue
            ys = [per_size[s].updates_per_sec for s in xs]
            bands = [per_size[s].throughput_band for s in xs]
            ax.errorbar(
                xs,
                ys,
                yerr=error_bars(ys, bands),
                color=COMMIT_COLORS[i % len(COMMIT_COLORS)],
                marker=COMMIT_MARKERS[i % len(COMMIT_MARKERS)],
                markersize=4.5,
                linewidth=1.8,
                **ERROR_KW,
            )
        ax.set(xscale="log", yscale="log", title=MODEL_TITLES[model])
        ax.set_xticks(sizes, minor=False)
        ax.set_xticks([], minor=True)
        ax.set_xticklabels([si(s) for s in sizes], rotation=45, ha="right")
        ax.yaxis.set_major_formatter(FuncFormatter(si))
        ax.set(xlabel="agents", ylabel="agent updates / sec")

    fig.suptitle("Throughput vs agent count, per agent model", fontsize=13, color=TEXT_PRIMARY, y=0.985)
    subtitle = "population swept at each model's default density, so the world grows with it · " + POLICY_NOTE
    if missing:
        subtitle += "\nnot benchmarked at " + ", ".join(missing)
    fig.text(0.5, 0.895, subtitle, ha="center", fontsize=8, va="top", color=TEXT_SECONDARY)
    commit_legend(fig, commits, ncol=2)
    # Top margin leaves room for the second subtitle line, which only appears when a commit is
    # missing agent rows.
    fig.tight_layout(rect=(0, 0.12, 1, 0.86 if missing else 0.89))
    fig.savefig(out)
    plt.close(fig)


def print_summary(commits, runs_by_commit) -> None:
    """Text version of figure 02, so the numbers are greppable without opening a PNG."""
    baseline = commits[0]
    print(f"\nbaseline: {baseline.order}. {baseline.label}")
    for model in GRID_MODELS + AGENT_MODELS:
        sizes, picked = series_for_model(commits, runs_by_commit, model)
        if not sizes:
            continue
        base = picked[baseline.sha]
        any_run = next((r for r in picked[baseline.sha].values()), None)
        config = (
            f"  ({any_run.steps} steps, global warmup {any_run.global_warmup},"
            f" per-rep warmup {any_run.warmup})"
            if any_run
            else ""
        )
        is_agent = model in AGENT_MODELS
        print(f"\n{MODEL_TITLES[model]}{config}")
        header = ("  agents" if is_agent else "  grid").ljust(12) + "".join(f"{c.short:>14}" for c in commits)
        print(header)
        for size in sizes:
            cells = f"  {si(size) if is_agent else grid_label(size)}".ljust(12)
            parts = []
            for commit in commits:
                run = picked[commit.sha].get(size)
                if run is None:
                    parts.append(f"{'--':>14}")
                elif commit is baseline or size not in base:
                    parts.append(f"{si(run.updates_per_sec):>14}")
                else:
                    parts.append(f"{si(run.updates_per_sec) + f' ({run.updates_per_sec / base[size].updates_per_sec:.2f}x)':>14}")
            print(cells + "".join(parts))


def main() -> int:
    parser = argparse.ArgumentParser(
        description="Plot benchmark-matrix changes across commits.",
        formatter_class=argparse.ArgumentDefaultsHelpFormatter,
    )
    parser.add_argument("--results", type=Path, default=DEFAULT_RESULTS, help="directory holding the benchmark CSVs")
    parser.add_argument("--out", type=Path, default=DEFAULT_RESULTS / "plots", help="directory to write PNGs into")
    parser.add_argument(
        "--sensitivity-cells",
        type=int,
        default=16777216,
        help="grid size (in cells) used for the config-sensitivity figure",
    )
    parser.add_argument("--no-summary", action="store_true", help="skip the text table on stdout")
    args = parser.parse_args()

    commits, runs_by_commit = load_results(args.results)
    if not commits:
        print(f"error: no bench matrix CSVs found in {args.results}", file=sys.stderr)
        return 1

    apply_style()
    args.out.mkdir(parents=True, exist_ok=True)
    print(f"{len(commits)} commit(s): " + ", ".join(f"{c.order}. {c.short}" for c in commits), file=sys.stderr)

    fig_scaling(commits, runs_by_commit, args.out / "01_scaling_by_model.png")
    if len(commits) > 1:
        fig_change_vs_baseline(commits, runs_by_commit, args.out / "02_change_vs_baseline.png")
    fig_config_sensitivity(commits, runs_by_commit, args.out / "03_config_sensitivity.png", args.sensitivity_cells)
    fig_agents(commits, runs_by_commit, args.out / "04_agent_scaling.png")

    if not args.no_summary:
        print_summary(commits, runs_by_commit)
    print(f"\nwrote figures to {args.out}", file=sys.stderr)
    return 0


if __name__ == "__main__":
    sys.exit(main())
