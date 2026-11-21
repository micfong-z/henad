//! Model files given with `--model-file`, read into the set of models a command line runs over.

use std::borrow::Cow;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context as _, Result, anyhow, bail};

use henad_compute::entry::ModelSet;
use henad_core::diagnostic::CodeDiagnostic;
use henad_explore::model_file::{ModelFile, ModelFileError};

/// Returns `models` with the model of each file in `paths` in place of the model with its id, or added after the
/// others, and each file in the order of `paths`.
///
/// A graph's or a sheet's warnings go to stderr first, each as `path:line:column: warning: message`. When `notes` is
/// set, a note on stderr names each model a file replaces. The set is cloned only when `paths` is not empty.
///
/// # Errors
///
/// Returns an error when a file cannot be read, does not hold a model file, or holds a model that does not compile,
/// with each problem at its line and column in the file, and when two files declare the same id.
pub(crate) fn with_model_files<'a>(
    models: &'a ModelSet,
    paths: &[PathBuf],
    notes: bool,
) -> Result<(Cow<'a, ModelSet>, Vec<ModelFile>)> {
    if paths.is_empty() {
        return Ok((Cow::Borrowed(models), Vec::new()));
    }
    let mut loaded = models.clone();
    let mut files: Vec<ModelFile> = Vec::with_capacity(paths.len());
    for path in paths {
        let file = read_model_file(path)?;
        let entry = file.entry().map_err(|error| load_error(&error, path))?;
        let id = entry.id().to_owned();
        // Files are pushed in the order of `paths`. An index into `files` is then an index into `paths`.
        if let Some(earlier) = files.iter().position(|earlier| earlier.id() == id) {
            bail!(
                "model files '{}' and '{}' both declare the model '{id}'",
                paths[earlier].display(),
                path.display()
            );
        }
        if loaded.replace(entry)?.is_some() && notes {
            eprintln!(
                "note: model file '{}' replaces the model '{id}' of this build",
                path.display()
            );
        }
        files.push(file);
    }
    Ok((Cow::Owned(loaded), files))
}

/// Checks that `output`, the path that `flag` writes, is none of the model files at `model_files`.
///
/// Two paths name the same file when both resolve to one canonical path, so `./sir.toml` and `sir.toml` match. An
/// output that does not exist yet names no model file.
///
/// # Errors
///
/// Returns an error naming both paths when `output` is a model file.
pub(crate) fn check_output_path(flag: &str, output: &Path, model_files: &[PathBuf]) -> Result<()> {
    let Ok(resolved) = output.canonicalize() else {
        return Ok(());
    };
    if let Some(model_file) = model_files
        .iter()
        .find(|model_file| model_file.canonicalize().is_ok_and(|path| path == resolved))
    {
        bail!(
            "{flag} '{}' is the model file '{}'. Write the output to another path",
            output.display(),
            model_file.display()
        );
    }
    Ok(())
}

/// Prints the canonical text of the graph that the model file at `path` builds as on stdout, and returns exit status
/// 0.
///
/// The file's warnings go to stderr first, as [`read_model_file`] prints them. Nothing is built.
///
/// # Errors
///
/// Returns an error when the file cannot be read or does not hold a model file, as [`read_model_file`] does, and when
/// it holds a grid rule, which builds as no graph.
pub(crate) fn print_lowered(path: &Path) -> Result<u8> {
    let file = read_model_file(path)?;
    let graph = file.lowered_graph().ok_or_else(|| {
        anyhow!(
            "model file '{}' holds a grid rule. A grid rule has no graph to print",
            path.display()
        )
    })?;
    print!("{}", graph.to_toml());
    Ok(0)
}

/// Reads the model file at `path`, printing its warnings to stderr, each as `path:line:column: warning: message`.
///
/// # Errors
///
/// Returns an error when the file cannot be read or does not hold a model file, with each problem as
/// [`load_error`] lists them.
fn read_model_file(path: &Path) -> Result<ModelFile> {
    let text = fs::read_to_string(path).with_context(|| format!("cannot read model file '{}'", path.display()))?;
    let file = ModelFile::parse(&text).map_err(|error| load_error(&error, path))?;
    let file_name = path.display().to_string();
    for warning in file.warnings() {
        eprintln!("{}", located(warning, &file_name));
    }
    Ok(file)
}

/// Returns the error of a model file at `path` that cannot load, listing each problem of `error` at its place.
///
/// Each problem reads `path:line:column: error: message`, with its notes on the lines below and the pass and node of a
/// graph's problem in a last note.
fn load_error(error: &ModelFileError, path: &Path) -> anyhow::Error {
    let file_name = path.display().to_string();
    let problems: Vec<String> = error
        .diagnostics()
        .iter()
        .map(|diagnostic| located(diagnostic.clone(), &file_name).to_string())
        .collect();
    anyhow!(problems.join("\n")).context(format!("cannot load model file '{}'", path.display()))
}

/// Returns `diagnostic` in the file `file_name`, with a note naming the pass and node of a graph's problem.
fn located(diagnostic: CodeDiagnostic, file_name: &str) -> CodeDiagnostic {
    let place = match (&diagnostic.pass, &diagnostic.node) {
        (Some(pass), Some(node)) => Some(format!("in node {node} of pass '{pass}'")),
        (Some(pass), None) => Some(format!("in pass '{pass}'")),
        (None, Some(node)) => Some(format!("in node {node}")),
        (None, None) => None,
    };
    let diagnostic = diagnostic.with_file(file_name);
    match place {
        Some(place) => diagnostic.with_note(place),
        None => diagnostic,
    }
}
