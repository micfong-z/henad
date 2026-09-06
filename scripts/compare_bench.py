#!/usr/bin/env python3
"""Benchmark Henad against the other ABM engines on the same four models, into a CSV.

Every engine is driven through the harness contract in ``benchmarks/protocol.md``: the same
arguments in, one JSON object per rep out. This script owns the ladder, the timeouts and the
arithmetic, so no engine's own summary reaches a published table.

An engine that is not installed is skipped with a reason rather than failing the sweep, so a
partial machine still produces a partial table.

Model parameters and the agent-density rule come from ``bench_matrix.py``, which reads them from
``henad-cli --params``. The reference engines are held to whatever Henad's own descriptors say.
"""

from __future__ import annotations

import argparse
import csv
import json
import os
import platform
import signal
import shutil
import statistics
import subprocess
import sys
import time
from dataclasses import dataclass, field
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

from bench_matrix import (  # noqa: E402
    ModelInfo,
    discover_params,
    extract_error,
    world_for_agents,
)
from progress import Progress  # noqa: E402

REPO_ROOT = Path(__file__).resolve().parent.parent
DEFAULT_BINARY = REPO_ROOT / "target" / "release" / "henad-cli"
BENCHMARKS = REPO_ROOT / "benchmarks"

# --- the ladder ------------------------------------------------------------------------------
#
# Grid models scale over grid size and agent models over population at the model's own default
# density, which is `bench_matrix.py`'s rule. Step counts are uniform across engines; a slow engine
# runs out of time rather than out of steps, and that is the measurement.

GRID_SIZES = [64, 256, 1024, 2048, 4096]

# Untimed steps on a throwaway state before Henad's GPU reps, enough to pay shader compilation and
# lift the clocks. Measured: without it rep 0 of a 1024 squared grid ran 2.3x the warm time.
GPU_RAMP_STEPS = 1000

LADDER: dict[str, dict] = {
    "game_of_life": {"axis": "grid", "points": GRID_SIZES, "steps": 100, "warmup": 10},
    "sir": {"axis": "grid", "points": GRID_SIZES, "steps": 100, "warmup": 10},
    "boids": {"axis": "agents", "points": [1_000, 10_000, 50_000, 100_000, 1_000_000], "steps": 100, "warmup": 10},
    # Ants stops at 200k. Constant density puts twenty field cells behind every ant, and the
    # field update is per cell per tick, so a 2M rung spends its time on the field rather than
    # on the agents. `bench_matrix.py` already carries Henad past this point.
    "ants": {"axis": "agents", "points": [2_000, 20_000, 200_000], "steps": 200, "warmup": 20},
}

MODELS = list(LADDER)
BASE_SEED = 42


@dataclass(frozen=True)
class Point:
    """One rung of one model's ladder."""

    model: str
    scale: int
    grid: tuple[int, int] | None
    agents: int | None
    world: tuple[float, float] | None
    steps: int
    warmup: int

    def overrides(self) -> dict[str, str]:
        if self.grid is not None:
            return {"grid_width": str(self.grid[0]), "grid_height": str(self.grid[1])}
        if self.agents is not None and self.world is not None:
            return {
                "num_agents": str(self.agents),
                "world_width": f"{self.world[0]:.6g}",
                "world_height": f"{self.world[1]:.6g}",
            }
        return {}

    def harness_args(self) -> list[str]:
        if self.grid is not None:
            return ["--grid", str(self.grid[0]), str(self.grid[1])]
        assert self.agents is not None and self.world is not None
        return [
            "--agents",
            str(self.agents),
            "--world",
            f"{self.world[0]:.6g}",
            f"{self.world[1]:.6g}",
        ]

    def label(self) -> str:
        if self.grid is not None:
            return f"{self.grid[0]}x{self.grid[1]}"
        return f"{self.agents:,} agents"


def build_points(infos: dict[str, ModelInfo], models: list[str]) -> list[Point]:
    points: list[Point] = []
    for model in models:
        spec = LADDER[model]
        info = infos[model]
        for scale in spec["points"]:
            if spec["axis"] == "grid":
                points.append(Point(model, scale, (scale, scale), None, None, spec["steps"], spec["warmup"]))
            else:
                points.append(
                    Point(model, scale, None, scale, world_for_agents(info, scale), spec["steps"], spec["warmup"])
                )
    return points


# --- engines ---------------------------------------------------------------------------------


