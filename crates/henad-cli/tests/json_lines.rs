//! Checks the lines that `--json` prints: the benchmark lines, which `benchmarks/protocol.md` fixes, and the sweep
//! lines, which `docs/reference/cli.md` lists.
//!
//! Each line kind holds exactly the fields its table lists. A driver reads the lines by name, and renaming or dropping
//! a field breaks the driver.

use std::collections::BTreeSet;
use std::path::PathBuf;
use std::process::Command;

use serde_json::{Value, json};

/// Directory under the system's temporary directory, removed with its contents on drop.
struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Self {
        let path = std::env::temp_dir().join(format!("henad-cli-{name}-{}", std::process::id()));
        drop(std::fs::remove_dir_all(&path));
        std::fs::create_dir_all(&path).expect("the scratch directory can be made");
        Self(path)
    }

    /// Returns the path of `name` inside the directory, as an argument.
    fn arg(&self, name: &str) -> String {
        self.0.join(name).to_str().expect("a UTF-8 path").to_owned()
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        drop(std::fs::remove_dir_all(&self.0));
    }
}

/// Runs `henad-cli` with `arguments`, and returns its exit status and the JSON lines it writes to standard output.
///
/// # Panics
///
/// Panics when the command does not run, or writes a line to standard output that is not JSON.
fn json_lines<S: AsRef<str>>(arguments: &[S]) -> (i32, Vec<Value>) {
    let arguments: Vec<&str> = arguments.iter().map(AsRef::as_ref).collect();
    let output = Command::new(env!("CARGO_BIN_EXE_henad-cli"))
        .args(&arguments)
        .output()
        .expect("henad-cli runs");
    let status = output.status.code().expect("henad-cli exits");
    let stdout = String::from_utf8(output.stdout).expect("henad-cli writes UTF-8");
    let lines = stdout
        .lines()
        .map(|line| {
            serde_json::from_str(line).unwrap_or_else(|error| {
                panic!(
                    "henad-cli {}: '{line}' is not JSON ({error}). stderr: {}",
                    arguments.join(" "),
                    String::from_utf8_lossy(&output.stderr)
                )
            })
        })
        .collect();
    (status, lines)
}

/// Returns the `kind` of each line.
fn kinds(lines: &[Value]) -> Vec<&str> {
    lines.iter().map(|line| line["kind"].as_str().unwrap_or("")).collect()
}

/// Checks that `object` holds exactly the fields in `fields`.
fn assert_fields(object: &Value, fields: &[&str]) {
    let held: BTreeSet<&str> = object
        .as_object()
        .unwrap_or_else(|| panic!("{object} is an object"))
        .keys()
        .map(String::as_str)
        .collect();
    let expected: BTreeSet<&str> = fields.iter().copied().collect();
    assert_eq!(held, expected, "{object}");
}

/// Checks that every line of kind `kind` in `lines` holds exactly `fields`, and returns their count.
fn assert_kind_fields(lines: &[Value], kind: &str, fields: &[&str]) -> usize {
    let matching: Vec<&Value> = lines.iter().filter(|line| line["kind"] == kind).collect();
    for line in &matching {
        assert_fields(line, fields);
    }
    matching.len()
}

// Fields of each line kind, `kind` included.
const PLAN_FIELDS: &[&str] = &[
    "kind",
    "model",
    "backend",
    "configs",
    "replicates",
    "runs",
    "blocks",
    "shard",
    "skipped",
    "pending",
    "cpu_lanes",
    "threads_per_lane",
    "gpu_tracks",
    "projected_bytes",
    "series_rows",
    "dry_run",
    "search",
];
const RUN_FIELDS: &[&str] = &[
    "kind",
    "run_id",
    "config_id",
    "rep",
    "seed",
    "status",
    "stop_reason",
    "ticks",
    "wall_ms",
];
const END_FIELDS: &[&str] = &[
    "kind",
    "end",
    "rows",
    "skipped",
    "ok",
    "non_finite",
    "failed",
    "elapsed_s",
    "output_dir",
];
const MERGE_FIELDS: &[&str] = &[
    "kind",
    "inputs",
    "rows",
    "ok",
    "non_finite",
    "failed",
    "missing",
    "output_dir",
];
const WARNING_FIELDS: &[&str] = &["kind", "warning", "message"];
const BATCH_FIELDS: &[&str] = &["kind", "batch", "evaluations", "runs"];

