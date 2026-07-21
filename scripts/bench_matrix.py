#!/usr/bin/env python3
from __future__ import annotations

import argparse
import csv
import json
import re
import subprocess
import sys
import time
from dataclasses import dataclass
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parent.parent
DEFAULT_BINARY = REPO_ROOT / "target" / "release" / "henad-cli"

GRID_SIZES: list[tuple[int, int]] = [
    (0, 0),
    (1, 1),
    (64, 64),
    (256, 256),
    (1024, 1024),
    (2048, 2048),
    (4096, 4096),
]
GRID_SIZES_GPU_ONLY: list[tuple[int, int]] = [(8192, 8192)]

GLOBAL_WARMUPS: list[int] = [0, 1000]
GLOBAL_WARMUPS_GPU_ONLY: list[int] = [10000]

WARMUP_FRACTIONS: list[float] = [0.0, 0.2]

STEPS: list[int] = [1000]
STEPS_GPU_ONLY: list[int] = [100000]

# Model ids carrying this prefix are GPU-backed (matches the registry convention).
GPU_PREFIX = "gpu_"

DEFAULT_EXCLUDED_MODELS: list[str] = ["boids"]

_DURATION_UNITS = {
    "s": 1.0,
    "ms": 1e-3,
    "µs": 1e-6,  # MICRO SIGN, what Rust actually emits
    "μs": 1e-6,  # GREEK SMALL LETTER MU, just in case
    "us": 1e-6,
    "ns": 1e-9,
}
_DURATION_RE = re.compile(r"^([0-9]*\.?[0-9]+)\s*([a-zµμ]+)$", re.IGNORECASE)

# numfmt renders grouped numbers with a space separator: "18 360 474 369.453".
_GROUP_CHARS = " \t   "

_STAT_RE = re.compile(r"^\s*(min|median|max|mean|std dev):\s+(\S+)\s*$")
_STEPS_PER_SEC_RE = re.compile(r"^\s*>\s*mean steps/sec:\s*(.+?)\s*$")
_UPDATES_PER_SEC_RE = re.compile(r"^\s*>\s*mean updates/sec:\s*(.+?)\s*$")
_GRID_SIZE_RE = re.compile(r"^\s*>\s*grid size:\s*(.+?)\s*$")
_REP_RE = re.compile(r"^\s*#\s*(\d+):\s*(\S+)\s*(.*)$")
_LIST_RE = re.compile(r"^\s{2,}(\S+)\s+(.+?)\s*$")


def parse_duration(text: str) -> float | None:
    """Parse a Rust ``Duration`` Debug string into seconds."""
    match = _DURATION_RE.match(text.strip())
    if not match:
        return None
    value, unit = match.group(1), match.group(2).lower()
    scale = _DURATION_UNITS.get(unit)
    if scale is None:
        return None
    return float(value) * scale


def parse_grouped_number(text: str) -> float | None:
    """Parse a numfmt-grouped number such as ``18 360 474 369.453``."""
    cleaned = text
    for ch in _GROUP_CHARS:
        cleaned = cleaned.replace(ch, "")
    if not cleaned:
        return None
    try:
        return float(cleaned)
    except ValueError:
        return None


def parse_report(stdout: str) -> dict[str, float | None]:
    """Pull every metric out of the ``benchmark result:`` block on stdout."""
    out: dict[str, float | None] = {
        "min_s": None,
        "median_s": None,
        "max_s": None,
        "mean_s": None,
        "std_dev_s": None,
        "steps_per_sec": None,
        "updates_per_sec": None,
        "grid_size": None,
    }
    key_by_label = {
        "min": "min_s",
        "median": "median_s",
        "max": "max_s",
        "mean": "mean_s",
        "std dev": "std_dev_s",
    }
    for line in stdout.splitlines():
        stat = _STAT_RE.match(line)
        if stat:
            out[key_by_label[stat.group(1)]] = parse_duration(stat.group(2))
            continue
        for regex, key in (
            (_STEPS_PER_SEC_RE, "steps_per_sec"),
            (_UPDATES_PER_SEC_RE, "updates_per_sec"),
            (_GRID_SIZE_RE, "grid_size"),
        ):
            match = regex.match(line)
            if match:
                out[key] = parse_grouped_number(match.group(1))
                break
    return out


