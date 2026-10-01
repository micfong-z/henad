//! File dialogs that save and open files on whichever target the app runs, each outcome tagged with the panel that
//! asked for it.
//!
//! `rfd` is async and the app has no executor, so each target drives the future its own way. Native blocks a thread
//! of its own. In a browser the task runs on the page's event loop.

pub mod open;
pub mod save;

use std::path::PathBuf;

/// Panel that asked for a save. The save's outcome goes back to it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SaveTarget {
    Export,
    /// A sweep spec, from the Sweep panel.
    SweepSpec,
    /// The files of a sweep held in memory, from the Sweep panel, by the number
    /// [`crate::ui::results::ResultsPanel::files_generation`] gave them.
    SweepResults(u64),
}

/// Result of one save.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SaveResult {
    /// Saved, under the name inside.
    Saved(String),
    Failed(String),
    /// The dialog was dismissed.
    Canceled,
}

/// Result of a save, with the panel it goes back to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SaveOutcome {
    pub target: SaveTarget,
    pub result: SaveResult,
}

/// Panel that asked for an open. It also fixes the files the dialog offers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpenTarget {
    /// A TOML sweep spec, for the Sweep panel.
    SweepSpec,
    /// A CSV design table, for the Sweep panel.
    DesignTable,
    /// Folder a sweep writes its results into. Only the desktop app picks folders.
    OutputFolder,
    /// Results written by `henad-cli --out`: the folder on native, its files in a browser.
    Results,
}

/// A file the user picked, read into memory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DialogFile {
    /// Name of the file, without its folder.
    pub name: String,
    pub bytes: Vec<u8>,
    /// Path of the file on native, `None` in a browser.
    pub path: Option<PathBuf>,
}

/// Result of one open.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OpenResult {
    Files(Vec<DialogFile>),
    Folder(PathBuf),
    Failed(String),
    /// The dialog was dismissed.
    Canceled,
}

/// Result of an open, with the panel it goes back to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenOutcome {
    pub target: OpenTarget,
    pub result: OpenResult,
}

#[cfg(not(target_arch = "wasm32"))]
fn spawn(task: impl Future<Output = ()> + Send + 'static) {
    std::thread::spawn(move || pollster::block_on(task));
}

/// Runs `task` on the page's event loop. A browser has no thread to spawn and allows no blocking on the main thread.
#[cfg(target_arch = "wasm32")]
fn spawn(task: impl Future<Output = ()> + 'static) {
    wasm_bindgen_futures::spawn_local(task);
}
