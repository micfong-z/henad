//! krABMaga's side of the cross-engine harness contract.
//!
//! Speaks the interface in `benchmarks/protocol.md`: the same arguments as every other engine, one
//! JSON object per line out, and a validate mode that writes the fixture its model's declaration
//! describes.
//!
//! Only the step loop is timed. Construction, the initial population and warm-up all sit outside
//! the window. The `simulate!` macro is not used, since it starts a terminal UI and a system
//! monitor inside the measured path. This drives the schedule directly, the way that macro's own
//! inner loop does.
//!
//! Both variants report one thread. Under `parallel`, `Schedule::step` takes the state lock at the
//! top of each agent and holds it across `before_step`, `step` and `after_step`. Two agents never
//! run at once. That feature also moves `DenseNumberGrid2D` and `Field2D` to hashmap storage, and
//! both grid models schedule a single sweeping agent. The `parallel` row measures a slower serial
//! configuration.

mod models;
mod scenarios;

use std::path::{Path, PathBuf};
use std::time::Instant;

use krabmaga::engine::schedule::Schedule;
use krabmaga::engine::state::State;

use models::ants::Ants;
use models::boids::Boids;
use models::game_of_life::GameOfLife;
use models::sir::Sir;

const ENGINE: &str = "krabmaga";
const ENGINE_VERSION: &str = "krABMaga 0.6.2";

const GAME_OF_LIFE_PARAMS: [&str; 1] = ["density"];
const SIR_PARAMS: [&str; 3] = ["infection_rate", "recovery_rate", "initial_infected_pct"];
const BOIDS_PARAMS: [&str; 7] = [
    "visual_range",
    "protected_range",
    "separation",
    "alignment",
    "cohesion",
    "max_speed",
    "min_speed",
];
const ANTS_PARAMS: [&str; 5] = ["update_cutdown", "reward", "momentum", "random_action", "evaporation"];

// --- the schedule ------------------------------------------------------------------------------

/// `Schedule::new` reads `--nt` off the real process argv under `parallel` and exits on anything it
/// does not recognise, so that constructor is unreachable from here.
#[cfg(feature = "parallel")]
fn schedule_for(threads: usize) -> Schedule {
    Schedule::with_threads(threads.max(1))
}

#[cfg(not(feature = "parallel"))]
fn schedule_for(_threads: usize) -> Schedule {
    Schedule::new()
}

fn variant() -> &'static str {
    if cfg!(feature = "parallel") {
        "parallel"
    } else {
        "default"
    }
}

/// Workers the schedule is built with. `0` leaves the count to the engine.
fn schedule_workers(requested: usize) -> usize {
    if !cfg!(feature = "parallel") {
        return 1;
    }
    if requested > 0 {
        requested
    } else {
        std::thread::available_parallelism().map_or(1, |n| n.get())
    }
}

/// Agents a run steps at once, which is one on either variant.
///
/// The parallel schedule spawns the workers it was asked for and then serialises them behind a
/// single state lock. Reporting the worker count would publish a row reading as parallel scaling.
fn threads_used() -> usize {
    1
}

fn run_rep<S: State>(state: &mut S, workers: usize, warmup: u32, steps: u32) -> f64 {
    let mut schedule = schedule_for(workers);
    state.init(&mut schedule);
    for _ in 0..warmup {
        schedule.step(state);
    }
    let started = Instant::now();
    for _ in 0..steps {
        schedule.step(state);
    }
    started.elapsed().as_secs_f64()
}

// --- arguments ---------------------------------------------------------------------------------

struct Args {
    model: Option<String>,
    grid: Option<(i32, i32)>,
    agents: Option<u32>,
    world: Option<(f32, f32)>,
    steps: u32,
    warmup: u32,
    reps: u32,
    seed: u64,
    threads: usize,
    set: Vec<(String, f64)>,
    validate: Option<String>,
    out: Option<PathBuf>,
}

