//! Tests of the time axis in the World tabs: the tick counter of a model with a time scale, and its hidden tick
//! length, which no panel shows a slider for and the Sweep tab still varies.

use henad_core::metadata::{TimeScale, TimeUnit};
use henad_core::params::{ParamKind, ParamValue};
use henad_graph::GraphFile;

use crate::state::{AppState, default_values};
use crate::tests::support::{accessible_names, set_of, sir_rates, step_past};
use crate::ui::editor::graph::GraphEditor;
use crate::ui::editor::graph::world::world_ui;
use crate::ui::params::params_ui;
use crate::ui::playback::tick_counter;
use crate::ui::sweep::draft::SweepDraft;
use crate::ui::time_axis::RunTime;

/// Label of the fixture's tick length.
const TICK_LENGTH_LABEL: &str = "One tick is (days)";

/// Label of a parameter of the fixture that the panels show.
const INFECTION_RATE_LABEL: &str = "Infection rate (per day)";

/// Returns the position and default of the fixture's tick length among its parameters.
fn tick_length() -> (usize, f32) {
    let entry = sir_rates();
    let index = entry
        .param_descriptors()
        .iter()
        .position(|param| param.id() == "tick_length")
        .expect("the fixture declares a tick length");
    let ParamKind::F32 { default, .. } = *entry.param_descriptors()[index].kind() else {
        panic!("the tick length is an f32");
    };
    (index, default)
}

#[test]
fn the_counter_names_the_days_of_a_timed_model_and_only_the_tick_of_another() {
    let mut models = set_of(sir_rates());
    let sir = henad_models::example_models()
        .get("sir")
        .cloned()
        .expect("SIR is registered");
    models.insert(sir).expect("the ids differ");
    let Some(mut app) = AppState::headless(models, true) else {
        return;
    };
    let (index, default) = tick_length();
    let id = sir_rates().id().to_owned();

    app.select_model(&id);
    app.reset_simulation();
    step_past(&mut app, 0);
    let tick = app.snapshot.as_ref().expect("a snapshot").tick;
    let days = TimeScale::time_at(tick, default);
    let unit = if days == 1.0 { "day" } else { "days" };
    assert_eq!(
        app.loaded_time(),
        Some(RunTime {
            unit: TimeUnit::Day,
            tick_length: default
        })
    );
    assert_eq!(tick_counter(&app), Some(format!("Tick {tick} ({days} {unit})")));

    app.param_values[index] = ParamValue::F32(0.5);
    app.reset_simulation();
    for tick in 0..10 {
        step_past(&mut app, tick);
    }
    assert_eq!(
        tick_counter(&app).as_deref(),
        Some("Tick 10 (5 days)"),
        "a half-day tick"
    );

    app.select_model("sir");
    app.reset_simulation();
    step_past(&mut app, 0);
    let tick = app.snapshot.as_ref().expect("a snapshot").tick;
    assert_eq!(app.loaded_time(), None);
    assert_eq!(tick_counter(&app), Some(format!("Tick {tick}")));
}

#[test]
fn a_hidden_tick_length_has_no_slider_on_the_parameters_tab() {
    let entry = sir_rates();
    let Some(mut app) = AppState::headless(set_of(entry.clone()), false) else {
        return;
    };
    app.select_model(entry.id());
    // A slider's name ends with the icon of its hint.
    let names = accessible_names(|ui| params_ui(ui, &mut app));
    assert!(
        names.iter().any(|name| name.starts_with(INFECTION_RATE_LABEL)),
        "{names:?}"
    );
    assert!(
        !names.iter().any(|name| name.starts_with(TICK_LENGTH_LABEL)),
        "{names:?}"
    );
}

#[test]
fn a_hidden_tick_length_is_left_out_of_the_world_panel() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../henad-models/tests/fixtures/lowered/rates/sir_rates.toml");
    let text = std::fs::read_to_string(&path).expect("the fixture reads");
    let graph = GraphEditor::new(&GraphFile::parse(&text).expect("the fixture reads as a graph"));
    let mut requests = Vec::new();
    let names = accessible_names(|ui| world_ui(ui, &graph, &mut requests));
    assert!(names.iter().any(|name| name == INFECTION_RATE_LABEL), "{names:?}");
    assert!(names.iter().any(|name| name == "Parameters (3)"), "{names:?}");
    assert!(!names.iter().any(|name| name == TICK_LENGTH_LABEL), "{names:?}");
}

#[test]
fn a_hidden_tick_length_stays_a_sweep_factor() {
    let entry = sir_rates();
    let schema = entry.schema();
    let (index, _) = tick_length();
    let mut draft = SweepDraft::new(&schema);
    assert_eq!(draft.factors.len(), schema.params.len(), "a row per parameter");
    draft.factors[index].vary = true;
    "0.5, 2".clone_into(&mut draft.factors[index].levels_text);
    let planned = draft
        .check(&schema, &default_values(&entry))
        .unwrap_or_else(|issues| panic!("the draft does not plan: {issues:?}"));
    assert_eq!(planned.counts().0, 2, "two tick lengths");
}
