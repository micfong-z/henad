#!/usr/bin/env julia
"""Agents.jl's side of the cross-engine harness contract.

Speaks the interface in `benchmarks/protocol.md`: the same arguments as every other engine, one
JSON object per line out, and a validate mode that writes the fixture its model's declaration
describes.

Only the step loop is timed, and the clock is `time_ns`. Construction, the initial population and
warm-up all sit outside the window. Julia compiles on first call, so a full untimed rep runs at the
same configuration before rep 0 and takes the compilation with it.
"""

using Agents
using Printf: @sprintf

include(joinpath(@__DIR__, "scenarios.jl"))
include(joinpath(@__DIR__, "src", "game_of_life.jl"))
include(joinpath(@__DIR__, "src", "sir.jl"))
include(joinpath(@__DIR__, "src", "boids.jl"))
include(joinpath(@__DIR__, "src", "ants.jl"))

const ENGINE = "agents_jl"

# Henad's parameter ids, per model. A `--set` naming anything else is an error rather than a silent
# default, since a mismatched parameter set means the two runs are not the same model.
const PARAMS = Dict(
    "game_of_life" => ("density",),
    "sir" => ("infection_rate", "recovery_rate", "initial_infected_pct"),
    "boids" => (
        "visual_range", "protected_range", "separation", "alignment", "cohesion",
        "max_speed", "min_speed",
    ),
    "ants" => ("update_cutdown", "reward", "momentum", "random_action", "evaporation"),
)

engine_version() = string(pkgversion(Agents))

# --- arguments -------------------------------------------------------------------------------

function parse_args(argv)
    opts = Dict{String, Any}(
        "model" => nothing, "grid" => nothing, "agents" => nothing, "world" => nothing,
        "steps" => 100, "warmup" => 0, "reps" => 1, "seed" => 42, "threads" => 1,
        "set" => String[], "validate" => nothing, "out" => nothing,
    )
    i = firstindex(argv)
    take(n) = (i + n <= lastindex(argv) || error("$(argv[i]) wants $n value(s)"); argv[(i + 1):(i + n)])
    while i <= lastindex(argv)
        flag = argv[i]
        if flag == "--model"
            opts["model"] = take(1)[1]
            i += 2
        elseif flag == "--grid"
            w, h = take(2)
            opts["grid"] = (parse(Int, w), parse(Int, h))
            i += 3
        elseif flag == "--agents"
            opts["agents"] = parse(Int, take(1)[1])
            i += 2
        elseif flag == "--world"
            w, h = take(2)
            opts["world"] = (parse(Float64, w), parse(Float64, h))
            i += 3
        elseif flag in ("--steps", "--warmup", "--reps", "--seed", "--threads")
            opts[flag[3:end]] = parse(Int, take(1)[1])
            i += 2
        elseif flag == "--set"
            push!(opts["set"], take(1)[1])
            i += 2
        elseif flag == "--validate"
            opts["validate"] = take(1)[1]
            i += 2
        elseif flag == "--out"
            opts["out"] = take(1)[1]
            i += 2
        else
            error("unknown argument '$flag'")
        end
    end
    return opts
end

# --- building --------------------------------------------------------------------------------

function overrides_for(model, pairs)
    known = PARAMS[model]
    values = Dict{String, Float64}()
    for pair in pairs
        parts = split(pair, '=', limit = 2)
        length(parts) == 2 || error("--set wants id=value, got '$pair'")
        id = String(parts[1])
        id in known || error("$model has no parameter '$id'")
        values[id] = parse(Float64, parts[2])
    end
    return values
end

"""The model a timed rep runs, at Henad's defaults with the sweep's overrides applied."""
function build(opts, seed)
    name = opts["model"]
    haskey(PARAMS, name) || error("unknown model '$name'")
    set = overrides_for(name, opts["set"])
    number(id, default) = get(set, id, default)

    if name in ("game_of_life", "sir")
        opts["grid"] === nothing && error("$name needs --grid W H")
        width, height = opts["grid"]
        if name == "game_of_life"
            return GameOfLife.build(width, height; density = number("density", 0.3), seed)
        end
        return Sir.build(
            width, height;
            infection_rate = number("infection_rate", 0.3),
            recovery_rate = number("recovery_rate", 0.05),
            initial_infected_pct = number("initial_infected_pct", 0.01),
            seed,
        )
    end

    (opts["agents"] === nothing || opts["world"] === nothing) && error("$name needs --agents N --world W H")
    num_agents = opts["agents"]
    world_width, world_height = opts["world"]
    if name == "boids"
        return Boids.build(;
            num_agents, world_width, world_height,
            visual_range = number("visual_range", 50.0),
            protected_range = number("protected_range", 8.0),
            separation = number("separation", 0.05),
            alignment = number("alignment", 0.05),
            cohesion = number("cohesion", 0.0005),
            max_speed = number("max_speed", 15.0),
            min_speed = number("min_speed", 3.0),
            seed,
        )
    end
    return Ants.build(;
        num_agents, world_width, world_height,
        update_cutdown = number("update_cutdown", 0.9),
        reward = number("reward", 1.0),
        momentum = number("momentum", 0.8),
        random_action = number("random_action", 0.1),
        evaporation = number("evaporation", 0.999),
        seed,
    )