fn parse_args() -> Result<Args, String> {
    let mut args = Args {
        model: None,
        grid: None,
        agents: None,
        world: None,
        steps: 100,
        warmup: 0,
        reps: 1,
        seed: 42,
        threads: 0,
        set: Vec::new(),
        validate: None,
        out: None,
    };

    let raw: Vec<String> = std::env::args().skip(1).collect();
    let mut i = 0;
    while i < raw.len() {
        let flag = raw[i].clone();
        let consumed = match flag.as_str() {
            "--model" => {
                args.model = Some(take(&raw, i, 1, &flag)?[0].clone());
                1
            }
            "--grid" => {
                let values = take(&raw, i, 2, &flag)?;
                args.grid = Some((number(&values[0])? as i32, number(&values[1])? as i32));
                2
            }
            "--agents" => {
                args.agents = Some(number(&take(&raw, i, 1, &flag)?[0])? as u32);
                1
            }
            "--world" => {
                let values = take(&raw, i, 2, &flag)?;
                args.world = Some((number(&values[0])? as f32, number(&values[1])? as f32));
                2
            }
            "--steps" => {
                args.steps = number(&take(&raw, i, 1, &flag)?[0])? as u32;
                1
            }
            "--warmup" => {
                args.warmup = number(&take(&raw, i, 1, &flag)?[0])? as u32;
                1
            }
            "--reps" => {
                args.reps = number(&take(&raw, i, 1, &flag)?[0])? as u32;
                1
            }
            "--seed" => {
                args.seed = number(&take(&raw, i, 1, &flag)?[0])? as u64;
                1
            }
            "--threads" => {
                args.threads = number(&take(&raw, i, 1, &flag)?[0])? as usize;
                1
            }
            "--set" => {
                let pair = take(&raw, i, 1, &flag)?[0].clone();
                let (id, value) = pair
                    .split_once('=')
                    .ok_or_else(|| format!("--set wants id=value, got `{pair}`"))?;
                args.set.push((id.to_owned(), number(value)?));
                1
            }
            "--validate" => {
                args.validate = Some(take(&raw, i, 1, &flag)?[0].clone());
                1
            }
            "--out" => {
                args.out = Some(PathBuf::from(take(&raw, i, 1, &flag)?[0].clone()));
                1
            }
            other => return Err(format!("unknown argument `{other}`")),
        };
        i += consumed + 1;
    }
    Ok(args)
}

fn take(raw: &[String], at: usize, count: usize, flag: &str) -> Result<Vec<String>, String> {
    if at + count >= raw.len() {
        return Err(format!("{flag} wants {count} value(s)"));
    }
    Ok(raw[at + 1..=at + count].to_vec())
}

fn number(text: &str) -> Result<f64, String> {
    text.parse::<f64>().map_err(|_| format!("`{text}` is not a number"))
}

/// A parameter the model does not have is an error, since a mismatched set means the two runs are
/// not the same model.
fn check_params(model: &str, set: &[(String, f64)], known: &[&str]) -> Result<(), String> {
    for (id, _) in set {
        if !known.contains(&id.as_str()) {
            return Err(format!("{model} has no parameter `{id}`"));
        }
    }
    Ok(())
}

fn value_of(set: &[(String, f64)], id: &str, default: f64) -> f64 {
    set.iter().rev().find(|(key, _)| key == id).map_or(default, |(_, v)| *v)
}

fn grid_of(args: &Args) -> Result<(i32, i32), String> {
    args.grid.ok_or_else(|| "a grid model needs --grid W H".to_owned())
}

fn population_of(args: &Args) -> Result<(u32, f32, f32), String> {
    let agents = args
        .agents
        .ok_or_else(|| "an agent model needs --agents N".to_owned())?;
    let world = args
        .world
        .ok_or_else(|| "an agent model needs --world W H".to_owned())?;
    Ok((agents, world.0, world.1))
}

// --- benchmark mode ----------------------------------------------------------------------------

