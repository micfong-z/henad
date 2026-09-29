//! Results tab, holding the runs of the sweep or search started from the Sweep tab or of an opened folder, and the
//! views that plot them.

pub mod heatmap;
pub mod plot;
pub mod response;
pub mod search;
pub mod series;
pub mod store;
pub mod table;

use std::collections::BTreeSet;
use std::path::PathBuf;
use std::sync::Arc;

use egui::{Id, Modal};
use henad_core::explore::plan::Plan;
use henad_explore::handle::{DEFAULT_SERIES_BUDGET, SweepEvent};
use henad_explore::output::memory::SweepFiles;
use henad_explore::result_set::ResultSet;
use henad_explore::search_run::{SearchPlan, SearchUpdate};
use henad_explore::sweep::SweepEnd;
use henad_models::registry::ModelEntry;

use crate::icons::material_design_icons::{MDI_FOLDER_OPEN_OUTLINE, MDI_PLAY};
use crate::state::{AppState, OpenAt};
use crate::ui::dock::Tab;
use crate::ui::files::{DialogFile, OpenResult, OpenTarget};
use crate::ui::plural;
use crate::ui::results::heatmap::HeatmapView;
use crate::ui::results::response::ResponseView;
use crate::ui::results::search::SearchView;
use crate::ui::results::series::SeriesView;
use crate::ui::results::store::{ResultsSource, ResultsStore, RunsFilter};
use crate::ui::results::table::TableView;
use crate::ui::sweep::draft::describe_error;
use crate::ui::sweep::session::SweepSession;

/// Configs a new store selects for the Series view.
const DEFAULT_SELECTED_CONFIGS: usize = 5;

/// Status line shown while picked files wait to be read.
const READING_RESULTS: &str = "Reading results";

/// View the Results tab shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ResultsView {
    /// Course of a search, offered for the results of a search alone.
    Search,
    #[default]
    Series,
    Response,
    Heatmap,
    Runs,
}

impl ResultsView {
    fn label(self) -> &'static str {
        match self {
            Self::Search => "Search",
            Self::Series => "Series",
            Self::Response => "Response",
            Self::Heatmap => "Heatmap",
            Self::Runs => "Runs",
        }
    }
}

/// Button press or selection the tab applies once it has drawn.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResultsRequest {
    /// Opens the dialog that picks results, asking first when unsaved results would be lost.
    OpenResults,
    /// Opens the dialog that picks results without asking.
    OpenResultsAnyway,
    ResumeSweep,
    SelectRun(u64),
    /// Selects these configs and lists their runs.
    FilterConfigs(Vec<u64>),
    ClearSelection,
    OpenRun {
        run_id: u64,
        start: OpenAt,
    },
    CopyCommand(u64),
    /// Reads the series of the drawn configs' runs from the folder.
    LoadSeries,
}

/// Series read from a folder on a thread of its own.
#[cfg(not(target_arch = "wasm32"))]
type SeriesRead = Result<henad_explore::result_set::DirectorySeries, String>;

/// Series a thread reads from the folder, for the runs the Series view draws.
#[cfg(not(target_arch = "wasm32"))]
struct SeriesLoad {
    /// Runs whose series stay held beside the ones read.
    kept_runs: BTreeSet<u64>,
    /// Runs whose series the thread reads.
    requested_runs: BTreeSet<u64>,
    receiver: flume::Receiver<SeriesRead>,
}

/// Files picked in a dialog, read on a frame after the one that first shows [`READING_RESULTS`].
struct PickedFiles {
    files: Vec<DialogFile>,
    /// Number of the frame that first saw the files, `None` before any frame has.
    first_frame: Option<u64>,
}

/// State of the Results tab.
#[derive(Default)]
pub struct ResultsPanel {
    store: Option<ResultsStore>,
    view: ResultsView,
    /// Configs the Series view draws, and the runs table can list alone.
    selected_configs: BTreeSet<u64>,
    /// Run the detail strip shows.
    selected_run: Option<u64>,
    series: SeriesView,
    response: ResponseView,
    heatmap: HeatmapView,
    table: TableView,
    search: SearchView,
    /// Files of a sweep held in memory, once it ends.
    files: Option<Arc<SweepFiles>>,
    /// Number of the files held, counted up each time a sweep hands the panel its files. A save names the files it
    /// wrote by this number.
    files_generation: u64,
    /// Whether `files` are saved.
    files_saved: bool,
    /// Line reporting the last open, load or copy.
    pub status: Option<String>,
    /// Results a thread reads from a folder, with the folder.
    #[cfg(not(target_arch = "wasm32"))]
    folder_load: Option<(PathBuf, flume::Receiver<Result<ResultSet, String>>)>,
    /// Series a thread reads from the folder.
    #[cfg(not(target_arch = "wasm32"))]
    series_load: Option<SeriesLoad>,
    /// Files picked in a dialog and not read yet.
    picked_files: Option<PickedFiles>,
    /// Folder to read again once the sweep that resumed it ends.
    reload_folder: Option<PathBuf>,
    /// Whether the modal asking to replace results held in memory is open.
    confirm_open: bool,
}

impl ResultsPanel {
    /// Clears the results for the sweep of `plan` that is about to start, over `entry`, the model at `model_index`
    /// in the registry, whose files go to `folder`.
    pub fn begin_sweep(&mut self, plan: Arc<Plan>, entry: &ModelEntry, model_index: usize, folder: Option<PathBuf>) {
        let store = ResultsStore::for_sweep(plan, entry, model_index, folder, DEFAULT_SERIES_BUDGET);
        self.set_store(store);
    }

    /// Clears the results for the search of `search_plan` that is about to start, over `entry`, the model at
    /// `model_index` in the registry, whose files go to `folder`.
    pub fn begin_search(
        &mut self,
        search_plan: Arc<SearchPlan>,
        entry: &ModelEntry,
        model_index: usize,
        folder: Option<PathBuf>,
    ) {
        let store = ResultsStore::for_search(search_plan, entry, model_index, folder, DEFAULT_SERIES_BUDGET);
        self.set_store(store);
    }

