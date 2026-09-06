#!/usr/bin/env python3
"""Check every reference implementation against Henad before anything is timed.

Each engine's harness runs in `--validate` mode, writes a fixture in the format its model's
declaration gives, and the existing consistency tests do the comparison. Reusing `cargo test` here
is deliberate: the gate a port has to pass is the same gate Henad holds itself to, and there is one
implementation of it rather than a second one in Python.

The declarations are the documents under `crates/henad-models/tests/fixtures/docs/`.

    uv run --project scripts scripts/validate_ports.py
    uv run --project scripts scripts/validate_ports.py --engines mesa --models boids
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import shutil
import subprocess
import sys
import tempfile
import time
from dataclasses import dataclass
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

from compare_bench import Henad, all_engines  # noqa: E402

REPO_ROOT = Path(__file__).resolve().parent.parent
FIXTURES = REPO_ROOT / "crates" / "henad-models" / "tests" / "fixtures"
BENCHMARKS = REPO_ROOT / "benchmarks"


@dataclass(frozen=True)
class Gate:
    """One model's gate: the scenarios to run, where their fixtures live, and what compares them."""

    model: str
    scenarios: tuple[str, ...]
    directory: Path
    filename: str
    test: str

    def target(self, scenario: str, engine: str) -> Path:
        return self.directory / self.filename.format(scenario=scenario.replace("-", "_"), engine=engine)


GATES = {
    "game_of_life": Gate(
        "game_of_life",
        ("glider", "r-pentomino"),
        FIXTURES / "game_of_life",
        "gol_{scenario}_64x64_{engine}.txt",
        "consistency_game_of_life",
    ),
    "boids": Gate("boids", ("boids-8", "sine-42"), FIXTURES / "boids", "{scenario}_{engine}.txt", "consistency_boids"),
    "ants": Gate("ants", ("ants-lattice",), FIXTURES / "ants", "{scenario}_{engine}.txt", "consistency_ants"),
}

# SIR is stochastic and compares distributions, so it does not go through a fixture. See
# `sir_fixture.md` for where the margins come from.
SIR_REPLICATES = 50
SIR_DIR = REPO_ROOT / "results" / "compare" / "sir"


def run(args: list[str], timeout: float, env: dict[str, str] | None = None) -> tuple[int, str, str]:
    try:
        proc = subprocess.run(args, capture_output=True, text=True, check=False, timeout=timeout, env=env)
        return proc.returncode, proc.stdout, proc.stderr
    except subprocess.TimeoutExpired:
        return -1, "", f"timed out after {timeout:.0f}s"
    except FileNotFoundError as missing:
        return -1, "", str(missing)


def cargo_test(test: str, timeout: float, fixture_root: Path) -> tuple[bool, str]:
    """The consistency test, pointed at one directory of candidates through `HENAD_FIXTURE_DIR`."""
    env = dict(os.environ, HENAD_FIXTURE_DIR=str(fixture_root))
    code, stdout, stderr = run(
        ["cargo", "test", "-p", "henad-models", "--test", test, "--", "matches_every_reference_fixture"],
        timeout,
        env,
    )
    return code == 0, (stdout + stderr)


def validate_fixture_model(engine, variant: str, gate: Gate, timeout: float, commit: bool) -> tuple[str, str]:
    """Produce this engine's fixtures for one model, judge them, and only then commit them.

    The candidates are compared alone in a temporary tree. Writing them into the tracked directory
    first meant a failure named whichever engine happened to be under test and took the correct
    engines' fixtures down with it.
    """
    with tempfile.TemporaryDirectory(prefix="henad-gate-") as tmp:
        staging = Path(tmp) / gate.directory.name
        staging.mkdir(parents=True)
        written: list[tuple[Path, Path]] = []
        for scenario in gate.scenarios:
            candidate = staging / gate.target(scenario, engine.name).name
            cmd = engine.validate_command(scenario, candidate, variant=variant)
            assert cmd is not None
            code, _, stderr = run(cmd, timeout)
            if code != 0 or not candidate.exists():
                last = stderr.strip().splitlines()[-1] if stderr.strip() else "no output"
                return "no", f"{scenario}: {last}"
            written.append((candidate, gate.target(scenario, engine.name)))

        passed, output = cargo_test(gate.test, timeout, Path(tmp))
        if not passed:
            # The line after `panicked at ...` is the assertion message, which names the agent or
            # cell that disagreed. Everything else cargo prints is noise to whoever reads this.
            lines = [l.strip() for l in output.splitlines()]
            panic = next((i for i, l in enumerate(lines) if "panicked at" in l), None)
            detail = lines[panic + 1] if panic is not None and panic + 1 < len(lines) else ""
            return "no", detail or "see cargo output"

        if commit:
            gate.directory.mkdir(parents=True, exist_ok=True)
            for candidate, target in written:
                shutil.copyfile(candidate, target)
    return "yes", ""


def sir_dir(engine, variant: str) -> Path:
    """Where one engine and variant's replicates live.

    Keyed on the engine alone, a second variant found the first's fifty CSVs already present and
    was recorded as passing a gate its harness never ran.
    """
    name = engine.name if variant == engine.variants[0] else f"{engine.name}-{variant}"
    return SIR_DIR / name


def harness_digest(engine) -> str:
    """A fingerprint of everything under this engine's directory that could change its answer.

    The replicate cache is keyed on it. Keyed on nothing, an edited port kept its verdict forever,
    and SIR is the one model with no fixture to catch the drift.
    """
    root = BENCHMARKS / engine.name
    digest = hashlib.sha256()
    for path in sorted(p for p in root.rglob("*") if p.is_file()):
        if any(part in {"target", "__pycache__", ".venv"} for part in path.parts):
            continue
        digest.update(path.relative_to(root).as_posix().encode())
        digest.update(path.read_bytes())
    return digest.hexdigest()[:16]


