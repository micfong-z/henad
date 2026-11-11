//! Files the app writes and reads: save and open dialogs routed to the panel that asked, the recording of the stats,
//! and the viewport's capture.

use std::sync::Arc;

use henad_compute::snapshot::Snapshot;
use henad_explore::output::memory::SweepFiles;

use super::AppState;
use crate::ui::export::Recording;
use crate::ui::files::open::spawn_open;
use crate::ui::files::save::{spawn_save, spawn_save_files};
use crate::ui::files::{OpenOutcome, OpenTarget, SaveOutcome, SaveResult, SaveTarget};

/// Status shown while a save is pending, followed by the file name.
#[cfg(not(target_arch = "wasm32"))]
const SAVE_PENDING: &str = "Select location to save";

/// Status shown while a download is pending, followed by the file name.
#[cfg(target_arch = "wasm32")]
const SAVE_PENDING: &str = "Downloading";

/// Status of the downloads of several files. A browser can hold back every download after the first.
const DOWNLOADS_STARTED: &str = "Downloads started. Press Save results again if your browser blocked any.";

impl AppState {
    /// Opens a save dialog for `bytes` under the name `name`, for the panel `target`.
    ///
    /// Results can be polled via [`Self::poll_saves`].
    pub fn save_as(&mut self, target: SaveTarget, name: &str, bytes: Vec<u8>) {
        self.set_save_status(target, format!("{SAVE_PENDING} {name}"), false);
        // A test opens no dialog, and reports the save's outcome through `finish_save`.
        #[cfg(test)]
        if let Some(requested) = &mut self.saves_requested {
            requested.push((target, name.to_owned()));
            return;
        }
        spawn_save(
            target,
            name.to_owned(),
            bytes,
            self.saves.0.clone(),
            self.egui_ctx.clone(),
        );
    }

    /// Reports `result` as the outcome of the first save requested through [`Self::save_as`], and polls it, as a dialog
    /// that ends would. Test builds only.
    ///
    /// # Panics
    ///
    /// Panics when no save was requested.
    #[cfg(test)]
    pub fn finish_save(&mut self, result: SaveResult) {
        let (target, _) = self.saves_requested.as_mut().expect("saves are recorded").remove(0);
        drop(self.saves.0.send(SaveOutcome { target, result }));
        self.poll_saves();
    }

    /// Opens a dialog that saves the four files of a sweep, `files`, together, for the panel `target`.
    ///
    /// Results can be polled via [`Self::poll_saves`].
    pub fn save_files(&mut self, target: SaveTarget, files: Arc<SweepFiles>) {
        self.set_save_status(target, format!("{SAVE_PENDING} {} files", files.entries().len()), false);
        spawn_save_files(target, files, self.saves.0.clone(), self.egui_ctx.clone());
    }

    /// Sets the status line a save for `target` reports to, to `text`, a failure when `failed` holds.
    fn set_save_status(&mut self, target: SaveTarget, text: String, failed: bool) {
        match target {
            SaveTarget::Export => self.export_status = Some(text),
            SaveTarget::SweepSpec | SaveTarget::SweepResults(_) => self.sweep.status = Some(text),
            SaveTarget::Model | SaveTarget::Counterpart => {
                self.editor.file_status = Some(crate::ui::editor::FileStatus { text, failed });
            }
        }
    }

    pub fn poll_saves(&mut self) {
        while let Ok(SaveOutcome { target, result }) = self.saves.1.try_recv() {
            // A download of one file counts as saved. The user requested it, and the app cannot read back anything
            // the browser asks the user. The downloads of several files leave them unsaved. A browser can hold back
            // every download after the first.
            if let SaveTarget::SweepResults(generation) = target
                && matches!(result, SaveResult::Saved(_) | SaveResult::Downloaded(_))
            {
                self.results.mark_files_saved(generation);
            }
            let saved = match &result {
                SaveResult::Saved(name) | SaveResult::Downloaded(name) => Some(name.clone()),
                SaveResult::DownloadsStarted | SaveResult::Failed(_) | SaveResult::Canceled => None,
            };
            if matches!(target, SaveTarget::Model | SaveTarget::Counterpart) {
                self.editor.save_finished(target, saved.clone());
            }
            let failed = matches!(result, SaveResult::Failed(_));
            let text = match result {
                SaveResult::Saved(name) => format!("Saved {name}"),
                SaveResult::Downloaded(name) => format!("Downloaded {name}"),
                SaveResult::DownloadsStarted => DOWNLOADS_STARTED.to_owned(),
                SaveResult::Failed(message) => format!("Save failed: {message}"),
                SaveResult::Canceled => "Save canceled".to_owned(),
            };
            self.set_save_status(target, text, failed);
            if matches!(target, SaveTarget::Model | SaveTarget::Counterpart) {
                crate::ui::editor::continue_replacement(self, target, saved.is_some());
            }
        }
    }

    /// Opens a dialog that picks the files or the folder that `target` requests.
    ///
    /// Results can be polled via [`Self::poll_opens`].
    pub fn open_file(&self, target: OpenTarget) {
        spawn_open(target, self.opens.0.clone(), self.egui_ctx.clone());
    }

    pub fn poll_opens(&mut self) {
        while let Ok(OpenOutcome { target, result }) = self.opens.1.try_recv() {
            match target {
                OpenTarget::SweepSpec | OpenTarget::DesignTable | OpenTarget::OutputFolder => {
                    crate::ui::sweep::receive_open(self, target, result);
                }
                OpenTarget::Results => crate::ui::results::receive_open(self, result),
                OpenTarget::Model => crate::ui::editor::receive_open(self, result),
                OpenTarget::Function => crate::ui::editor::receive_import(self, result),
            }
        }
    }

    /// Appends the snapshot's stats to the recording, and stops the recording on a write error.
    pub fn record(&mut self, snapshot: &Snapshot) {
        if let Err(err) = self.recording.push(snapshot.tick, &snapshot.stats) {
            self.export_status = Some(format!("Recording stopped: {err}"));
            self.recording = Recording::Off;
        }
    }

    /// Stops recording and closes the CSV file.
    pub fn stop_recording(&mut self) {
        match std::mem::replace(&mut self.recording, Recording::Off).stop(self.current_tick()) {
            Ok(recording) => self.recording = recording,
            Err(err) => self.export_status = Some(format!("Recording failed: {err}")),
        }
    }

    /// Draws the layers into an offscreen target at their own resolution and starts reading it back.
    ///
    /// Results can be polled via [`Self::poll_capture`].
    pub fn request_viewport_capture(&mut self, name: String) {
        match crate::ui::export::image::start(self, name) {
            Ok(pending) => {
                self.export_status = Some("Capturing viewport".to_owned());
                self.capture = Some(pending);
            }
            Err(err) => self.export_status = Some(format!("Capture failed: {err}")),
        }
    }

    pub fn poll_capture(&mut self) {
        let Some(pending) = &self.capture else {
            return;
        };
        let Some(result) = pending.poll(&self.render_ctx.device) else {
            return;
        };
        let name = pending.name.clone();
        self.capture = None;
        match result {
            Ok(png) => self.save_as(SaveTarget::Export, &name, png),
            Err(err) => self.export_status = Some(format!("Capture failed: {err}")),
        }
    }
}
