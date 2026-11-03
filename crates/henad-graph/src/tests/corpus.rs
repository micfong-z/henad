//! The graph files the tests check: the starters, and the exact ports beside henad-models' tests.

use crate::starters::starters;

/// Returns the graph files under henad-models' fixtures, read at run time, since a packaged crate has no path out of
/// itself. A checkout without them returns none.
pub(super) fn fixtures() -> Vec<(String, String)> {
    let directory = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../henad-models/tests/fixtures/graphs");
    let Ok(entries) = std::fs::read_dir(directory) else {
        return Vec::new();
    };
    let mut files: Vec<(String, String)> = entries
        .filter_map(Result::ok)
        .filter(|entry| entry.path().extension().is_some_and(|extension| extension == "toml"))
        .filter_map(|entry| {
            let text = std::fs::read_to_string(entry.path()).ok()?;
            Some((entry.file_name().to_string_lossy().into_owned(), text))
        })
        .collect();
    files.sort();
    files
}

/// Returns every corpus file, starters first.
pub(super) fn all() -> Vec<(String, String)> {
    starters()
        .iter()
        .map(|starter| (starter.file_name.to_owned(), starter.text.to_owned()))
        .chain(fixtures())
        .collect()
}