@dataclass
class Engine:
    """One engine, in as many variants as it is worth measuring separately.

    `one_rep_per_process` is for an engine whose reps would otherwise share a trajectory. No engine
    needs it now that henad-cli seeds rep `i` from `base + i`, and running Henad one process per rep
    put GPU ramp-up inside every timed window.

    `variants` is every build that exists, which is what gets gated and built. `timed` is the subset
    the sweep puts on the ladder.
    """

    name: str
    variants: list[str] = field(default_factory=lambda: ["default"])
    untimed: tuple[str, ...] = ()
    one_rep_per_process: bool = False
    version: str = ""
    _reason: str | None = None

    @property
    def timed(self) -> list[str]:
        return [v for v in self.variants if v not in self.untimed]

    def detect(self) -> str | None:
        """Return why this engine cannot run here, or None when it can."""
        raise NotImplementedError

    def prepare(self) -> None:
        """Compile or instantiate, once per sweep, so no build lands in a timed window."""

    @staticmethod
    def build_with(args: list[str], what: str) -> None:
        result = subprocess.run(args, capture_output=True, text=True, check=False, timeout=1800)
        if result.returncode != 0:
            tail = (result.stderr or result.stdout).strip().splitlines()[-6:]
            raise SystemExit(f"building {what} failed:\n  " + "\n  ".join(tail))

    def launcher(self, variant: str) -> list[str]:
        """How this engine's harness is started, before any harness argument."""
        raise NotImplementedError

    def command(
        self, variant: str, point: Point, reps: int, seed: int, threads: int, resolved: dict[str, str]
    ) -> list[str]:
        return [*self.launcher(variant), *common_harness_args(point, reps, seed, threads, resolved)]

    def validate_command(
        self, scenario: str, out: Path, seed: int | None = None, variant: str | None = None
    ) -> list[str] | None:
        """Run one gate scenario and write its fixture. None when the harness has no validate mode.

        `variant` defaults to the first. Gating every variant means passing this one explicitly.
        """
        args = [*self.launcher(variant or self.variants[0]), "--validate", scenario, "--out", str(out)]
        if seed is not None:
            args += ["--seed", str(seed)]
        return args

    def threads_for(self, variant: str) -> int:
        return 1

    def model_for(self, variant: str, model: str) -> str:
        return model


class Henad(Engine):
    def __init__(self, binary: Path) -> None:
        super().__init__(name="henad", variants=["1t", "all", "gpu"])
        self.binary = binary

    def detect(self) -> str | None:
        if not self.binary.exists():
            return f"{self.binary} not built (cargo build --release -p henad-cli)"
        return None

    def prepare(self) -> None:
        self.version = describe_commit()

    def threads_for(self, variant: str) -> int:
        return 1 if variant == "1t" else 0

    def model_for(self, variant: str, model: str) -> str:
        return f"gpu_{model}" if variant == "gpu" else model

    def validate_command(
        self, scenario: str, out: Path, seed: int | None = None, variant: str | None = None
    ) -> list[str] | None:
        return None  # Henad is what the ports are gated against, so it has no gate of its own.

    def command(
        self, variant: str, point: Point, reps: int, seed: int, threads: int, resolved: dict[str, str]
    ) -> list[str]:
        args = [
            str(self.binary),
            self.model_for(variant, point.model),
            "--json",
            "--steps",
            str(point.steps),
            "--warmup",
            str(point.warmup),
            "--reps",
            str(reps),
            "--seed",
            str(seed),
            "--threads",
            str(threads),
        ]
        if variant == "gpu":
            # First use compiles shaders and the clocks are still low. Without this the whole cost
            # lands in rep 0's timed window.
            args += ["--global-warmup", str(GPU_RAMP_STEPS)]
        for key, value in point.overrides().items():
            args += ["--set", f"{key}={value}"]
        return args


class Mesa(Engine):
    def __init__(self) -> None:
        super().__init__(name="mesa")
        self.project = BENCHMARKS / "mesa"

    def detect(self) -> str | None:
        if not (self.project / "bench.py").exists():
            return "benchmarks/mesa is missing"
        if shutil.which("uv") is None:
            return "uv not on PATH"
        return None

    def launcher(self, variant: str) -> list[str]:
        return ["uv", "run", "--project", str(self.project), "python", str(self.project / "bench.py")]