    /// Shows `store`, with the first configs selected and every view reset.
    ///
    /// A folder or series still being read is dropped and never lands in `store`.
    fn set_store(&mut self, store: ResultsStore) {
        self.drop_pending_loads();
        self.selected_configs = store.config_ids().take(DEFAULT_SELECTED_CONFIGS).collect();
        self.selected_run = None;
        self.series = SeriesView::default();
        self.response = ResponseView::default();
        self.heatmap = HeatmapView::default();
        self.table = TableView::default();
        self.search = SearchView::default();
        self.view = if store.is_search() {
            ResultsView::Search
        } else {
            match self.view {
                ResultsView::Search => ResultsView::default(),
                view => view,
            }
        };
        self.files = None;
        self.files_saved = false;
        self.status = None;
        self.reload_folder = None;
        self.store = Some(store);
    }

    /// Takes the events of the running sweep that arrived since the last frame, in the order they arrived.
    ///
    /// The configs of the search batches among `events` join the store together, after the other events.
    pub fn ingest(&mut self, events: impl IntoIterator<Item = SweepEvent>) {
        let Some(store) = &mut self.store else {
            return;
        };
        let mut updates: Vec<Arc<SearchUpdate>> = Vec::new();
        for event in events {
            match event {
                SweepEvent::Planned(outline) => store.set_columns(&outline.stat_columns, &outline.reducer_columns),
                SweepEvent::RunFinished {
                    outcome,
                    series_dropped,
                } => store.push_run(*outcome, series_dropped),
                SweepEvent::Finished(record) => {
                    let record = *record;
                    store.set_complete(record.report.end == SweepEnd::Complete);
                    self.files = record.files.map(Arc::new);
                    self.files_generation += 1;
                    self.files_saved = false;
                    self.reload_folder = resumed_folder(store);
                }
                SweepEvent::Failed(_) => self.reload_folder = resumed_folder(store),
                SweepEvent::SearchBatchTold(update) => updates.push(update),
                SweepEvent::Warned(_) => {}
            }
        }
        let mut pending = 0;
        for (index, update) in updates.iter().enumerate() {
            // A search's first candidates arrive with its first batch, after the store was set. The selection reads
            // the store as that batch leaves it.
            if self.selected_configs.is_empty() && update.batch == 0 {
                store.push_search_updates(updates[pending..=index].iter().map(Arc::as_ref));
                self.selected_configs = store.config_ids().take(DEFAULT_SELECTED_CONFIGS).collect();
                pending = index + 1;
            }
        }
        if pending < updates.len() {
            store.push_search_updates(updates[pending..].iter().map(Arc::as_ref));
        }
    }

    /// Files of a sweep held in memory, `None` for a sweep written to a folder or one still running.
    pub fn files(&self) -> Option<&Arc<SweepFiles>> {
        self.files.as_ref()
    }

    /// Returns whether the panel holds files of a sweep that exist nowhere else.
    pub fn holds_unsaved_files(&self) -> bool {
        self.files.is_some() && !self.files_saved
    }

    /// Returns whether the panel holds the results of a search.
    pub fn holds_search(&self) -> bool {
        self.store.as_ref().is_some_and(ResultsStore::is_search)
    }

    /// Results the panel shows, `None` before a sweep starts or a folder opens.
    pub fn store(&self) -> Option<&ResultsStore> {
        self.store.as_ref()
    }

    /// Lists the failed runs in the Runs view.
    pub fn show_failed_runs(&mut self) {
        self.view = ResultsView::Runs;
        self.table.filter = RunsFilter::Failed;
    }

    /// Selects the first run of search candidate `candidate_id`, which opens the run strip. A candidate the panel
    /// holds no run of changes nothing.
    pub fn select_candidate(&mut self, candidate_id: u64) {
        let first_run = self
            .store
            .as_ref()
            .and_then(|store| store.config_runs(candidate_id).next())
            .map(|outcome| outcome.run.run_id);
        if first_run.is_some() {
            self.selected_run = first_run;
        }
    }

    /// Number of the files held, for a save to name them by.
    pub fn files_generation(&self) -> u64 {
        self.files_generation
    }

    /// Records that the files of number `generation` are saved. Files the panel no longer holds change nothing.
    pub fn mark_files_saved(&mut self, generation: u64) {
        if generation == self.files_generation {
            self.files_saved = true;
        }
    }

    /// Drops the folder, files and series being read, so a read that finishes later goes nowhere.
    fn drop_pending_loads(&mut self) {
        self.picked_files = None;
        #[cfg(not(target_arch = "wasm32"))]
        {
            self.folder_load = None;
            self.series_load = None;
        }
    }

    /// Shows the results `set`, read from `source`, replaying through the models of `registry`.
    fn show_result_set(&mut self, set: ResultSet, source: ResultsSource, registry: &[ModelEntry]) {
        let search = SearchView::for_result_set(&set);
        let store = ResultsStore::from_result_set(set, source, registry, DEFAULT_SERIES_BUDGET);
        self.set_store(store);
        self.search = search;
    }

    /// Holds the picked `files` to read on a later frame, and reports the wait on the status line.
    fn hold_picked_files(&mut self, files: Vec<DialogFile>) {
        self.status = Some(READING_RESULTS.to_owned());
        self.picked_files = Some(PickedFiles {
            files,
            first_frame: None,
        });
    }

    /// Returns the picked files once the current frame, `frame`, comes after the first frame that saw them.
    ///
    /// The first frame draws [`READING_RESULTS`] before the read holds up the next.
    fn due_picked_files(&mut self, frame: u64) -> Option<Vec<DialogFile>> {
        let picked = self.picked_files.as_mut()?;
        if *picked.first_frame.get_or_insert(frame) == frame {
            return None;
        }
        self.picked_files.take().map(|picked| picked.files)
    }
}

/// Returns the folder `store` was opened from, `None` for a sweep or picked files.
fn resumed_folder(store: &ResultsStore) -> Option<PathBuf> {
    match &store.source {
        ResultsSource::Folder(folder) => Some(folder.clone()),
        ResultsSource::Sweep | ResultsSource::Files(_) => None,
    }
}

