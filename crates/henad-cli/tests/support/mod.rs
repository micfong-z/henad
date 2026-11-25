//! Helpers the integration tests of henad-cli share: a scratch directory, the binary run with arguments, its output
//! read back, and the manifest a sweep writes.

#![allow(dead_code, reason = "each test crate uses its own part of the module")]

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use serde_json::Value;

/// Directory under the system's temporary directory, made empty on creation and removed with its contents on drop.
pub struct Scratch(PathBuf);

impl Scratch {
    /// Returns the empty directory `henad-cli-<name>-<process id>`.
    pub fn new(name: &str) -> Self {
        let path = std::env::temp_dir().join(format!("henad-cli-{name}-{}", std::process::id()));
        drop(std::fs::remove_dir_all(&path));
        std::fs::create_dir_all(&path).expect("the scratch directory can be made");
        Self(path)
    }

    /// Returns the path of `name` inside the directory.
    pub fn path(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }

    /// Returns the path of `name` inside the directory, as an argument.
    pub fn arg(&self, name: &str) -> String {
        argument(&self.path(name))
    }

    /// Writes `text` to the file `name` inside the directory, and returns its path as an argument.
    pub fn write(&self, name: &str, text: &str) -> String {
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
pub fn argument(path: &Path) -> String {
    path.to_str().expect("a UTF-8 path").to_owned()
}

/// Runs `henad-cli` with `arguments` and returns its output.
///
/// # Panics
///
/// Panics when the command does not run.
pub fn run<S: AsRef<str>>(arguments: &[S]) -> Output {
    let arguments: Vec<&str> = arguments.iter().map(AsRef::as_ref).collect();
    Command::new(env!("CARGO_BIN_EXE_henad-cli"))
        .args(&arguments)
        .output()
        .expect("henad-cli runs")
}

/// Runs `henad-cli` with `arguments`, checks that it succeeds, and returns its output.
///
/// # Panics
///
/// Panics when the command does not run or exits with a failure.
pub fn run_ok<S: AsRef<str>>(arguments: &[S]) -> Output {
    let output = run(arguments);
    assert!(
        output.status.success(),
        "henad-cli {} failed: {}",
        arguments.iter().map(AsRef::as_ref).collect::<Vec<_>>().join(" "),
        stderr(&output)
    );
    output
}

/// Returns the standard output of `output`.
///
/// # Panics
///
/// Panics when the output is not UTF-8.
pub fn stdout(output: &Output) -> String {
    String::from_utf8(output.stdout.clone()).expect("henad-cli writes UTF-8")
}

/// Returns the standard error of `output`, any bytes that are not UTF-8 replaced.
pub fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

/// Returns the manifest in the output directory `dir`.
///
/// # Panics
///
/// Panics when the manifest is missing or is not JSON.
pub fn manifest(dir: &Path) -> Value {
    let text = std::fs::read_to_string(dir.join("manifest.json")).expect("the manifest is written");
    serde_json::from_str(&text).expect("the manifest is JSON")
}