/// Checks the `runtime`, `info`, `rep` and `summary` lines of a two-rep benchmark, in order.
#[test]
fn benchmark_lines_follow_the_protocol() {
    let (status, lines) = json_lines(&[
        "sir",
        "--json",
        "--info",
        "--steps",
        "3",
        "--warmup",
        "2",
        "--reps",
        "2",
        "--seed",
        "42",
        "--set",
        "grid_width=16",
        "--set",
        "grid_height=12",
        "--act",
        "seed_outbreak@0",
    ]);
    assert_eq!(status, 0);
    assert_eq!(kinds(&lines), ["runtime", "info", "rep", "rep", "summary"]);

    let runtime_fields = [
        "kind",
        "os",
        "arch",
        "logical_cpus",
        "worker_threads",
        "adapter",
        "adapter_backend",
        "adapter_type",
    ];
    assert_fields(&lines[0], &runtime_fields);

    let info = &lines[1];
    let info_fields = [
        "kind",
        "engine",
        "engine_version",
        "model",
        "variant",
        "threads",
        "parallel_jobs",
        "adapter",
        "debug_build",
    ];
    assert_fields(info, &info_fields);
    assert_eq!(
        (&info["engine"], &info["model"], &info["variant"]),
        (&json!("henad"), &json!("sir"), &json!("cpu"))
    );

    let rep_fields = [
        "kind",
        "rep",
        "seed",
        "steps",
        "warmup",
        "elapsed_s",
        "population",
        "heap_bytes",
    ];
    for (index, rep) in lines[2..4].iter().enumerate() {
        assert_fields(rep, &rep_fields);
        assert_eq!(rep["rep"], json!(index), "{rep}");
        assert_eq!(rep["seed"], json!(42 + index), "rep i is seeded with base + i: {rep}");
        assert_eq!((&rep["steps"], &rep["warmup"]), (&json!(3), &json!(2)), "{rep}");
        assert_eq!(
            rep["population"],
            json!(16 * 12),
            "a grid's population is its cell count: {rep}"
        );
        assert!(rep["elapsed_s"].is_f64(), "{rep}");
    }

    let summary = &lines[4];
    let summary_fields = [
        "kind",
        "reps",
        "min_s",
        "median_s",
        "max_s",
        "mean_s",
        "std_dev_s",
        "steps_per_sec",
        "updates_per_sec",
        "grid_w",
        "grid_h",
        "params",
        "actions",
    ];
    assert_fields(summary, &summary_fields);
    assert_eq!(summary["reps"], json!(2));
    assert_eq!((&summary["grid_w"], &summary["grid_h"]), (&json!(16), &json!(12)));
    assert_eq!(summary["params"]["grid_width"], json!(16));
    assert_eq!(summary["actions"], json!([{ "id": "seed_outbreak", "tick": 0 }]));
}

/// Returns the arguments of a four-run sweep of SIR on an 8 by 8 grid into `out`, with `extra` after them.
fn sweep_arguments(out: &str, extra: &[&str]) -> Vec<String> {
    let mut arguments: Vec<String> = [
        "sir",
        "--json",
        "--set",
        "grid_width=8",
        "--set",
        "grid_height=8",
        "--steps",
        "4",
        "--reps",
        "2",
        "--vary",
        "infection_rate=0.1,0.2",
        "--out",
        out,
    ]
    .map(str::to_owned)
    .to_vec();
    arguments.extend(extra.iter().map(|&argument| argument.to_owned()));
    arguments
}

/// Checks the lines of a sweep, a dry run, the shards of a sweep and their merges.
#[test]
fn sweep_lines_hold_the_documented_fields() {
    let scratch = Scratch::new("json-lines-sweep");

    let (status, lines) = json_lines(&sweep_arguments(&scratch.arg("whole"), &[]));
    assert_eq!(status, 0);
    let order: Vec<&str> = kinds(&lines)
        .into_iter()
        .filter(|&kind| kind != "explore_progress")
        .collect();
    assert_eq!(
        order,
        [
            "explore_plan",
            "explore_run",
            "explore_run",
            "explore_run",
            "explore_run",
            "explore_end"
        ]
    );
    let plan = &lines[0];
    assert_fields(plan, PLAN_FIELDS);
    assert_fields(&plan["blocks"][0], &["design", "configs", "design_seed"]);
    assert_fields(&plan["shard"], &["index", "count"]);
    assert_eq!((&plan["runs"], &plan["search"]), (&json!(4), &Value::Null));
    assert_eq!(assert_kind_fields(&lines, "explore_run", RUN_FIELDS), 4);
    let run_ids: Vec<&Value> = lines
        .iter()
        .filter(|line| line["kind"] == "explore_run")
        .map(|line| &line["run_id"])
        .collect();
    assert_eq!(
        run_ids,
        [&json!(0), &json!(1), &json!(2), &json!(3)],
        "runs come in plan order"
    );
    assert_kind_fields(
        &lines,
        "explore_progress",
        &["kind", "done", "total", "skipped", "failed", "elapsed_s", "remaining_s"],
    );
    let end = lines.last().expect("an end line");
    assert_fields(end, END_FIELDS);
    assert_eq!((&end["end"], &end["ok"]), (&json!("complete"), &json!(4)));

    // Twenty million steps would write that many series rows per run. The dry run plans them and steps nothing.
    let (status, lines) = json_lines(&[
        "sir",
        "--json",
        "--set",
        "grid_width=8",
        "--set",
        "grid_height=8",
        "--steps",
        "20000000",
        "--dry-run",
    ]);
    assert_eq!(status, 0);
    assert_eq!(kinds(&lines), ["explore_plan", "explore_warning", "explore_end"]);
    assert_fields(&lines[0], PLAN_FIELDS);
    assert_eq!(lines[0]["dry_run"], json!(true));
    assert_fields(&lines[1], WARNING_FIELDS);
    assert_eq!(lines[1]["warning"], json!("series_rows"));
    assert_fields(&lines[2], END_FIELDS);
    assert_eq!(
        (&lines[2]["end"], &lines[2]["output_dir"]),
        (&json!("planned"), &Value::Null)
    );

    for (index, shard) in ["0/2", "1/2"].iter().enumerate() {
        let out = scratch.arg(&format!("shard-{index}"));
        let (status, _) = json_lines(&sweep_arguments(&out, &["--shard", shard]));
        assert_eq!(status, 0, "shard {shard}");
    }
    let (status, lines) = json_lines(&[
        "--json",
        "--merge",
        &scratch.arg("shard-0"),
        &scratch.arg("shard-1"),
        "--out",
        &scratch.arg("merged"),
    ]);
    assert_eq!(status, 0);
    assert_eq!(kinds(&lines), ["explore_merge"]);
    assert_fields(&lines[0], MERGE_FIELDS);
    assert_eq!((&lines[0]["rows"], &lines[0]["missing"]), (&json!(4), &json!(0)));

    let (status, lines) = json_lines(&[
        "--json",
        "--merge",
        &scratch.arg("shard-0"),
        "--out",
        &scratch.arg("partial"),
    ]);
    assert_eq!(status, 3, "a merge that lacks runs exits with SOME_RUNS_NOT_OK");
    assert_eq!(kinds(&lines), ["explore_warning", "explore_merge"]);
    assert_fields(&lines[0], WARNING_FIELDS);
    assert_eq!(lines[0]["warning"], json!("missing_runs"));
    assert_eq!(lines[1]["missing"], json!(2));
}