/// Takes the result of an Open results dialog.
pub fn receive_open(app: &mut AppState, result: OpenResult) {
    match result {
        #[cfg(not(target_arch = "wasm32"))]
        OpenResult::Folder(folder) => open_folder(app, folder),
        #[cfg(target_arch = "wasm32")]
        OpenResult::Folder(_) => app.results.status = Some("Use the desktop app to open folders".to_owned()),
        OpenResult::Files(files) => app.results.hold_picked_files(files),
        OpenResult::Failed(message) => app.results.status = Some(format!("Open failed: {message}")),
        OpenResult::Canceled => {}
    }
}

/// Reads the results in `folder` on a thread of its own, and shows them once read.
#[cfg(not(target_arch = "wasm32"))]
pub fn open_folder(app: &mut AppState, folder: PathBuf) {
    let (sender, receiver) = flume::bounded(1);
    let wake = app.repaint_waker();
    let path = folder.clone();
    let spawned = std::thread::Builder::new()
        .name("henad-results".to_owned())
        .spawn(move || {
            let read = ResultSet::open_dir(&path, DEFAULT_SERIES_BUDGET).map_err(|error| describe_error(&error));
            // The receiver is gone once the app is closing, and nothing is left to report to.
            drop(sender.send(read));
            wake();
        });
    match spawned {
        Ok(_) => {
            app.results.status = Some(format!("Opening {}", folder.display()));
            app.results.folder_load = Some((folder, receiver));
        }
        Err(error) => app.results.status = Some(format!("Open failed: {error}")),
    }
}

/// Reads the picked files `files` of one folder, and shows them.
fn read_files(app: &mut AppState, files: Vec<DialogFile>) {
    let names: Vec<String> = files.iter().map(|file| file.name.clone()).collect();
    let file_bytes = files.into_iter().map(|file| (file.name, file.bytes)).collect();
    match ResultSet::from_files(file_bytes, DEFAULT_SERIES_BUDGET) {
        Ok(set) => show_results(app, set, ResultsSource::Files(names)),
        Err(error) => app.results.status = Some(format!("Open failed: {}", describe_error(&error))),
    }
}

/// Shows `set`, read from `source`, and brings the Results tab to the front.
fn show_results(app: &mut AppState, set: ResultSet, source: ResultsSource) {
    app.results.show_result_set(set, source, &app.registry);
    app.focus_request = Some(Tab::Results);
}

/// Takes the results and series the tab has read, reads picked files once `ctx` has drawn a frame since they
/// arrived, and reads a resumed folder again once its sweep ends.
pub fn poll(ctx: &egui::Context, app: &mut AppState) {
    poll_picked_files(ctx, app);
    #[cfg(not(target_arch = "wasm32"))]
    {
        poll_folder_load(app);
        poll_series_load(&mut app.results);
        if !app.sweep.is_running()
            && let Some(folder) = app.results.reload_folder.take()
        {
            open_folder(app, folder);
        }
    }
    #[cfg(target_arch = "wasm32")]
    {
        app.results.reload_folder = None;
    }
}

/// Reads the picked files on the frame after the first one that saw them, and repaints until then.
fn poll_picked_files(ctx: &egui::Context, app: &mut AppState) {
    let Some(files) = app.results.due_picked_files(ctx.cumulative_frame_nr()) else {
        if app.results.picked_files.is_some() {
            ctx.request_repaint();
        }
        return;
    };
    // The sweep's events go to the store it started with.
    if app.sweep.is_running() {
        app.results.status = None;
        return;
    }
    read_files(app, files);
}

