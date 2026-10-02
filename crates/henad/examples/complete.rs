// Cargo.toml: henad = { version = "0.3", features = ["example-models", "app"] }
// Copy the template's profile block, `[profile.dev] opt-level = 1`, `[profile.dev.package."*"] opt-level = 2` and
// `[profile.release] opt-level = 2`. Without it a debug build runs the example kernels at opt-level 0, and a release
// build at 3 where Henad measures at 2. A crate that registers models of its own also calls
// `henad_build::stamp_commit()` from build.rs, with henad-build under [build-dependencies]. This one registers none.

#![expect(clippy::print_stdout, reason = "the program reports on standard output")]

use std::ops::ControlFlow;

use henad::prelude::*;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    henad::install_panic_hook();
    let models = henad::models::example_models();
    let sir = models.get("sir").ok_or("the example set holds SIR")?;

    // SIR on a 256 by 256 grid with seed 7, an outbreak at tick 100, stepped to tick 300.
    let mut simulation = sir
        .setup()
        .set("grid_width", 256u32)?
        .set("grid_height", 256u32)?
        .set_text("infection_rate", "0.3")?
        .with_seed(7)
        .act_at("seed_outbreak", 100)?
        .build(None)?;
    simulation.run_to(300)?;
    let stats = simulation.stats()?;
    println!("tick {}: {:?} infected", stats.tick(), stats.scalar("Infected"));

    // A live edit and a second outbreak, then up to 200 more ticks sampled every 50, stopping once nobody is infected.
    simulation.set_param("infection_rate", 0.1f32)?;
    simulation.act("seed_outbreak")?;
    let flow = simulation.run_sampled(500, 50, |sample| {
        println!("tick {}: {:?} infected", sample.tick(), sample.scalar("Infected"));
        match sample.scalar("Infected") {
            Some(infected) if infected < 1.0 => ControlFlow::Break(sample.tick()),
            _ => ControlFlow::Continue(()),
        }
    })?;
    if let ControlFlow::Break(tick) = flow {
        println!("the outbreak ended by tick {tick}");
    }

    // Three infection rates, four replicates each, written to a folder.
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
    let folder = std::env::temp_dir().join("sir-rates");
    let mut options = SweepOptions::new(Provenance::new(henad::build_info!(), std::env::args().collect()));
    options.spec_source = loaded.spec_source.clone();
    options.apply_execution(&loaded.execution);
    let record = run_spec(
        sir,
        None,
        &loaded.spec,
        SweepOutput::Directory(folder.clone()),
        &options,
        &mut NoProgress,
    )?;
    println!("{} of {} runs ok", record.report.counts.ok, record.report.counts.rows);

    // Read the folder back, rebuild run 5 headlessly, then open the app on the same run.
    let results = ResultSet::open_dir(&folder, 64 << 20)?;
    let replay = results.replay(sir.schema(), 5)?;
    let mut rebuilt = RunSetup::from_replay(sir, &replay)?.build(None)?;
    rebuilt.run_to(replay.ticks)?;
    println!("run 5 ends with {:?} infected", rebuilt.stats()?.scalar("Infected"));

    let options = AppOptions::new(models, "SIR study", henad::build_info!()).opening(AppOpening::Run {
        replay,
        open_at: OpenAt::Start,
    });
    henad::app::run_native(options)?;
    Ok(())
}
