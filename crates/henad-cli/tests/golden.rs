//! Golden tests: `--list`, `--params` and `--params --json` print what Henad 0.2.0 printed, byte for byte.
//!
//! Scripts parse all three, `scripts/bench_matrix.py` among them. The reference output sits in `tests/golden/`, with
//! the procedure that recorded it from the `v0.2.0` tag in `tests/golden/README.md`.

use std::path::{Path, PathBuf};
use std::process::Command;

use henad_explore::device::acquire_headless;

/// Returns the folder of reference output.
fn golden_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden")
}

/// Returns whether this machine offers a compute adapter, through the call that the CLI uses to acquire its device.
///
/// An adapter below the WebGPU baseline yields no device there, and the CLI then lists only the CPU models.
///
/// # Panics
///
/// Panics when `HENAD_REQUIRE_GPU` is set to anything but empty or `0` and no device is available.
fn has_adapter() -> bool {
    match acquire_headless(henad_models::example_models().gpu_needs()) {
        Ok(_) => true,
        Err(error) => {
            let required = std::env::var_os("HENAD_REQUIRE_GPU").is_some_and(|value| !value.is_empty() && value != "0");
            assert!(
                !required,
                "HENAD_REQUIRE_GPU is set, but the CLI finds no device: {error}"
            );
            false
        }
    }
}

/// Returns the standard output of `henad-cli` run with `arguments`.
///
/// # Panics
///
/// Panics when the command does not run or exits with a failure.
fn stdout_of(arguments: &[&str]) -> String {
    let output = Command::new(env!("CARGO_BIN_EXE_henad-cli"))
        .args(arguments)
        .output()
        .expect("henad-cli runs");
    assert!(
        output.status.success(),
        "henad-cli {} failed: {}",
        arguments.join(" "),
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).expect("henad-cli writes UTF-8")
}

/// Returns the contents of reference file `name` in `dir`.
fn reference(dir: &Path, name: &str) -> String {
    let path = dir.join(name);
    std::fs::read_to_string(&path).unwrap_or_else(|error| panic!("{}: {error}", path.display()))
}

#[test]
fn list_prints_what_0_2_0_printed() {
    let dir = golden_dir();
    let name = if has_adapter() {
        "list.txt"
    } else {
        "list-without-adapter.txt"
    };
    assert_eq!(stdout_of(&["--list"]), reference(&dir, name), "{name}");
}

/// Checks `--params` and `--params --json` for every example model, a GPU model only where an adapter exists.
#[test]
fn params_print_what_0_2_0_printed() {
    let dir = golden_dir();
    let adapter = has_adapter();
    let models = henad_models::example_models();
    let mut compared = 0;
    for entry in &models {
        if entry.gpu_needs().is_some() && !adapter {
            continue;
        }
        let id = entry.id();
        let text = format!("params/{id}.txt");
        assert_eq!(stdout_of(&[id, "--params"]), reference(&dir, &text), "{text}");
        let json = format!("params-json/{id}.json");
        assert_eq!(stdout_of(&[id, "--params", "--json"]), reference(&dir, &json), "{json}");
        compared += 1;
    }
    assert!(compared >= 6, "every CPU model has reference output");
}
