//! Tests of the time axis in the World tabs: the tick counter of a model with a time scale, its time in the unit that
//! fits it, and its hidden tick length, which no panel shows a slider for and the Sweep tab still varies.

use henad_core::metadata::{TimeScale, TimeUnit};
use henad_core::params::{ParamDescriptor, ParamKind, ParamValue};
use henad_graph::GraphFile;

use crate::state::{AppState, default_values};
use crate::tests::panes::apply_sheet_edit;
use crate::tests::support::{
    accessible_names, accessible_texts, headless_app, set_of, settle_preview, sir_rates, starter_text, step_past,
    wait_for_snapshot,
};
use crate::ui::editor::graph::GraphEditor;
use crate::ui::editor::graph::world::world_ui;
use crate::ui::editor::{EditorModel, open_text};
use crate::ui::export::{Recording, history_csv, start_recording};
use crate::ui::model::model_ui;
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

/// One tick is on a sheet with no rate line still gives the preview its time scale, and Playback's counter shows the
/// time in the unit that fits it: 60 ticks of an hour read as days.
#[test]
fn a_sheet_s_tick_length_reaches_the_counter_in_the_unit_that_fits() {
    use henad_graph::sheet::document::SheetTime;
    use henad_graph::sheet::edit::SheetEdit;

    let Some(mut app) = headless_app() else {
        return;
    };
    open_text(
        &mut app.editor,
        starter_text("game_of_life.toml").to_owned(),
        Some("opened.toml".to_owned()),
    );
    let EditorModel::Sheet(sheet) = &mut app.editor.model else {
        panic!("a sheet");
    };
    apply_sheet_edit(
        sheet,
        SheetEdit::SetTime {
            time: Some(SheetTime::new(1.0, TimeUnit::Hour)),
        },
    );
    settle_preview(&mut app);
    app.wait_for_build();
    assert_eq!(
        app.loaded_time(),
        Some(RunTime {
            unit: TimeUnit::Hour,
            tick_length: 1.0
        })
    );
    app.run_to(60);
    wait_for_snapshot(&mut app, |tick| tick >= 60);
    assert_eq!(tick_counter(&app).as_deref(), Some("Tick 60 (2.5 days)"));
}

/// The Export tab's stats and recording write the `time` column of a timed model after `tick`, as `--export-stats`
/// writes it, and a model with no time scale writes none.
#[test]
fn the_export_tab_writes_the_time_of_a_timed_model() {
    let mut models = set_of(sir_rates());
    let sir = henad_models::example_models()
        .get("sir")
        .cloned()
        .expect("SIR is registered");
    models.insert(sir).expect("the ids differ");
    let Some(mut app) = AppState::headless(models, true) else {
        return;
    };
    let (index, _) = tick_length();
    let header = |csv: &[u8]| {
        String::from_utf8(csv.to_vec())
            .expect("CSV is UTF-8")
            .lines()
            .next()
            .unwrap_or_default()
            .to_owned()
    };
    // Each sample goes to the history and the recording through the app's intake of a snapshot.
    let record = |app: &mut AppState| {
        let snapshot = app.snapshot.take().expect("a snapshot");
        app.receive_snapshot(snapshot);
    };

    app.select_model(sir_rates().id());
    app.param_values[index] = ParamValue::F32(0.5);
    app.reset_simulation();
    step_past(&mut app, 0);
    start_recording(&mut app);
    record(&mut app);
    step_past(&mut app, 1);
    record(&mut app);
    let history = history_csv(&app).expect("a history").expect("one layout");
    assert!(header(&history).starts_with("tick,time,"), "{}", header(&history));
    let rows: Vec<String> = String::from_utf8(history)
        .expect("CSV is UTF-8")
        .lines()
        .skip(1)
        .map(|row| row.split(',').take(2).collect::<Vec<_>>().join(","))
        .collect();
    assert_eq!(rows, ["1,0.5", "2,1"], "half a day a tick");
    app.stop_recording();
    let Recording::Done { csv, .. } = &app.recording else {
        panic!("the recording did not stop");
    };
    assert!(header(csv).starts_with("tick,time,"), "{}", header(csv));

    app.recording = Recording::Off;
    app.select_model("sir");
    app.reset_simulation();
    step_past(&mut app, 0);
    start_recording(&mut app);
    record(&mut app);
    app.stop_recording();
    let Recording::Done { csv, .. } = &app.recording else {
        panic!("the recording did not stop");
    };
    assert!(!header(csv).contains("time"), "{}", header(csv));
    let history = history_csv(&app).expect("a history").expect("one layout");
    assert!(!header(&history).contains("time"), "{}", header(&history));
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

/// The Model tab's Parameters row counts the parameters the Parameters tab shows, the hidden tick length left out.
#[test]
fn the_model_tab_counts_the_parameters_the_parameters_tab_shows() {
    let entry = sir_rates();
    let Some(mut app) = AppState::headless(set_of(entry.clone()), true) else {
        return;
    };
    app.select_model(entry.id());
    // The row reads the count, and the number of those applied on reload in brackets.
    let count = |params: &[&ParamDescriptor]| {
        let reload = params.iter().filter(|param| !param.is_live()).count();
        if reload == 0 {
            params.len().to_string()
        } else {
            format!("{} ({reload} reload)", params.len())
        }
    };
    let declared: Vec<&ParamDescriptor> = entry.param_descriptors().iter().collect();
    let shown: Vec<&ParamDescriptor> = declared.iter().copied().filter(|param| !param.is_hidden()).collect();
    assert_ne!(count(&shown), count(&declared), "the fixture hides its tick length");
    // AccessKit lists the nodes in no order, so the row is found by its value.
    let texts = accessible_texts(
        |ui| model_ui(ui, &mut app),
        |node| node.value().or_else(|| node.label()),
    );
    assert!(texts.contains(&"Parameters".to_owned()), "{texts:?}");
    assert!(texts.contains(&count(&shown)), "{texts:?}");
    assert!(!texts.contains(&count(&declared)), "{texts:?}");
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
    assert_eq!(planned.counts().configs, 2, "two tick lengths");
}