#[cfg(not(target_arch = "wasm32"))]
fn poll_folder_load(app: &mut AppState) {
    let Some((_, receiver)) = &app.results.folder_load else {
        return;
    };
    let read = match receiver.try_recv() {
        Ok(read) => read,
        Err(flume::TryRecvError::Empty) => return,
        Err(flume::TryRecvError::Disconnected) => Err("Reading ended unexpectedly".to_owned()),
    };
    let Some((folder, _)) = app.results.folder_load.take() else {
        return;
    };
    // The sweep's events go to the store it started with.
    if app.sweep.is_running() {
        app.results.status = None;
        return;
    }
    match read {
        Ok(set) => show_results(app, set, ResultsSource::Folder(folder)),
        Err(message) => app.results.status = Some(format!("Open failed: {message}")),
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn poll_series_load(panel: &mut ResultsPanel) {
    let Some(load) = &panel.series_load else {
        return;
    };
    let read = match load.receiver.try_recv() {
        Ok(read) => read,
        Err(flume::TryRecvError::Empty) => return,
        Err(flume::TryRecvError::Disconnected) => Err("Reading ended unexpectedly".to_owned()),
    };
    let Some(load) = panel.series_load.take() else {
        return;
    };
    panel.status = Some(match (read, &mut panel.store) {
        (Ok(read), Some(store)) => {
            let empty_runs: BTreeSet<u64> = load
                .requested_runs
                .iter()
                .copied()
                .filter(|run_id| !read.series.contains_key(run_id) && !read.dropped_runs.contains(run_id))
                .collect();
            store.record_empty_series(&empty_runs);
            let held = store.insert_series(read.series, &load.kept_runs) as u64;
            series_load_text(load.requested_runs.len() as u64, held, empty_runs.len() as u64)
        }
        (Ok(_), None) => return,
        (Err(message), _) => format!("Series load failed: {message}"),
    });
}

/// Returns the status line of a series load that asked for `requested` runs, holds the series of `held`, and found
/// no rows for `empty`.
///
/// The memory budget accounts for the rest.
#[cfg(not(target_arch = "wasm32"))]
fn series_load_text(requested: u64, held: u64, empty: u64) -> String {
    if held == requested {
        return format!("Loaded series for {held} {}", plural(held, "run"));
    }
    let mut text = format!("Loaded series for {held} of {requested} {}.", plural(requested, "run"));
    if empty > 0 {
        text.push_str(&format!(" {empty} recorded no series."));
    }
    let over_budget = requested.saturating_sub(held + empty);
    if over_budget > 0 {
        text.push_str(&format!(" Memory budget excludes {over_budget}."));
    }
    text
}

pub fn results_ui(ui: &mut egui::Ui, app: &mut AppState) {
    let mut request = None;
    toolbar(ui, app, &mut request);
    let sweep_running = app.sweep.is_running();
    let ResultsPanel {
        store,
        view,
        selected_configs,
        selected_run,
        series,
        response,
        heatmap,
        table,
        search,
        ..
    } = &mut app.results;
    let Some(store) = store.as_ref() else {
        ui.label("No results. Run a sweep in the Sweep tab, or press Open results.");
        apply_request(ui.ctx(), app, request);
        return;
    };
    summary_line(ui, store);
    ui.horizontal(|ui| {
        let views = [
            ResultsView::Search,
            ResultsView::Series,
            ResultsView::Response,
            ResultsView::Heatmap,
            ResultsView::Runs,
        ];
        for choice in views {
            if choice != ResultsView::Search || store.is_search() {
                ui.selectable_value(view, choice, choice.label());
            }
        }
    });
    ui.separator();
    // At the bottom of the tab. Above the view, the strip's first appearance pushes the rows under the pointer down.
    if let Some(run_id) = *selected_run {
        egui::Panel::bottom("henad_results_run_strip")
            .frame(egui::Frame::NONE.inner_margin(egui::Margin::symmetric(0, 4)))
            .show(ui, |ui| table::detail_strip(ui, store, run_id, &mut request));
    }
    match view {
        ResultsView::Search => search::search_ui(ui, store, search, sweep_running, &mut request),
        ResultsView::Series => series::series_ui(ui, store, series, selected_configs, &mut request),
        ResultsView::Response => response::response_ui(ui, store, response),
        ResultsView::Heatmap => heatmap::heatmap_ui(ui, store, heatmap, &mut request),
        ResultsView::Runs => table::table_ui(ui, store, table, selected_configs, *selected_run, &mut request),
    }
    apply_request(ui.ctx(), app, request);
}

/// Draws the source of the results, Open results and Resume sweep, and the status line.
fn toolbar(ui: &mut egui::Ui, app: &mut AppState, request: &mut Option<ResultsRequest>) {
    let sweep_running = app.sweep.is_running();
    let running_noun = app.sweep.session.as_ref().map_or("sweep", SweepSession::noun);
    let running_hover = format!("A {running_noun} is running");
    let panel = &app.results;
    ui.horizontal_wrapped(|ui| {
        if let Some(store) = &panel.store {
            source_label(ui, &store.source, store.is_search());
        }
        let open_tooltip = if cfg!(target_arch = "wasm32") {
            "Select manifest.json and CSV files from a results folder"
        } else {
            "Open results folder"
        };
        let open = ui
            .add_enabled(
                !sweep_running,
                egui::Button::new(format!("{MDI_FOLDER_OPEN_OUTLINE} Open results")),
            )
            .on_hover_text(open_tooltip)
            .on_disabled_hover_text(&running_hover);
        if open.clicked() {
            *request = Some(ResultsRequest::OpenResults);
        }
        let resumable = panel.store.as_ref().is_some_and(|store| {
            cfg!(not(target_arch = "wasm32"))
                && matches!(store.source, ResultsSource::Folder(_))
                && !store.complete
                && store.model_index.is_some()
        });
        if resumable {
            let noun = if panel.store.as_ref().is_some_and(ResultsStore::is_search) {
                "search"
            } else {
                "sweep"
            };
            let resume = ui
                .add_enabled(!sweep_running, egui::Button::new(format!("{MDI_PLAY} Resume {noun}")))
                .on_hover_text("Run missing runs")
                .on_disabled_hover_text(&running_hover);
            if resume.clicked() {
                *request = Some(ResultsRequest::ResumeSweep);
            }
        }
    });
    if let Some(status) = &panel.status {
        ui.label(status);
    }
    if app.results.confirm_open {
        open_modal(ui.ctx(), &mut app.results.confirm_open, request);
    }
}

/// Draws the source of the results, the search or sweep in memory or the files read.
fn source_label(ui: &mut egui::Ui, source: &ResultsSource, is_search: bool) {
    match source {
        ResultsSource::Sweep if is_search => {
            ui.strong("Current search");
        }
        ResultsSource::Sweep => {
            ui.strong("Current sweep");
        }
        ResultsSource::Folder(folder) => {
            let name = folder.file_name().map_or_else(
                || folder.display().to_string(),
                |name| name.to_string_lossy().into_owned(),
            );
            ui.strong(name).on_hover_text(folder.display().to_string());
        }
        ResultsSource::Files(names) => {
            ui.strong(names.join(", "));
        }
    }
}

/// Draws the model and the count of runs, and whether the sweep is missing runs.
fn summary_line(ui: &mut egui::Ui, store: &ResultsStore) {
    ui.horizontal_wrapped(|ui| {
        let runs = store.runs().len() as u64;
        ui.label(format!(
            "{}: {runs} {}, {} failed",
            store.model_name,
            plural(runs, "run"),
            store.failed_count()
        ));
        if !store.complete && !matches!(store.source, ResultsSource::Sweep) {
            ui.weak("Incomplete");
        }
    });
}

/// Asks before an open drops results held in memory that are not saved.
fn open_modal(ctx: &egui::Context, open: &mut bool, request: &mut Option<ResultsRequest>) {
    let mut closed = false;
    let response = Modal::new(Id::new("henad_open_results_modal")).show(ctx, |ui| {
        ui.set_max_width(420.0);
        ui.heading("Replace results?");
        ui.label("Opening results will clear the unsaved results in memory.");
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            if ui.button("Open anyway").clicked() {
                *request = Some(ResultsRequest::OpenResultsAnyway);
                closed = true;
            }
            if ui.button("Cancel").clicked() {
                closed = true;
            }
        });
    });
    if closed || response.should_close() {
        *open = false;
    }
}

