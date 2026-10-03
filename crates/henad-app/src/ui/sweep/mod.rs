//! Sweep tab. It builds a sweep or a search of the selected model, runs it and follows its progress.
//!
//! The tab holds four panels. A header of fixed height carries the mode switch, the model and the Spec menu, and a
//! footer of fixed height carries the plan's total with Start, or the progress with Pause and Abort. A Plan panel
//! sits on the right while the tab is wide enough, and the form or the progress scrolls between them.

pub mod builder;
pub mod draft;
pub mod footer;
pub mod header;
pub mod layout;
pub mod parameters;
pub mod plan;
pub mod progress;
pub mod search;
pub mod session;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use egui::{CentralPanel, Frame, Id, Margin, Panel, ScrollArea};
use henad_compute::cpu::sim_thread::WakeFn;
use henad_compute::entry::ModelEntry;
use henad_compute::gpu::GpuContext;
use henad_core::explore::plan::{ModelSchema, PlanWarning};
use henad_core::export::StatColumns;
use henad_core::metadata::Backend;
use henad_core::params::ParamValue;
use henad_explore::output::manifest::{ManifestMode, ManifestStatus};
use henad_explore::output::{MANIFEST_FILE, OutputDir};
use henad_explore::probe::ProbeReport;
use henad_explore::spec_file::SpecFile;
use serde::Deserialize;

use crate::icons::material_design_icons::MDI_FLASK_OUTLINE;
use crate::state::{AppState, lookup_message};
use crate::ui::dock::Tab;
use crate::ui::files::{DialogFile, OpenResult, OpenTarget, SaveTarget};
use crate::ui::plural;
use crate::ui::sweep::builder::banner_line;
use crate::ui::sweep::draft::{
    DesignTableDraft, DraftAlgorithm, DraftDesign, DraftIssue, DraftMode, DraftPlan, DraftSite, IssueKind,
    LevelPreview, LevelPreviews, SweepDraft, capitalize, describe_error,
};
use crate::ui::sweep::footer::{AbortModal, BuilderFooter, SessionFooter};
use crate::ui::sweep::header::{BuilderHeader, SessionHeader};
use crate::ui::sweep::layout::{FormState, IssueCount, Reveal, SweepSection};
use crate::ui::sweep::plan::PlanSummary;
use crate::ui::sweep::progress::SessionResult;
use crate::ui::sweep::session::{KeptDraft, SessionExecution, SessionState, SweepSession};

/// Id of the Plan panel.
const PLAN_PANEL_ID: &str = "henad_sweep_plan";

/// Width the Plan panel starts at, in points.
const PLAN_PANEL_WIDTH: f32 = 280.0;

/// Narrowest the Plan panel gets, in points.
const PLAN_PANEL_MIN_WIDTH: f32 = 220.0;

/// Widest the Plan panel gets, as a share of the tab's width.
const PLAN_PANEL_MAX_SHARE: f32 = 0.4;

/// Id under which egui keeps whether the Plan panel is open.
const PLAN_OPEN_ID: &str = "henad_sweep_plan_open";

/// State of the Sweep tab.
#[derive(Default)]
pub struct SweepPanel {
    /// Draft of each model the tab has shown, by model id.
    drafts: BTreeMap<String, SweepDraft>,
    /// Last check of a draft, kept while the draft and the Parameters tab values stay the same.
    last_check: Option<DraftCheck>,
    pub session: Option<SweepSession>,
    /// Line reporting the last save or load.
    pub status: Option<String>,
    /// Reason the last press of Start failed, drawn in the footer.
    pub start_failure: Option<String>,
    /// Layout flags of the tab.
    form: FormState,
    /// Whether the modal asking to abort the sweep is open.
    confirm_abort: bool,
    /// Whether the modal asking to replace results held in memory is open.
    confirm_replace: bool,
    /// Build of each model the tab has shown, for the stat columns it samples, by model id.
    column_builds: BTreeMap<String, ColumnsBuild>,
    /// Program the tab's advice names, `None` for none.
    pub cli_command: Option<String>,
}

/// Build of a model at its default values, for the stat columns its sample at tick 0 has.
enum ColumnsBuild {
    /// Build running on a thread of its own. The thread sends the columns once sampled.
    #[cfg(not(target_arch = "wasm32"))]
    Running(flume::Receiver<Option<StatColumns>>),
    /// Columns of the build, `None` for a build that failed or a model that cannot build here.
    Sampled(Option<StatColumns>),
}

impl ColumnsBuild {
    /// Starts a build of `entry`'s model.
    ///
    /// On native the model builds on a thread of its own, and `wake` runs once the build reports. A GPU model builds
    /// on a device of its own, as a sweep does, so a fault in the build never reaches the live model. A browser builds
    /// a CPU model at once and never builds a GPU model. A GPU sample blocks, and a browser cannot block.
    #[cfg(not(target_arch = "wasm32"))]
    fn start(entry: &ModelEntry, wake: &WakeFn) -> Self {
        let (sender, receiver) = flume::bounded(1);
        let entry = entry.clone();
        let wake = Arc::clone(wake);
        let spawned = std::thread::Builder::new()
            .name("henad-stat-columns".to_owned())
            .spawn(move || {
                let columns = default_stat_columns(&entry);
                // The receiver is gone once the app is closing, and nothing is left to report to.
                drop(sender.send(columns));
                wake();
            });
        match spawned {
            Ok(_) => Self::Running(receiver),
            Err(_) => Self::Sampled(None),
        }
    }

    #[cfg(target_arch = "wasm32")]
    fn start(entry: &ModelEntry, _wake: &WakeFn) -> Self {
        Self::Sampled(match entry.metadata().backend {
            Backend::Cpu => sampled_stat_columns(entry, None),
            Backend::Gpu => None,
        })
    }

    /// Takes the columns of a build that has reported since the last poll.
    #[cfg(not(target_arch = "wasm32"))]
    fn poll(&mut self) {
        if let Self::Running(receiver) = self {
            match receiver.try_recv() {
                Ok(columns) => *self = Self::Sampled(columns),
                Err(flume::TryRecvError::Empty) => {}
                Err(flume::TryRecvError::Disconnected) => *self = Self::Sampled(None),
            }
        }
    }
}

/// A draft and the Parameters tab values it was checked against, with the result.
struct DraftCheck {
    draft: SweepDraft,
    panel_values: Vec<ParamValue>,
    result: Result<DraftPlan, Vec<DraftIssue>>,
    /// Values of each parameter, then of each action tick, as [`SweepDraft::level_previews`] gives them.
    level_previews: LevelPreviews,
    /// Results the draft's output folder held when the draft was checked, `None` for a folder free to take them.
    folder_results: Option<FolderResults>,
}