class NetLogo(Engine):
    def __init__(self) -> None:
        super().__init__(name="netlogo")
        self.home = Path(os.environ.get("NETLOGO_HOME", "/Applications/NetLogo 7.0.4"))
        self.classes = REPO_ROOT / "target" / "bench" / "netlogo"

    def detect(self) -> str | None:
        if not (BENCHMARKS / "netlogo" / "NetLogoBench.java").exists():
            return "benchmarks/netlogo is missing"
        if not self.home.exists():
            return f"NETLOGO_HOME not found at {self.home}"
        if shutil.which("java") is None:
            return "java not on PATH"
        return None

    def jar(self) -> Path:
        jars = sorted(self.home.glob("app/netlogo-*.jar"))
        if not jars:
            raise SystemExit(f"no netlogo jar under {self.home / 'app'}")
        return jars[-1]

    def prepare(self) -> None:
        self.classes.mkdir(parents=True, exist_ok=True)
        sources = sorted(str(p) for p in (BENCHMARKS / "netlogo").glob("*.java"))
        self.build_with(["javac", "-nowarn", "-d", str(self.classes), "-cp", str(self.jar()), *sources], "netlogo")
        self.version = self.jar().stem.replace("netlogo-", "NetLogo ")

    def launcher(self, variant: str) -> list[str]:
        # What `netlogo-headless.sh` passes, minus everything about the GUI.
        return [
            "java",
            "-XX:MaxRAMPercentage=50",
            "-Dfile.encoding=UTF-8",
            f"-Dnetlogo.extensions.dir={self.home / 'extensions'}",
            "--add-exports=java.base/java.lang=ALL-UNNAMED",
            "--add-exports=java.desktop/sun.awt=ALL-UNNAMED",
            "--add-exports=java.desktop/sun.java2d=ALL-UNNAMED",
            "-cp",
            f"{self.jar()}:{self.classes}",
            "NetLogoBench",
        ]


class Mason(Engine):
    def __init__(self) -> None:
        super().__init__(name="mason")
        self.jar = Path(os.environ.get("MASON_JAR", BENCHMARKS / "mason" / "mason.22.jar"))
        self.classes = REPO_ROOT / "target" / "bench" / "mason"

    def detect(self) -> str | None:
        if not (BENCHMARKS / "mason" / "Bench.java").exists():
            return "benchmarks/mason is missing"
        if not self.jar.exists():
            return f"{self.jar} missing (run benchmarks/mason/fetch_mason.sh)"
        if shutil.which("java") is None:
            return "java not on PATH"
        return None

    def prepare(self) -> None:
        self.classes.mkdir(parents=True, exist_ok=True)
        sources = sorted(str(p) for p in (BENCHMARKS / "mason").glob("*.java"))
        self.build_with(["javac", "-nowarn", "-d", str(self.classes), "-cp", str(self.jar), *sources], "mason")
        self.version = "MASON 22"

    def launcher(self, variant: str) -> list[str]:
        return ["java", "-cp", f"{self.jar}:{self.classes}", "Bench"]


class AgentsJl(Engine):
    def __init__(self) -> None:
        super().__init__(name="agents_jl")
        self.project = BENCHMARKS / "agents_jl"
        self.julia = ""

    def find_julia(self) -> str | None:
        """`$JULIA`, then the path, then juliaup's shim directory.

        juliaup puts its shims on the interactive path only, so a sweep started from a script finds
        nothing and would skip the engine without saying anything useful.
        """
        if explicit := os.environ.get("JULIA"):
            return explicit if Path(explicit).exists() else None
        if found := shutil.which("julia"):
            return found
        shim = Path.home() / ".juliaup" / "bin" / "julia"
        return str(shim) if shim.exists() else None

    def detect(self) -> str | None:
        if not (self.project / "bench.jl").exists():
            return "benchmarks/agents_jl is missing"
        julia = self.find_julia()
        if julia is None:
            return "julia not found (set $JULIA, or put juliaup's bin on PATH)"
        self.julia = julia
        return None

    def prepare(self) -> None:
        self.build_with([self.julia, f"--project={self.project}", "-e", "using Pkg; Pkg.instantiate()"], "agents_jl")

    def launcher(self, variant: str) -> list[str]:
        return [self.julia, f"--project={self.project}", "--threads", "1", str(self.project / "bench.jl")]


