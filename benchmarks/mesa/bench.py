#!/usr/bin/env python3
"""Mesa's side of the cross-engine harness contract.

Speaks the interface in `benchmarks/protocol.md`: the same arguments as every other engine, one
JSON object per line out, and a validate mode that writes the fixture its model's declaration
describes.

Only the step loop is timed. Construction, the initial population and warm-up all sit outside the
window, and the garbage collector is off inside it so a collection cycle does not land in one rep
and not another.
"""

from __future__ import annotations

import argparse
import gc
import json
import sys
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

import mesa  # noqa: E402

import scenarios  # noqa: E402
from models.ants import Ants  # noqa: E402
from models.boids import Boids  # noqa: E402
from models.game_of_life import GameOfLife  # noqa: E402
from models.sir import Sir  # noqa: E402

ENGINE = "mesa"


# What each model will answer to. `protocol.md` makes a parameter this engine does not have an
# error, since a silently ignored one means the two runs are no longer the same model.
PARAMS = {
    "game_of_life": ("density",),
    "sir": ("infection_rate", "recovery_rate", "initial_infected_pct"),
    "boids": (
        "visual_range",
        "protected_range",
        "separation",
        "alignment",
        "cohesion",
        "max_speed",
        "min_speed",
    ),
    "ants": ("update_cutdown", "reward", "momentum", "random_action", "evaporation"),
}


def overrides_for(model, pairs):
    known = PARAMS.get(model)
    if known is None:
        raise SystemExit(f"unknown model '{model}'")
    out = {}
    for pair in pairs:
        if "=" not in pair:
            raise SystemExit(f"--set wants id=value, got '{pair}'")
        key, value = pair.split("=", 1)
        if key not in known:
            raise SystemExit(f"{model} has no parameter '{key}'")
        out[key] = value
    return out


def build(args, seed):
    """The model a timed rep runs, at Henad's defaults with the sweep's overrides applied."""
    overrides = overrides_for(args.model, args.set)
    number = lambda key, default: float(overrides.get(key, default))  # noqa: E731

    if args.model == "game_of_life":
        return GameOfLife(args.grid[0], args.grid[1], density=number("density", 0.3), rng=seed)
    if args.model == "sir":
        return Sir(
            args.grid[0],
            args.grid[1],
            infection_rate=number("infection_rate", 0.3),
            recovery_rate=number("recovery_rate", 0.05),
            initial_infected_pct=number("initial_infected_pct", 0.01),
            rng=seed,
        )
    if args.model == "boids":
        return Boids(
            num_agents=args.agents,
            world_width=args.world[0],
            world_height=args.world[1],
            visual_range=number("visual_range", 50.0),
            protected_range=number("protected_range", 8.0),
            separation=number("separation", 0.05),
            alignment=number("alignment", 0.05),
            cohesion=number("cohesion", 0.0005),
            max_speed=number("max_speed", 15.0),
            min_speed=number("min_speed", 3.0),
            rng=seed,
        )
    if args.model == "ants":
        return Ants(
            num_agents=args.agents,
            world_width=args.world[0],
            world_height=args.world[1],
            update_cutdown=number("update_cutdown", 0.9),
            reward=number("reward", 1.0),
            momentum=number("momentum", 0.8),
            random_action=number("random_action", 0.1),
            evaporation=number("evaporation", 0.999),
            rng=seed,
        )
    raise SystemExit(f"unknown model '{args.model}'")


def emit(obj):
    print(json.dumps(obj), flush=True)


def benchmark(args):
    emit(
        {
            "kind": "info",
            "engine": ENGINE,
            "engine_version": mesa.__version__,
            "model": args.model,
            "variant": "default",
            "threads": 1,
        }
    )
    for rep in range(args.reps):
        seed = args.seed + rep
        model = build(args, seed)
        for _ in range(args.warmup):
            model.step()
        population = model.population()

        gc.collect()
        gc.disable()
        try:
            started = time.perf_counter()
            for _ in range(args.steps):
                model.step()
            elapsed = time.perf_counter() - started
        finally:
            gc.enable()

        emit(
            {
                "kind": "rep",
                "rep": rep,
                "seed": seed,
                "steps": args.steps,
                "warmup": args.warmup,
                "elapsed_s": elapsed,
                "population": population,
                "heap_bytes": None,
            }
        )