fn benchmark(args: &Args) -> Result<(), String> {
    let model = args.model.clone().expect("model checked by the caller");
    let known: &[&str] = match model.as_str() {
        "game_of_life" => &GAME_OF_LIFE_PARAMS,
        "sir" => &SIR_PARAMS,
        "boids" => &BOIDS_PARAMS,
        "ants" => &ANTS_PARAMS,
        other => return Err(format!("unknown model `{other}`")),
    };
    check_params(&model, &args.set, known)?;

    let workers = schedule_workers(args.threads);
    let threads = threads_used();
    if cfg!(feature = "parallel") {
        eprintln!(
            "note: krABMaga's parallel schedule holds one state lock across each agent's step. \
             {workers} workers step one agent at a time."
        );
    } else if args.threads > 1 {
        eprintln!(
            "note: krABMaga's sequential schedule runs on one thread, ignoring --threads {}",
            args.threads
        );
    }
    println!(
        concat!(
            r#"{{"kind":"info","engine":"{}","engine_version":"{}","model":"{}","#,
            r#""variant":"{}","features":[{}],"threads":{}}}"#
        ),
        ENGINE,
        ENGINE_VERSION,
        model,
        variant(),
        if cfg!(feature = "parallel") { "\"parallel\"" } else { "" },
        threads,
    );

    for rep in 0..args.reps {
        let seed = args.seed + u64::from(rep);
        let (elapsed, population) = match model.as_str() {
            "game_of_life" => {
                let (w, h) = grid_of(args)?;
                let mut state = GameOfLife::new(w, h, value_of(&args.set, "density", 0.3), seed);
                (
                    run_rep(&mut state, workers, args.warmup, args.steps),
                    state.population(),
                )
            }
            "sir" => {
                let (w, h) = grid_of(args)?;
                let mut state = Sir::new(
                    w,
                    h,
                    value_of(&args.set, "infection_rate", 0.3),
                    value_of(&args.set, "recovery_rate", 0.05),
                    value_of(&args.set, "initial_infected_pct", 0.01),
                    seed,
                );
                (
                    run_rep(&mut state, workers, args.warmup, args.steps),
                    state.population(),
                )
            }
            "boids" => {
                let (agents, world_w, world_h) = population_of(args)?;
                let mut state = Boids::new(
                    agents,
                    world_w,
                    world_h,
                    value_of(&args.set, "visual_range", 50.0) as f32,
                    value_of(&args.set, "protected_range", 8.0) as f32,
                    value_of(&args.set, "separation", 0.05) as f32,
                    value_of(&args.set, "alignment", 0.05) as f32,
                    value_of(&args.set, "cohesion", 0.0005) as f32,
                    value_of(&args.set, "max_speed", 15.0) as f32,
                    value_of(&args.set, "min_speed", 3.0) as f32,
                    seed,
                )?;
                (
                    run_rep(&mut state, workers, args.warmup, args.steps),
                    state.population(),
                )
            }
            "ants" => {
                let (agents, world_w, world_h) = population_of(args)?;
                let mut state = Ants::new(
                    agents,
                    world_w,
                    world_h,
                    value_of(&args.set, "update_cutdown", 0.9) as f32,
                    value_of(&args.set, "reward", 1.0) as f32,
                    value_of(&args.set, "momentum", 0.8),
                    value_of(&args.set, "random_action", 0.1),
                    value_of(&args.set, "evaporation", 0.999) as f32,
                    seed,
                );
                (
                    run_rep(&mut state, workers, args.warmup, args.steps),
                    state.population(),
                )
            }
            other => return Err(format!("unknown model `{other}`")),
        };

        println!(
            concat!(
                r#"{{"kind":"rep","rep":{},"seed":{},"steps":{},"warmup":{},"#,
                r#""elapsed_s":{:.9},"population":{},"heap_bytes":null}}"#
            ),
            rep, seed, args.steps, args.warmup, elapsed, population,
        );
    }
    Ok(())
}

// --- validate mode -----------------------------------------------------------------------------

/// `%.9e`, the format the other ports write their fixtures in.
fn sci(value: f32) -> String {
    let text = format!("{:.9e}", value);
    let (mantissa, exponent) = text.split_once('e').expect("scientific notation");
    let exponent: i32 = exponent.parse().expect("integer exponent");
    format!(
        "{mantissa}e{}{:02}",
        if exponent < 0 { '-' } else { '+' },
        exponent.abs()
    )
}

fn header(scenario: &str, steps: u32) -> Vec<String> {
    vec![
        format!("# engine: {ENGINE_VERSION}"),
        "# model: Henad rule, ported for this comparison".to_owned(),
        format!("# scenario: {scenario}"),
        format!("# steps: {steps}"),
    ]
}