end

# --- output ----------------------------------------------------------------------------------

json(value::AbstractString) = string('"', value, '"')
json(::Nothing) = "null"
json(value::Integer) = string(value)
json(value::AbstractFloat) = string(value)

function emit(fields...)
    println("{", join((string('"', key, "\":", json(value)) for (key, value) in fields), ','), "}")
    flush(stdout)
    return
end

# --- timing ----------------------------------------------------------------------------------

"""Build, warm up and step once, returning the seconds the step loop took."""
function run_rep(opts, seed)
    model = build(opts, seed)
    step!(model, opts["warmup"])
    population = nagents(model)
    started = time_ns()
    step!(model, opts["steps"])
    return (time_ns() - started) / 1.0e9, population
end

function benchmark(opts)
    emit(
        "kind" => "info", "engine" => ENGINE, "engine_version" => engine_version(),
        "model" => opts["model"], "variant" => "default", "threads" => 1,
    )
    # Everything below is compiled during this one. Rep 0 then measures the model, not Julia.
    run_rep(opts, opts["seed"])
    for rep in 0:(opts["reps"] - 1)
        seed = opts["seed"] + rep
        elapsed, population = run_rep(opts, seed)
        emit(
            "kind" => "rep", "rep" => rep, "seed" => seed, "steps" => opts["steps"],
            "warmup" => opts["warmup"], "elapsed_s" => elapsed,
            "population" => population, "heap_bytes" => nothing,
        )
    end
    return
end

# --- validate mode ---------------------------------------------------------------------------

function header(scenario, steps, extra...)
    lines = [
        "# engine: Agents.jl $(engine_version())",
        "# model: Henad rule, ported for this comparison",
        "# scenario: $scenario",
        "# steps: $steps",
    ]
    append!(lines, ["# $key: $value" for (key, value) in extra])
    return lines
end

function validate(opts)
    scenario, out = opts["validate"], opts["out"]
    mkpath(dirname(abspath(out)))

    if haskey(Scenarios.GAME_OF_LIFE, scenario)
        steps = Scenarios.GAME_OF_LIFE_STEPS[scenario]
        model = GameOfLife.build(64, 64; live = Scenarios.GAME_OF_LIFE[scenario], seed = 1)
        step!(model, steps)
        lines = header(scenario, steps, "width" => 64, "height" => 64)
        append!(lines, GameOfLife.bitmap(model))

    elseif scenario in ("boids-8", "sine-42")
        agents = scenario == "boids-8" ? Scenarios.BOIDS_8 : Scenarios.SINE_42
        model = Boids.build(; agents, seed = 1, Scenarios.BOIDS_PARAMS...)
        step!(model, 1)
        lines = header(scenario, 1, "world" => 100)
        append!(lines, [join((@sprintf("%.9e", v) for v in row), ' ') for row in Boids.rows(model)])

    elseif scenario == "ants-lattice"
        steps, width, height = 4, 32, 32
        field = Dict(name => Scenarios.ants_field(width, height, name) for name in ("to_food", "to_home"))
        model = Ants.build(; agents = Scenarios.ANTS_AGENTS, field, seed = 1, Scenarios.ANTS_PARAMS...)
        step!(model, steps)
        lines = header(
            scenario, steps,
            "width" => width, "height" => height, "agents" => length(Scenarios.ANTS_AGENTS),
        )
        push!(lines, "# --- agents: x y last_step has_food reward")
        for (x, y, last_step, has_food, reward) in Ants.rows(model)
            push!(lines, "$x $y $last_step $has_food " * @sprintf("%.9e", reward))
        end
        for name in ("to_food", "to_home")
            data = Ants.trail(model, name)
            push!(lines, "# --- $name")
            # A row of the fixture is a row of the lattice, ascending in x then in y.
            for y in 1:height
                push!(lines, join((@sprintf("%.9e", data[x, y]) for x in 1:width), ' '))
            end
        end

    elseif scenario == "sir-replicates"
        model = Sir.build(
            256, 256;
            infection_rate = 0.08, recovery_rate = 0.3, initial_infected_pct = 0.01,
            seed = opts["seed"],
        )
        lines = ["tick,Susceptible,Infected,Recovered"]
        for tick in 0:300
            tick > 0 && step!(model, 1)
            push!(lines, string(tick, ',', join(Sir.counts(model), ',')))
        end

    else
        error("unknown scenario '$scenario'")
    end

    write(out, join(lines, '\n') * "\n")
    return
end

# --- entry point -----------------------------------------------------------------------------

function main(argv)
    opts = parse_args(argv)
    if opts["validate"] !== nothing
        opts["out"] === nothing && error("--validate needs --out")
        validate(opts)
        return
    end
    opts["model"] === nothing && error("--model is required unless --validate names a scenario")
    if !(opts["threads"] in (0, 1))
        # Agents.jl steps a `StandardABM` on one thread. Its parallelism is `ensemblerun!` across
        # independent replicates, which is not what this asks for.
        println(stderr, "note: Agents.jl is single threaded, ignoring --threads $(opts["threads"])")
    end
    benchmark(opts)
    return
end

main(ARGS)