fn apply_request(ctx: &egui::Context, app: &mut AppState, request: Option<ResultsRequest>) {
    let Some(request) = request else {
        return;
    };
    let panel = &mut app.results;
    match request {
        ResultsRequest::OpenResults if panel.holds_unsaved_files() => panel.confirm_open = true,
        ResultsRequest::OpenResults | ResultsRequest::OpenResultsAnyway => app.open_file(OpenTarget::Results),
        ResultsRequest::ResumeSweep => resume_sweep(app),
        ResultsRequest::SelectRun(run_id) => panel.selected_run = Some(run_id),
        ResultsRequest::FilterConfigs(configs) => {
            panel.selected_configs = configs.into_iter().collect();
            panel.table.filter = RunsFilter::SelectedConfigs;
            panel.view = ResultsView::Runs;
        }
        ResultsRequest::ClearSelection => panel.selected_configs.clear(),
        ResultsRequest::OpenRun { run_id, start } => {
            let opened = panel.store.as_ref().map(|store| store.replay(run_id));
            match opened {
                Some(Ok(replay)) => {
                    if let Err(message) = app.open_run(replay, start) {
                        app.results.status = Some(message);
                    }
                }
                Some(Err(message)) => panel.status = Some(message),
                None => {}
            }
        }
        ResultsRequest::CopyCommand(run_id) => {
            let command = panel.store.as_ref().map(|store| store.cli_command(run_id));
            match command {
                Some(Ok(command)) => {
                    ctx.copy_text(command);
                    panel.status = Some(format!("Copied command for run {run_id}"));
                }
                Some(Err(message)) => panel.status = Some(message),
                None => {}
            }
        }
        ResultsRequest::LoadSeries => load_series(app),
    }
}

/// Resumes the sweep of the opened folder, and brings the Sweep tab to the front to show its progress.
#[cfg(not(target_arch = "wasm32"))]
fn resume_sweep(app: &mut AppState) {
    let Some(store) = &app.results.store else {
        return;
    };
    let (Some(folder), model_id) = (store.folder().map(std::path::Path::to_path_buf), store.model_id.clone()) else {
        return;
    };
    match SweepSession::resume_folder(app, folder, &model_id) {
        Ok(session) => {
            app.results.drop_pending_loads();
            app.sweep.session = Some(session);
            app.sweep.status = None;
            app.sweep.start_failure = None;
            app.results.status = None;
            app.focus_request = Some(Tab::Sweep);
        }
        Err(message) => app.results.status = Some(format!("Resume failed: {message}")),
    }
}

#[cfg(target_arch = "wasm32")]
fn resume_sweep(app: &mut AppState) {
    let noun = if app.results.store.as_ref().is_some_and(ResultsStore::is_search) {
        "search"
    } else {
        "sweep"
    };
    app.results.status = Some(format!("Use the desktop app to resume a {noun}"));
}

/// Reads the series the drawn configs' runs lack from the folder, on a thread of its own.
#[cfg(not(target_arch = "wasm32"))]
fn load_series(app: &mut AppState) {
    let wake = app.repaint_waker();
    let panel = &mut app.results;
    let Some(store) = &panel.store else {
        return;
    };
    let Some(folder) = store.folder().map(std::path::Path::to_path_buf) else {
        return;
    };
    let drawn: BTreeSet<u64> = panel
        .selected_configs
        .iter()
        .take(series::MAX_DRAWN_CONFIGS)
        .copied()
        .collect();
    let missing = store.runs_without_series(&drawn);
    let kept_runs: BTreeSet<u64> = drawn
        .iter()
        .flat_map(|&config_id| store.config_runs(config_id))
        .map(|outcome| outcome.run.run_id)
        .collect();
    let stat_columns = store.stat_columns().to_vec();
    let series_budget = store.series_room(&kept_runs);
    let requested_runs = missing.clone();
    let (sender, receiver) = flume::bounded(1);
    let spawned = std::thread::Builder::new()
        .name("henad-series".to_owned())
        .spawn(move || {
            let read =
                henad_explore::result_set::read_directory_series(&folder, &stat_columns, &missing, series_budget)
                    .map_err(|error| describe_error(&error));
            drop(sender.send(read));
            wake();
        });
    match spawned {
        Ok(_) => {
            panel.status = Some("Loading series".to_owned());
            panel.series_load = Some(SeriesLoad {
                kept_runs,
                requested_runs,
                receiver,
            });
        }
        Err(error) => panel.status = Some(format!("Series load failed: {error}")),
    }
}