fn validate(scenario: &str, out: &Path, seed: u64) -> Result<(), String> {
    let lines = match scenario {
        "glider" | "r-pentomino" => {
            let (pattern, steps): (&[(i32, i32)], u32) = if scenario == "glider" {
                (&scenarios::GLIDER, 101)
            } else {
                (&scenarios::R_PENTOMINO, 500)
            };
            let side = scenarios::LIFE_WORLD;
            let mut state = GameOfLife::with_live(side, side, pattern);
            let mut schedule = schedule_for(1);
            state.init(&mut schedule);
            for _ in 0..steps {
                schedule.step(&mut state);
            }
            let mut lines = header(scenario, steps);
            lines.push(format!("# width: {side}"));
            lines.push(format!("# height: {side}"));
            lines.extend(state.bitmap());
            lines
        }

        "boids-8" | "sine-42" => {
            let agents: &[(f32, f32, f32, f32)] = if scenario == "boids-8" {
                &scenarios::BOIDS_8
            } else {
                &scenarios::SINE_42
            };
            let world = scenarios::BOIDS_WORLD;
            let mut state = Boids::new(
                agents.len() as u32,
                world,
                world,
                scenarios::BOIDS_VISUAL_RANGE,
                scenarios::BOIDS_PROTECTED_RANGE,
                scenarios::BOIDS_SEPARATION,
                scenarios::BOIDS_ALIGNMENT,
                scenarios::BOIDS_COHESION,
                scenarios::BOIDS_MAX_SPEED,
                scenarios::BOIDS_MIN_SPEED,
                1,
            )?
            .with_agents(agents);
            let mut schedule = schedule_for(1);
            state.init(&mut schedule);
            schedule.step(&mut state);

            let mut lines = header(scenario, 1);
            lines.push(format!("# world: {world:.0}"));
            for (x, y, vx, vy) in models::boids::rows(&schedule) {
                lines.push(format!("{} {} {} {}", sci(x), sci(y), sci(vx), sci(vy)));
            }
            lines
        }

        "ants-lattice" => {
            let side = scenarios::ANTS_WORLD;
            let steps = scenarios::ANTS_STEPS;
            let mut state = Ants::new(
                scenarios::ANTS_AGENTS.len() as u32,
                side as f32,
                side as f32,
                scenarios::ANTS_CUTDOWN,
                scenarios::ANTS_REWARD,
                scenarios::ANTS_MOMENTUM,
                scenarios::ANTS_RANDOM_ACTION,
                scenarios::ANTS_EVAPORATION,
                1,
            )
            .with_gate_start(&scenarios::ANTS_AGENTS);
            let mut schedule = schedule_for(1);
            state.init(&mut schedule);
            for _ in 0..steps {
                schedule.step(&mut state);
            }

            let mut lines = header(scenario, steps);
            lines.push(format!("# width: {side}"));
            lines.push(format!("# height: {side}"));
            lines.push(format!("# agents: {}", scenarios::ANTS_AGENTS.len()));
            lines.push("# --- agents: x y last_step has_food reward".to_owned());
            for (x, y, last_step, has_food, reward) in models::ants::rows(&schedule) {
                lines.push(format!("{x} {y} {last_step} {has_food} {}", sci(reward)));
            }
            for (name, to_food) in [("to_food", true), ("to_home", false)] {
                lines.push(format!("# --- {name}"));
                for row in state.layer(to_food) {
                    lines.push(row.iter().map(|&v| sci(v)).collect::<Vec<_>>().join(" "));
                }
            }
            lines
        }

        "sir-replicates" => {
            let side = scenarios::SIR_WORLD;
            let mut state = Sir::new(
                side,
                side,
                scenarios::SIR_INFECTION_RATE,
                scenarios::SIR_RECOVERY_RATE,
                scenarios::SIR_INITIAL_INFECTED,
                seed,
            );
            let mut schedule = schedule_for(1);
            state.init(&mut schedule);

            let mut lines = vec!["tick,Susceptible,Infected,Recovered".to_owned()];
            for tick in 0..=scenarios::SIR_TICKS {
                if tick > 0 {
                    schedule.step(&mut state);
                }
                let counts = state.counts();
                lines.push(format!("{tick},{},{},{}", counts[0], counts[1], counts[2]));
            }
            lines
        }

        other => return Err(format!("unknown scenario `{other}`")),
    };

    if let Some(parent) = out.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("cannot create {}: {e}", parent.display()))?;
    }
    let mut text = lines.join("\n");
    text.push('\n');
    std::fs::write(out, text).map_err(|e| format!("cannot write {}: {e}", out.display()))
}

// --- entry point -------------------------------------------------------------------------------

fn run() -> Result<(), String> {
    let args = parse_args()?;
    // Not required with --validate, since the scenario names the model.
    if let Some(scenario) = &args.validate {
        let out = args.out.as_deref().ok_or_else(|| "--validate needs --out".to_owned())?;
        return validate(scenario, out, args.seed);
    }
    if args.model.is_none() {
        return Err("--model is required unless --validate names a scenario".to_owned());
    }
    benchmark(&args)
}

fn main() {
    if let Err(message) = run() {
        eprintln!("{message}");
        std::process::exit(2);
    }
}