/// Results an output folder holds already.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FolderResults {
    /// Mode the folder's manifest records, `None` for results with no readable manifest to name it.
    pub mode: Option<ManifestMode>,
    /// Whether the manifest says runs are left for a resume to finish.
    pub resumable: bool,
}

impl FolderResults {
    /// Returns the noun phrase naming the results, as in "search results".
    fn phrase(self) -> &'static str {
        match self.mode {
            Some(ManifestMode::Sweep) => "sweep results",
            Some(ManifestMode::Search) => "search results",
            None => "results",
        }
    }

    /// Returns the issue of the Folder field. It blocks Start.
    pub fn warning(self) -> String {
        format!(
            "This folder holds {}. Select another folder without results.",
            self.phrase()
        )
    }

    /// Returns the reason Start refuses the folder.
    pub fn refusal(self) -> String {
        format!("Folder holds {}", self.phrase())
    }
}

/// Mode and status fields of a manifest, read without the rest of it.
#[derive(Deserialize)]
struct ManifestHeader {
    mode: ManifestMode,
    #[serde(default)]
    status: Option<ManifestStatus>,
}

/// Returns the results the folder at `path` holds already, `None` for a folder free to take them.
///
/// Any file a sweep or a search writes counts as results, and the manifest names their mode.
pub fn folder_results(path: &Path) -> Option<FolderResults> {
    if !OutputDir::holds_results(path) {
        return None;
    }
    let header = std::fs::read_to_string(path.join(MANIFEST_FILE))
        .ok()
        .and_then(|text| serde_json::from_str::<ManifestHeader>(&text).ok());
    Some(FolderResults {
        mode: header.as_ref().map(|header| header.mode),
        resumable: header
            .and_then(|header| header.status)
            .is_some_and(|status| status != ManifestStatus::Complete),
    })
}

impl SweepPanel {
    /// Returns the draft of `schema`'s model, a new one the first time.
    pub fn draft_mut(&mut self, schema: &ModelSchema<'_>) -> &mut SweepDraft {
        self.drafts
            .entry(schema.id.to_owned())
            .or_insert_with(|| SweepDraft::new(schema))
    }

    /// Returns the check of the draft of `schema`'s model against `panel_values`.
    ///
    /// The check runs again only when the draft or the values change. Such a change also drops
    /// [`Self::start_failure`]. The failure described the draft as it was.
    fn cached_check(&mut self, schema: &ModelSchema<'_>, panel_values: &[ParamValue]) -> &DraftCheck {
        let draft = self
            .drafts
            .entry(schema.id.to_owned())
            .or_insert_with(|| SweepDraft::new(schema));
        draft.cli_command.clone_from(&self.cli_command);
        if self
            .last_check
            .as_ref()
            .is_some_and(|last| last.draft != *draft || last.panel_values != panel_values)
        {
            self.last_check = None;
            self.start_failure = None;
        }
        self.last_check.get_or_insert_with(|| DraftCheck {
            result: draft.check(schema, panel_values),
            level_previews: draft.level_previews(schema),
            folder_results: draft
                .output_folder()
                .and_then(|folder| folder_results(Path::new(folder))),
            panel_values: panel_values.to_vec(),
            draft: draft.clone(),
        })
    }

    /// Gives the draft of `entry`'s model the stat columns of a build of the model at its default values.
    ///
    /// The first call for a model starts the build, as [`ColumnsBuild::start`] describes. Until the build reports, the
    /// draft cannot start. A build that fails leaves the draft counting every stat as one column.
    fn learn_stat_columns(&mut self, entry: &ModelEntry, wake: &WakeFn) {
        let build = self
            .column_builds
            .entry(entry.id().to_owned())
            .or_insert_with(|| ColumnsBuild::start(entry, wake));
        #[cfg(not(target_arch = "wasm32"))]
        build.poll();
        let draft = self
            .drafts
            .entry(entry.id().to_owned())
            .or_insert_with(|| SweepDraft::new(&entry.schema()));
        match &*build {
            #[cfg(not(target_arch = "wasm32"))]
            ColumnsBuild::Running(_) => draft.columns_pending = true,
            ColumnsBuild::Sampled(Some(columns)) => draft.set_stat_columns(columns),
            ColumnsBuild::Sampled(None) => draft.columns_pending = false,
        }
    }

    /// Drops the last check, so the next one reads the output folder again.
    fn clear_check(&mut self) {
        self.last_check = None;
    }

    /// Drops the ended session, closes its modals, and checks the draft again on the next frame.
    ///
    /// The session might have written results into the draft's output folder.
    pub fn clear_session(&mut self) {
        self.session = None;
        self.status = None;
        self.confirm_abort = false;
        self.confirm_replace = false;
        self.clear_check();
    }

    /// Returns whether a sweep is running, paused ones included.
    pub fn is_running(&self) -> bool {
        self.session.as_ref().is_some_and(SweepSession::is_running)
    }

    /// Returns whether a sweep is running and not paused.
    pub fn is_stepping(&self) -> bool {
        self.session
            .as_ref()
            .is_some_and(|session| session.is_running() && !session.is_paused())
    }

    /// Returns the title of the Sweep tab, with the state of its session.
    pub fn tab_title(&self) -> String {
        let state = self.session.as_ref().map(|session| {
            let progress = session.progress();
            (session.state(&progress), progress::finished_fraction(&progress))
        });
        tab_title(state)
    }
}

/// Returns the stat columns of `entry`'s model at its default values, `None` when the build fails or no GPU device
/// can be acquired for a GPU model.
///
/// A GPU model builds on a device sized to its own needs.
#[cfg(not(target_arch = "wasm32"))]
fn default_stat_columns(entry: &ModelEntry) -> Option<StatColumns> {
    let gpu = match entry.gpu_needs() {
        Some(needs) => Some(henad_explore::device::acquire_headless(needs).ok()?),
        None => None,
    };
    sampled_stat_columns(entry, gpu.as_ref())
}

/// Returns the stat columns of a build of `entry`'s model at its default values, `None` when the build fails.
///
/// A GPU model builds on `gpu`.
fn sampled_stat_columns(entry: &ModelEntry, gpu: Option<&GpuContext>) -> Option<StatColumns> {
    let values: Vec<ParamValue> = entry
        .param_descriptors()
        .iter()
        .map(|descriptor| descriptor.kind.default_value())
        .collect();
    ProbeReport::build(entry, gpu, &values, None)
        .ok()
        .map(|report| report.columns)
}

