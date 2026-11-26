//! Previews of the Editor's model in the live simulation. Neither the selection nor the next build's fields follow a
//! preview.

use henad_compute::entry::ModelEntry;
use henad_compute::simulation::RunSetup;
use henad_core::params::ParamValue;

use super::AppState;

/// Size of a preview of the Editor's graph model or sheet, as the live simulation runs it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PreviewRun {
    /// A grid model.
    Grid {
        /// Width of the preview's grid in cells.
        width: u32,
        /// Height of the preview's grid in cells.
        height: u32,
        /// Whether the grid is smaller than the model's own.
        scaled: bool,
    },
    /// An agent model.
    Agents {
        /// Number of the preview's agents.
        count: u32,
        /// Whether the preview runs fewer agents than the model's own number.
        scaled: bool,
    },
}

impl PreviewRun {
    /// Returns the size as the app shows it, such as `256 × 256 cells` or `4096 agents`.
    pub fn size_text(self) -> String {
        match self {
            Self::Grid { width, height, .. } => format!("{width} × {height} cells"),
            Self::Agents { count: 1, .. } => "1 agent".to_owned(),
            Self::Agents { count, .. } => format!("{count} agents"),
        }
    }

    /// Returns whether the preview runs smaller than the model's own size. Apply then runs the model at its own size.
    pub fn is_scaled(self) -> bool {
        match self {
            Self::Grid { scaled, .. } | Self::Agents { scaled, .. } => scaled,
        }
    }
}

impl AppState {
    /// Tears down the live simulation and builds `setup` as the Editor's preview `preview`, which neither the
    /// selection nor the next build's fields follow.
    ///
    /// With `plays` the preview plays from tick 0 once it is built, and otherwise it waits at tick 0 for Play. A build
    /// that fails goes to the Editor. The model selected now is recorded for [`Self::build_replays_preview`].
    ///
    /// Note that a build can fail before this returns: inline in a browser, or natively when the build thread does
    /// not start. The fault then offloads the preview, which stays paused with nothing loaded.
    pub fn load_preview(&mut self, setup: RunSetup, preview: PreviewRun, plays: bool) {
        self.preview_selection.clone_from(&self.selected_model);
        self.load_setup(Some(setup), Some(preview));
        if plays && self.preview.is_some() {
            self.play();
        }
    }

    /// Preview of the Editor's graph model or sheet that the live simulation runs or builds, `None` for any other run.
    pub fn preview(&self) -> Option<PreviewRun> {
        self.preview
    }

    /// Returns whether the live simulation runs the Editor's sheet: as its preview, or as the model the Editor
    /// applied last. Playback reads a tick in the sheet's words then.
    pub fn runs_editor_sheet(&self) -> bool {
        self.previews_editor_sheet()
            || (self.editor.model.sheet().is_some()
                && self
                    .loaded
                    .as_ref()
                    .is_some_and(|loaded| self.editor.is_applied(loaded.source())))
    }

    /// Returns whether the live simulation runs the preview of the Editor's sheet.
    ///
    /// A preview of another document is not the sheet's. It runs until the sheet's own preview replaces it.
    pub fn previews_editor_sheet(&self) -> bool {
        let sheet_id = self
            .editor
            .model
            .sheet()
            .map(|sheet| sheet.document().sheet.id.as_str());
        self.preview.is_some() && sheet_id.is_some() && self.loaded.as_ref().map(ModelEntry::id) == sheet_id
    }

    /// Returns whether the preview of the Editor's sheet has left its start: it has passed tick 0, it runs, or a button
    /// of the model was pressed in it.
    ///
    /// Until then a sheet edit draws the start again at tick 0. After that, an edit starts the world again from tick 0
    /// with a notice, and a changed start waits for Start again.
    pub fn sheet_preview_past_start(&self) -> bool {
        self.previews_editor_sheet() && (self.editor.preview.acted || !self.run_at_start())
    }

    /// Returns whether Playback's Build builds the preview of the Editor's sheet again at the same seed: the preview
    /// runs, and the selection has not changed since it loaded.
    ///
    /// Once another model is selected, Build builds that model as it does behind any preview.
    pub fn build_replays_preview(&self) -> bool {
        self.previews_editor_sheet() && self.preview_selection == self.selected_model
    }

    /// Ends the hold of the running preview over the selected model, as a selection of another model does: Build, the
    /// Parameters tab and the Model tab follow the selected model again, while the preview runs on.
    ///
    /// Note that with no model selected the preview keeps its hold.
    pub fn release_preview_selection(&mut self) {
        self.preview_selection = None;
    }

    /// Returns whether the Parameters tab draws the preview's parameters and sends a change to it: a preview runs or
    /// builds, and the selection has not changed since it loaded.
    ///
    /// The selected model's values, seed and schedule stay as they are. Once another model is selected, the tab draws
    /// that model's parameters for the next build, as Build builds it.
    pub fn params_drive_preview(&self) -> bool {
        self.preview.is_some() && self.loaded.is_some() && self.preview_selection == self.selected_model
    }

    /// Sends a live edit of parameter `index` to the preview, and records `value` as a value the preview runs with.
    ///
    /// Returns whether the edit was sent. It is sent only while a preview runs or builds.
    pub fn send_preview_param(&mut self, index: usize, value: ParamValue) -> bool {
        if self.preview.is_none() || !self.has_simulation() {
            return false;
        }
        self.send_param(index, value);
        true
    }

    /// Number of builds started so far that were no preview. A preview that waits compares it, and gives way to a
    /// build started after its edit.
    pub fn builds(&self) -> u64 {
        self.builds
    }

    /// Returns whether the Editor's preview can replace the live simulation: nothing is loaded, the loaded run is a
    /// preview, or the loaded run has built and is at tick 0.
    pub fn preview_can_load(&self) -> bool {
        if self.preview.is_some() {
            return true;
        }
        match &self.loaded {
            None => !self.has_simulation(),
            Some(_) => !self.is_building() && self.pending_rebuild.is_none() && self.run_at_start(),
        }
    }
}