class Krabmaga(Engine):
    def __init__(self) -> None:
        # `parallel` is gated and built, never timed. Its scheduler holds one state lock across
        # each agent's whole step, so the workers serialise, and the feature swaps the flat field
        # vectors for sharded hash maps. A ladder of it measures krABMaga's serial path carrying
        # that overhead. `benchmarks/README.md` records the penalty and where it comes from.
        super().__init__(name="krabmaga", variants=["default", "parallel"], untimed=("parallel",))
        self.crate = BENCHMARKS / "krabmaga"

    def detect(self) -> str | None:
        if not (self.crate / "Cargo.toml").exists():
            return "benchmarks/krabmaga is missing"
        if shutil.which("cargo") is None:
            return "cargo not on PATH"
        return None

    def threads_for(self, variant: str) -> int:
        return 1 if variant == "default" else 0

    def prepare(self) -> None:
        for variant in self.variants:
            args = [
                "cargo",
                "build",
                "--release",
                "--manifest-path",
                str(self.crate / "Cargo.toml"),
                "--target-dir",
                str(REPO_ROOT / "target" / "bench" / f"krabmaga-{variant}"),
            ]
            if variant != "default":
                args += ["--features", variant]
            self.build_with(args, f"krabmaga ({variant})")
        self.version = "krABMaga 0.6.2"

    def binary_for(self, variant: str) -> Path:
        return REPO_ROOT / "target" / "bench" / f"krabmaga-{variant}" / "release" / "krabmaga-bench"

    def launcher(self, variant: str) -> list[str]:
        return [str(self.binary_for(variant))]


# Passed through `--grid`, `--agents` and `--world` rather than `--set`.
SIZE_KEYS = {"grid_width", "grid_height", "num_agents", "world_width", "world_height"}


def common_harness_args(point: Point, reps: int, seed: int, threads: int, resolved: dict[str, str]) -> list[str]:
    """The contract's arguments, including every parameter Henad resolved for this point.

    Sending them is what makes the docstring true: the reference engines are held to Henad's own
    descriptors. Before this only size keys were ever sent, so each port ran its own hard-coded
    defaults while the CSV recorded Henad's.
    """
    args = [
        "--model",
        point.model,
        *point.harness_args(),
        "--steps",
        str(point.steps),
        "--warmup",
        str(point.warmup),
        "--reps",
        str(reps),
        "--seed",
        str(seed),
        "--threads",
        str(threads),
    ]
    for key, value in sorted(resolved.items()):
        if key in SIZE_KEYS:
            continue
        args += ["--set", f"{key}={value}"]
    return args


def all_engines(binary: Path) -> list[Engine]:
    return [Henad(binary), Mesa(), NetLogo(), Mason(), AgentsJl(), Krabmaga()]


# --- running ---------------------------------------------------------------------------------


def describe_commit() -> str:
    """The commit a row was measured at, marked when the tree it was built from was not clean.

    The published sweep stamps a commit whose henad-cli accepts neither flag the driver passes, so
    nothing identifies the code that actually ran.
    """
    try:
        out = subprocess.run(
            ["git", "-C", str(REPO_ROOT), "rev-parse", "--short", "HEAD"],
            capture_output=True,
            text=True,
            check=False,
            timeout=10,
        )
        commit = out.stdout.strip() or "unknown"
        dirty = subprocess.run(
            ["git", "-C", str(REPO_ROOT), "status", "--porcelain"],
            capture_output=True,
            text=True,
            check=False,
            timeout=30,
        )
        return f"{commit}-dirty" if dirty.stdout.strip() else commit
    except (OSError, subprocess.SubprocessError):
        return "unknown"


@dataclass
class Invocation:
    """What one harness process reported."""

    code: int
    reps: list[dict]
    info: dict
    stderr: str
    wall: float
    status: str


def as_text(stream) -> str:
    """Whatever a stream gives back, as text.

    `TimeoutExpired` carries raw bytes even from a `text=True` child, since the decode only happens
    on the path where the process finished.
    """
    if stream is None:
        return ""
    return stream.decode(errors="replace") if isinstance(stream, bytes) else stream


def kill_tree(proc: subprocess.Popen) -> None:
    """Kill the child and anything it started.

    Mesa runs behind `uv run`, so signalling the direct child alone leaves a Python process holding
    a core against every later point of the sweep.
    """
    try:
        os.killpg(os.getpgid(proc.pid), signal.SIGKILL)
    except (ProcessLookupError, PermissionError):
        proc.kill()