/// Settings every search spec of [`search_lines_hold_the_documented_fields`] shares: SIR on an 8 by 8 grid, five
/// steps, no series.
const SEARCH_SETUP: &str = r#"
model = "sir"

[set]
grid_width = 8
grid_height = 8

[run]
steps = 5
replicates = 2

[measure]
series_every = 0
default_reducers = false
reducers = [{ column = "Infected", kinds = ["max", "argmax"] }]
"#;

/// Checks the plan, batch and end lines of a genetic algorithm and of a Pattern Space Exploration.
#[test]
fn search_lines_hold_the_documented_fields() {
    let scratch = Scratch::new("json-lines-search");
    let genetic = r#"
[search]
algorithm = "genetic"
max_evaluations = 8
batch_size = 4
objective = { column = "Infected:max", goal = "maximize", aggregate = "median" }
space = [{ param = "infection_rate", range = { min = 0.05, max = 0.9 } }]

[search.genetic]
population = 4
elite_count = 1
tournament_size = 2
"#;
    let pse = r#"
[search]
algorithm = "pse"
max_evaluations = 4
batch_size = 2
space = [{ param = "infection_rate", range = { min = 0.05, max = 0.9 } }]

[search.pse]
x_axis = { column = "Infected:max", min = 0, max = 64, cells = 4 }
y_axis = { column = "Infected:argmax", min = 0, max = 5, cells = 4 }
initial_samples = 2
"#;
    let search_fields = [
        "algorithm",
        "max_evaluations",
        "batch_size",
        "objective",
        "watched_columns",
        "axes",
        "space",
        "search_seed",
    ];
    let scored_fields = ["best_candidate_id", "best_objective", "best_replicates"];

    for (name, table) in [("genetic", genetic), ("pse", pse)] {
        let spec = scratch.0.join(format!("{name}.toml"));
        std::fs::write(&spec, format!("{SEARCH_SETUP}{table}")).expect("the spec can be written");
        let spec = spec.to_str().expect("a UTF-8 path");
        let (status, lines) = json_lines(&["--json", "--spec", spec, "--out", &scratch.arg(name)]);
        assert_eq!(status, 0, "{name}");

        let plan = &lines[0];
        assert_eq!(plan["kind"], json!("explore_plan"), "{name}");
        assert_fields(plan, PLAN_FIELDS);
        assert_eq!(plan["configs"], Value::Null, "{name}");
        assert_fields(&plan["search"], &search_fields);

        let mut batch_fields = BATCH_FIELDS.to_vec();
        let mut end_fields = END_FIELDS.to_vec();
        end_fields.push("evaluations");
        if name == "pse" {
            assert_eq!(plan["search"]["objective"], Value::Null);
            batch_fields.push("filled_cells");
            end_fields.push("filled_cells");
        } else {
            assert_fields(&plan["search"]["objective"], &["column", "goal", "aggregate"]);
            batch_fields.extend(scored_fields);
            batch_fields.push("generation_count");
            end_fields.extend(&scored_fields[..2]);
        }
        let batches = assert_kind_fields(&lines, "explore_search_batch", &batch_fields);
        assert!(batches >= 2, "{name}: {batches} batches");
        assert!(assert_kind_fields(&lines, "explore_run", RUN_FIELDS) >= 8, "{name}");
        let end = lines.last().expect("an end line");
        assert_eq!(end["kind"], json!("explore_end"), "{name}");
        assert_fields(end, &end_fields);
    }
}