#[cfg(target_arch = "wasm32")]
fn load_series(app: &mut AppState) {
    app.results.status = Some("Use the desktop app to load series".to_owned());
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use std::collections::{BTreeMap, BTreeSet};
    use std::sync::Arc;

    use henad_core::explore::design::DesignKind;
    use henad_core::explore::factor::{FactorSpec, LevelSpec};
    use henad_core::explore::measure::SeriesBuffer;
    use henad_core::explore::outcome::{RunOutcome, RunStatus, StopReason};
    use henad_core::explore::plan::Plan;
    use henad_core::explore::reducer::{ReducerKind, ReducerSpec};
    use henad_core::explore::search::genetic::GeneticSettings;
    use henad_core::explore::search::pse::{PatternAxis, PatternSpaceSettings};
    use henad_core::explore::search::{Aggregate, Goal, Objective, SearchAlgorithm, SearchSpec};
    use henad_core::explore::spec::{BlockSpec, SweepSpec};
    use henad_explore::handle::{SweepEvent, SweepOutput, SweepRun, SweepRunOptions};
    use henad_explore::output::GENERATIONS_FILE;
    use henad_explore::result_set::{DirectorySeries, ResultSet};
    use henad_explore::schema::model_schema;
    use henad_models::registry::{ModelEntry, model_registry};

    use super::{READING_RESULTS, ResultsPanel, ResultsView, SeriesLoad, poll_series_load};
    use crate::ui::files::DialogFile;
    use crate::ui::results::store::{ResultsSource, ResultsStore};

    /// Returns a series of one sample whose three stats are all `value`.
    fn series(value: f64) -> SeriesBuffer {
        let mut series = SeriesBuffer::new(3);
        series.push(0, &[value; 3]);
        series
    }

    fn sir() -> ModelEntry {
        model_registry(None)
            .into_iter()
            .find(|entry| entry.id == "sir")
            .expect("SIR is registered")
    }

    /// Returns the plan of SIR over two infection rates with `replicates` runs each.
    fn plan(sir: &ModelEntry, replicates: u64) -> Arc<Plan> {
        let mut spec = SweepSpec::new("sir");
        spec.run.steps = 10;
        spec.run.replicates = replicates;
        spec.blocks = vec![BlockSpec {
            design: DesignKind::Factorial,
            factors: vec![FactorSpec::param(
                "infection_rate",
                LevelSpec::Values(vec!["0.1".to_owned(), "0.2".to_owned()]),
            )],
            design_seed: None,
        }];
        Arc::new(spec.plan(&model_schema(sir)).expect("a valid spec"))
    }

    /// Returns run `run_id` of `plan` with `series`.
    fn outcome(plan: &Plan, run_id: u64, series: SeriesBuffer) -> RunOutcome {
        let run = plan.run(run_id).expect("a planned run");
        RunOutcome {
            run,
            run_key: plan.run_key(&run),
            status: RunStatus::Ok,
            stop_reason: StopReason::Steps,
            ticks: 0,
            population: 1,
            build_ms: 1.0,
            wall_ms: 1.0,
            reducers: Vec::new(),
            series,
            note: None,
        }
    }

    #[test]
    fn a_series_load_pending_when_a_sweep_starts_never_reaches_it() {
        let sir = sir();
        let plan = plan(&sir, 1);

        let mut panel = ResultsPanel::default();
        panel.begin_sweep(Arc::clone(&plan), &sir, 0, None);
        let (sender, receiver) = flume::bounded(1);
        panel.series_load = Some(SeriesLoad {
            kept_runs: BTreeSet::new(),
            requested_runs: [0].into(),
            receiver,
        });

        panel.begin_sweep(Arc::clone(&plan), &sir, 0, None);
        let store = panel.store.as_mut().expect("the sweep's store");
        store.push_run(outcome(&plan, 0, series(1.0)), false);

        let stale = DirectorySeries {
            series: BTreeMap::from([(0, series(9.0))]),
            dropped_runs: BTreeSet::new(),
        };
        assert!(sender.send(Ok(stale)).is_err(), "the load's receiver is dropped");
        poll_series_load(&mut panel);
        let store = panel.store.as_ref().expect("the sweep's store");
        assert_eq!(store.series(0), Some(&series(1.0)), "run 0 keeps the sweep's series");
        assert_eq!(panel.status, None);
    }

    /// Removes the folder it names once dropped, so a failed test leaves nothing behind.
    struct ScratchFolder(std::path::PathBuf);

    impl Drop for ScratchFolder {
        fn drop(&mut self) {
            drop(std::fs::remove_dir_all(&self.0));
        }
    }

    /// Returns a search of SIR by `algorithm` over the infection rate, 6 evaluations of 2 replicates.
    fn search_spec(algorithm: SearchAlgorithm) -> SweepSpec {
        let mut spec = SweepSpec::new("sir");
        spec.fixed = [("grid_width", "16"), ("grid_height", "16")]
            .map(|(id, value)| (id.to_owned(), value.to_owned()))
            .to_vec();
        spec.run.steps = 30;
        spec.run.replicates = 2;
        spec.measure.stats_every = 5;
        spec.measure.series_every = 5;
        spec.measure.reducers = vec![ReducerSpec {
            column: "Infected".to_owned(),
            kind: ReducerKind::ArgMax,
        }];
        spec.seeds.root = 5;
        let has_objective = !matches!(algorithm, SearchAlgorithm::PatternSpaceExploration(_));
        spec.search = Some(SearchSpec {
            algorithm,
            max_evaluations: 6,
            batch_size: 3,
            objective: has_objective.then(|| Objective {
                column: "Infected:max".to_owned(),
                goal: Goal::Maximize,
                aggregate: Aggregate::Median,
            }),
            space: vec![FactorSpec::param(
                "infection_rate",
                LevelSpec::Range {
                    min: 0.05,
                    max: 0.9,
                    step: None,
                },
            )],
        });
        spec
    }

    /// Returns the searches whose live and opened stores are compared, each with its name.
    fn compared_searches() -> [(&'static str, SearchAlgorithm); 3] {
        let axis = |column: &str, max, cells| PatternAxis::bounded(column, 0.0, max, cells);
        let pattern = PatternSpaceSettings {
            initial_samples: 3,
            ..PatternSpaceSettings::new(axis("Infected:max", 256.0, 8), axis("Infected:argmax", 30.0, 6))
        };
        let automatic = PatternSpaceSettings {
            initial_samples: 3,
            ..PatternSpaceSettings::new(
                PatternAxis::automatic("Infected:max", 8),
                PatternAxis::automatic("Infected:argmax", 6),
            )
        };
        [
            ("random", SearchAlgorithm::Random),
            ("pse", SearchAlgorithm::PatternSpaceExploration(pattern)),
            ("pse-automatic", SearchAlgorithm::PatternSpaceExploration(automatic)),
        ]
    }

    #[test]
    fn a_search_held_live_matches_its_folder_and_replays_from_either() {
        let registry = model_registry(None);
        for (name, algorithm) in compared_searches() {
            let folder =
                ScratchFolder(std::env::temp_dir().join(format!("henad-app-search-{name}-{}", std::process::id())));
            drop(std::fs::remove_dir_all(&folder.0));
            let output = SweepOutput::Directory(folder.0.clone());
            let mut run = SweepRun::start(sir(), None, search_spec(algorithm), output, SweepRunOptions::default())
                .expect("the search starts");
            let search_plan = Arc::clone(run.search_plan().expect("the spec is a search"));
            let mut panel = ResultsPanel::default();
            panel.begin_search(search_plan, &sir(), 0, Some(folder.0.clone()));
            assert_eq!(
                panel.view,
                ResultsView::Search,
                "{name}: a search opens on its own view"
            );
            let started = std::time::Instant::now();
            while !run.is_ended() {
                let events: Vec<SweepEvent> = std::iter::from_fn(|| run.try_recv()).collect();
                if let Some(SweepEvent::Failed(message)) = events.last() {
                    panic!("{name}: the search failed: {message}");
                }
                panel.ingest(events);
                assert!(started.elapsed().as_secs() < 60, "{name}: the search ends");
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
            let live = panel.store.as_ref().expect("the search's store");

            let set = ResultSet::open_dir(&folder.0, usize::MAX).expect("the folder reads");
            let command = first_run_command(&set);
            let opened =
                ResultsStore::from_result_set(set, ResultsSource::Folder(folder.0.clone()), &registry, 1 << 20);
            for store in [live, &opened] {
                assert!(store.is_search(), "{name}");
                assert_eq!(store.runs().len(), 12, "{name}: every run of every evaluation");
                assert_eq!(store.config_count(), 6, "{name}: one config per candidate");
                assert!(
                    (0..6).all(|candidate_id| store.config_runs(candidate_id).count() == 2),
                    "{name}: each candidate holds its replicates"
                );
                assert_eq!(
                    store.axes().len(),
                    1,
                    "{name}: the infection rate varies across candidates"
                );
            }
            let (live_log, opened_log) = (
                live.search_log().expect("a search log"),
                opened.search_log().expect("a search log"),
            );
            assert_eq!(opened_log.table_error, None, "{name}");
            assert_eq!(live_log.history.batches, opened_log.history.batches, "{name}: batches");
            assert_eq!(live_log.history.archive, opened_log.history.archive, "{name}: archive");
            assert_eq!(
                live_log.history.pattern_settings, opened_log.history.pattern_settings,
                "{name}: the range of each axis"
            );
            assert_eq!(
                live_log.history.outside_count, opened_log.history.outside_count,
                "{name}: outside"
            );
            assert_eq!(live_log.history.batches.len(), 2, "{name}: two batches of three");
            if let SearchAlgorithm::PatternSpaceExploration(_) = live_log.spec.algorithm {
                assert!(!live_log.history.archive.is_empty(), "{name}: the grid fills");
                assert!(
                    live_log
                        .history
                        .pattern_settings
                        .as_ref()
                        .is_some_and(PatternSpaceSettings::has_ranges),
                    "{name}: each axis has its range"
                );
            }
            for outcome in live.runs() {
                let run_id = outcome.run.run_id;
                let replay = live.replay(run_id).expect("a live run replays");
                assert_eq!(replay.seed, outcome.run.seed, "{name}: run {run_id}");
                assert_eq!(
                    Ok(&replay),
                    opened.replay(run_id).as_ref(),
                    "{name}: run {run_id} replays the same from its folder"
                );
                assert!(live.cli_command(run_id).is_ok(), "{name}: run {run_id}");
            }
            for store in [live, &opened] {
                assert_eq!(
                    store.cli_command(0).as_ref(),
                    Ok(&command),
                    "{name}: the command sets the fixed values and candidate 0's rate"
                );
            }
        }
    }

    /// Returns the command that replays run 0 of the search of [`search_spec`] in `set`.
    ///
    /// The seed and the rate come from `runs.csv`. The command decodes the rate from the candidate, as a replay does.
    fn first_run_command(set: &ResultSet) -> String {
        let recorded = set.run(0).expect("run 0 is recorded");
        let rate_column = set
            .value_columns()
            .iter()
            .position(|column| column == "infection_rate")
            .expect("the infection rate is a value column");
        format!(
            "henad-cli sir --seed {} --set grid_width=16 --set grid_height=16 --set infection_rate={} --warmup 0 \
             --steps 30 --stats-every 5 --export-stats run-0.csv",
            recorded.outcome.run.seed, recorded.values[rate_column]
        )
    }

    /// Returns a panel that held a genetic search of SIR in memory, whose budget of 6 evaluations ends inside its
    /// first generation of 32.
    fn short_genetic_search() -> ResultsPanel {
        let spec = search_spec(SearchAlgorithm::Genetic(GeneticSettings::default()));
        let mut run = SweepRun::start(sir(), None, spec, SweepOutput::Memory, SweepRunOptions::default())
            .expect("the search starts");
        let search_plan = Arc::clone(run.search_plan().expect("the spec is a search"));
        let mut panel = ResultsPanel::default();
        panel.begin_search(search_plan, &sir(), 0, None);
        let started = std::time::Instant::now();
        while !run.is_ended() {
            let events: Vec<SweepEvent> = std::iter::from_fn(|| run.try_recv()).collect();
            if let Some(SweepEvent::Failed(message)) = events.last() {
                panic!("the search failed: {message}");
            }
            panel.ingest(events);
            assert!(started.elapsed().as_secs() < 60, "the search ends");
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        panel
    }

    #[test]
    fn a_frame_of_search_events_leaves_the_panel_as_a_frame_per_event_does() {
        let mut spec = search_spec(SearchAlgorithm::Random);
        if let Some(search) = &mut spec.search {
            search.max_evaluations = 12;
        }
        let mut run = SweepRun::start(sir(), None, spec, SweepOutput::Memory, SweepRunOptions::default())
            .expect("the search starts");
        let search_plan = Arc::clone(run.search_plan().expect("the spec is a search"));
        let mut events = Vec::new();
        let started = std::time::Instant::now();
        while !run.is_ended() {
            events.extend(std::iter::from_fn(|| run.try_recv()));
            assert!(started.elapsed().as_secs() < 60, "the search ends");
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        if let Some(SweepEvent::Failed(message)) = events.last() {
            panic!("the search failed: {message}");
        }

        let (mut together, mut one_at_a_time) = (ResultsPanel::default(), ResultsPanel::default());
        together.begin_search(Arc::clone(&search_plan), &sir(), 0, None);
        one_at_a_time.begin_search(search_plan, &sir(), 0, None);
        for event in &events {
            one_at_a_time.ingest([event.clone()]);
        }
        together.ingest(events);

        assert_eq!(
            together.selected_configs,
            BTreeSet::from([0, 1, 2]),
            "the first batch's candidates are selected"
        );
        assert_eq!(together.selected_configs, one_at_a_time.selected_configs);
        let (together, one_at_a_time) = (
            together.store.as_ref().expect("the search's store"),
            one_at_a_time.store.as_ref().expect("the search's store"),
        );
        assert_eq!(together.search_log(), one_at_a_time.search_log(), "the search's course");
        assert_eq!(
            together.search_log().map(|log| log.history.batches.len()),
            Some(4),
            "four batches of three"
        );
        assert_eq!(together.runs(), one_at_a_time.runs());
        assert!(together.config_ids().eq(0..12), "one config per candidate");
        assert!(together.config_ids().eq(one_at_a_time.config_ids()));
        let axis_levels = |store: &ResultsStore| -> Vec<(String, Vec<String>, Vec<f64>)> {
            let axes = store.axes().iter();
            axes.map(|axis| (axis.label.clone(), axis.levels.clone(), axis.positions.clone()))
                .collect()
        };
        assert_eq!(axis_levels(together), axis_levels(one_at_a_time), "the axes");
        assert_eq!(together.axes().len(), 1, "the infection rate varies");
        for config_id in 0..12 {
            assert_eq!(
                together.config_level(config_id, 0),
                one_at_a_time.config_level(config_id, 0),
                "config {config_id}"
            );
            let run_ids = |store: &ResultsStore| -> Vec<u64> {
                let runs = store.config_runs(config_id);
                runs.map(|outcome| outcome.run.run_id).collect()
            };
            assert_eq!(
                run_ids(together),
                [2 * config_id, 2 * config_id + 1],
                "config {config_id}"
            );
            assert_eq!(run_ids(together), run_ids(one_at_a_time), "config {config_id}");
        }
    }

    /// Returns the files `panel` holds as a dialog picks them, leaving out the file named `left_out`.
    fn picked_files(panel: &ResultsPanel, left_out: &str) -> Vec<DialogFile> {
        let files = panel.files().expect("a search held in memory hands over its files");
        files
            .entries()
            .into_iter()
            .filter(|(name, _)| *name != left_out)
            .map(|(name, bytes)| DialogFile {
                name: name.to_owned(),
                bytes: bytes.to_vec(),
                path: None,
            })
            .collect()
    }

    #[test]
    fn a_genetic_search_that_ends_in_its_first_generation_tells_it_from_a_missing_table() {
        let live = short_genetic_search();
        let log = live
            .store
            .as_ref()
            .and_then(ResultsStore::search_log)
            .expect("a search log");
        assert_eq!(log.history.batches.len(), 2, "two batches of three");
        assert!(log.history.generations.is_empty(), "no generation finished");
        assert!(!live.search.generations_missing, "a live search has every table");

        let registry = model_registry(None);
        let open = |files: Vec<DialogFile>| {
            let names = files.iter().map(|file| file.name.clone()).collect();
            let entries = files.into_iter().map(|file| (file.name, file.bytes)).collect();
            let set = ResultSet::from_files(entries, usize::MAX).expect("the files read");
            let mut panel = ResultsPanel::default();
            panel.show_result_set(set, ResultsSource::Files(names), &registry);
            panel
        };
        let whole = open(picked_files(&live, ""));
        assert!(!whole.search.generations_missing, "the table holds its header alone");
        assert_eq!(whole.view, ResultsView::Search);
        let without = open(picked_files(&live, GENERATIONS_FILE));
        assert!(without.search.generations_missing);
    }

    #[test]
    fn picked_files_are_read_on_a_later_frame_than_the_status() {
        let files = vec![DialogFile {
            name: "manifest.json".to_owned(),
            bytes: b"{}".to_vec(),
            path: None,
        }];
        let mut panel = ResultsPanel::default();
        panel.hold_picked_files(files.clone());
        assert_eq!(panel.status.as_deref(), Some(READING_RESULTS));
        assert_eq!(panel.due_picked_files(7), None, "the first frame shows the status");
        assert_eq!(
            panel.due_picked_files(7),
            None,
            "a second pass of that frame reads nothing"
        );
        assert_eq!(
            panel.due_picked_files(8),
            Some(files.clone()),
            "the next frame reads them"
        );
        assert_eq!(panel.due_picked_files(9), None, "the files are read once");

        let sir = sir();
        panel.hold_picked_files(files);
        panel.begin_sweep(plan(&sir, 1), &sir, 0, None);
        assert!(panel.picked_files.is_none(), "a sweep that starts drops them");
    }

    #[test]
    fn a_series_load_tells_runs_with_no_series_from_runs_past_the_budget() {
        let sir = sir();
        let plan = plan(&sir, 2);
        let mut panel = ResultsPanel::default();
        panel.begin_sweep(Arc::clone(&plan), &sir, 0, None);
        let store = panel.store.as_mut().expect("the sweep's store");
        for run_id in 0..4 {
            store.push_run(outcome(&plan, run_id, SeriesBuffer::new(3)), true);
        }

        let (sender, receiver) = flume::bounded(1);
        let requested_runs: BTreeSet<u64> = [0, 1, 2, 3].into();
        panel.series_load = Some(SeriesLoad {
            kept_runs: requested_runs.clone(),
            requested_runs,
            receiver,
        });
        let read = DirectorySeries {
            series: BTreeMap::from([(0, series(1.0))]),
            dropped_runs: [2, 3].into(),
        };
        sender.send(Ok(read)).expect("the load is pending");
        poll_series_load(&mut panel);

        assert_eq!(
            panel.status.as_deref(),
            Some("Loaded series for 1 of 4 runs. 1 recorded no series. Memory budget excludes 2.")
        );
        let store = panel.store.as_ref().expect("the sweep's store");
        assert_eq!(store.series(0), Some(&series(1.0)));
        assert_eq!(store.empty_series_count(), 1);
        assert_eq!(
            store.runs_without_series(&[0, 1].into()),
            [2, 3].into(),
            "run 1 is never loaded again"
        );
    }
}