/// Returns the title of the Sweep tab for a session in a state with a share of its runs finished, `None` for no
/// session: "Sweep 42%" while it runs, "Sweep paused", then "Sweep done", "Sweep stopped" or "Sweep failed".
fn tab_title(state: Option<(SessionState, f32)>) -> String {
    let text = match state {
        None => "Sweep".to_owned(),
        Some((SessionState::Planning | SessionState::Running, fraction)) => {
            format!("Sweep {}%", (fraction * 100.0).floor() as u32)
        }
        Some((SessionState::Paused, _)) => "Sweep paused".to_owned(),
        Some((SessionState::Finished, _)) => "Sweep done".to_owned(),
        Some((SessionState::Aborted | SessionState::Stopped, _)) => "Sweep stopped".to_owned(),
        Some((SessionState::Failed, _)) => "Sweep failed".to_owned(),
    };
    format!("{MDI_FLASK_OUTLINE}  {text}")
}

/// Button press or link the tab applies once every panel has drawn, when the whole app state is free to borrow.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SweepRequest {
    Start,
    /// Start, dropping the results held in memory.
    StartAnyway,
    SaveSpec,
    LoadSpec,
    LoadTable,
    ChooseFolder,
    SaveResults,
    ShowResults,
    /// Brings the Model tab to the front.
    ShowModel,
    /// Drops the ended session and returns to the settings.
    EditSweep,
    Pause,
    Resume,
    /// Opens the modal that asks before aborting.
    AskAbort,
    Abort,
    /// Opens a section of the form and scrolls it, or one of its rows, into view.
    Reveal(Reveal),
    DismissNotification,
    /// Opens the results the draft's output folder holds in the Results tab.
    OpenFolderResults,
    /// Lists the session's failed runs in the Results tab.
    ShowFailedRuns,
    /// Selects the first run of a search candidate in the Results tab.
    ShowCandidate(u64),
}

/// An issue of a draft as the footer, its chips and the Plan panel list it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IssueLine {
    pub kind: IssueKind,
    /// The issue, led by the row it belongs to.
    pub text: String,
    /// Section holding the row.
    pub section: SweepSection,
    /// Row the issue belongs to.
    pub site: DraftSite,
}

impl IssueLine {
    /// Returns the reveal of the line's row.
    pub fn reveal(&self) -> Reveal {
        Reveal {
            section: self.section,
            site: Some(self.site),
        }
    }
}

/// Result of a draft's check, as the tab draws it.
#[derive(Debug, Clone, Default)]
pub struct CheckSummary {
    /// Configurations or evaluations, replicates and runs of the plan, `None` while the draft has issues.
    pub counts: Option<(u64, u64, u64)>,
    /// Approximate bytes of the series once counted.
    pub series_bytes: Option<u64>,
    pub warnings: Vec<String>,
    /// Warning of each action row due past the last tick, by the row's position.
    pub action_warnings: Vec<(usize, String)>,
    pub issues: Vec<DraftIssue>,
    /// Each issue as a line, invalid input first.
    pub lines: Vec<IssueLine>,
    pub param_previews: Vec<Option<LevelPreview>>,
    pub tick_previews: Vec<Option<LevelPreview>>,
    /// Results the output folder holds already, `None` for a folder free to take them.
    pub folder_results: Option<FolderResults>,
}

impl CheckSummary {
    fn new(check: &DraftCheck, entry: &ModelEntry, schema: &ModelSchema<'_>) -> Self {
        let (counts, series_bytes, mut warnings, mut issues) = match &check.result {
            Ok(planned) => (
                Some(planned.counts()),
                Some(planned.series_bytes),
                planned
                    .plan
                    .warnings()
                    .iter()
                    .map(|warning| capitalize(&warning.to_string()))
                    .collect::<Vec<String>>(),
                Vec::new(),
            ),
            Err(issues) => (None, None, Vec::new(), issues.clone()),
        };
        let action_warnings = check
            .result
            .as_ref()
            .map_or_else(|_| Vec::new(), |planned| action_warnings(&check.draft, planned));
        if let Some(results) = check.folder_results {
            issues.push(DraftIssue::new(DraftSite::Execution, results.warning()));
        }
        let search = &check.draft.search;
        if check.result.is_ok()
            && check.draft.mode == DraftMode::Search
            && search.algorithm == DraftAlgorithm::PatternSpace
            && search.pattern_space.initial_samples >= search.max_evaluations
        {
            warnings.push("Initial samples cover every evaluation. The search will be entirely random.".to_owned());
        }
        if cfg!(target_arch = "wasm32") && entry.metadata().backend == Backend::Gpu {
            issues.push(DraftIssue::new(
                draft::DraftSite::Sweep,
                "GPU sweeps are unavailable in a browser.",
            ));
        }
        let mut lines: Vec<IssueLine> = issues
            .iter()
            .map(|issue| IssueLine {
                kind: issue.kind,
                text: banner_line(issue, &check.draft, schema),
                section: SweepSection::of_site(issue.site, check.draft.mode),
                site: issue.site,
            })
            .collect();
        // A stable sort keeps the issues of one kind in the order the check found them.
        lines.sort_by_key(|line| line.kind);
        let (param_previews, tick_previews) = check.level_previews.clone();
        Self {
            counts: counts.filter(|_| issues.is_empty()),
            series_bytes,
            warnings,
            action_warnings,
            issues,
            lines,
            param_previews,
            tick_previews,
            folder_results: check.folder_results,
        }
    }

    /// Returns the issues of the row of `site`.
    pub fn issues_at(&self, site: DraftSite) -> impl Iterator<Item = &DraftIssue> {
        self.issues.iter().filter(move |issue| issue.site == site)
    }

    /// Returns the issues of the rows in `section`.
    pub fn section_issues(&self, section: SweepSection) -> IssueCount {
        let mut count = IssueCount::default();
        for line in self.lines.iter().filter(|line| line.section == section) {
            count.add(line.kind);
        }
        count
    }
}

/// Returns the warning of each action row that the plan finds due past the last tick, by the row's position.
fn action_warnings(draft: &SweepDraft, planned: &DraftPlan) -> Vec<(usize, String)> {
    planned
        .plan
        .warnings()
        .iter()
        .filter_map(|warning| match warning {
            PlanWarning::ActionAfterEnd {
                name,
                last_tick,
                config_count,
                ..
            } => {
                let position = draft.actions.iter().position(|action| action.name == *name)?;
                let text = format!(
                    "Past the last tick {last_tick} in {config_count} {}. The action will not run there.",
                    plural(*config_count, "configuration")
                );
                Some((position, text))
            }
        })
        .collect()
}

/// Steps a sweep within the frame's budget in a browser, and takes every event it has sent.
pub fn update(app: &mut AppState, dt: f64) {
    if let Some(session) = &mut app.sweep.session {
        session.update(dt, &mut app.results);
    }
}

