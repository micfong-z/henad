//! Checks `--model-file`: a model file's model sweeps, resumes and merges only under its own code, can take the place
//! of an example model, shows in `--list` and `--params --json`, and is refused at the line of a problem. A graph model
//! file checks, prints its problems at their lines, and runs. A sheet runs, prints its problems at the lines of the
//! sheet, and prints the graph it lowers to.
//!
//! A grid rule and a graph run on the GPU. The tests that build one skip on a machine without a device, and fail there
//! when `HENAD_REQUIRE_GPU` is set.

mod support;

use std::path::{Path, PathBuf};

use henad_explore::model_file::ModelFile;
use henad_explore::testing::{TestDeviceRequest, headless_test_device};
use serde_json::Value;
use support::{Scratch, argument, manifest, run, run_ok, stderr, stdout};

/// Returns the text of the example model file, the Life rule.
fn life_rule() -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../henad-explore/specs/models/life_rule.toml");
    std::fs::read_to_string(path).expect("the example model file is in the repository")
}

/// Returns the Life rule with its step changed, so that a dead cell with six live neighbours also comes alive.
fn edited_life_rule() -> String {
    let text = life_rule();
    let edited = text.replace("alive == 3u ||", "alive == 3u || alive == 6u ||");
    assert_ne!(edited, text, "the step holds the birth rule");
    edited
}

/// Returns whether this machine has a device. Without one, a test that builds a grid rule skips.
///
/// # Panics
///
/// Panics when `HENAD_REQUIRE_GPU` is set and no device is available.
fn has_device() -> bool {
    headless_test_device(&TestDeviceRequest::baseline()).is_some()
}

/// Returns the arguments of a sweep of the Life rule in `model_file`, two densities on a 16 by 16 grid for four steps
/// with `replicates` replicates, into `dir`.
fn sweep_arguments(model_file: &str, replicates: &str, dir: &Path) -> Vec<String> {
    [
        "life_rule",
        "--model-file",
        model_file,
        "--set",
        "grid_width=16",
        "--set",
        "grid_height=16",
        "--steps",
        "4",
        "--vary",
        "density=0.2,0.4",
        "--reps",
        replicates,
        "--out",
        &argument(dir),
    ]
    .map(str::to_owned)
    .to_vec()
}

/// Returns the number of records in `runs.csv` in `dir`, its header left out.
fn run_count(dir: &Path) -> usize {
    let text = std::fs::read_to_string(dir.join("runs.csv")).expect("runs.csv is written");
    text.lines().count() - 1
}

/// Returns the code hash of the model file `text`, as 16 hexadecimal digits.
fn code_hash(text: &str) -> String {
    format!("{:016x}", ModelFile::parse(text).expect("a model file").code().hash())
}

#[test]
fn a_model_file_sweeps_and_records_its_code() {
    if !has_device() {
        return;
    }
    let scratch = Scratch::new("sweep");
    let text = life_rule();
    let model_file = scratch.write("life_rule.toml", &text);
    let dir = scratch.path("sweep");
    run_ok(&sweep_arguments(&model_file, "2", &dir));
    assert_eq!(run_count(&dir), 4, "two densities, two replicates");

    let manifest = manifest(&dir);
    let canonical = ModelFile::parse(&text).expect("a model file").to_toml();
    assert_eq!(manifest["model"]["id"], "life_rule");
    assert_eq!(manifest["model"]["code"]["format"], "henad-grid-rule/1");
    assert_eq!(manifest["model"]["code"]["text"], canonical.as_str());
    assert_eq!(manifest["model"]["code"]["fnv1a64"], code_hash(&text).as_str());
    let source = &manifest["sessions"][0]["model_source"];
    assert_eq!(source["origin"], "runtime");
    assert_eq!(source["code_hash"], code_hash(&text).as_str());
    assert_eq!(source["type_path"], "runtime:life_rule");
    let arguments = manifest["argv"].to_string();
    assert!(arguments.contains("--model-file"), "{arguments}");
}