# --- validate mode ---------------------------------------------------------------------------


def header(scenario, steps, **extra):
    lines = [
        f"# engine: Mesa {mesa.__version__}",
        "# model: Henad rule, ported for this comparison",
        f"# scenario: {scenario}",
        f"# steps: {steps}",
    ]
    lines += [f"# {key}: {value}" for key, value in extra.items()]
    return lines


def validate(args):
    scenario, out = args.validate, args.out
    out.parent.mkdir(parents=True, exist_ok=True)

    if scenario in scenarios.GAME_OF_LIFE:
        steps = scenarios.GAME_OF_LIFE_STEPS[scenario]
        model = GameOfLife(64, 64, live=scenarios.GAME_OF_LIFE[scenario], rng=1)
        for _ in range(steps):
            model.step()
        lines = header(scenario, steps, width=64, height=64) + model.bitmap()

    elif scenario in ("boids-8", "sine-42"):
        agents = scenarios.BOIDS_8 if scenario == "boids-8" else scenarios.SINE_42
        model = Boids(agents=agents, rng=1, **scenarios.BOIDS_PARAMS)
        model.step()
        lines = header(scenario, 1, world=100) + [
            " ".join(f"{value:.9e}" for value in row) for row in model.rows()
        ]

    elif scenario == "ants-lattice":
        steps, width, height = 4, 32, 32
        field = {name: scenarios.ants_field(width, height, name) for name in ("to_food", "to_home")}
        model = Ants(agents=scenarios.ANTS_AGENTS, field=field, rng=1, **scenarios.ANTS_PARAMS)
        for _ in range(steps):
            model.step()
        lines = header(scenario, steps, width=width, height=height, agents=len(scenarios.ANTS_AGENTS))
        lines.append("# --- agents: x y last_step has_food reward")
        for x, y, last_step, has_food, reward in model.rows():
            lines.append(f"{x} {y} {last_step} {has_food} {reward:.9e}")
        for name in ("to_food", "to_home"):
            data = model.trail(name)
            lines.append(f"# --- {name}")
            # A row of the fixture is a row of the lattice, ascending in x then in y.
            lines += [" ".join(f"{data[x, y]:.9e}" for x in range(width)) for y in range(height)]

    elif scenario == "sir-replicates":
        model = Sir(256, 256, infection_rate=0.08, recovery_rate=0.3, initial_infected_pct=0.01, rng=args.seed)
        lines = ["tick,Susceptible,Infected,Recovered"]
        for tick in range(301):
            if tick:
                model.step()
            lines.append(f"{tick}," + ",".join(str(n) for n in model.counts()))

    else:
        raise SystemExit(f"unknown scenario '{scenario}'")

    out.write_text("\n".join(lines) + "\n")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    # Not required with --validate: the scenario names the model.
    parser.add_argument("--model")
    parser.add_argument("--grid", type=int, nargs=2)
    parser.add_argument("--agents", type=int)
    parser.add_argument("--world", type=float, nargs=2)
    parser.add_argument("--steps", type=int, default=100)
    parser.add_argument("--warmup", type=int, default=0)
    parser.add_argument("--reps", type=int, default=1)
    parser.add_argument("--seed", type=int, default=42)
    parser.add_argument("--threads", type=int, default=1)
    parser.add_argument("--set", action="append", default=[], metavar="ID=VALUE")
    parser.add_argument("--validate", metavar="SCENARIO")
    parser.add_argument("--out", type=Path)
    args = parser.parse_args()

    if args.validate:
        if args.out is None:
            raise SystemExit("--validate needs --out")
        validate(args)
        return
    if not args.model:
        raise SystemExit("--model is required unless --validate names a scenario")
    # Before the info line, so a rejected parameter set produces an error and nothing else.
    overrides_for(args.model, args.set)
    if args.threads not in (0, 1):
        print(f"note: Mesa is single threaded; ignoring --threads {args.threads}", file=sys.stderr)
    benchmark(args)


if __name__ == "__main__":
    main()
