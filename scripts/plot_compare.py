#!/usr/bin/env python3
# /// script
# requires-python = ">=3.10"
# dependencies = ["matplotlib>=3.8"]
# ///
"""Turn a cross-engine sweep into the figures and tables the benchmarks page includes.

Reads whatever `compare_bench.py` wrote and produces, per model, a throughput curve against scale
and a table of median times normalised to Henad on one thread. Lines of code come from
`benchmarks/loc_manifest.toml`, the secondary metric the Agents.jl comparison reports next to time:
how much a model costs to express.

    uv run scripts/plot_compare.py results/compare/<host>_<date>.csv
    uv run scripts/plot_compare.py results/compare/<host>_<date>.csv --publish
"""

from __future__ import annotations

import argparse
import csv
import sys
import tomllib
from collections import defaultdict
from dataclasses import dataclass
from pathlib import Path

import matplotlib

matplotlib.use("Agg")
import matplotlib.pyplot as plt
from matplotlib.ticker import FuncFormatter

REPO_ROOT = Path(__file__).resolve().parent.parent
PUBLISH_DIR = REPO_ROOT / "docs" / "assets" / "benchmarks"

TEXT_PRIMARY = "#0b0b0b"
TEXT_SECONDARY = "#52514e"
SURFACE = "#fcfcfb"

# One colour per engine, so a reader tracks an engine across every figure. Henad's variants share
# its colour and differ by dash, since they are one engine measured three ways.
ENGINE_COLORS = {
    "henad": "#2a78d6",
    "mesa": "#eb6834",
    "netlogo": "#1baf7a",
    "mason": "#8b5cc7",
    "agents_jl": "#c2185b",
    "krabmaga": "#a8791c",
}
HENAD_STYLE = {"1t": ("-", "o"), "all": ("--", "s"), "gpu": (":", "^")}

MODEL_TITLES = {
    "game_of_life": "Game of Life",
    "sir": "SIR",
    "boids": "Boids",
    "ants": "Ant foraging",
}
AXIS_LABELS = {"grid": "grid side", "agents": "agents"}
# A curve that stops says as much as one that continues, so the reason is drawn, not dropped.
INCOMPLETE = {
    "over_budget": "over budget",
    "timeout": "over budget",
    "oom": "out of memory",
    "error": "failed",
    "skipped": "not reached",
}


@dataclass(frozen=True)
class Series:
    engine: str
    variant: str

    @property
    def label(self) -> str:
        if self.engine == "henad":
            return {"1t": "Henad, 1 thread", "all": "Henad, all cores", "gpu": "Henad, GPU"}.get(
                self.variant, f"Henad ({self.variant})"
            )
        pretty = {"agents_jl": "Agents.jl", "netlogo": "NetLogo", "mesa": "Mesa", "mason": "MASON"}
        name = pretty.get(self.engine, self.engine)
        return name if self.variant == "default" else f"{name} ({self.variant})"

    @property
    def style(self) -> tuple[str, str, str]:
        color = ENGINE_COLORS.get(self.engine, "#666666")
        if self.engine == "henad":
            line, marker = HENAD_STYLE.get(self.variant, ("-", "o"))
        else:
            line, marker = "-", "o"
        return color, line, marker


def load(path: Path) -> list[dict]:
    with path.open(newline="") as handle:
        return list(csv.DictReader(handle))


def number(row: dict, key: str) -> float | None:
    try:
        return float(row[key])
    except (KeyError, TypeError, ValueError):
        return None


def si(value: float, _pos: float = 0) -> str:
    for limit, suffix in ((1e12, "T"), (1e9, "G"), (1e6, "M"), (1e3, "k")):
        if abs(value) >= limit:
            return f"{value / limit:g}{suffix}"
    return f"{value:g}"


def scale_label(scale: int, axis: str) -> str:
    return f"{scale}²" if axis == "grid" else si(scale)