#[test]
fn a_resume_with_an_edited_model_file_is_refused() {
    if !has_device() {
        return;
    }
    let scratch = Scratch::new("resume");
    let original = scratch.write("life_rule.toml", &life_rule());
    let edited = scratch.write("edited.toml", &edited_life_rule());
    let dir = scratch.path("sweep");
    run_ok(&sweep_arguments(&original, "1", &dir));
    let manifest_before = std::fs::read(dir.join("manifest.json")).expect("the manifest is written");

    let resume =
        |model_file: &str| run(&[sweep_arguments(model_file, "2", &dir), vec!["--resume".to_owned()]].concat());
    let refused = resume(&edited);
    assert_eq!(refused.status.code(), Some(1), "{}", stderr(&refused));
    let message = stderr(&refused);
    assert!(message.contains("different model code"), "{message}");
    assert!(message.contains(&code_hash(&life_rule())), "{message}");
    assert!(message.contains(&code_hash(&edited_life_rule())), "{message}");
    assert_eq!(run_count(&dir), 2, "the refused resume runs nothing");
    assert_eq!(
        std::fs::read(dir.join("manifest.json")).expect("the manifest is kept"),
        manifest_before,
        "the refused resume leaves the manifest alone"
    );

    let resumed = resume(&original);
    assert!(resumed.status.success(), "{}", stderr(&resumed));
    assert_eq!(run_count(&dir), 4, "the second replicate of each density");
}

#[test]
fn a_merge_of_shards_with_different_model_files_is_refused() {
    if !has_device() {
        return;
    }
    let scratch = Scratch::new("merge");
    let original = scratch.write("life_rule.toml", &life_rule());
    let edited = scratch.write("edited.toml", &edited_life_rule());
    let shard = |model_file: &str, shard: &str, name: &str| {
        let dir = scratch.path(name);
        run_ok(
            &[
                sweep_arguments(model_file, "1", &dir),
                vec!["--shard".to_owned(), shard.to_owned()],
            ]
            .concat(),
        );
        argument(&dir)
    };
    let (first, second, other) = (
        shard(&original, "0/2", "shard-0"),
        shard(&original, "1/2", "shard-1"),
        shard(&edited, "1/2", "shard-1-edited"),
    );

    let merged = scratch.path("merged");
    run_ok(&["--merge", &first, &second, "--out", &argument(&merged)]);
    assert_eq!(run_count(&merged), 2);

    let refused_dir = scratch.path("refused");
    let refused = run(&["--merge", &first, &other, "--out", &argument(&refused_dir)]);
    assert_eq!(refused.status.code(), Some(1), "{}", stderr(&refused));
    let message = stderr(&refused);
    assert!(message.contains("ran different model code"), "{message}");
    assert!(message.contains(&code_hash(&edited_life_rule())), "{message}");
    assert!(!refused_dir.exists(), "the refused merge writes nothing");
}