pub fn sweep_ui(ui: &mut egui::Ui, app: &mut AppState) {
    // Read before any panel takes its share.
    let tab_width = ui.available_width();
    app.sweep.form.fit_tab(tab_width);
    let mut request = None;
    if app.sweep.session.is_some() {
        session_frame(ui, app, tab_width, &mut request);
    } else {
        builder_frame(ui, app, tab_width, &mut request);
    }
    if let Some(request) = request {
        let title = app.sweep.tab_title();
        apply(app, request);
        // The tab bar drew its title before the tab. A pass drawn again shows the new one at once.
        if app.sweep.tab_title() != title {
            ui.ctx().request_discard("Sweep tab title changed");
        }
    }
}

/// Returns whether the Plan panel is open, as egui keeps it.
fn plan_open(ctx: &egui::Context) -> bool {
    ctx.data_mut(|data| *data.get_persisted_mut_or(Id::new(PLAN_OPEN_ID), true))
}

fn store_plan_open(ctx: &egui::Context, open: bool) {
    ctx.data_mut(|data| data.insert_persisted(Id::new(PLAN_OPEN_ID), open));
}

/// Adds the header panel, with `add_contents` in it.
fn header_panel(ui: &mut egui::Ui, add_contents: impl FnOnce(&mut egui::Ui)) {
    Panel::top(header::HEADER_ID)
        .resizable(false)
        .show_separator_line(true)
        .exact_size(header::header_height(ui.style()))
        .frame(header::header_frame())
        .show(ui, add_contents);
}

/// Adds the footer panel, with `add_contents` in it.
fn footer_panel(ui: &mut egui::Ui, add_contents: impl FnOnce(&mut egui::Ui)) {
    Panel::bottom(footer::FOOTER_ID)
        .resizable(false)
        .show_separator_line(true)
        .exact_size(footer::footer_height(ui.style()))
        .frame(footer::footer_frame())
        .show(ui, add_contents);
}

/// Adds the Plan panel for a tab `tab_width` wide, with `add_contents` in it, while `plan_open` is set.
fn plan_panel(ui: &mut egui::Ui, tab_width: f32, plan_open: &mut bool, add_contents: impl FnOnce(&mut egui::Ui)) {
    let max_width = (PLAN_PANEL_MAX_SHARE * tab_width).max(PLAN_PANEL_MIN_WIDTH);
    Panel::right(PLAN_PANEL_ID)
        .resizable(true)
        .default_size(PLAN_PANEL_WIDTH)
        .size_range(PLAN_PANEL_MIN_WIDTH..=max_width)
        .frame(Frame::NONE.inner_margin(Margin {
            left: 8,
            ..Margin::ZERO
        }))
        .show_collapsible(ui, plan_open, |ui| {
            ScrollArea::vertical()
                .id_salt("henad_sweep_plan_scroll")
                .auto_shrink([false; 2])
                .show(ui, add_contents);
        });
}

/// Draws the tab while it builds a sweep: its header, footer, plan and form.
fn builder_frame(ui: &mut egui::Ui, app: &mut AppState, tab_width: f32, request: &mut Option<SweepRequest>) {
    let Some(entry) = app.selected_entry().cloned() else {
        ui.label("No model selected.");
        return;
    };
    let entry = &entry;
    let schema = entry.schema();
    let wake = app.repaint_waker();
    app.sweep.learn_stat_columns(entry, &wake);
    // Every text the panels draw is built here. The form borrows the draft once they have drawn.
    let check = app.sweep.cached_check(&schema, &app.param_values);
    let summary = CheckSummary::new(check, entry, &schema);
    let plan = PlanSummary::for_draft(check, &schema, entry.name(), &summary);
    let sections = builder::SectionSummaries::new(check, &schema, &summary);
    let mode = check.draft.mode;
    let steps_per_run = check.draft.warmup.saturating_add(check.draft.steps);
    let folder = (!check.draft.holds_results_in_memory()).then(|| PathBuf::from(check.draft.output_dir_text.trim()));
    let unsaved_results = app.results.holds_unsaved_files();
    let search_files = app.results.holds_search();
    let results = app
        .results
        .store()
        .filter(|store| store.model_id == entry.id() && !store.runs().is_empty());
    let ctx = ui.ctx().clone();
    let mut plan_open = plan_open(&ctx);

    let SweepPanel {
        drafts,
        last_check,
        form,
        status,
        start_failure,
        confirm_abort,
        confirm_replace,
        cli_command,
        ..
    } = &mut app.sweep;
    let Some(draft) = drafts.get_mut(schema.id) else {
        return;
    };
    let modal_open = *confirm_abort || *confirm_replace;

    header_panel(ui, |ui| {
        let header = BuilderHeader {
            model_name: entry.name(),
            notification: status.as_deref(),
            gpu_refused: cfg!(target_arch = "wasm32") && entry.metadata().backend == Backend::Gpu,
            cli_command: cli_command.as_deref(),
            plan_fits: form.plan_fits,
        };
        if header::builder_header(ui, &header, &mut draft.mode, &mut plan_open, request) {
            *status = None;
        }
    });
    footer_panel(ui, |ui| {
        let footer = BuilderFooter {
            mode,
            summary: &summary,
            start_failure: start_failure.as_deref(),
            steps_per_run,
            folder: folder.as_deref(),
            unsaved_results,
            search_files,
            modal_open,
        };
        footer::builder_footer(ui, &footer, request);
    });
    if form.plan_fits {
        plan_panel(ui, tab_width, &mut plan_open, |ui| {
            plan::plan_ui(ui, &plan, None, true, request);
        });
    }
    store_plan_open(&ctx, plan_open);
    CentralPanel::no_frame().show(ui, |ui| {
        ScrollArea::vertical()
            .id_salt("henad_sweep_form")
            .auto_shrink([false; 2])
            .show(ui, |ui| {
                layout::cap_form_width(ui);
                let form_input = builder::FormInput {
                    entry,
                    schema: &schema,
                    panel_values: &app.param_values,
                    summary: &summary,
                    sections: &sections,
                    plan: &plan,
                    results,
                    cli_command: cli_command.as_deref(),
                };
                builder::form_ui(ui, draft, form, &form_input, request);
            });
    });
    // An edit in the form drops the notification. A loaded spec replaced the draft before the check, and its
    // notification stays.
    if last_check.as_ref().is_some_and(|checked| checked.draft != *draft) {
        *status = None;
    }

    if *confirm_replace {
        footer::replace_modal(&ctx, header::noun(mode), confirm_replace, request);
    }
}