def parse_rep_times(stderr: str) -> tuple[float | None, list[float]]:
    """Extract the per-rep timings from stderr.

    Returns ``(global_warmup_seconds, [rep_seconds, ...])``. Rep 0 is the
    discarded global warm-up run when present.
    """
    warmup: float | None = None
    reps: list[float] = []
    for line in stderr.splitlines():
        match = _REP_RE.match(line)
        if not match:
            continue
        index, value = int(match.group(1)), parse_duration(match.group(2))
        if value is None:
            continue
        if index == 0:
            warmup = value
        else:
            reps.append(value)
    return warmup, reps


# --- running ---------------------------------------------------------------


@dataclass(frozen=True)
class Config:
    """One point in the sweep."""

    model: str
    is_gpu: bool
    grid: tuple[int, int] | None
    steps: int
    warmup: int
    global_warmup: int

    def cli_args(self, reps: int) -> list[str]:
        args = [
            self.model,
            "--steps",
            str(self.steps),
            "--reps",
            str(reps),
            "--warmup",
            str(self.warmup),
            "--global-warmup",
            str(self.global_warmup),
        ]
        if self.grid is not None:
            width, height = self.grid
            args += ["--set", f"grid_width={width}", "--set", f"grid_height={height}"]
        return args

    def key(self) -> tuple:
        """Identity used for --resume de-duplication."""
        width, height = self.grid if self.grid is not None else (-1, -1)
        return (self.model, width, height, self.steps, self.warmup, self.global_warmup)

    def estimated_cost(self, reps: int) -> int:
        """Rough work estimate (cell-steps), used to order cheap runs first."""
        if self.grid is not None:
            cells = max(self.grid[0] * self.grid[1], 1)
        else:
            cells = 1_000_000  # unknown (continuous model); assume mid-sized
        total_steps = (self.steps + self.warmup) * reps + self.global_warmup
        return total_steps * cells


def run_cli(binary: Path, args: list[str], timeout: float) -> tuple[int, str, str, float]:
    """Run henad-cli, returning (returncode, stdout, stderr, wall_seconds)."""
    started = time.perf_counter()
    try:
        proc = subprocess.run(
            [str(binary), *args],
            capture_output=True,
            text=True,
            timeout=timeout,
            check=False,
        )
    except subprocess.TimeoutExpired as exc:
        elapsed = time.perf_counter() - started
        stdout = exc.stdout.decode() if isinstance(exc.stdout, bytes) else (exc.stdout or "")
        stderr = exc.stderr.decode() if isinstance(exc.stderr, bytes) else (exc.stderr or "")
        return -1, stdout, stderr + "\n[timed out]", elapsed
    return proc.returncode, proc.stdout, proc.stderr, time.perf_counter() - started


def discover_models(binary: Path, timeout: float) -> list[str]:
    """Return every model id reported by ``--list``."""
    code, stdout, stderr, _ = run_cli(binary, ["--list"], timeout)
    if code != 0:
        raise SystemExit(f"`henad-cli --list` failed ({code}):\n{stderr}")
    models: list[str] = []
    for line in stdout.splitlines():
        if line.startswith("available models"):
            continue
        match = _LIST_RE.match(line)
        if match:
            models.append(match.group(1))
    if not models:
        raise SystemExit(f"could not parse any models from --list output:\n{stdout}")
    return models


def model_supports_grid(binary: Path, model: str, timeout: float) -> bool:
    """Probe whether a model accepts grid_width/grid_height (grid vs continuous)."""
    code, _, _, _ = run_cli(
        binary,
        [model, "--steps", "1", "--reps", "1", "--set", "grid_width=64", "--set", "grid_height=64"],
        timeout,
    )
    return code == 0


def build_configs(models: list[str], grid_capable: dict[str, bool], reps: int) -> list[Config]:
    """Expand the matrix into concrete configurations, cheapest first."""
    configs: list[Config] = []
    for model in models:
        is_gpu = model.startswith(GPU_PREFIX)

        grids: list[tuple[int, int] | None]
        if grid_capable[model]:
            grids = list(GRID_SIZES)
            if is_gpu:
                grids += GRID_SIZES_GPU_ONLY
        else:
            grids = [None]

        global_warmups = list(GLOBAL_WARMUPS) + (GLOBAL_WARMUPS_GPU_ONLY if is_gpu else [])
        step_counts = list(STEPS) + (STEPS_GPU_ONLY if is_gpu else [])

        for grid in grids:
            for steps in step_counts:
                for fraction in WARMUP_FRACTIONS:
                    for global_warmup in global_warmups:
                        configs.append(
                            Config(
                                model=model,
                                is_gpu=is_gpu,
                                grid=grid,
                                steps=steps,
                                warmup=int(steps * fraction),
                                global_warmup=global_warmup,
                            )
                        )
    configs.sort(key=lambda c: (c.model, c.estimated_cost(reps)))
    return configs