#[test]
fn a_model_file_can_shadow_an_example_model() {
    if !has_device() {
        return;
    }
    let scratch = Scratch::new("shadow");
    let shadow = life_rule().replace("id = \"life_rule\"", "id = \"gpu_game_of_life\"");
    let model_file = scratch.write("shadow.toml", &shadow);

    let output = run_ok(&["gpu_game_of_life", "--model-file", &model_file, "--params"]);
    let note = stderr(&output);
    assert!(
        note.contains("note: model file '") && note.contains("replaces the model 'gpu_game_of_life'"),
        "a note names the model the file replaces: {note}"
    );

    let output = run_ok(&["gpu_game_of_life", "--model-file", &model_file, "--params", "--json"]);
    assert!(
        !stderr(&output).contains("replaces the model"),
        "no note under --json: {}",
        stderr(&output)
    );
    let line: Value = serde_json::from_str(stdout(&output).trim()).expect("one JSON line");
    assert_eq!(line["kind"], "params", "no new line kind");
    assert_eq!(line["name"], "Life rule");
    assert_eq!(line["origin"], "runtime");
    assert_eq!(line["code_hash"], code_hash(&shadow).as_str());

    let compiled = run_ok(&["gpu_game_of_life", "--params", "--json"]);
    let line: Value = serde_json::from_str(stdout(&compiled).trim()).expect("one JSON line");
    assert!(line.get("origin").is_none(), "the example model is compiled: {line}");

    let listed = stdout(&run_ok(&["--list", "--model-file", &model_file]));
    assert_eq!(
        listed.matches("gpu_game_of_life").count(),
        1,
        "the file takes the example model's place: {listed}"
    );
    assert!(listed.contains("Life rule"), "{listed}");

    let dir = scratch.path("sweep");
    run_ok(&[
        "gpu_game_of_life",
        "--model-file",
        &model_file,
        "--set",
        "grid_width=16",
        "--set",
        "grid_height=16",
        "--steps",
        "4",
        "--out",
        &argument(&dir),
    ]);
    let manifest = manifest(&dir);
    assert_eq!(manifest["model"]["id"], "gpu_game_of_life");
    assert_eq!(manifest["sessions"][0]["model_source"]["origin"], "runtime");
}

#[test]
fn model_file_is_allowed_with_spec_and_resume() {
    if !has_device() {
        return;
    }
    let scratch = Scratch::new("spec");
    let model_file = scratch.write("life_rule.toml", &life_rule());
    let spec = scratch.write(
        "life_sweep.toml",
        "model = \"life_rule\"\n\n[set]\ngrid_width = 16\ngrid_height = 16\n\n[run]\nsteps = 4\nreplicates = 2\n\n\
         [[block]]\ndesign = \"factorial\"\nfactors = [{ param = \"density\", values = [0.2, 0.4] }]\n",
    );
    let dir = scratch.path("sweep");
    let out = argument(&dir);

    let planned = run_ok(&["--spec", &spec, "--model-file", &model_file, "--dry-run"]);
    assert!(
        stdout(&planned).contains("2 configs x 2 replicates"),
        "{}",
        stdout(&planned)
    );
    assert!(!dir.exists(), "a dry run writes nothing");

    run_ok(&["--spec", &spec, "--model-file", &model_file, "--out", &out]);
    assert_eq!(run_count(&dir), 4);
    let resumed = run_ok(&["--spec", &spec, "--model-file", &model_file, "--out", &out, "--resume"]);
    assert!(stderr(&resumed).contains("skipped 4 runs"), "{}", stderr(&resumed));

    let without = run(&["--spec", &spec, "--out", &out, "--resume"]);
    assert_eq!(without.status.code(), Some(1));
    assert!(
        stderr(&without).contains("does not include model 'life_rule'"),
        "{}",
        stderr(&without)
    );
}

#[test]
fn list_and_params_json_show_a_model_file() {
    if !has_device() {
        return;
    }
    let scratch = Scratch::new("list");
    let text = life_rule();
    let model_file = scratch.write("life_rule.toml", &text);

    let listed = stdout(&run_ok(&["--list", "--model-file", &model_file]));
    assert!(
        listed.lines().any(|line| line.trim_start().starts_with("life_rule ")),
        "{listed}"
    );
    assert!(listed.contains("game_of_life"), "the example models stay: {listed}");

    // The one file's model is the model, with no id on the command line.
    for arguments in [
        vec!["life_rule", "--model-file", &model_file, "--params", "--json"],
        vec!["--model-file", &model_file, "--params", "--json"],
    ] {
        let line: Value = serde_json::from_str(stdout(&run_ok(&arguments)).trim()).expect("one JSON line");
        assert_eq!(line["kind"], "params");
        assert_eq!(line["model"], "life_rule");
        assert_eq!(line["backend"], "gpu");
        assert_eq!(line["origin"], "runtime");
        assert_eq!(line["code_hash"], code_hash(&text).as_str());
        let ids: Vec<&str> = line["params"]
            .as_array()
            .expect("a list of parameters")
            .iter()
            .filter_map(|param| param["id"].as_str())
            .collect();
        assert_eq!(ids, ["grid_width", "grid_height", "density"]);
        assert_eq!(line["stat_columns"], serde_json::json!(["Dead", "Alive"]));
    }

    // `--info` prints its header before the parameters of the one file's model.
    for arguments in [
        vec!["--model-file", &model_file, "--params"],
        vec!["--model-file", &model_file, "--info", "--params"],
    ] {
        let text = stdout(&run_ok(&arguments));
        assert!(text.contains("parameters for life_rule (Life rule):"), "{text}");
    }
}