def invoke(args: list[str], timeout: float) -> Invocation:
    """Run one harness process. A timeout keeps whatever reps it streamed before it was killed."""
    started = time.perf_counter()
    try:
        # `start_new_session` puts the harness in its own process group, which is what makes
        # `kill_tree` able to reach a wrapper's children. `subprocess.run`'s own timeout would
        # signal the wrapper only.
        proc = subprocess.Popen(
            args,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            start_new_session=True,
        )
    except FileNotFoundError as missing:
        return Invocation(-1, [], {}, str(missing), time.perf_counter() - started, "error")

    try:
        raw_out, raw_err = proc.communicate(timeout=max(timeout, 1.0))
        code = proc.returncode
        status = "ok" if code == 0 else "error"
    except subprocess.TimeoutExpired:
        kill_tree(proc)
        raw_out, raw_err = proc.communicate()
        code = -1
        status = "over_budget"
    except KeyboardInterrupt:
        # The harness is in its own group, so the terminal's interrupt never reached it.
        kill_tree(proc)
        raise
    stdout, stderr = as_text(raw_out), as_text(raw_err)
    wall = time.perf_counter() - started

    reps: list[dict] = []
    info: dict = {}
    for line in stdout.splitlines():
        line = line.strip()
        if not line.startswith("{"):
            continue
        try:
            obj = json.loads(line)
        except json.JSONDecodeError:
            continue
        kind = obj.get("kind")
        if kind == "rep":
            reps.append(obj)
        elif kind == "info":
            info = obj
    if status == "ok" and not reps:
        status = "error"
    if status == "over_budget" and not reps:
        status = "timeout"
    # Only a run that already failed. A marker in a healthy run's stderr was relabelling a good
    # measurement as an allocation failure.
    if status != "ok" and is_oom(stderr, code):
        status = "oom"
    return Invocation(code, reps, info, stderr, wall, status)


def run_point(
    engine: Engine,
    variant: str,
    point: Point,
    reps: int,
    threads: int,
    budget: float,
    resolved: dict[str, str],
    note=None,
) -> Invocation:
    """Collect up to `reps` timed reps for one point, inside one wall-clock budget.

    The budget is per point rather than per process, so an engine whose single rep is slower than
    the whole allowance is reported as over budget rather than run five times. Whatever reps did
    finish are kept: a median over two reps still says where an engine stands, as long as the row
    says it was two.
    """
    if not engine.one_rep_per_process:
        if note:
            note(f"{reps} reps")
        return invoke(engine.command(variant, point, reps, BASE_SEED, threads, resolved), budget)

    collected: list[dict] = []
    info_line: dict = {}
    spent = 0.0
    status = "ok"
    stderr = ""
    code = 0
    for rep in range(reps):
        remaining = budget - spent
        # A rep that cannot finish inside what is left is a rep whose time is thrown away, so stop
        # rather than spend the remainder producing nothing.
        if remaining <= 0 or (collected and remaining < spent / len(collected)):
            status = "over_budget"
            break
        if note:
            note(f"rep {rep + 1}/{reps} · {remaining:.0f}s left")
        single = invoke(engine.command(variant, point, 1, BASE_SEED + rep, threads, resolved), remaining)
        spent += single.wall
        info_line = info_line or single.info
        collected.extend(single.reps)
        if single.status != "ok":
            status, stderr, code = single.status, single.stderr, single.code
            break
        if spent >= budget:
            # The next rep would run past the allowance, and a partial rep measures nothing.
            status = "over_budget" if rep + 1 < reps else "ok"
            break
    if status in {"timeout", "over_budget"}:
        # Nothing at all is a timeout; some reps but not all is the budget running out.
        status = "over_budget" if collected else "timeout"
    return Invocation(code, collected, info_line, stderr, spent, status)


def is_oom(stderr: str, code: int) -> bool:
    markers = ("MemoryError", "OutOfMemoryError", "memory allocation of", "Cannot allocate memory", "std::bad_alloc")
    # 137 comes through a shell, -9 from a direct exec. Both are the kernel reaping the process.
    return code in (137, -9) or any(m in stderr for m in markers)


def summarise(times: list[float], steps: int, population: int) -> dict:
    if not times:
        return {}
    mean = statistics.fmean(times)
    median = statistics.median(times)
    return {
        "min_s": min(times),
        "median_s": median,
        "max_s": max(times),
        "mean_s": mean,
        # Throughput from the median, which is what the ratio tables rank on. Derived from the mean
        # these disagreed with the table beside them by up to 8%.
        "std_dev_s": statistics.pstdev(times) if len(times) > 1 else 0.0,
        "steps_per_sec": steps / median if median > 0 else "",
        "updates_per_sec": steps * population / median if median > 0 else "",
    }


# --- output ----------------------------------------------------------------------------------