/// Draws the tab while a session exists: its state, progress, plan and runs.
fn session_frame(ui: &mut egui::Ui, app: &mut AppState, tab_width: f32, request: &mut Option<SweepRequest>) {
    let unsaved_results = app.results.holds_unsaved_files();
    let search_files = app.results.holds_search();
    let ctx = ui.ctx().clone();
    let mut plan_open = plan_open(&ctx);
    let SweepPanel {
        session,
        status,
        form,
        confirm_abort,
        confirm_replace,
        ..
    } = &mut app.sweep;
    let Some(session) = session.as_ref() else {
        return;
    };
    let progress = session.progress();
    let state = session.state(&progress);
    let noun = session.noun();
    let resumed = session.is_resumed();
    let modal_open = settle_session_modals(state, confirm_abort, confirm_replace);
    let resumed_plan;
    let plan = if let Some(kept) = &session.kept {
        &kept.plan
    } else {
        resumed_plan = PlanSummary::for_resumed(session);
        &resumed_plan
    };
    let caption = match state {
        SessionState::Planning | SessionState::Running => format!("of the running {noun}"),
        SessionState::Paused => format!("of the paused {noun}"),
        SessionState::Finished | SessionState::Aborted | SessionState::Stopped | SessionState::Failed => {
            format!("of this {noun}")
        }
    };

    header_panel(ui, |ui| {
        let header = SessionHeader {
            state,
            noun,
            model_name: &session.model_name,
            notification: status.as_deref(),
            resumed,
            plan_fits: form.plan_fits,
        };
        header::session_header(ui, &header, &mut plan_open, request);
    });
    footer_panel(ui, |ui| {
        let result = (!state.is_running())
            .then(|| SessionResult::new(noun, session.report.as_ref(), session.failure.as_deref(), &progress));
        let footer = SessionFooter {
            noun,
            state,
            progress: &progress,
            result,
            unsaved_results,
            search_files,
            resumed,
            modal_open,
        };
        footer::session_footer(ui, &footer, request);
    });
    if form.plan_fits {
        plan_panel(ui, tab_width, &mut plan_open, |ui| {
            plan::plan_ui(ui, plan, Some(&caption), false, request);
        });
    }
    store_plan_open(&ctx, plan_open);
    let results = &app.results;
    CentralPanel::no_frame().show(ui, |ui| {
        let view = progress::ProgressView {
            session,
            progress: &progress,
            state,
            results,
        };
        let plan_section = (!form.plan_fits).then_some(plan);
        progress_view(ui, &view, plan_section, request);
    });

    if *confirm_abort {
        let modal = AbortModal {
            noun,
            progress: &progress,
            paused: state.is_paused(),
            in_folder: session.output_dir.is_some(),
        };
        footer::abort_modal(&ctx, &modal, confirm_abort, request);
    }
}

/// Closes each modal that a session in `state` leaves nothing to answer, and returns whether the abort modal is still
/// open.
///
/// The replace modal belongs to the form, and a session takes the form's place. The abort modal closes once the sweep
/// ends.
fn settle_session_modals(state: SessionState, confirm_abort: &mut bool, confirm_replace: &mut bool) -> bool {
    *confirm_replace = false;
    *confirm_abort &= state.is_running();
    *confirm_abort
}

/// Draws the progress `view` shows in a scroll area, with the plan `plan_section` under it while the Plan panel does
/// not fit.
fn progress_view(
    ui: &mut egui::Ui,
    view: &progress::ProgressView<'_>,
    plan_section: Option<&PlanSummary>,
    request: &mut Option<SweepRequest>,
) {
    ScrollArea::vertical()
        .id_salt("henad_sweep_progress")
        .auto_shrink([false; 2])
        .show(ui, |ui| {
            layout::cap_form_width(ui);
            progress::progress_ui(ui, view, request);
            let Some(plan) = plan_section else {
                return;
            };
            ui.separator();
            let section = SweepSection::Plan;
            let heading = layout::SectionHeading {
                title: section.title(),
                issues: IssueCount::default(),
                summary: &plan.section_summary(),
            };
            layout::section(
                ui,
                section.id_salt(),
                heading,
                section.default_open(),
                false,
                |ui, _body| {
                    // The plan of a session opens no section, and requests nothing.
                    plan::plan_body(ui, plan, false, &mut None);
                },
            );
        });
}

fn apply(app: &mut AppState, request: SweepRequest) {
    match request {
        SweepRequest::Start if app.results.holds_unsaved_files() => app.sweep.confirm_replace = true,
        SweepRequest::Start | SweepRequest::StartAnyway => start(app),
        SweepRequest::SaveSpec => save_spec(app),
        SweepRequest::LoadSpec => app.open_file(OpenTarget::SweepSpec),
        SweepRequest::LoadTable => app.open_file(OpenTarget::DesignTable),
        SweepRequest::ChooseFolder => app.open_file(OpenTarget::OutputFolder),
        SweepRequest::SaveResults => save_results(app),
        SweepRequest::ShowResults => app.focus_request = Some(Tab::Results),
        SweepRequest::ShowModel => app.focus_request = Some(Tab::Model),
        SweepRequest::EditSweep => edit_sweep(app),
        SweepRequest::AskAbort => app.sweep.confirm_abort = true,
        SweepRequest::Reveal(reveal) => app.sweep.form.reveal = Some(reveal),
        SweepRequest::DismissNotification => app.sweep.status = None,
        // A browser has no output folder.
        SweepRequest::OpenFolderResults => {
            #[cfg(not(target_arch = "wasm32"))]
            open_folder_results(app);
        }
        SweepRequest::ShowFailedRuns => {
            app.results.show_failed_runs();
            app.focus_request = Some(Tab::Results);
        }
        SweepRequest::ShowCandidate(candidate_id) => {
            app.results.select_candidate(candidate_id);
            app.focus_request = Some(Tab::Results);
        }
        SweepRequest::Pause => {
            if let Some(session) = &mut app.sweep.session {
                session.pause();
            }
        }
        SweepRequest::Resume => {
            if let Some(session) = &mut app.sweep.session {
                session.resume();
            }
        }
        SweepRequest::Abort => {
            if let Some(session) = &mut app.sweep.session {
                session.abort();
            }
        }
    }
}