/// Returns the path of the hand-lowered SIR with rates, whose tick length is a hidden parameter in days.
fn sir_rates() -> String {
    argument(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../henad-models/tests/fixtures/lowered/rates/sir_rates.toml"))
}

#[test]
fn a_tick_length_is_marked_hidden_and_sweeps_like_any_parameter() {
    if !has_device() {
        return;
    }
    let model_file = sir_rates();
    let text = stdout(&run_ok(&["--model-file", &model_file, "--params"]));
    let hidden: Vec<&str> = text.lines().filter(|line| line.contains(" hidden ")).collect();
    assert_eq!(hidden.len(), 1, "{text}");
    assert!(
        hidden[0].contains("id=tick_length kind=f32 default=1 min=0.01 max=30 apply=reload hidden label="),
        "{text}"
    );

    let line: Value =
        serde_json::from_str(stdout(&run_ok(&["--model-file", &model_file, "--params", "--json"])).trim())
            .expect("one JSON line");
    assert_eq!(
        line["time"],
        serde_json::json!({ "unit": "day", "tick_length": "tick_length" })
    );
    for param in line["params"].as_array().expect("a list of parameters") {
        let is_tick_length = param["id"] == "tick_length";
        assert_eq!(param.get("hidden").is_some(), is_tick_length, "{param}");
    }
    assert_eq!(line["params"][2]["hidden"], true);

    let planned = stdout(&run_ok(&[
        "--model-file",
        &model_file,
        "--vary",
        "tick_length=0.25:1:0.25",
        "--steps",
        "4",
        "--dry-run",
    ]));
    assert!(planned.contains("4 configs x 1 replicate = 4 runs"), "{planned}");
}

/// Returns the first column of `csv` named `name`, as numbers, with the header checked to open with `header`.
fn column(csv: &str, header: &str, name: &str) -> Vec<f64> {
    let mut lines = csv.lines();
    let columns: Vec<&str> = lines.next().expect("a header").split(',').collect();
    assert!(columns.join(",").starts_with(header), "{csv}");
    let index = columns.iter().position(|column| *column == name).expect("the column");
    lines
        .map(|line| {
            line.split(',')
                .nth(index)
                .and_then(|field| field.parse().ok())
                .expect("a number")
        })
        .collect()
}

#[test]
fn a_tick_length_writes_the_time_column_in_stats_and_series() {
    if !has_device() {
        return;
    }
    let model_file = sir_rates();
    let scratch = Scratch::new("time");
    let small = ["--set", "grid_width=16", "--set", "grid_height=16"];

    let stats = argument(&scratch.path("stats.csv"));
    let mut arguments = vec!["--model-file", &model_file, "--steps", "6", "--stats-every", "3"];
    arguments.extend(small);
    arguments.extend(["--set", "tick_length=0.25", "--export-stats", &stats]);
    run_ok(&arguments);
    let csv = std::fs::read_to_string(&stats).expect("the stats are written");
    assert_eq!(column(&csv, "tick,time,Susceptible", "tick"), [0.0, 3.0, 6.0]);
    assert_eq!(column(&csv, "tick,time,Susceptible", "time"), [0.0, 0.75, 1.5]);

    let dir = scratch.path("sweep");
    let out = argument(&dir);
    let mut arguments = vec!["--model-file", &model_file, "--steps", "6", "--stats-every", "3"];
    arguments.extend(small);
    arguments.extend(["--vary", "tick_length=0.5,2", "--out", &out]);
    run_ok(&arguments);
    let series = std::fs::read_to_string(dir.join("series.csv")).expect("the series are written");
    let ticks = column(&series, "run_id,tick,time,Susceptible", "tick");
    let times = column(&series, "run_id,tick,time,Susceptible", "time");
    let runs = column(&series, "run_id,tick,time,Susceptible", "run_id");
    let lengths = [0.5, 2.0];
    for ((run, tick), time) in runs.iter().zip(&ticks).zip(&times) {
        assert_eq!(*time, tick * lengths[*run as usize], "{series}");
    }
    assert_eq!(times.last(), Some(&12.0));

    let set = henad_explore::result_set::ResultSet::open_dir(&dir, usize::MAX).expect("the sweep reads back");
    assert_eq!(set.held_series_count(), 2);
    for run in set.runs() {
        assert_eq!(run.outcome.series.ticks(), [0, 3, 6]);
    }
}

/// Returns the path of the SIR sheet starter, whose rates are per day and whose tick is 1 day.
fn sir_sheet() -> String {
    argument(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../henad-graph/sheets/sir.toml"))
}

#[test]
fn a_sheet_with_rates_sweeps_its_tick_length_with_a_time_column() {
    if !has_device() {
        return;
    }
    let model_file = sir_sheet();
    let text = stdout(&run_ok(&["--model-file", &model_file, "--params"]));
    assert!(
        text.lines()
            .any(|line| line.contains(" hidden ") && line.contains("id=tick_length kind=f32 default=1 min=0.01 max=30")),
        "{text}"
    );
    let scratch = Scratch::new("sheet-time");
    let dir = scratch.path("sweep");
    let out = argument(&dir);
    run_ok(&[
        "--model-file",
        &model_file,
        "--set",
        "grid_width=16",
        "--set",
        "grid_height=16",
        "--vary",
        "tick_length=0.25:1:0.25",
        "--steps",
        "4",
        "--stats-every",
        "2",
        "--out",
        &out,
    ]);
    let series = std::fs::read_to_string(dir.join("series.csv")).expect("the series are written");
    let header = "run_id,tick,time,Susceptible";
    let runs = column(&series, header, "run_id");
    let ticks = column(&series, header, "tick");
    let times = column(&series, header, "time");
    let lengths = [0.25, 0.5, 0.75, 1.0];
    for ((run, tick), time) in runs.iter().zip(&ticks).zip(&times) {
        assert_eq!(*time, tick * lengths[*run as usize], "{series}");
    }
    assert_eq!(times.last(), Some(&4.0));
}

#[test]
fn a_model_file_error_points_at_its_line() {
    let scratch = Scratch::new("error");
    let text = life_rule();
    let step_line = "    let alive = moore_count(ALIVE);";
    let line = text
        .lines()
        .position(|line| line == step_line)
        .expect("the step counts the live neighbours")
        + 1;

    // A loop added above that line is refused at the loop, with nothing built and no device acquired.
    let looping = scratch.write(
        "looping.toml",
        &text.replace(step_line, &format!("    loop {{ break; }}\n{step_line}")),
    );
    let output = run(&["life_rule", "--model-file", &looping, "--params"]);
    assert_eq!(output.status.code(), Some(1));
    let message = stderr(&output);
    assert!(
        message.contains(&format!("cannot load model file '{looping}'")),
        "{message}"
    );
    assert!(
        message.contains(&format!("{looping}:{line}:5: error: loops are not allowed")),
        "{message}"
    );
    assert!(stdout(&output).is_empty(), "{}", stdout(&output));

    // An unknown key is refused at its line.
    let key_line = text
        .lines()
        .position(|line| line == "boundary = \"torus\"")
        .expect("the grid's boundary")
        + 2;
    let unknown = scratch.write(
        "unknown.toml",
        &text.replace("boundary = \"torus\"", "boundary = \"torus\"\ndepth = 2"),
    );
    let message = stderr(&run(&["--list", "--model-file", &unknown]));
    assert!(
        message.contains(&format!("{unknown}:{key_line}:1: error: unknown field `depth`")),
        "{message}"
    );

    let missing = argument(&scratch.path("missing.toml"));
    let output = run(&["--list", "--model-file", &missing]);
    assert_eq!(output.status.code(), Some(1));
    assert!(stderr(&output).contains(&format!("cannot read model file '{missing}'")));

    let first = scratch.write("first.toml", &text);
    let other = scratch.write("other.toml", &text.replace("id = \"life_rule\"", "id = \"other_rule\""));
    let output = run(&["--model-file", &first, "--model-file", &other, "--params"]);
    assert_eq!(
        output.status.code(),
        Some(2),
        "two files and no model id: {}",
        stderr(&output)
    );
    assert!(
        stderr(&output).contains("a model id is required with more than one --model-file"),
        "{}",
        stderr(&output)
    );

    let second = scratch.write("second.toml", &text);
    let output = run(&["--list", "--model-file", &first, "--model-file", &second]);
    assert_eq!(output.status.code(), Some(1));
    assert!(
        stderr(&output).contains("both declare the model 'life_rule'"),
        "{}",
        stderr(&output)
    );

    let output = run(&["--merge", "a", "--out", "b", "--model-file", &first]);
    assert_eq!(output.status.code(), Some(2), "--merge takes no model file");
}

/// Returns the text of the Game of Life graph starter.
fn life_graph() -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../henad-graph/starters/game_of_life.toml");
    std::fs::read_to_string(path).expect("the graph starter is in the repository")
}

#[test]
fn a_graph_model_file_checks_runs_and_prints_its_problems() {
    let scratch = Scratch::new("graph");
    let text = life_graph();

    // A graph that checks loads, lists its parameters, and sweeps under its own code where a device is.
    let graph = scratch.write("life.toml", &text);
    let params = stdout(&run_ok(&["node_game_of_life", "--model-file", &graph, "--params"]));
    assert!(params.contains("density"), "{params}");
    if has_device() {
        let dir = scratch.path("sweep");
        run_ok(&[
            "node_game_of_life",
            "--model-file",
            &graph,
            "--set",
            "grid_width=16",
            "--set",
            "grid_height=16",
            "--steps",
            "4",
            "--vary",
            "density=0.2,0.4",
            "--reps",
            "1",
            "--out",
            &argument(&dir),
        ]);
        assert_eq!(run_count(&dir), 2, "two densities, one replicate");
        let manifest = manifest(&dir);
        assert_eq!(manifest["model"]["id"], "node_game_of_life");
        assert_eq!(manifest["model"]["code"]["format"], "henad-graph/1");
        assert_eq!(manifest["sessions"][0]["model_source"]["origin"], "runtime");
    }

    // A checker error is refused at its line, with its pass and node in a note.
    let unknown_line = text
        .lines()
        .position(|line| line == "op = \"grid.life_like\"")
        .expect("the step's rule")
        + 1;
    let broken = scratch.write(
        "broken.toml",
        &text.replace("op = \"grid.life_like\"", "op = \"grid.lifelike\""),
    );
    let message = stderr(&run(&["--list", "--model-file", &broken]));
    assert!(
        message.contains(&format!("{broken}:{unknown_line}:6: error:")) && message.contains("grid.lifelike"),
        "{message}"
    );
    assert!(message.contains("note: in node n12 of pass 'step'"), "{message}");

    // A warning prints on stderr at its line and blocks nothing the checker allows.
    let unreached = scratch.write(
        "unreached.toml",
        &text.replace(
            "[[pass]]\nid = \"counts\"",
            "[[pass.node]]\nid = \"n14\"\nop = \"ctx.index\"\n\n[[pass]]\nid = \"counts\"",
        ),
    );
    let message = stderr(&run(&["--list", "--model-file", &unreached]));
    assert!(
        message.contains(&format!("{unreached}:")) && message.contains(": warning: "),
        "{message}"
    );
    assert!(message.contains("note: in node n14 of pass 'step'"), "{message}");
    assert!(!message.contains("error"), "{message}");
}

/// Returns the path of the Game of Life sheet starter.
fn life_sheet() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../henad-graph/sheets/game_of_life.toml")
}

