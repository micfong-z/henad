// Cargo.toml: henad = { version = "0.3", features = ["example-models", "app"] }
// Copy the template's profile block, `[profile.dev] opt-level = 1`, `[profile.dev.package."*"] opt-level = 2` and
// `[profile.release] opt-level = 2`. Without it a debug build runs the example kernels at opt-level 0, and a release
// build at 3 where Henad measures at 2. A crate that registers its own models also calls `henad_build::stamp_commit()`
// from build.rs, with henad-build under [build-dependencies]. This example registers no models.

use std::io::Write;
use std::ops::ControlFlow;
use std::path::Path;

use henad::prelude::*;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    henad::install_panic_hook();
    let models = henad::models::example_models();
    // A sweep rejects a folder that already holds results. Each run of the program starts from an empty folder.
    let folder = std::env::temp_dir().join("sir-rates");
    if folder.exists() {
        std::fs::remove_dir_all(&folder)?;
    }
    let replay = study(&models, &folder, &mut std::io::stdout())?;

    // --8<-- [start:app]
    // Then open the app on the run the study rebuilt.
    let options = AppOptions::new(models, "SIR study", henad::build_info!()).opening(AppOpening::Run {
        replay,
        open_at: OpenAt::Start,
    });
    henad::app::run_native(options)?;
    // --8<-- [end:app]
    Ok(())
}

/// Runs SIR from `models`, sweeps it into `folder` and rebuilds one run of the sweep, writes what it finds to `out`,
/// and returns the rebuilt run's replay.
fn study(models: &ModelSet, folder: &Path, out: &mut impl Write) -> Result<Replay, Box<dyn std::error::Error>> {
    // --8<-- [start:build]
    let sir = models.get("sir").ok_or("the example set lacks SIR")?;

    // SIR on a 256 by 256 grid with seed 7. One cell in a thousand starts infected, an outbreak at tick 50 adds more,
    // and an infection rate of 0.05 spreads the epidemic slowly up to tick 100.
    let mut simulation = sir
        .setup()
        .set("grid_width", 256u32)?
        .set("grid_height", 256u32)?
        .set("initial_infected_pct", 0.001f32)?
        .set_text("infection_rate", "0.05")?
        .with_seed(7)
        .act_at("seed_outbreak", 50)?
        .build(None)?;
    simulation.run_to(100)?;
    let stats = simulation.stats()?;
    let susceptible = stats.scalar("Susceptible");
    writeln!(out, "tick {}: {susceptible:?} susceptible", stats.tick())?;
    // --8<-- [end:build]

    // --8<-- [start:live]
    // A more infectious variant arrives. A live edit raises the infection rate and a second outbreak seeds it. Up to
    // 400 more ticks follow, sampled every 20, stopping once nobody is infected.
    simulation.set_param("infection_rate", 0.3f32)?;
    simulation.act("seed_outbreak")?;
    let mut samples = Vec::new();
    let flow = simulation.run_sampled(500, 20, |sample| {
        samples.push((sample.tick(), sample.scalar("Susceptible"), sample.scalar("Infected")));
        match sample.scalar("Infected") {
            Some(infected) if infected < 1.0 => ControlFlow::Break(sample.tick()),
            _ => ControlFlow::Continue(()),
        }
    })?;
    for (tick, susceptible, infected) in samples {
        writeln!(out, "tick {tick}: {susceptible:?} susceptible, {infected:?} infected")?;
    }
    if let ControlFlow::Break(tick) = flow {
        writeln!(out, "the epidemic ended by tick {tick}")?;
    }
    // --8<-- [end:live]

    // --8<-- [start:sweep]
    // Three infection rates, four replicates each, written to `folder`.
    let loaded = LoadedSpec::parse(
        r#"
        model = "sir"
        [set]
        grid_width = 128
        grid_height = 128
        [run]
        steps = 300
        replicates = 4
        [[block]]
        design = "factorial"
        factors = [{ param = "infection_rate", values = [0.2, 0.3, 0.4] }]
        "#,
    )?;
    let mut options = SweepOptions::new(Provenance::new(henad::build_info!(), std::env::args().collect()));
    options.spec_source = loaded.spec_source.clone();
    options.apply_execution(&loaded.execution);
    let record = run_spec(
        sir,
        None,
        &loaded.spec,
        SweepOutput::Directory(folder.to_owned()),
        &options,
        &mut NoProgress,
    )?;
    let counts = &record.report.counts;
    writeln!(out, "{} of {} runs ok", counts.ok, counts.rows)?;
    // --8<-- [end:sweep]

    // --8<-- [start:replay]
    // Read the folder back and rebuild run 5 headlessly.
    let results = ResultSet::open_dir(folder, 64 << 20)?;
    let replay = results.replay(sir.schema(), 5)?;
    let mut rebuilt = RunSetup::from_replay(sir, &replay)?.build(None)?;
    rebuilt.run_to(replay.ticks)?;
    let infected = rebuilt.stats()?.scalar("Infected");
    writeln!(out, "run 5 ends with {infected:?} infected")?;
    // --8<-- [end:replay]
    Ok(replay)
}