/// Starts the checked draft of the selected model.
///
/// The draft is checked again first. The output folder might have taken results since the last check.
fn start(app: &mut AppState) {
    let Some(entry) = app.selected_entry().cloned() else {
        return;
    };
    let entry = &entry;
    let schema = entry.schema();
    let wake = app.repaint_waker();
    app.sweep.learn_stat_columns(entry, &wake);
    app.sweep.clear_check();
    let check = app.sweep.cached_check(&schema, &app.param_values);
    let Ok(planned) = &check.result else {
        return;
    };
    let summary = CheckSummary::new(check, entry, &schema);
    let kept = KeptDraft {
        draft: check.draft.clone(),
        panel_values: check.panel_values.clone(),
        plan: PlanSummary::for_draft(check, &schema, entry.name(), &summary),
    };
    let spec = planned.spec.clone();
    let noun = if planned.search_plan.is_some() {
        "Search"
    } else {
        "Sweep"
    };
    let folder_results = check.folder_results;
    let execution = SessionExecution {
        concurrency: check.draft.concurrency,
        memory_budget: check.draft.memory_budget,
        gpu_memory_budget: check.draft.gpu_memory_budget,
    };
    let output_dir = check.draft.output_folder().map(PathBuf::from);
    let folder = output_dir.clone();
    app.sweep.start_failure = None;
    if let Some(results) = folder_results {
        app.sweep.start_failure = Some(format!("{noun} start failed: {}", results.refusal()));
        return;
    }
    match SweepSession::start(app, spec, execution, output_dir) {
        Ok(mut session) => {
            if let Some(entry) = app.models.get(session.plan().model()) {
                if let Some(search_plan) = session.search_plan() {
                    let search_plan = Arc::clone(search_plan);
                    app.results.begin_search(search_plan, entry, folder);
                } else {
                    let plan = Arc::clone(session.plan());
                    app.results.begin_sweep(plan, entry, folder);
                }
            }
            session.kept = Some(Box::new(kept));
            app.sweep.session = Some(session);
            app.sweep.status = None;
        }
        Err(message) => app.sweep.start_failure = Some(format!("{noun} start failed: {message}")),
    }
}

/// Saves the spec of the session's draft while a session exists, and of the selected model's draft otherwise.
fn save_spec(app: &mut AppState) {
    let (draft, panel_values) = if let Some(session) = &app.sweep.session {
        let Some(kept) = &session.kept else {
            return;
        };
        (kept.draft.clone(), kept.panel_values.clone())
    } else {
        let Some(entry) = app.selected_entry().cloned() else {
            return;
        };
        let schema = entry.schema();
        let draft = app.sweep.draft_mut(&schema).clone();
        (draft, app.param_values.clone())
    };
    let Some(entry) = app.models.get(&draft.model_id) else {
        return;
    };
    let schema = entry.schema();
    let name = match draft.mode {
        DraftMode::Sweep => format!("henad-{}-sweep.toml", entry.id()),
        DraftMode::Search => format!("henad-{}-search.toml", entry.id()),
    };
    match draft.to_toml(&schema, &panel_values) {
        Ok(text) => app.save_as(SaveTarget::SweepSpec, &name, text.into_bytes()),
        Err(issues) => {
            let reasons: Vec<&str> = issues.iter().map(|issue| issue.message.as_str()).collect();
            app.sweep.status = Some(format!("Spec save failed: {}", reasons.join(". ")));
            if let Some(first) = issues.first() {
                app.sweep.form.reveal = Some(Reveal::site(first.site, draft.mode));
            }
        }
    }
}

/// Opens the dialog that saves the results held in memory.
fn save_results(app: &mut AppState) {
    if let Some(files) = app.results.files() {
        let files = Arc::clone(files);
        let generation = app.results.files_generation();
        app.save_files(SaveTarget::SweepResults(generation), files);
    }
}

/// Opens the results the selected draft's output folder holds in the Results tab.
#[cfg(not(target_arch = "wasm32"))]
fn open_folder_results(app: &mut AppState) {
    let folder = selected_draft(app).and_then(|draft| draft.output_folder().map(PathBuf::from));
    if let Some(folder) = folder {
        crate::ui::results::open_folder(app, folder);
        app.focus_request = Some(Tab::Results);
    }
}

/// Takes the result of a file dialog the Sweep tab opened for `target`.
///
/// A folder dialog that ends without a folder, while the Folder field is empty, returns the results to memory.
pub fn receive_open(app: &mut AppState, target: OpenTarget, result: OpenResult) {
    if target == OpenTarget::OutputFolder
        && !matches!(result, OpenResult::Folder(_))
        && let Some(draft) = selected_draft(app)
        && draft.output_dir_text.trim().is_empty()
    {
        draft.results_in_folder = false;
    }
    let file = match result {
        OpenResult::Files(files) => files.into_iter().next(),
        OpenResult::Folder(path) => {
            if let Some(draft) = selected_draft(app) {
                draft.output_dir_text = path.display().to_string();
                draft.results_in_folder = true;
            }
            return;
        }
        OpenResult::Failed(message) => {
            app.sweep.status = Some(format!("Open failed: {message}"));
            return;
        }
        OpenResult::Canceled => return,
    };
    let Some(file) = file else {
        return;
    };
    match target {
        OpenTarget::SweepSpec => load_spec(app, &file),
        OpenTarget::DesignTable => load_table(app, file),
        OpenTarget::OutputFolder | OpenTarget::Results => {}
    }
}

/// Returns the draft of the selected model.
fn selected_draft(app: &mut AppState) -> Option<&mut SweepDraft> {
    let entry = app.models.get(app.selected_model.as_deref()?)?;
    Some(app.sweep.draft_mut(&entry.schema()))
}

/// Selects the model a spec file names, and replaces its draft and Parameters tab values with the spec's.
fn load_spec(app: &mut AppState, file: &DialogFile) {
    match read_spec(app, file) {
        Ok(()) => app.sweep.status = Some(format!("Loaded {}", file.name)),
        Err(message) => app.sweep.status = Some(format!("Spec load failed: {message}")),
    }
}

fn read_spec(app: &mut AppState, file: &DialogFile) -> Result<(), String> {
    // A spec loaded from disk reads its design tables beside it. A browser has no path to read them from.
    let spec_file = if let Some(path) = &file.path {
        SpecFile::load(path).map(|(spec_file, _)| spec_file)
    } else {
        let text = std::str::from_utf8(&file.bytes).map_err(|error| error.to_string())?;
        SpecFile::parse(text)
    }
    .map_err(|error| describe_error(&error))?;
    let entry = app.lookup(&spec_file.model).map_err(|error| lookup_message(&error))?;
    let model_id = entry.id().to_owned();
    let (draft, panel_values) = SweepDraft::from_spec_file(spec_file, &entry.schema())?;
    app.select_model(&model_id);
    apply_panel_values(app, &panel_values);
    app.sweep.drafts.insert(draft.model_id.clone(), draft);
    Ok(())
}