def apply_style() -> None:
    plt.rcParams.update(
        {
            "figure.facecolor": SURFACE,
            "axes.facecolor": SURFACE,
            "savefig.facecolor": SURFACE,
            "axes.edgecolor": "#d5d4cf",
            "axes.labelcolor": TEXT_SECONDARY,
            "axes.titlecolor": TEXT_PRIMARY,
            "axes.grid": True,
            "grid.color": "#e6e5e0",
            "grid.linewidth": 0.6,
            "xtick.color": TEXT_SECONDARY,
            "ytick.color": TEXT_SECONDARY,
            "font.size": 9,
            "legend.frameon": False,
        }
    )


def series_of(rows: list[dict]) -> dict[Series, list[dict]]:
    grouped: dict[Series, list[dict]] = defaultdict(list)
    for row in rows:
        grouped[Series(row["engine"], row["variant"])].append(row)
    for points in grouped.values():
        points.sort(key=lambda r: int(r["scale"]))
    return grouped


def draw_model(rows: list[dict], model: str, metric: str, out: Path) -> bool:
    points = [r for r in rows if r["model"] == model]
    if not points:
        return False
    apply_style()
    fig, ax = plt.subplots(figsize=(6.4, 4.0))
    axis = points[0]["axis"]

    drawn = False
    for series, group in sorted(series_of(points).items(), key=lambda kv: kv[0].label):
        xs = [int(r["scale"]) for r in group if r["status"] == "ok" and number(r, metric)]
        ys = [number(r, metric) for r in group if r["status"] == "ok" and number(r, metric)]
        if not xs:
            continue
        color, line, marker = series.style
        ax.plot(xs, ys, line, marker=marker, color=color, label=series.label, linewidth=1.6, markersize=4.5)
        drawn = True
        stopped = next((r for r in group if r["status"] in INCOMPLETE and INCOMPLETE[r["status"]]), None)
        if stopped:
            ax.annotate(
                INCOMPLETE[stopped["status"]],
                xy=(xs[-1], ys[-1]),
                xytext=(4, -10),
                textcoords="offset points",
                fontsize=7,
                color=color,
            )
    if not drawn:
        plt.close(fig)
        return False

    ax.set_xscale("log")
    ax.set_yscale("log")
    ax.set_xlabel(AXIS_LABELS.get(axis, axis))
    ax.set_ylabel("steps / second" if metric == "steps_per_sec" else "cell or agent updates / second")
    ax.set_title(MODEL_TITLES.get(model, model))
    ax.yaxis.set_major_formatter(FuncFormatter(si))
    ax.xaxis.set_major_formatter(FuncFormatter(si))
    ax.legend(loc="best", fontsize=8)
    fig.tight_layout()
    out.parent.mkdir(parents=True, exist_ok=True)
    fig.savefig(out, dpi=160)
    plt.close(fig)
    return True


def ratio_table(rows: list[dict], model: str) -> str:
    """Median time against Henad on one thread. Above 1 is slower than Henad."""
    points = [r for r in rows if r["model"] == model]
    if not points:
        return ""
    scales = sorted({int(r["scale"]) for r in points})
    baseline = {
        int(r["scale"]): number(r, "median_s")
        for r in points
        if r["engine"] == "henad" and r["variant"] == "1t" and r["status"] == "ok"
    }

    axis = points[0]["axis"]
    header = ["engine"] + [scale_label(s, axis) for s in scales]
    lines = ["| " + " | ".join(header) + " |", "|" + "---|" * len(header)]
    for series, group in sorted(series_of(points).items(), key=lambda kv: kv[0].label):
        by_scale = {int(r["scale"]): r for r in group}
        cells = []
        for scale in scales:
            row = by_scale.get(scale)
            if row is None:
                cells.append("")
                continue
            if row["status"] != "ok":
                cells.append(INCOMPLETE.get(row["status"], row["status"]) or "not run")
                continue
            mine, base = number(row, "median_s"), baseline.get(scale)
            cells.append(f"{mine / base:.2f}" if mine and base else "")
        lines.append("| " + " | ".join([series.label] + cells) + " |")
    return "\n".join(lines) + "\n"