CSV_FIELDS = [
    "model",
    "is_gpu",
    "grid_w",
    "grid_h",
    "requested_cells",
    "steps",
    "warmup",
    "global_warmup",
    "reps",
    "status",
    "min_s",
    "median_s",
    "max_s",
    "mean_s",
    "std_dev_s",
    "steps_per_sec",
    "updates_per_sec",
    "grid_size",
    "global_warmup_s",
    "rep_times_s",
    "wall_s",
    "error",
]


def extract_error(stderr: str, code: int) -> str:
    """Pick the most informative failure line out of stderr.

    A Rust panic puts its message on the line *after* ``panicked at ...``, and
    anyhow prints ``Error: <cause>``; either beats the trailing RUST_BACKTRACE
    note that would otherwise be the last line.
    """
    lines = [ln.strip() for ln in stderr.strip().splitlines() if ln.strip()]
    for index, line in enumerate(lines):
        if "panicked at" in line:
            message = lines[index + 1] if index + 1 < len(lines) else ""
            return f"panic: {message or line}"[:300]
    for line in lines:
        if line.startswith("Error:"):
            return line[:300]
    return (lines[-1] if lines else f"exit {code}")[:300]


def run_config(binary: Path, cfg: Config, reps: int, timeout: float) -> dict:
    """Execute one configuration and flatten it into a CSV row."""
    code, stdout, stderr, wall = run_cli(binary, cfg.cli_args(reps), timeout)

    row: dict = {
        "model": cfg.model,
        "is_gpu": cfg.is_gpu,
        "grid_w": cfg.grid[0] if cfg.grid else "",
        "grid_h": cfg.grid[1] if cfg.grid else "",
        "requested_cells": (cfg.grid[0] * cfg.grid[1]) if cfg.grid else "",
        "steps": cfg.steps,
        "warmup": cfg.warmup,
        "global_warmup": cfg.global_warmup,
        "reps": reps,
        "wall_s": round(wall, 6),
        "error": "",
    }
    row.update({field: None for field in CSV_FIELDS if field not in row})

    if code == -1:
        row["status"] = "timeout"
        row["error"] = f"exceeded {timeout}s"
        return row
    if code != 0:
        row["status"] = "error"
        row["error"] = extract_error(stderr, code)
        return row

    row["status"] = "ok"
    row.update(parse_report(stdout))
    warmup_s, rep_times = parse_rep_times(stderr)
    row["global_warmup_s"] = warmup_s
    row["rep_times_s"] = json.dumps([round(t, 9) for t in rep_times])
    return row


def load_done_keys(path: Path) -> set[tuple]:
    """Read an existing CSV so --resume can skip finished configurations."""
    if not path.exists():
        return set()
    done: set[tuple] = set()
    with path.open(newline="", encoding="utf-8") as handle:
        for row in csv.DictReader(handle):
            try:
                grid_w = int(row["grid_w"]) if row["grid_w"] else -1
                grid_h = int(row["grid_h"]) if row["grid_h"] else -1
                done.add(
                    (
                        row["model"],
                        grid_w,
                        grid_h,
                        int(row["steps"]),
                        int(row["warmup"]),
                        int(row["global_warmup"]),
                    )
                )
            except (KeyError, ValueError, TypeError):
                continue
    return done