/// Drops the ended session and returns to the draft that started it.
///
/// When the Model tab picked another model meanwhile, the session's model is selected again, with the Parameters tab
/// values the sweep held fixed.
fn edit_sweep(app: &mut AppState) {
    let kept = app.sweep.session.as_ref().and_then(|session| session.kept.as_deref());
    let reselect = kept.and_then(|kept| {
        let id = app.models.get(&kept.draft.model_id)?.id().to_owned();
        let panel_values = || kept.panel_values.iter().cloned().map(Some).collect::<Vec<_>>();
        (app.selected_model.as_deref() != Some(id.as_str())).then(|| (id, panel_values()))
    });
    if let Some((id, panel_values)) = reselect {
        app.select_model(&id);
        apply_panel_values(app, &panel_values);
    }
    app.sweep.clear_session();
}

/// Writes each value a spec holds fixed into the Parameters tab, as an edit there would.
fn apply_panel_values(app: &mut AppState, panel_values: &[Option<ParamValue>]) {
    let Some(entry) = app.selected_entry().cloned() else {
        return;
    };
    let loaded = app.selection_is_loaded();
    let mut sent_live = false;
    for (index, (value, descriptor)) in panel_values.iter().zip(entry.param_descriptors()).enumerate() {
        let Some(value) = value else {
            continue;
        };
        if app.param_values.get(index) == Some(value) {
            continue;
        }
        app.param_values[index] = value.clone();
        if !descriptor.is_live() {
            app.pending_reload[index] = true;
        } else if loaded && app.send_live_param(index, value.clone()) {
            sent_live = true;
        }
    }
    if sent_live {
        app.mark_opened_run_modified();
    }
}

/// Sets the design of the selected model's draft to the rows of `file`.
fn load_table(app: &mut AppState, file: DialogFile) {
    let text = match String::from_utf8(file.bytes) {
        Ok(text) => text,
        Err(error) => {
            app.sweep.status = Some(format!("Design table load failed: {error}"));
            return;
        }
    };
    if let Some(draft) = selected_draft(app) {
        draft.design = DraftDesign::Table;
        draft.table = Some(DesignTableDraft {
            file_name: file.name,
            text,
        });
    }
}

