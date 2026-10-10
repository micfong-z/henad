//! Checks `--model-file`: a model file's model sweeps, resumes and merges only under its own code, can take the place
//! of an example model, shows in `--list` and `--params --json`, and is refused at the line of a problem.
//!
//! A grid rule runs on the GPU. The tests that build one skip on a machine without a device, and fail there when
//! `HENAD_REQUIRE_GPU` is set.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use henad_explore::model_file::ModelFile;
use henad_explore::testing::{TestDeviceRequest, headless_test_device};
use serde_json::Value;

/// Directory under the system's temporary directory, removed with its contents on drop.
struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Self {
        let path = std::env::temp_dir().join(format!("henad-cli-model-file-{name}-{}", std::process::id()));
        drop(std::fs::remove_dir_all(&path));
        std::fs::create_dir_all(&path).expect("the scratch directory can be made");
        Self(path)
    }

    /// Returns the path of `name` inside the directory.
    fn path(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }

    /// Writes `text` to the file `name` inside the directory, and returns its path as an argument.
    fn write(&self, name: &str, text: &str) -> String {
        let path = self.path(name);
        std::fs::write(&path, text).expect("the scratch file can be written");
        argument(&path)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        drop(std::fs::remove_dir_all(&self.0));
    }
}

/// Returns `path` as a command-line argument.
fn argument(path: &Path) -> String {
    path.to_str().expect("a UTF-8 path").to_owned()
}

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

/// Runs `henad-cli` with `arguments` and returns its output.
fn run<S: AsRef<str>>(arguments: &[S]) -> Output {
    let arguments: Vec<&str> = arguments.iter().map(AsRef::as_ref).collect();
    Command::new(env!("CARGO_BIN_EXE_henad-cli"))
        .args(&arguments)
        .output()
        .expect("henad-cli runs")
}

/// Runs `henad-cli` with `arguments`, checks that it succeeds, and returns its output.
fn run_ok<S: AsRef<str>>(arguments: &[S]) -> Output {
    let output = run(arguments);
    assert!(
        output.status.success(),
        "henad-cli {} failed: {}",
        arguments.iter().map(AsRef::as_ref).collect::<Vec<_>>().join(" "),
        stderr(&output)
    );
    output
}

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
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

/// Returns the manifest in `dir`.
fn manifest(dir: &Path) -> Value {
    let text = std::fs::read_to_string(dir.join("manifest.json")).expect("the manifest is written");
    serde_json::from_str(&text).expect("the manifest is JSON")
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
