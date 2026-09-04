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
import json
import subprocess
import sys
import time
from dataclasses import dataclass
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

from compare_bench import Henad, all_engines  # noqa: E402

REPO_ROOT = Path(__file__).resolve().parent.parent
FIXTURES = REPO_ROOT / "crates" / "henad-models" / "tests" / "fixtures"


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


def run(args: list[str], timeout: float) -> tuple[int, str, str]:
    try:
        proc = subprocess.run(args, capture_output=True, text=True, check=False, timeout=timeout)
        return proc.returncode, proc.stdout, proc.stderr
    except subprocess.TimeoutExpired:
        return -1, "", f"timed out after {timeout:.0f}s"
    except FileNotFoundError as missing:
        return -1, "", str(missing)


def cargo_test(test: str, timeout: float) -> tuple[bool, str]:
    code, stdout, stderr = run(
        ["cargo", "test", "-p", "henad-models", "--test", test, "--", "matches_every_reference_fixture"],
        timeout,
    )
    return code == 0, (stdout + stderr)


def validate_fixture_model(engine, gate: Gate, timeout: float) -> tuple[str, str]:
    """Produce this engine's fixtures for one model, then let the consistency test judge them."""
    gate.directory.mkdir(parents=True, exist_ok=True)
    written: list[Path] = []
    for scenario in gate.scenarios:
        target = gate.target(scenario, engine.name)
        cmd = engine.validate_command(scenario, target)
        assert cmd is not None
        code, _, stderr = run(cmd, timeout)
        if code != 0 or not target.exists():
            for path in written:
                path.unlink(missing_ok=True)
            return "no", f"{scenario}: {stderr.strip().splitlines()[-1] if stderr.strip() else 'no output'}"
        written.append(target)

    passed, output = cargo_test(gate.test, timeout)
    if passed:
        return "yes", ""
    for path in written:
        path.unlink(missing_ok=True)
    detail = next((l.strip() for l in output.splitlines() if "disagree" in l or "assertion" in l), "see cargo output")
    return "no", detail


def validate_sir(engine, timeout: float) -> tuple[str, str]:
    """Fifty replicates each side, then the equivalence test in `compare_sir.py`."""
    out_dir = SIR_DIR / engine.name
    out_dir.mkdir(parents=True, exist_ok=True)
    for seed in range(1, SIR_REPLICATES + 1):
        target = out_dir / f"sir_{engine.name}_{seed:02d}.csv"
        if target.exists():
            continue
        cmd = engine.validate_command("sir-replicates", target, seed=seed)
        assert cmd is not None
        code, _, stderr = run(cmd, timeout)
        if code != 0 or not target.exists():
            return "no", f"seed {seed}: {stderr.strip().splitlines()[-1] if stderr.strip() else 'no output'}"

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
        ],
        timeout,
    )
    if code == 0:
        return "yes", ""
    verdict = next((l.strip() for l in stdout.splitlines() if "DIFFERENT" in l or "INCONCLUSIVE" in l), "")
    return "no", verdict or (stderr.strip().splitlines()[-1] if stderr.strip() else "compare_sir.py failed")


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--binary", type=Path, default=REPO_ROOT / "target" / "release" / "henad-cli")
    parser.add_argument("--engines", help="comma separated engine names")
    parser.add_argument("--models", help="comma separated model ids")
    parser.add_argument("--timeout", type=float, default=1800.0)
    parser.add_argument("--out", type=Path, default=REPO_ROOT / "results" / "compare" / "validated.json")
    args = parser.parse_args()

    models = [m.strip() for m in args.models.split(",")] if args.models else ["game_of_life", "boids", "sir", "ants"]

    # Henad is the reference every port is measured against, never a port itself.
    engines = [e for e in all_engines(args.binary) if not isinstance(e, Henad)]
    if args.engines:
        wanted = {e.strip() for e in args.engines.split(",")}
        engines = [e for e in engines if e.name in wanted]

    results: dict[str, dict[str, list[str]]] = {}
    ran = 0
    for engine in engines:
        reason = engine.detect()
        if reason:
            print(f"skipping {engine.name}: {reason}", file=sys.stderr)
            continue
        if engine.validate_command("probe", Path("/dev/null")) is None:
            print(f"skipping {engine.name}: harness has no validate mode", file=sys.stderr)
            continue
        engine.prepare()
        results[engine.name] = {}
        for model in models:
            print(f"  gate  {engine.name} {model}", file=sys.stderr)
            if model == "sir":
                verdict, detail = validate_sir(engine, args.timeout)
            else:
                verdict, detail = validate_fixture_model(engine, GATES[model], args.timeout)
            results[engine.name][model] = [verdict, detail]
            ran += 1
            print(f"        {verdict}{': ' + detail if detail else ''}", file=sys.stderr)

    if not ran:
        print("no port has a validate mode yet, so nothing was gated", file=sys.stderr)
        return 0

    args.out.parent.mkdir(parents=True, exist_ok=True)
    args.out.write_text(json.dumps({"generated": time.strftime("%Y-%m-%d %H:%M"), "results": results}, indent=2))
    print(f"wrote {args.out}", file=sys.stderr)
    failed = [(e, m) for e, ms in results.items() for m, (v, _) in ms.items() if v != "yes"]
    for engine_name, model in failed:
        print(f"FAILED {engine_name} {model}", file=sys.stderr)
    return 1 if failed else 0


if __name__ == "__main__":
    raise SystemExit(main())