CSV_FIELDS = [
    "engine",
    "engine_version",
    "variant",
    "threads",
    "model",
    "axis",
    "scale",
    "grid_w",
    "grid_h",
    "num_agents",
    "world_w",
    "world_h",
    "params_json",
    "steps",
    "warmup",
    "reps",
    "reps_done",
    "seed",
    "status",
    "validated",
    "min_s",
    "median_s",
    "max_s",
    "mean_s",
    "std_dev_s",
    "steps_per_sec",
    "updates_per_sec",
    "population",
    "heap_bytes",
    "rep_times_s",
    "wall_s",
    "host",
    "henad_commit",
    "error",
]


def summarise_run(result: Invocation, reps: int) -> str:
    """The one useful number a finished run leaves behind."""
    if result.status != "ok":
        return describe_failure(result, reps)
    times = [r["elapsed_s"] for r in result.reps if "elapsed_s" in r]
    if not times:
        return ""
    median = statistics.median(times)
    unit = f"{median * 1000:.1f} ms" if median < 1 else f"{median:.2f} s"
    return f"{len(times)} reps · {unit} median"


def describe_failure(result: Invocation, reps: int) -> str:
    """What the status column cannot say on its own."""
    if result.status == "ok":
        return ""
    if result.status in {"over_budget", "timeout"}:
        return f"{len(result.reps)} of {reps} reps in {result.wall:.0f}s"
    return extract_error(result.stderr, result.code)


def row_for(
    engine: Engine,
    variant: str,
    point: Point,
    info: ModelInfo,
    reps: int,
    threads: int,
    result: Invocation | None,
    validated: str,
    host: str,
    commit: str,
    skipped_because: str = "",
) -> dict:
    row = {name: "" for name in CSV_FIELDS}
    row.update(
        {
            "engine": engine.name,
            "engine_version": (result.info.get("engine_version", engine.version) if result else engine.version),
            "variant": variant,
            # What the harness says it used, not what was requested. An engine whose workers
            # serialise reports 1, and recording the request would publish it as "all".
            "threads": (result.info.get("threads", threads) if result and result.info else threads),
            "model": point.model,
            "axis": "grid" if point.grid else "agents",
            "scale": point.scale,
            "grid_w": point.grid[0] if point.grid else "",
            "grid_h": point.grid[1] if point.grid else "",
            "num_agents": point.agents if point.agents else "",
            "world_w": f"{point.world[0]:.6g}" if point.world else "",
            "world_h": f"{point.world[1]:.6g}" if point.world else "",
            "params_json": json.dumps(info.resolved(point.overrides()), sort_keys=True),
            "steps": point.steps,
            "warmup": point.warmup,
            "reps": reps,
            "seed": BASE_SEED,
            "validated": validated,
            "host": host,
            "henad_commit": commit,
        }
    )
    if result is None:
        row["status"] = "skipped"
        row["error"] = skipped_because
        return row

    times = [r["elapsed_s"] for r in result.reps if "elapsed_s" in r]
    population = int(result.reps[0].get("population") or 0) if result.reps else 0
    heap = result.reps[0].get("heap_bytes") if result.reps else None
    row.update(
        {
            "status": result.status,
            "reps_done": len(times),
            "population": population,
            "heap_bytes": heap if heap is not None else "",
            "rep_times_s": json.dumps([round(t, 9) for t in times]),
            "wall_s": round(result.wall, 3),
            "error": describe_failure(result, reps),
        }
    )
    row.update(summarise(times, point.steps, population))
    return row


# --- driver ----------------------------------------------------------------------------------


# A point that ended any of these ways stops its ladder being climbed further.
STOPPED = {"over_budget", "timeout", "oom", "error"}


def load_done(path: Path) -> tuple[set[tuple], set[tuple[str, str, str]]]:
    """What an earlier run of this sweep already settled.

    Returns the points to skip and the engine, variant and model triples whose ladder had already
    stopped. Rebuilding the second is what makes a resumed sweep behave like an uninterrupted one:
    without it, a resume that lands just after a point went over budget would climb into the larger
    rungs it was meant to give up on, at a thousand seconds each.
    """
    if not path.exists():
        return set(), set()
    done: set[tuple] = set()
    stalled: set[tuple[str, str, str]] = set()
    with path.open(newline="") as handle:
        for row in csv.DictReader(handle):
            # A row cut short by a hard kill is missing its trailing fields, which `DictReader`
            # fills with None. Leave it out of `done` so its point runs again and lands complete.
            if None in row.values() or not row.get("status"):
                continue
            # A crash or a skip is not a settled point. Left in `done` they could never be retried
            # without a full re-sweep.
            if row["status"] in {"error", "skipped"}:
                continue
            done.add(
                (row["engine"], row["variant"], row["model"], row["scale"], row["steps"], row["warmup"])
            )
            if row["status"] in STOPPED:
                stalled.add((row["engine"], row["variant"], row["model"]))
    return done, stalled