/// Returns `duration` rounded down for a progress line.
///
/// It gives "under 1 ms" below a millisecond, milliseconds under a second, seconds under a minute, minutes and
/// seconds under ten minutes, minutes under an hour, and hours and minutes past that.
pub fn format_duration(duration: Duration) -> String {
    let seconds = duration.as_secs();
    let (hours, minutes) = (seconds / 3600, seconds % 3600 / 60);
    match seconds {
        0 if duration.subsec_millis() == 0 => "under 1 ms".to_owned(),
        0 => format!("{} ms", duration.subsec_millis()),
        1..60 => format!("{seconds} s"),
        60..600 => format!("{minutes} min {} s", seconds % 60),
        600..3600 => format!("{minutes} min"),
        _ => format!("{hours} h {minutes} min"),
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::sync::Arc;
    use std::time::Duration;

    use henad_compute::cpu::sim_thread::WakeFn;
    use henad_core::export::StatColumns;
    use henad_core::params::ParamValue;
    use henad_explore::output::manifest::ManifestMode;
    use henad_explore::output::{EVALUATIONS_FILE, MANIFEST_FILE, RUNS_FILE};
    use henad_models::example_models;

    use super::{
        CheckSummary, ColumnsBuild, FolderResults, SweepPanel, folder_results, format_duration, sampled_stat_columns,
        settle_session_modals, tab_title,
    };
    use crate::icons::material_design_icons::MDI_FLASK_OUTLINE;
    use crate::ui::sweep::draft::{COLUMNS_PENDING, DraftIssue, DraftSite, IssueKind};
    use crate::ui::sweep::layout::SweepSection;
    use crate::ui::sweep::session::SessionState;

    /// Folder under the system's temporary folder, removed when dropped.
    struct ScratchFolder(PathBuf);

    impl Drop for ScratchFolder {
        fn drop(&mut self) {
            drop(std::fs::remove_dir_all(&self.0));
        }
    }

    #[test]
    fn a_duration_is_rounded_to_the_units_a_progress_line_needs() {
        assert_eq!(format_duration(Duration::ZERO), "under 1 ms");
        assert_eq!(format_duration(Duration::from_micros(900)), "under 1 ms");
        assert_eq!(format_duration(Duration::from_micros(40_700)), "40 ms");
        assert_eq!(format_duration(Duration::from_millis(480)), "480 ms");
        assert_eq!(format_duration(Duration::from_millis(999)), "999 ms");
        assert_eq!(format_duration(Duration::from_millis(1_000)), "1 s");
        assert_eq!(format_duration(Duration::from_millis(45_900)), "45 s");
        assert_eq!(format_duration(Duration::from_secs(250)), "4 min 10 s");
        assert_eq!(format_duration(Duration::from_secs(1_530)), "25 min");
        assert_eq!(format_duration(Duration::from_secs(4_830)), "1 h 20 min");
    }

    #[test]
    fn a_session_closes_the_modals_it_leaves_nothing_to_answer() {
        let (mut confirm_abort, mut confirm_replace) = (true, true);
        assert!(settle_session_modals(
            SessionState::Running,
            &mut confirm_abort,
            &mut confirm_replace
        ));
        assert!(confirm_abort, "a running sweep can still be aborted");
        assert!(!confirm_replace, "the form that asked to replace results is gone");

        assert!(!settle_session_modals(
            SessionState::Aborted,
            &mut confirm_abort,
            &mut confirm_replace
        ));
        assert!(!confirm_abort, "an ended sweep has nothing to abort");

        let mut panel = SweepPanel {
            confirm_abort: true,
            confirm_replace: true,
            ..SweepPanel::default()
        };
        panel.clear_session();
        assert!(
            !panel.confirm_abort && !panel.confirm_replace,
            "the form opens with no modal"
        );
    }

    #[test]
    fn the_tab_title_follows_the_session() {
        let title = |state| tab_title(state).trim_start_matches(MDI_FLASK_OUTLINE).trim().to_owned();
        assert_eq!(title(None), "Sweep");
        assert_eq!(title(Some((SessionState::Planning, 0.0))), "Sweep 0%");
        assert_eq!(title(Some((SessionState::Running, 0.429))), "Sweep 42%");
        assert_eq!(title(Some((SessionState::Paused, 0.5))), "Sweep paused");
        assert_eq!(title(Some((SessionState::Finished, 1.0))), "Sweep done");
        assert_eq!(title(Some((SessionState::Aborted, 0.3))), "Sweep stopped");
        assert_eq!(title(Some((SessionState::Stopped, 0.3))), "Sweep stopped");
        assert_eq!(title(Some((SessionState::Failed, 0.3))), "Sweep failed");
    }

    #[test]
    fn a_folder_names_the_mode_of_the_results_it_holds() {
        let folder = ScratchFolder(std::env::temp_dir().join(format!("henad-app-folder-{}", std::process::id())));
        drop(std::fs::remove_dir_all(&folder.0));
        assert_eq!(folder_results(&folder.0), None, "a missing folder is free");
        std::fs::create_dir_all(&folder.0).expect("the folder is created");
        std::fs::write(folder.0.join("notes.txt"), "kept").expect("a scratch file");
        assert_eq!(
            folder_results(&folder.0),
            None,
            "a file no sweep writes leaves the folder free"
        );

        std::fs::write(folder.0.join(RUNS_FILE), "").expect("a scratch file");
        let unknown = FolderResults {
            mode: None,
            resumable: false,
        };
        assert_eq!(folder_results(&folder.0), Some(unknown), "no manifest names the mode");
        let manifest = folder.0.join(MANIFEST_FILE);
        std::fs::write(&manifest, "not json").expect("a scratch file");
        assert_eq!(folder_results(&folder.0), Some(unknown));
        std::fs::write(
            &manifest,
            r#"{"format":"henad-explore","mode":"sweep","status":"complete"}"#,
        )
        .expect("a scratch file");
        assert_eq!(
            folder_results(&folder.0),
            Some(FolderResults {
                mode: Some(ManifestMode::Sweep),
                resumable: false,
            }),
            "a complete sweep leaves nothing to resume"
        );

        std::fs::remove_file(folder.0.join(RUNS_FILE)).expect("runs.csv is removed");
        std::fs::write(folder.0.join(EVALUATIONS_FILE), "").expect("a scratch file");
        std::fs::write(
            &manifest,
            r#"{"format":"henad-explore","mode":"search","status":"running"}"#,
        )
        .expect("a scratch file");
        assert_eq!(
            folder_results(&folder.0),
            Some(FolderResults {
                mode: Some(ManifestMode::Search),
                resumable: true,
            }),
            "a running search has runs to resume"
        );
    }

    #[test]
    fn a_start_failure_lasts_until_the_draft_changes() {
        let sir = example_models().get("sir").cloned().expect("SIR is registered");
        let schema = sir.schema();
        let panel_values: Vec<ParamValue> = sir
            .param_descriptors()
            .iter()
            .map(|descriptor| descriptor.kind.default_value())
            .collect();
        let mut panel = SweepPanel::default();
        panel.cached_check(&schema, &panel_values);
        panel.start_failure = Some("Sweep start failed: Folder holds search results".to_owned());

        panel.cached_check(&schema, &panel_values);
        assert!(panel.start_failure.is_some(), "the same draft keeps its failure");
        panel.draft_mut(&schema).output_dir_text = "elsewhere".to_owned();
        panel.cached_check(&schema, &panel_values);
        assert_eq!(panel.start_failure, None, "an edited draft drops it");
    }

    #[test]
    fn a_check_lists_its_issues_by_section_and_counts_them() {
        let sir = example_models().get("sir").cloned().expect("SIR is registered");
        let schema = sir.schema();
        let panel_values: Vec<ParamValue> = sir
            .param_descriptors()
            .iter()
            .map(|descriptor| descriptor.kind.default_value())
            .collect();
        let mut panel = SweepPanel::default();
        let draft = panel.draft_mut(&schema);
        draft.factors[2].vary = true;
        draft.factors[2].levels_text = "0.1:x".to_owned();
        draft.root_seed_text = "seed".to_owned();
        let check = panel.cached_check(&schema, &panel_values);
        let summary = CheckSummary::new(check, &sir, &schema);
        assert_eq!(summary.counts, None);
        let sections: Vec<SweepSection> = summary.lines.iter().map(|line| line.section).collect();
        assert_eq!(
            sections,
            [SweepSection::Parameters, SweepSection::Seeds],
            "in the order of the rows"
        );
        assert!(summary.lines.iter().all(|line| line.kind == IssueKind::Invalid));
        assert_eq!(summary.section_issues(SweepSection::Parameters).invalid, 1);
        assert_eq!(summary.section_issues(SweepSection::Design).total(), 0);
    }

    #[test]
    fn a_draft_waits_for_the_build_of_its_columns() {
        let boids = example_models().get("boids").cloned().expect("boids is registered");
        let schema = boids.schema();
        let panel_values: Vec<ParamValue> = boids
            .param_descriptors()
            .iter()
            .map(|descriptor| descriptor.kind.default_value())
            .collect();
        let wake: WakeFn = Arc::new(|| {});
        let pending = [DraftIssue::missing(DraftSite::Outputs, COLUMNS_PENDING)];

        let mut panel = SweepPanel::default();
        let (sender, receiver) = flume::bounded(1);
        panel
            .column_builds
            .insert(boids.id().to_owned(), ColumnsBuild::Running(receiver));
        panel.learn_stat_columns(&boids, &wake);
        let check = panel.cached_check(&schema, &panel_values);
        assert_eq!(check.result.as_ref().err().map(Vec::as_slice), Some(&pending[..]));
        sender
            .send(sampled_stat_columns(&boids, None))
            .expect("the panel holds the receiver");
        panel.learn_stat_columns(&boids, &wake);
        let check = panel.cached_check(&schema, &panel_values);
        assert!(check.result.is_ok(), "the issue clears once the build reports");
        assert!(check.draft.stat_columns.is_some());

        let mut panel = SweepPanel::default();
        let (sender, receiver) = flume::bounded::<Option<StatColumns>>(1);
        panel
            .column_builds
            .insert(boids.id().to_owned(), ColumnsBuild::Running(receiver));
        panel.learn_stat_columns(&boids, &wake);
        assert!(panel.cached_check(&schema, &panel_values).result.is_err());
        drop(sender);
        panel.learn_stat_columns(&boids, &wake);
        let check = panel.cached_check(&schema, &panel_values);
        assert!(
            check.result.is_ok(),
            "a failed build leaves the stat labels to check against"
        );
        assert!(check.draft.stat_columns.is_none());
    }

    #[test]
    fn folder_messages_name_a_sweep_or_a_search() {
        let results = |mode| FolderResults { mode, resumable: false };
        assert_eq!(
            results(Some(ManifestMode::Search)).warning(),
            "This folder holds search results. Select another folder without results."
        );
        assert_eq!(
            results(Some(ManifestMode::Sweep)).warning(),
            "This folder holds sweep results. Select another folder without results."
        );
        assert_eq!(
            results(None).warning(),
            "This folder holds results. Select another folder without results."
        );
        assert_eq!(
            results(Some(ManifestMode::Search)).refusal(),
            "Folder holds search results"
        );
        assert_eq!(
            results(Some(ManifestMode::Sweep)).refusal(),
            "Folder holds sweep results"
        );
    }
}