def validate_sir(engine, variant: str, timeout: float, refresh: bool, binary: Path) -> tuple[str, str]:
    """Fifty replicates each side, then the equivalence test in `compare_sir.py`."""
    out_dir = sir_dir(engine, variant)
    out_dir.mkdir(parents=True, exist_ok=True)
    stamp = out_dir / "harness.sha256"
    digest = f"{variant}:{harness_digest(engine)}"
    stale = refresh or not stamp.exists() or stamp.read_text().strip() != digest
    if stale:
        for path in out_dir.glob("*.csv"):
            path.unlink()
    for seed in range(1, SIR_REPLICATES + 1):
        target = out_dir / f"sir_{engine.name}_{seed:02d}.csv"
        if target.exists():
            continue
        cmd = engine.validate_command("sir-replicates", target, seed=seed, variant=variant)
        assert cmd is not None
        code, _, stderr = run(cmd, timeout)
        if code != 0 or not target.exists():
            return "no", f"seed {seed}: {stderr.strip().splitlines()[-1] if stderr.strip() else 'no output'}"
    stamp.write_text(digest + "\n")

    code, stdout, stderr = run(
        [
            "uv",
            "run",
            "--project",
            str(REPO_ROOT / "scripts"),
            str(REPO_ROOT / "scripts" / "compare_sir.py"),
            "--reference",
            str(out_dir),
            "--generate",
            str(SIR_REPLICATES),
            "--binary",
            str(binary),
        ],
        timeout,
    )
    if code == 0:
        return "yes", ""
    verdict = next((l.strip() for l in stdout.splitlines() if "DIFFERENT" in l or "INCONCLUSIVE" in l), "")
    # 2 is undecided rather than wrong, and the answer to it is more replicates.
    outcome = "inconclusive" if code == 2 else "no"
    return outcome, verdict or (stderr.strip().splitlines()[-1] if stderr.strip() else "compare_sir.py failed")


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument(
        "--binary", type=Path, default=REPO_ROOT / "target" / "release" / "henad-cli", help="the Henad side of every gate"
    )
    parser.add_argument("--engines", help="comma separated engine names")
    parser.add_argument("--models", help="comma separated model ids")
    parser.add_argument("--timeout", type=float, default=1800.0, help="seconds allowed per subprocess")
    parser.add_argument("--refresh-sir", action="store_true", help="discard cached SIR replicates and redo them")
    parser.add_argument("--out", type=Path, default=REPO_ROOT / "results" / "compare" / "validated.json")
    args = parser.parse_args()

    if not args.binary.exists():
        raise SystemExit(f"{args.binary} not found; run `cargo build --release -p henad-cli` first")

    known = ["game_of_life", "boids", "sir", "ants"]
    models = [m.strip() for m in args.models.split(",")] if args.models else known
    if unknown := [m for m in models if m not in known]:
        raise SystemExit(f"unknown model(s): {', '.join(unknown)}")

    # Henad is the reference every port is measured against, never a port itself.
    engines = [e for e in all_engines(args.binary) if not isinstance(e, Henad)]
    if args.engines:
        wanted = {e.strip() for e in args.engines.split(",")}
        engines = [e for e in engines if e.name in wanted]
        if missing := wanted - {e.name for e in engines}:
            raise SystemExit(f"unknown engine(s): {', '.join(sorted(missing))}")

    results: dict[str, dict[str, list[str]]] = {}
    ran = 0
    for engine in engines:
        reason = engine.detect()
        if reason:
            print(f"skipping {engine.name}: {reason}", file=sys.stderr)
            continue
        engine.prepare()
        # Every variant is gated, timed or not. krABMaga's `parallel` build swaps its field storage
        # outright, and a release that changes the swap shows up here.
        for variant in engine.variants:
            key = engine.name if variant == engine.variants[0] else f"{engine.name}/{variant}"
            results[key] = {}
            for model in models:
                print(f"  gate  {key} {model}", file=sys.stderr)
                if model == "sir":
                    verdict, detail = validate_sir(engine, variant, args.timeout, args.refresh_sir, args.binary)
                else:
                    verdict, detail = validate_fixture_model(
                        engine, variant, GATES[model], args.timeout, commit=variant == engine.variants[0]
                    )
                results[key][model] = [verdict, detail]
                ran += 1
                print(f"        {verdict}{': ' + detail if detail else ''}", file=sys.stderr)

    if not ran:
        raise SystemExit("no engine was available, so nothing was gated")

    # Merged, not replaced: gates are run a model or an engine at a time, and a slow one run on its
    # own should not erase the verdicts around it.
    args.out.parent.mkdir(parents=True, exist_ok=True)
    merged: dict[str, dict[str, list[str]]] = {}
    if args.out.exists():
        try:
            merged = json.loads(args.out.read_text()).get("results", {})
        except (OSError, json.JSONDecodeError):
            merged = {}
    for engine_name, models_run in results.items():
        merged.setdefault(engine_name, {}).update(models_run)
    args.out.write_text(json.dumps({"generated": time.strftime("%Y-%m-%d %H:%M"), "results": merged}, indent=2))
    print(f"wrote {args.out}", file=sys.stderr)
    failed = [(e, m, v) for e, ms in results.items() for m, (v, _) in ms.items() if v != "yes"]
    print(f"{ran - len(failed)}/{ran} gates passed", file=sys.stderr)
    for engine_name, model, verdict in failed:
        print(f"{verdict.upper():<12} {engine_name} {model}", file=sys.stderr)
    return 1 if failed else 0


if __name__ == "__main__":
    raise SystemExit(main())