VALIDATED = REPO_ROOT / "results" / "compare" / "validated.json"


def load_validated(path: Path) -> dict[tuple[str, str], dict[str, str]]:
    """The verdicts `validate_ports.py` left, keyed by engine, variant and model.

    A port that has not passed its gate is not timed. Six documents said so before anything read
    this file.
    """
    if not path.exists():
        return {}
    try:
        results = json.loads(path.read_text()).get("results", {})
    except (OSError, json.JSONDecodeError):
        return {}
    verdicts: dict[tuple[str, str], dict[str, str]] = {}
    for key, models in results.items():
        engine, _, variant = key.partition("/")
        verdicts[(engine, variant)] = {m: v[0] if isinstance(v, list) else str(v) for m, v in models.items()}
    return verdicts


def gate_verdict(
    verdicts: dict[tuple[str, str], dict[str, str]], engine: Engine, variant: str, model: str
) -> str:
    """This run's gate verdict, `reference` for Henad and `ungated` when nothing recorded one."""
    if isinstance(engine, Henad):
        return "reference"
    # A non-default variant is recorded under `engine/variant`, the default under the bare name.
    key = (engine.name, "" if variant == engine.variants[0] else variant)
    return verdicts.get(key, {}).get(model, "ungated")


def newest_sweep(directory: Path, host: str) -> Path | None:
    """The sweep a bare `--resume` should continue.

    The default output name carries the date, so an overnight sweep resumed the next morning would
    otherwise start a second file and silently redo everything. Restricted to this host, since two
    machines' numbers never belong in one file.
    """
    candidates = sorted(directory.glob(f"{host}_*.csv"), key=lambda p: p.stat().st_mtime)
    return candidates[-1] if candidates else None


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--binary", type=Path, default=DEFAULT_BINARY)
    parser.add_argument("--out", type=Path, help="CSV to write (default results/compare/<host>_<date>.csv)")
    parser.add_argument("--engines", help="comma separated engine names")
    parser.add_argument("--models", help="comma separated model ids")
    parser.add_argument("--reps", type=int, default=5)
    parser.add_argument(
        "--budget", type=float, default=1000.0, help="wall-clock seconds allowed per point, across its reps"
    )
    parser.add_argument("--timeout", type=float, default=120.0, help="seconds allowed for a --params probe")
    parser.add_argument("--dry-run", action="store_true", help="print the matrix and the engines found, run nothing")
    parser.add_argument("--smoke", action="store_true", help="one small point per engine and model")
    parser.add_argument("--resume", action="store_true", help="skip points already in the output CSV")
    parser.add_argument("--plain", action="store_true", help="one line per run, never a live display")
    parser.add_argument("--force", action="store_true", help="replace an existing sweep instead of resuming it")
    parser.add_argument("--allow-unvalidated", action="store_true", help="time ports that fail their gate")
    args = parser.parse_args()

    host = platform.node().split(".")[0] or "unknown"
    commit = describe_commit()
    sweeps = REPO_ROOT / "results" / "compare"
    out = args.out
    if out is None and args.resume and (previous := newest_sweep(sweeps, host)):
        out = previous
        print(f"resuming {out}", file=sys.stderr)
    if out is None:
        # A smoke run writes elsewhere. Sharing the name let its short rows stand in for the real
        # ladder's smallest rung on the next resume.
        suffix = "_smoke" if args.smoke else ""
        out = sweeps / f"{host}_{time.strftime('%Y%m%d')}{suffix}.csv"
    if out.exists() and not (args.resume or args.force or args.dry_run):
        raise SystemExit(f"{out} already holds a sweep; pass --resume to continue it or --force to replace it")

    models = [m.strip() for m in args.models.split(",")] if args.models else list(MODELS)
    unknown = [m for m in models if m not in LADDER]
    if unknown:
        raise SystemExit(f"unknown model(s): {', '.join(unknown)}")

    engines = all_engines(args.binary)
    if args.engines:
        wanted = {e.strip() for e in args.engines.split(",")}
        engines = [e for e in engines if e.name in wanted]
        missing = wanted - {e.name for e in engines}
        if missing:
            raise SystemExit(f"unknown engine(s): {', '.join(sorted(missing))}")

    henad = next((e for e in engines if isinstance(e, Henad)), Henad(args.binary))
    if reason := henad.detect():
        raise SystemExit(f"henad-cli is needed for the model descriptors: {reason}")
    infos = {m: discover_params(henad.binary, m, args.timeout) for m in models}

    available: list[Engine] = []
    for engine in engines:
        reason = engine.detect()
        if reason:
            print(f"skipping {engine.name}: {reason}", file=sys.stderr)
            continue
        # Not before the dry run, which promises to run nothing and was compiling four toolchains.
        if not args.dry_run:
            engine.prepare()
        available.append(engine)
    if not available:
        raise SystemExit("no engines available")

    points = build_points(infos, models)
    if args.smoke:
        points = [p for p in points if p.scale == LADDER[p.model]["points"][0]]
        points = [Point(p.model, p.scale, p.grid, p.agents, p.world, 10, 1) for p in points]

    runs = [(engine, variant, point) for engine in available for variant in engine.timed for point in points]

    if args.dry_run:
        print(f"engines: {', '.join(e.name for e in available)}")
        print(f"models:  {', '.join(models)}")
        print(f"{len(runs)} runs at up to {args.reps} reps, {args.budget:.0f}s per point -> {out}")
        for engine, variant, point in runs:
            print(f"  {engine.name}/{variant:<8} {point.model:<13} {point.label()}")
        return 0

    verdicts = load_validated(VALIDATED)
    if not verdicts and not args.allow_unvalidated:
        print(f"no gate verdicts at {VALIDATED}; run scripts/validate_ports.py first", file=sys.stderr)

    done, stalled = load_done(out) if args.resume else (set(), set())
    out.parent.mkdir(parents=True, exist_ok=True)
    fresh = not out.exists() or not args.resume
    # Steps and warm-up are part of the key, so a `--smoke` row at 10 steps cannot satisfy the real
    # ladder's rung at 100.
    pending = [
        r
        for r in runs
        if (r[0].name, r[1], r[2].model, str(r[2].scale), str(r[2].steps), str(r[2].warmup)) not in done
    ]
    subtitle = f"up to {args.reps} reps · {args.budget:.0f}s per point"
    if done:
        subtitle += f" · {len(done)} already done"

    if not fresh and out.exists() and (raw := out.read_bytes()) and not raw.endswith(b"\n"):
        # A hard kill can leave the last row without its newline, and appending would splice the
        # next row onto it.
        with out.open("ab") as handle:
            handle.write(b"\n")

    with out.open("w" if fresh else "a", newline="") as handle:
        writer = csv.DictWriter(handle, fieldnames=CSV_FIELDS)
        if fresh:
            writer.writeheader()
        with Progress(len(pending), subtitle, live=False if args.plain else None) as progress:
            for engine, variant, point in pending:
                threads = engine.threads_for(variant)
                label = f"{engine.name}/{variant}  {point.model}  {point.label()}"

                verdict = gate_verdict(verdicts, engine, variant, point.model)
                if verdict not in {"yes", "reference"} and not args.allow_unvalidated:
                    reason = f"gate verdict is `{verdict}`"
                    writer.writerow(
                        row_for(
                            engine,
                            variant,
                            point,
                            infos[point.model],
                            args.reps,
                            threads,
                            None,
                            verdict,
                            host,
                            commit,
                            reason,
                        )
                    )
                    handle.flush()
                    progress.finish(label, "skipped", reason)
                    continue

                if (engine.name, variant, point.model) in stalled:
                    reason = "a smaller point did not finish"
                    writer.writerow(
                        row_for(
                            engine,
                            variant,
                            point,
                            infos[point.model],
                            args.reps,
                            threads,
                            None,
                            verdict,
                            host,
                            commit,
                            reason,
                        )
                    )
                    handle.flush()
                    progress.finish(label, "skipped", reason)
                    continue

                progress.start(label)
                resolved = infos[point.model].resolved(point.overrides())
                merged = run_point(
                    engine, variant, point, args.reps, threads, args.budget, resolved, progress.note
                )

                if merged.status != "ok":
                    stalled.add((engine.name, variant, point.model))
                writer.writerow(
                    row_for(
                        engine, variant, point, infos[point.model], args.reps, threads, merged, verdict, host, commit
                    )
                )
                handle.flush()
                progress.finish(label, merged.status, summarise_run(merged, args.reps))

    print(f"wrote {out}", file=sys.stderr)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