def main() -> int:
    parser = argparse.ArgumentParser(
        description="Benchmark all Henad models across a configuration matrix.",
        formatter_class=argparse.ArgumentDefaultsHelpFormatter,
    )
    parser.add_argument("--binary", type=Path, default=DEFAULT_BINARY, help="path to henad-cli")
    parser.add_argument("--out", type=Path, default=REPO_ROOT / "results" / "bench_matrix.csv", help="CSV output path")
    parser.add_argument("--reps", type=int, default=3, help="timed reps per configuration")
    parser.add_argument("--timeout", type=float, default=900.0, help="per-run timeout in seconds")
    parser.add_argument("--models", nargs="*", help="only these model ids (default: all)")
    parser.add_argument(
        "--exclude",
        nargs="*",
        default=list(DEFAULT_EXCLUDED_MODELS),
        help="model ids to skip; ignored when --models is given. Pass --exclude with no values to run everything",
    )
    parser.add_argument("--dry-run", action="store_true", help="print the matrix and exit")
    parser.add_argument("--resume", action="store_true", help="skip configs already in the output CSV")
    args = parser.parse_args()

    if not args.binary.exists():
        print(
            f"error: {args.binary} not found.\nBuild it first:  cargo build --release -p henad-cli",
            file=sys.stderr,
        )
        return 1

    models = discover_models(args.binary, args.timeout)
    if args.models:
        unknown = sorted(set(args.models) - set(models))
        if unknown:
            print(f"error: unknown model(s): {', '.join(unknown)}\navailable: {', '.join(models)}", file=sys.stderr)
            return 1
        # An explicit --models list wins over --exclude, so `--models boids` still works.
        models = [m for m in models if m in args.models]
    elif args.exclude:
        skipped = [m for m in models if m in args.exclude]
        models = [m for m in models if m not in args.exclude]
        if skipped:
            print(f"skipping {', '.join(skipped)} (--exclude)", file=sys.stderr)

    if not models:
        print("error: every discovered model was excluded", file=sys.stderr)
        return 1

    print(f"models: {', '.join(models)}", file=sys.stderr)
    grid_capable = {m: model_supports_grid(args.binary, m, args.timeout) for m in models}
    for model, capable in grid_capable.items():
        print(f"  {model:<20} {'grid' if capable else 'continuous (no grid axis)'}", file=sys.stderr)

    configs = build_configs(models, grid_capable, args.reps)

    done = load_done_keys(args.out) if args.resume else set()
    if done:
        before = len(configs)
        configs = [c for c in configs if c.key() not in done]
        print(f"resume: skipping {before - len(configs)} completed configs", file=sys.stderr)

    print(f"\n{len(configs)} configurations x {args.reps} reps", file=sys.stderr)
    if args.dry_run:
        for cfg in configs:
            grid = f"{cfg.grid[0]}x{cfg.grid[1]}" if cfg.grid else "n/a"
            print(
                f"  {cfg.model:<20} grid={grid:<12} steps={cfg.steps:<7}"
                f" warmup={cfg.warmup:<6} global={cfg.global_warmup}",
                file=sys.stderr,
            )
        return 0

    args.out.parent.mkdir(parents=True, exist_ok=True)
    appending = args.resume and args.out.exists()
    rows: list[dict] = []

    # Append as we go: a long sweep that gets interrupted keeps everything so far.
    with args.out.open("a" if appending else "w", newline="", encoding="utf-8") as handle:
        writer = csv.DictWriter(handle, fieldnames=CSV_FIELDS)
        if not appending:
            writer.writeheader()

        for index, cfg in enumerate(configs, start=1):
            grid = f"{cfg.grid[0]}x{cfg.grid[1]}" if cfg.grid else "n/a"
            label = (
                f"[{index}/{len(configs)}] {cfg.model} grid={grid} steps={cfg.steps}"
                f" warmup={cfg.warmup} global={cfg.global_warmup}"
            )
            print(label, file=sys.stderr, flush=True)

            row = run_config(args.binary, cfg, args.reps, args.timeout)
            writer.writerow(row)
            handle.flush()
            rows.append(row)

            if row["status"] == "ok":
                mean_s = row.get("mean_s")
                updates = row.get("updates_per_sec")
                summary = f"mean={mean_s * 1e3:.3f}ms" if mean_s else "mean=?"
                if updates:
                    summary += f"  updates/s={updates:,.0f}"
            else:
                summary = f"{row['status']}: {row['error']}"
            print(f"    -> {summary}", file=sys.stderr, flush=True)

    json_path = args.out.with_suffix(".json")
    json_path.write_text(json.dumps(rows, indent=2), encoding="utf-8")

    ok = sum(1 for r in rows if r["status"] == "ok")
    print(f"\nwrote {len(rows)} rows ({ok} ok) to {args.out} and {json_path}", file=sys.stderr)
    return 0


if __name__ == "__main__":
    sys.exit(main())