def engines_table(rows: list[dict]) -> str:
    """What actually ran, so a table's provenance travels with it."""
    seen: dict[tuple[str, str], dict] = {}
    for row in rows:
        seen.setdefault((row["engine"], row["variant"]), row)
    header = ["engine", "version", "variant", "threads", "host", "Henad commit"]
    lines = ["| " + " | ".join(header) + " |", "|" + "---|" * len(header)]
    for (engine, variant), row in sorted(seen.items()):
        # A GPU row's thread count is the host side of a device queue, which says nothing useful.
        threads = "—" if variant == "gpu" else (row["threads"] or "?")
        lines.append(
            "| "
            + " | ".join(
                [
                    Series(engine, variant).label,
                    row["engine_version"] or "unknown",
                    variant,
                    "all" if threads == "0" else threads,
                    row["host"],
                    row["henad_commit"],
                ]
            )
            + " |"
        )
    return "\n".join(lines) + "\n"


def count_lines(path: Path) -> int:
    """Non-blank, non-comment lines. A NetLogo model counts its code section only."""
    if not path.exists():
        return 0
    text = path.read_text(errors="replace")
    if path.suffix in {".nlogox", ".nlogo"}:
        start = text.find("<code>")
        end = text.find("</code>")
        text = text[start + 6 : end] if 0 <= start < end else ""
    total = 0
    for line in text.splitlines():
        stripped = line.strip()
        if not stripped or stripped.startswith(("#", "//", ";", "///", "/*", "*")):
            continue
        total += 1
    return total


def loc_table(manifest: Path) -> str:
    if not manifest.exists():
        return ""
    data = tomllib.loads(manifest.read_text())
    models = ["game_of_life", "boids", "sir", "ants"]
    header = ["engine"] + [MODEL_TITLES[m] for m in models]
    lines = ["| " + " | ".join(header) + " |", "|" + "---|" * len(header)]
    for engine, entry in data.items():
        cells = []
        for model in models:
            files = entry.get(model, [])
            total = sum(count_lines(REPO_ROOT / f) for f in files)
            cells.append(str(total) if total else "")
        lines.append("| " + " | ".join([engine] + cells) + " |")
    return "\n".join(lines) + "\n"


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("csv", type=Path, nargs="?", help="sweep CSV (default: newest in results/compare)")
    parser.add_argument("--out", type=Path, help="where figures and tables go")
    parser.add_argument("--publish", action="store_true", help="write into docs/assets/benchmarks instead")
    args = parser.parse_args()

    source = args.csv
    if source is None:
        candidates = sorted((REPO_ROOT / "results" / "compare").glob("*.csv"), key=lambda p: p.stat().st_mtime)
        if not candidates:
            raise SystemExit("no sweep CSV found; run compare_bench.py first")
        source = candidates[-1]

    out = args.out or (PUBLISH_DIR if args.publish else REPO_ROOT / "results" / "compare" / "plots")
    rows = load(source)
    if not rows:
        raise SystemExit(f"{source} has no rows")

    out.mkdir(parents=True, exist_ok=True)
    (out / "tables").mkdir(exist_ok=True)

    made = []
    for model in sorted({r["model"] for r in rows}):
        for metric, suffix in (("steps_per_sec", "steps"), ("updates_per_sec", "updates")):
            target = out / f"{model}_{suffix}_per_sec.png"
            if draw_model(rows, model, metric, target):
                made.append(target)
        table = ratio_table(rows, model)
        if table:
            path = out / "tables" / f"ratio_{model}.md"
            path.write_text(table)
            made.append(path)

    (out / "tables" / "engines.md").write_text(engines_table(rows))
    made.append(out / "tables" / "engines.md")

    loc = loc_table(REPO_ROOT / "benchmarks" / "loc_manifest.toml")
    if loc:
        path = out / "tables" / "loc.md"
        path.write_text(loc)
        made.append(path)

    if args.publish:
        target = PUBLISH_DIR / source.name
        target.write_bytes(source.read_bytes())
        made.append(target)

    for path in made:
        shown = path.relative_to(REPO_ROOT) if path.is_relative_to(REPO_ROOT) else path
        print(shown, file=sys.stderr)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
