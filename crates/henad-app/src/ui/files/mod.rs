//! File dialogs that save and open files on whichever target the app runs, each outcome tagged with the panel that
//! requested it.
//!
//! `rfd` is async and the app has no executor, so each target drives the future its own way. Native blocks a dedicated
//! thread. In a browser the task runs on the page's event loop, and a save is a download with no dialog.

pub mod open;
pub mod save;

use std::path::PathBuf;

/// Panel that requested a save. The save's outcome goes back to it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SaveTarget {
    Export,
    /// A sweep spec, from the Sweep panel.
    SweepSpec,
    /// The files of a sweep held in memory, from the Sweep panel, identified by their generation number
    /// ([`crate::ui::results::ResultsPanel::files_generation`]).
    SweepResults(u64),
}

/// Result of one save.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SaveResult {
    /// Saved under the file name it holds.
    #[cfg_attr(
        target_arch = "wasm32",
        expect(dead_code, reason = "a browser saves through its downloads")
    )]
    Saved(String),
    /// Passed to the browser's downloads as one file, under the file name it holds. A browser reports nothing further.
    #[cfg_attr(not(target_arch = "wasm32"), expect(dead_code, reason = "only a browser downloads"))]
    Downloaded(String),
    /// Passed to the browser's downloads as several files. A browser reports nothing further, and can hold back every
    /// download after the first.
    #[cfg_attr(not(target_arch = "wasm32"), expect(dead_code, reason = "only a browser downloads"))]
    DownloadsStarted,
    Failed(String),
    /// The dialog was dismissed.
    #[cfg_attr(
        target_arch = "wasm32",
        expect(dead_code, reason = "a browser download has no dialog")
    )]
    Canceled,
}

/// Result of a save, with the panel it goes back to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SaveOutcome {
    pub target: SaveTarget,
    pub result: SaveResult,
}

/// Panel that requested an open. It also determines which files the dialog offers.
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
    #[cfg_attr(target_arch = "wasm32", expect(dead_code, reason = "a browser picks no folder"))]
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
