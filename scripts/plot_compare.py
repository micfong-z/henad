#!/usr/bin/env python3
# /// script
# requires-python = ">=3.10"
# dependencies = ["matplotlib>=3.8"]
# ///
"""Turn a cross-engine sweep into the figures and tables the benchmarks page includes.

Reads whatever `compare_bench.py` wrote and produces, per model, the measured time against scale,
two throughput curves derived from it, and a table of median times normalised to Henad on one
thread. Lines of code come from `benchmarks/loc_manifest.toml`, the secondary metric the Agents.jl
comparison reports next to time: how much a model costs to express.

    uv run scripts/plot_compare.py results/compare/<host>_<date>.csv
    uv run scripts/plot_compare.py results/compare/<host>_<date>.csv --publish
"""

from __future__ import annotations

import argparse
import csv
import math
import sys
import tomllib
from collections import defaultdict
from dataclasses import dataclass
from pathlib import Path

import matplotlib

matplotlib.use("Agg")
import matplotlib.pyplot as plt
from matplotlib.ticker import FuncFormatter, NullFormatter

REPO_ROOT = Path(__file__).resolve().parent.parent
PUBLISH_DIR = REPO_ROOT / "docs" / "assets" / "benchmarks"

# Neutrals per theme. The engine hues below are shared, so a reader tracks an engine across both.
# Backgrounds stay transparent, letting each figure sit on whatever the page is painted.
THEMES = {
    "light": {"text": "#0b0b0b", "muted": "#52514e", "edge": "#d5d4cf", "grid": "#e6e5e0"},
    "dark": {"text": "#e8e7e3", "muted": "#a3a29d", "edge": "#4b4a46", "grid": "#3a3936"},
}

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
# One dash and marker per variant, so an engine measured two ways gives two readable curves. The
# page asks the reader to compare krABMaga's builds against each other.
VARIANT_STYLE = {
    "1t": ("-", "o"),
    "all": ("--", "s"),
    "gpu": (":", "^"),
    "default": ("-", "o"),
    "parallel": ("--", "s"),
}

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
        pretty = {
            "agents_jl": "Agents.jl",
            "netlogo": "NetLogo",
            "mesa": "Mesa",
            "mason": "MASON",
            "krabmaga": "krABMaga",
        }
        name = pretty.get(self.engine, self.engine)
        return name if self.variant == "default" else f"{name} ({self.variant})"

    @property
    def style(self) -> tuple[str, str, str]:
        color = ENGINE_COLORS.get(self.engine, "#666666")
        line, marker = VARIANT_STYLE.get(self.variant, ("-", "o"))
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