#[test]
fn a_sheet_model_file_runs_and_prints_its_problems_at_its_lines() {
    let scratch = Scratch::new("sheet");
    let sheet = argument(&life_sheet());
    let text = std::fs::read_to_string(life_sheet()).expect("the sheet starter is in the repository");

    // The sheet's id is its own, so the sheet runs beside the example model `game_of_life` and replaces nothing.
    if has_device() {
        let output = run_ok(&["--model-file", &sheet, "--steps", "20"]);
        assert!(!stderr(&output).contains("replaces the model"), "{}", stderr(&output));
        let listed = stdout(&run_ok(&["--list", "--model-file", &sheet]));
        for id in ["game_of_life", "sheet_game_of_life"] {
            assert!(
                listed
                    .lines()
                    .any(|line| line.trim_start().starts_with(&format!("{id} "))),
                "{id} is listed: {listed}"
            );
        }
    }

    // A Become line that names a state the property does not hold is refused at the line's table, in the sheet's
    // words, with nothing built.
    let broken_text = text.replacen("new_state = \"alive\"", "new_state = \"zombie\"", 1);
    assert_ne!(broken_text, text);
    let line = broken_text
        .lines()
        .position(|line| line == "[card.line.l2]")
        .expect("the sheet holds line l2")
        + 1;
    let broken = scratch.write("broken.toml", &broken_text);
    let output = run(&["--model-file", &broken, "--steps", "20"]);
    assert_eq!(output.status.code(), Some(1), "{}", stderr(&output));
    let message = stderr(&output);
    assert!(message.contains(&format!("{broken}:{line}:1: error:")), "{message}");
    assert!(message.contains("zombie"), "{message}");
    let words = message.replace(&broken, "");
    assert!(!words.contains("node") && !words.contains("pass"), "{message}");
    assert!(stdout(&output).is_empty(), "{}", stdout(&output));

    // A sheet with no property is refused at its world in the sheet's words, never in the lowered graph's.
    let empty_text = &text[..text.find("[[cells.property]]").expect("the sheet holds a property")];
    let world = empty_text
        .lines()
        .position(|line| line == "[world]")
        .expect("the sheet holds a world")
        + 1;
    let empty = scratch.write("empty.toml", empty_text);
    let output = run(&["--model-file", &empty, "--steps", "20"]);
    assert_eq!(output.status.code(), Some(1), "{}", stderr(&output));
    let message = stderr(&output);
    assert!(
        message.contains(&format!("{empty}:{world}:1: error: Every cell")),
        "{message}"
    );
    let words = message.replace(&empty, "");
    for word in ["node", "pass", "lane"] {
        assert!(!words.contains(word), "{message}");
    }
}