def apply_style(theme: dict[str, str]) -> None:
    plt.rcParams.update(
        {
            "figure.facecolor": "none",
            "axes.facecolor": "none",
            "savefig.facecolor": "none",
            "savefig.transparent": True,
            # Text as paths. An SVG in an `img` tag cannot reach the page's own `@font-face`, so
            # keeping it as text would render in whatever the viewer happens to have.
            "svg.fonttype": "path",
            # Legend labels and any bare `text` call read this, not `axes.labelcolor`.
            "text.color": theme["text"],
            "axes.edgecolor": theme["edge"],
            "axes.labelcolor": theme["muted"],
            "axes.titlecolor": theme["text"],
            "axes.grid": True,
            "grid.color": theme["grid"],
            "grid.linewidth": 0.6,
            "xtick.color": theme["muted"],
            "ytick.color": theme["muted"],
            "font.size": 9,
            # The site's own face. It ships as woff2, which matplotlib cannot read, so this picks up
            # a system install and falls back rather than failing on a machine without it.
            "font.family": "sans-serif",
            "font.sans-serif": ["IBM Plex Sans", "DejaVu Sans"],
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


# The intro page's one chart: a single rung, every engine that reached it, fastest first.
HEADLINE = {"model": "game_of_life", "scale": 1024, "stem": "headline_game_of_life"}


def human_time(seconds: float) -> str:
    if seconds < 1.0:
        return f"{seconds * 1000:.1f} ms" if seconds < 0.01 else f"{seconds * 1000:.0f} ms"
    return f"{seconds:.1f} s" if seconds < 10 else f"{seconds:.0f} s"


def speedup(factor: float) -> str:
    """A multiple against the slowest bar, at three significant figures.

    Mesa's row is a median over four reps, so the sixth digit of `178171` would be invented.
    """
    if factor < 10:
        return f"{factor:.1f}x"
    if factor < 1000:
        return f"{factor:.0f}x"
    digits = math.floor(math.log10(factor))
    return f"{round(factor, -(digits - 2)):,.0f}x"


def draw_headline(rows: list[dict], out: Path, theme: dict[str, str]) -> bool:
    """One rung as a sorted bar chart. Log axis, since the rung spans four orders of magnitude."""
    points = [
        r
        for r in rows
        if r["model"] == HEADLINE["model"] and int(r["scale"]) == HEADLINE["scale"] and number(r, "median_s")
    ]
    if not points:
        return False
    ranked = sorted(
        ((number(r, "median_s"), Series(r["engine"], r["variant"]), r["status"]) for r in points),
        key=lambda p: p[0],
    )

    apply_style(theme)
    fig, ax = plt.subplots(figsize=(7.9, 3.4))
    times = [t for t, _, _ in ranked]
    ys = list(range(len(ranked) - 1, -1, -1))
    ax.barh(ys, times, color=[series.style[0] for _, series, _ in ranked], height=0.62)

    floor = 10 ** math.floor(math.log10(min(times)))
    ax.set_xscale("log")
    ax.set_xlim(floor, max(times) * 18)
    ax.set_yticks(ys, [series.label for _, series, _ in ranked])
    for tick, (_, series, _) in zip(ax.get_yticklabels(), ranked):
        tick.set_fontweight("bold" if series is ranked[0][1] else "normal")
    slowest = max(times)
    for y, (t, _, status) in zip(ys, ranked):
        star = "" if status == "ok" else " *"
        ax.text(
            t * 1.3,
            y,
            f"{human_time(t)}{star}  ({speedup(slowest / t)})",
            va="center",
            fontsize=8.5,
            color=theme["text"],
        )

    ax.xaxis.set_major_formatter(FuncFormatter(lambda v, _p: human_time(v).replace(".0", "")))
    ax.xaxis.set_minor_formatter(NullFormatter())
    ax.set_xlabel("time")
    ax.grid(axis="y", visible=False)
    for side in ("left", "right", "top"):
        ax.spines[side].set_visible(False)
    fig.subplots_adjust(left=0.22, right=0.97, top=0.95, bottom=0.20)
    out.parent.mkdir(parents=True, exist_ok=True)
    fig.savefig(out)
    plt.close(fig)
    return True


def y_label(metric: str, points: list[dict]) -> str:
    if metric == "steps_per_sec":
        return "steps / second"
    if metric == "updates_per_sec":
        return "cell or agent updates / second"
    steps = {r["steps"] for r in points}
    return f"seconds for {steps.pop()} steps" if len(steps) == 1 else "seconds"


def draw_model(rows: list[dict], model: str, metric: str, out: Path, theme: dict[str, str]) -> bool:
    points = [r for r in rows if r["model"] == model]
    if not points:
        return False
    apply_style(theme)
    # Wider than the plot needs, since the legend sits outside the axes on the left.
    fig, ax = plt.subplots(figsize=(7.9, 4.0))
    axis = points[0]["axis"]

    drawn = False
    for series, group in sorted(series_of(points).items(), key=lambda kv: kv[0].label):
        measured = sorted((r for r in group if number(r, metric)), key=lambda r: int(r["scale"]))
        if not measured:
            continue
        xs = [int(r["scale"]) for r in measured]
        ys = [number(r, metric) for r in measured]
        color, line, marker = series.style
        ax.plot(xs, ys, line, marker=marker, color=color, label=series.label, linewidth=1.6, markersize=4.5)
        # A rung that ran out of budget still timed the reps it finished. Hollow, since a median
        # over two reps is not the same measurement as one over five.
        short = [(int(r["scale"]), number(r, metric)) for r in measured if r["status"] != "ok"]
        if short:
            ax.plot(
                [x for x, _ in short],
                [y for _, y in short],
                linestyle="none",
                marker=marker,
                color=color,
                markersize=7.0,
                markerfacecolor="none",
                markeredgewidth=1.3,
            )
        drawn = True
        stopped = next((r for r in group if r["status"] in INCOMPLETE and INCOMPLETE[r["status"]]), None)
        if stopped:
            last = xs[-1] == max(int(r["scale"]) for r in points)
            ax.annotate(
                INCOMPLETE[stopped["status"]],
                xy=(xs[-1], ys[-1]),
                xytext=(-6 if last else 4, -10),
                textcoords="offset points",
                fontsize=7,
                color=color,
                ha="right" if last else "left",
            )
    if not drawn:
        plt.close(fig)
        return False

    ax.set_xscale("log")
    ax.set_yscale("log")
    ax.set_xlabel(AXIS_LABELS.get(axis, axis))
    ax.set_ylabel(y_label(metric, points))
    ax.set_title(MODEL_TITLES.get(model, model))
    ax.yaxis.set_major_formatter(FuncFormatter(si))
    ax.xaxis.set_major_formatter(FuncFormatter(si))
    # Outside the axes. Inside, a legend covers data on every figure where a slow engine stops
    # early and leaves the top-left empty on some models and not others.
    # Anchored by its right edge, so the gap to the axes holds whatever the labels are.
    ax.legend(loc="center right", bbox_to_anchor=(-0.15, 0.5), fontsize=8)
    fig.subplots_adjust(left=0.28, right=0.975, top=0.91, bottom=0.13)
    out.parent.mkdir(parents=True, exist_ok=True)
    fig.savefig(out)
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
            if not base:
                # No Henad row to divide by, or a zero one. An empty cell here would read as
                # "this engine never ran".
                cells.append("no baseline")
            else:
                cells.append(f"{mine / base:.2f}" if mine else "")
        lines.append("| " + " | ".join([series.label] + cells) + " |")
    return "\n".join(lines) + "\n"


def engines_table(rows: list[dict]) -> str:
    """What actually ran, so a table's provenance travels with it."""
    seen: dict[tuple[str, str], list[dict]] = defaultdict(list)
    for row in rows:
        seen[(row["engine"], row["variant"])].append(row)
    header = ["engine", "version", "variant", "threads", "host", "Henad commit"]
    lines = ["| " + " | ".join(header) + " |", "|" + "---|" * len(header)]
    for (engine, variant), group in sorted(seen.items()):
        row = group[0]
        # A resumed sweep can span a rebuild, and publishing the first row as though it covered
        # every number hid that.
        commits = sorted({r["henad_commit"] for r in group if r["henad_commit"]})
        hosts = sorted({r["host"] for r in group if r["host"]})
        # A GPU row's thread count is the host side of a device queue, which says nothing useful.
        threads = "—" if variant == "gpu" else (row["threads"] or "?")
        lines.append(
            "| "
            + " | ".join(
                [
                    Series(engine, variant).label,
                    row["engine_version"] or "unknown",
                    variant,
                    "all" if str(threads) == "0" else str(threads),
                    ", ".join(hosts) or "?",
                    ", ".join(commits) or "?",
                ]
            )
            + " |"
        )
    return "\n".join(lines) + "\n"


# Line comments by language. One rule for all six charged Python and Julia for their docstrings
# while excusing every other language's comments, and read Rust attributes as comments.
LINE_COMMENTS = {
    ".py": ("#",),
    ".jl": ("#",),
    ".rs": ("//",),
    ".java": ("//",),
    ".nlogox": (";",),
    ".nlogo": (";",),
}
# Languages whose idiomatic comment is a string literal, which no prefix test sees.
DOCSTRING_FENCES = ('"""', "'''")
DOCSTRING_LANGUAGES = {".py", ".jl"}
# Languages with a delimited block comment, which a line-prefix test also never sees.
BLOCK_COMMENTS = {".rs": ("/*", "*/"), ".java": ("/*", "*/")}


def strip_netlogo(text: str) -> str:
    """A NetLogo model's code section, without the XML wrapper around it.

    The CDATA markers are markup, and were counting as two lines of every NetLogo model.
    """
    start, end = text.find("<code>"), text.find("</code>")
    if not 0 <= start < end:
        return ""
    body = text[start + len("<code>") : end]
    for marker in ("<![CDATA[", "]]>"):
        body = body.replace(marker, "")
    return body


def strip_rust_tests(text: str) -> str:
    """Rust source without its `#[cfg(test)]` blocks.

    Henad's model files carry their unit tests inline where no port file does, and counting them
    put Game of Life at more than twice its real size.
    """
    out, lines = [], text.splitlines()
    i = 0
    while i < len(lines):
        if lines[i].strip().startswith("#[cfg(test)]"):
            depth, seen = 0, False
            while i < len(lines) and not (seen and depth == 0):
                depth += lines[i].count("{") - lines[i].count("}")
                seen = seen or "{" in lines[i]
                i += 1
            continue
        out.append(lines[i])
        i += 1
    return "\n".join(out)


def count_lines(path: Path) -> int:
    """Non-blank, non-comment lines, by the rules of the language the file is written in."""
    if not path.exists():
        return 0
    text = path.read_text(errors="replace")
    if path.suffix in {".nlogox", ".nlogo"}:
        text = strip_netlogo(text)
    if path.suffix == ".rs":
        text = strip_rust_tests(text)
    prefixes = LINE_COMMENTS.get(path.suffix, ("#", "//", ";"))
    docstrings = path.suffix in DOCSTRING_LANGUAGES
    block = BLOCK_COMMENTS.get(path.suffix)
    total = 0
    closer = ""
    for line in text.splitlines():
        stripped = line.strip()
        if closer:
            if closer in stripped:
                closer = ""
            continue
        if not stripped:
            continue
        if docstrings and (opener := next((q for q in DOCSTRING_FENCES if stripped.startswith(q)), "")):
            # One that opens and closes on the same line is still only a comment.
            if not (stripped.endswith(opener) and len(stripped) > len(opener)):
                closer = opener
            continue
        if block and stripped.startswith(block[0]):
            if block[1] not in stripped[len(block[0]) :]:
                closer = block[1]
            continue
        if stripped.startswith(prefixes):
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

    def themed(draw, stem: Path) -> list[Path]:
        """One figure per theme, named `<stem>-light.svg` and `<stem>-dark.svg`."""
        written = []
        for name, theme in THEMES.items():
            target = stem.with_name(f"{stem.name}-{name}.svg")
            if draw(target, theme):
                written.append(target)
        return written

    made = []
    for model in sorted({r["model"] for r in rows}):
        for metric, stem in (
            ("median_s", "seconds"),
            ("steps_per_sec", "steps_per_sec"),
            ("updates_per_sec", "updates_per_sec"),
        ):
            made += themed(
                lambda t, th, m=model, mt=metric: draw_model(rows, m, mt, t, th),
                out / f"{model}_{stem}",
            )
        table = ratio_table(rows, model)
        if table:
            path = out / "tables" / f"ratio_{model}.snippet"
            path.write_text(table)
            made.append(path)

    made += themed(lambda t, th: draw_headline(rows, t, th), out / HEADLINE["stem"])

    (out / "tables" / "engines.snippet").write_text(engines_table(rows))
    made.append(out / "tables" / "engines.snippet")

    loc = loc_table(REPO_ROOT / "benchmarks" / "loc_manifest.toml")
    if loc:
        path = out / "tables" / "loc.snippet"
        path.write_text(loc)
        made.append(path)

    if args.publish and out == PUBLISH_DIR:
        target = PUBLISH_DIR / source.name
        target.write_bytes(source.read_bytes())
        made.append(target)

    for path in made:
        shown = path.relative_to(REPO_ROOT) if path.is_relative_to(REPO_ROOT) else path
        print(shown, file=sys.stderr)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