/// A boids sheet whose look-around covers a small world with more birds than one fold may meet is refused before it
/// builds, with the lines of its looks in the sheet's words and no pass of the lowered graph.
#[test]
fn a_sheet_past_the_work_budget_is_refused_at_its_lines() {
    if !has_device() {
        return;
    }
    let sheet = argument(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../henad-graph/sheets/boids.toml"));
    let output = run(&[
        "--model-file",
        &sheet,
        "--steps",
        "5",
        "--set",
        "num_agents=70000",
        "--set",
        "world_width=200",
        "--set",
        "world_height=200",
        "--set",
        "visual_range=200",
    ]);
    assert_eq!(output.status.code(), Some(1), "{}", stderr(&output));
    let message = stderr(&output);
    assert!(
        message.contains("Lines 1 and 2, Flockmates and Too close: looks at too many birds at 70,000 birds."),
        "{message}"
    );
    let words = message.replace(&sheet, "");
    for word in ["node", "pass", "units of work"] {
        assert!(!words.contains(word), "{message}");
    }
}

/// A sweep's plan refuses the config of the boids sheet past the work budget in the sheet's lines, as a single run does,
/// and leaves the config that fits alone.
#[test]
fn a_sweep_of_a_sheet_past_the_work_budget_is_refused_at_its_lines() {
    if !has_device() {
        return;
    }
    let sheet = argument(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../henad-graph/sheets/boids.toml"));
    let output = run(&[
        "--model-file",
        &sheet,
        "--dry-run",
        "--steps",
        "5",
        "--vary",
        "num_agents=100,70000",
        "--set",
        "world_width=200",
        "--set",
        "world_height=200",
        "--set",
        "visual_range=200",
    ]);
    assert_eq!(output.status.code(), Some(1), "{}", stderr(&output));
    let message = stderr(&output);
    assert!(
        message.contains("config 1: Lines 1 and 2, Flockmates and Too close: looks at too many birds at 70,000 birds."),
        "{message}"
    );
    assert!(message.contains("1 config does not fit"), "{message}");
    let words = message.replace(&sheet, "");
    for word in ["node", "pass", "units of work"] {
        assert!(!words.contains(word), "{message}");
    }
}

#[test]
fn print_lowered_prints_the_graph_a_sheet_lowers_to() {
    let sheet = argument(&life_sheet());
    let text = std::fs::read_to_string(life_sheet()).expect("the sheet starter is in the repository");
    let output = run_ok(&["--model-file", &sheet, "--print-lowered"]);
    let printed = stdout(&output);
    assert!(printed.contains("\nkind = \"graph\"\n"), "{printed}");
    let lowered = ModelFile::parse(&printed).unwrap_or_else(|error| panic!("{error}\n{printed}"));
    let graph = lowered.graph().expect("the printed text is a graph");
    assert_eq!(graph.to_toml(), printed, "the printed text is canonical");
    let file = ModelFile::parse(&text).expect("the sheet starter reads");
    assert_eq!(lowered.code(), file.code());
    assert_eq!(lowered.id(), file.id());

    // A graph prints its own canonical text, and a grid rule builds as no graph.
    let graph_text = life_graph();
    let scratch = Scratch::new("print-lowered");
    let graph_file = scratch.write("life.toml", &graph_text);
    let printed = stdout(&run_ok(&["--model-file", &graph_file, "--print-lowered"]));
    assert_eq!(
        printed,
        ModelFile::parse(&graph_text)
            .expect("the graph starter reads")
            .to_toml()
    );
    let rule = scratch.write("life_rule.toml", &life_rule());
    let output = run(&["--model-file", &rule, "--print-lowered"]);
    assert_eq!(output.status.code(), Some(1));
    assert!(
        stderr(&output).contains("holds a grid rule. A grid rule has no graph to print"),
        "{}",
        stderr(&output)
    );

    // The flag takes exactly one model file and runs nothing else.
    let output = run(&["--model-file", &sheet, "--model-file", &graph_file, "--print-lowered"]);
    assert_eq!(output.status.code(), Some(2), "{}", stderr(&output));
    assert!(stderr(&output).contains("--print-lowered requires exactly one --model-file"));
    let run_flags: [&[&str]; 5] = [
        &["--params"],
        &["--set", "grid_width=64"],
        &["--act", "clear@3"],
        &["--steps", "5"],
        &["--threads", "2"],
    ];
    for flags in run_flags {
        let arguments: Vec<&str> = ["--model-file", sheet.as_str(), "--print-lowered"]
            .into_iter()
            .chain(flags.iter().copied())
            .collect();
        let output = run(&arguments);
        assert_eq!(output.status.code(), Some(2), "{flags:?}: {}", stderr(&output));
    }
    let output = run(&["--print-lowered"]);
    assert_eq!(output.status.code(), Some(2), "{}", stderr(&output));
}
