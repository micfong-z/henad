//! The expert mode around a sheet's node graph copy: the conversion, the copy's banner, the sheet's line that opens
//! the copy, and the side panel's section about the copy's source.
//!
//! A *conversion* copies a sheet to a node graph with an id and a name of its own, through
//! [`henad_graph::sheet::convert`]. The copy opens in the Editor, and the sheet stays in the Editor's memory as the
//! copy's *counterpart*, so one press returns to it and another to the copy, each with its own undo history. On native,
//! a sheet opened from a file has its copy written beside that file.
//!
//! The *original* of a copy is the sheet its origin record names: the counterpart when it holds that sheet, or else
//! the file the record names beside the copy's own file. The banner compares the original's text with the record to
//! tell an unedited copy, an edited one, a copy whose original changed since, and one whose original is not found.
//! For an edited copy, Show changes lists the nodes added, removed and changed by frame in the side panel and marks
//! them on the canvas.

use std::hash::{DefaultHasher, Hash as _, Hasher as _};
use std::path::PathBuf;

use egui::{Id, Modal, RichText};
use henad_graph::GraphFile;
use henad_graph::editor::{ModelOrigin, NoteLayout};
use henad_graph::sheet::sentence::{
    CANCEL, COMPARE_NEEDS_NATIVE, COMPARE_RUNS, CONVERT, CONVERT_HEADING, FROM_AUTHOR_MODEL, SHOW_CHANGES,
    convert_text, converted_banner, converted_date, edited_banner, node_edits, open_original, original_changed_banner,
    original_not_beside,
};
use henad_graph::sheet::{
    ChangeGroup, ChangesError, ConvertOptions, ConvertRefusal, KIND_SHEET, SheetFile, changes_since_conversion, convert,
};
use web_time::{SystemTime, UNIX_EPOCH};

use super::graph::GraphEditor;
use super::graph::changes::CopyChanges;
use super::sheet::SheetEditor;
use super::sheet::widgets::{
    DialogButton, DialogButtonKind, Notice, NoticeKind, dialog_buttons, dialog_frame, dialog_title,
};
use super::{BannerKey, EditorModel, EditorPanel, EditorRequest, FileStatus};
use crate::state::AppState;
use crate::ui::dock::Tab;

/// Width in points of the conversion dialog.
const DIALOG_WIDTH: f32 = 560.0;

/// The model the Editor holds beside the one it shows: the sheet of a copy converted in this session, or the copy of a
/// sheet whose original was opened from the copy.
#[derive(Debug)]
pub struct Counterpart {
    /// The model.
    pub model: EditorModel,
    /// Name of the model's file, `None` for a model with no file.
    pub file_name: Option<String>,
    /// Path of the model's file on native, `None` for a model with no file or in a browser.
    pub file_path: Option<PathBuf>,
    /// Edit mark of the model when it was opened, made or last saved, as [`EditorPanel`] keeps it for the model shown.
    pub saved_mark: u64,
    /// Edit mark of the model when its save started, until the save reports.
    pub saving: Option<u64>,
}

impl Counterpart {
    /// Returns whether the model holds edits since it was opened, made or last saved. An empty model holds none.
    pub fn is_unsaved(&self) -> bool {
        !self.model.is_empty() && self.model.edit_mark() != self.saved_mark
    }
}

/// State of a copy against its original, which the banner shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CopyState {
    /// The copy is as the conversion wrote it, and the original is unchanged.
    Unedited,
    /// The copy was edited since the conversion, and the edits change the nodes `edits` names.
    Edited(String),
    /// The original changed since the conversion.
    OriginalChanged,
    /// The original is not found.
    NotFound,
}

/// Banner of a converted copy: the copy's origin record, its state against the original and the nodes its edits
/// changed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CopyBanner {
    /// Origin record of the copy.
    pub origin: ModelOrigin,
    /// State of the copy against its original.
    pub state: CopyState,
    /// Nodes added, removed and changed since the conversion, by frame, empty unless the state is
    /// [`CopyState::Edited`].
    pub changes: CopyChanges,
}

impl CopyBanner {
    /// Returns the banner's text.
    pub fn text(&self) -> String {
        let origin = &self.origin;
        match &self.state {
            CopyState::Unedited => converted_banner(&origin.name, &origin.converted),
            CopyState::Edited(edits) => edited_banner(&origin.name, &origin.converted, edits),
            CopyState::OriginalChanged => original_changed_banner(&origin.name),
            CopyState::NotFound => original_not_beside(&origin.name),
        }
    }

    /// Returns whether the banner offers to open the original.
    pub fn opens_original(&self) -> bool {
        self.state != CopyState::NotFound
    }

    /// Returns whether the banner offers to list the changes since the conversion.
    ///
    /// Note that an edit that changes no node, such as a new category or a parameter's default, leaves the copy
    /// edited with no change to list. The banner offers nothing to show then.
    pub fn shows_changes(&self) -> bool {
        matches!(self.state, CopyState::Edited(_)) && !self.changes.is_empty()
    }

    /// Returns whether the banner offers to run the copy beside its original. The original has to be the sheet the
    /// copy was converted from.
    pub fn compares_runs(&self) -> bool {
        matches!(self.state, CopyState::Unedited | CopyState::Edited(_))
    }
}

/// Returns the reason `sheet` does not convert, `None` for a sheet that converts.
pub fn convert_refusal(sheet: &SheetEditor) -> Option<ConvertRefusal> {
    ConvertRefusal::of(sheet.report()).or_else(|| sheet.has_errors().then_some(ConvertRefusal::Errors))
}

/// Returns today's date in UTC, written as `2026-11-07`.
pub fn today() -> String {
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since_epoch| since_epoch.as_secs());
    let (year, month, day) = civil_date(seconds / 86_400);
    format!("{year:04}-{month:02}-{day:02}")
}

/// Returns the year, month and day of the date `days` after 1970-01-01 in the proleptic Gregorian calendar.
///
/// The algorithm is Howard Hinnant's `civil_from_days`, over eras of 400 years that start on 1 March.
fn civil_date(days: u64) -> (u64, u64, u64) {
    let shifted = days + 719_468;
    let era = shifted / 146_097;
    let day_of_era = shifted % 146_097;
    let year_of_era = (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_from_march = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_from_march + 2) / 5 + 1;
    let month = if month_from_march < 10 {
        month_from_march + 3
    } else {
        month_from_march - 9
    };
    let year = year_of_era + era * 400 + u64::from(month <= 2);
    (year, month, day)
}

/// Opens the conversion dialog for the sheet in the Editor, unless the sheet does not convert.
pub fn open_convert(panel: &mut EditorPanel) {
    if let EditorModel::Sheet(sheet) = &panel.model
        && convert_refusal(sheet).is_none()
    {
        panel.convert = Some(sheet.document().sheet.name.clone());
    }
}

/// Draws the conversion dialog while [`EditorPanel::convert`] holds the sheet's name, and returns whether Convert was
/// pressed.
///
/// Cancel, Escape and a click beside the dialog close it.
pub fn convert_modal(ctx: &egui::Context, panel: &mut EditorPanel) -> bool {
    let Some(name) = &panel.convert else {
        return false;
    };
    let mut confirmed = false;
    let mut closed = false;
    let modal = Modal::new(Id::new("henad_convert_modal")).frame(dialog_frame(ctx));
    let response = modal.show(ctx, |ui| {
        dialog_title(ui, CONVERT_HEADING, CONVERT_HEADING, DIALOG_WIDTH);
        for (index, paragraph) in convert_text(name).iter().enumerate() {
            if index > 0 {
                ui.add_space(8.0);
            }
            for line in paragraph.lines() {
                ui.label(line);
            }
        }
        let buttons = [
            DialogButton::new(CANCEL, DialogButtonKind::Plain),
            DialogButton::new(CONVERT, DialogButtonKind::Primary),
        ];
        match dialog_buttons(ui, &buttons) {
            Some(0) => closed = true,
            Some(_) => confirmed = true,
            None => {}
        }
    });
    if confirmed || closed || response.should_close() {
        panel.convert = None;
    }
    confirmed
}

/// Converts the sheet in the Editor to a node graph copy, opens the copy in its place and keeps the sheet as the
/// counterpart.
///
/// The copy takes the first id and name that no model of the app and no file beside the sheet holds. On native, a
/// sheet with a file has its copy written beside it, and the status line names the file. A sheet that does not convert
/// leaves the Editor alone.
pub fn convert_sheet(app: &mut AppState) {
    let panel = &app.editor;
    let EditorModel::Sheet(sheet) = &panel.model else {
        return;
    };
    let Some(file) = sheet.file() else {
        return;
    };
    let folder = panel
        .file_path
        .as_ref()
        .and_then(|path| path.parent())
        .map(PathBuf::from);
    let models = &app.models;
    let taken = |id: &str, name: &str| {
        models.get(id).is_some()
            || models.iter().any(|entry| entry.name() == name)
            || folder
                .as_ref()
                .is_some_and(|folder| folder.join(format!("{id}.toml")).exists())
    };
    let mut options = ConvertOptions::new(sheet.document(), &today(), taken);
    if let Some(file_name) = panel.file_name.as_ref().filter(|name| name.ends_with(".toml")) {
        options.file.clone_from(file_name);
    }
    let copy = match convert(file, &options) {
        Ok(copy) => copy,
        Err(error) => {
            app.editor.file_status = Some(FileStatus::failure(error.to_string()));
            return;
        }
    };
    let copy_name = format!("{}.toml", options.id);
    let (copy_path, status) = save_beside(folder.as_ref(), &copy_name, &copy);
    let panel = &mut app.editor;
    let counterpart = Counterpart {
        model: std::mem::take(&mut panel.model),
        file_name: panel.file_name.take(),
        file_path: panel.file_path.take(),
        saved_mark: panel.saved_mark,
        saving: panel.saving.take(),
    };
    panel.load_graph(&copy, Some(copy_name));
    panel.file_path = copy_path;
    panel.counterpart = Some(Box::new(counterpart));
    panel.file_status = status;
    app.focus_request = Some(Tab::Editor);
}

/// Writes `copy` as `copy_name` into `folder`, and returns its path with the status line to show, or no path and no
/// status without a folder.
#[cfg(not(target_arch = "wasm32"))]
fn save_beside(folder: Option<&PathBuf>, copy_name: &str, copy: &GraphFile) -> (Option<PathBuf>, Option<FileStatus>) {
    let Some(folder) = folder else {
        return (None, None);
    };
    let path = folder.join(copy_name);
    match std::fs::write(&path, copy.to_toml()) {
        Ok(()) => (Some(path), Some(FileStatus::note(format!("Saved {copy_name}")))),
        Err(error) => (None, Some(FileStatus::failure(format!("Save failed: {error}")))),
    }
}

/// Returns no path and no status. A browser writes no file beside another.
#[cfg(target_arch = "wasm32")]
fn save_beside(
    _folder: Option<&PathBuf>,
    _copy_name: &str,
    _copy: &GraphFile,
) -> (Option<PathBuf>, Option<FileStatus>) {
    (None, None)
}

/// Reads the original of the graph in the Editor from beside its file into [`EditorPanel::beside`], or clears it for
/// a graph with no origin, no file or no original beside it.
pub fn find_original(panel: &mut EditorPanel) {
    panel.beside = None;
    let Some(origin) = panel
        .model
        .graph()
        .and_then(|graph| graph.editor_region().model_origin())
    else {
        return;
    };
    let Some(folder) = panel.file_path.as_ref().and_then(|path| path.parent()) else {
        return;
    };
    let path = folder.join(&origin.file);
    let file = std::fs::read_to_string(&path)
        .ok()
        .and_then(|text| SheetFile::parse(&text).ok());
    if let Some(file) = file
        && file.document().sheet.id == origin.model
    {
        panel.beside = Some((path, Box::new(file)));
    }
}

/// Original of the graph in the Editor, as the banner reads it.
enum Original<'a> {
    /// A sheet that lowers.
    Sheet(&'a SheetFile),
    /// The sheet the record names, holding errors that stop it lowering.
    Broken,
    /// No sheet the record names.
    Missing,
}

/// Returns whether the counterpart holds the sheet that `origin` names.
fn counterpart_is_original(panel: &EditorPanel, origin: &ModelOrigin) -> bool {
    origin.kind == KIND_SHEET
        && panel
            .counterpart
            .as_ref()
            .and_then(|counterpart| counterpart.model.sheet())
            .is_some_and(|sheet| sheet.document().sheet.id == origin.model)
}

/// Returns the original that `origin` names: the counterpart when it holds that sheet, else the sheet read from beside
/// the copy.
fn original<'a>(panel: &'a EditorPanel, origin: &ModelOrigin) -> Original<'a> {
    if origin.kind != KIND_SHEET {
        return Original::Missing;
    }
    if counterpart_is_original(panel, origin) {
        let file = panel
            .counterpart
            .as_ref()
            .and_then(|counterpart| counterpart.model.sheet())
            .and_then(SheetEditor::file);
        return file.map_or(Original::Broken, Original::Sheet);
    }
    match &panel.beside {
        Some((_, file)) => Original::Sheet(file),
        None => Original::Missing,
    }
}

/// Returns the canonical text of the original of the graph in the Editor, `None` for a graph with no original that
/// lowers.
pub fn original_text(panel: &EditorPanel) -> Option<String> {
    let origin = panel.model.graph()?.editor_region().model_origin()?;
    match original(panel, &origin) {
        Original::Sheet(file) => Some(file.to_toml().to_owned()),
        Original::Broken | Original::Missing => None,
    }
}

/// Returns a hash of `text`, which keys the banner on the original's text without a copy of it.
fn text_hash(text: &str) -> u64 {
    let mut hasher = DefaultHasher::new();
    text.hash(&mut hasher);
    hasher.finish()
}

/// Returns the key of the copy `graph` and its original `original`: the load and code revision of `graph` and the hash
/// of the text of `original`. The banner and a comparison are keyed on it.
fn copy_key(graph: &GraphEditor, original: &Original<'_>) -> BannerKey {
    let original_hash = match original {
        Original::Sheet(file) => Some(text_hash(file.to_toml())),
        Original::Broken | Original::Missing => None,
    };
    let [load, revision, _] = graph.preview_key();
    (load, revision, original_hash)
}

/// Returns the key of the copy in the Editor and its original, `None` while the Editor shows no copy converted from a
/// sheet.
///
/// The key changes when the copy is loaded again or edited, and when the original's text changes.
pub fn current_copy_key(panel: &EditorPanel) -> Option<BannerKey> {
    let graph = panel.model.graph()?;
    let origin = graph.editor_region().model_origin()?;
    Some(copy_key(graph, &original(panel, &origin)))
}

/// Returns the banner of the graph in the Editor, `None` for a graph that records no conversion from a sheet or for
/// another model.
///
/// The banner is worked out again only when the graph's code, the load or the original changes.
pub fn copy_banner(panel: &mut EditorPanel) -> Option<CopyBanner> {
    let graph = panel.model.graph()?;
    let origin = graph.editor_region().model_origin()?;
    let original = original(panel, &origin);
    let key = copy_key(graph, &original);
    if let Some((cached, banner)) = &panel.banner
        && *cached == key
    {
        return banner.clone();
    }
    let (state, changes) = match original {
        Original::Missing => (CopyState::NotFound, CopyChanges::default()),
        Original::Broken => (CopyState::OriginalChanged, CopyChanges::default()),
        Original::Sheet(file) => {
            let copy = graph.to_file()?;
            let edited = graph
                .code_hash()
                .is_some_and(|hash| format!("{hash:016x}") != origin.copy_code);
            match changes_since_conversion(&copy, file) {
                Err(ChangesError::OriginalChanged { .. }) => (CopyState::OriginalChanged, CopyChanges::default()),
                Err(_) => return None,
                Ok(_) if !edited => (CopyState::Unedited, CopyChanges::default()),
                Ok(groups) => {
                    let count = |list: fn(&ChangeGroup) -> usize| groups.iter().map(list).sum();
                    let edits = node_edits(
                        count(|group| group.added.len()),
                        count(|group| group.removed.len()),
                        count(|group| group.changed.len()),
                    );
                    (
                        CopyState::Edited(edits),
                        CopyChanges::new(groups, file.lowered().document()),
                    )
                }
            }
        }
    };
    let banner = Some(CopyBanner { origin, state, changes });
    panel.banner = Some((key, banner.clone()));
    banner
}

/// Draws `banner` as an information notice with its small buttons after its text, and sets `request` when a button is
/// pressed.
/// `showing_changes` is whether the changes since the conversion are listed, which Show changes toggles.
pub fn banner_ui(ui: &mut egui::Ui, banner: &CopyBanner, showing_changes: bool, request: &mut Option<EditorRequest>) {
    let text = banner.text();
    Notice::new(NoticeKind::Info, &text).show_with(ui, |ui| {
        if banner.opens_original() && ui.small_button(open_original(&banner.origin.name)).clicked() {
            *request = Some(EditorRequest::OpenOriginal);
        }
        if banner.compares_runs() {
            let enabled = cfg!(not(target_arch = "wasm32"));
            let compare = ui
                .add_enabled(enabled, egui::Button::new(COMPARE_RUNS).small())
                .on_disabled_hover_text(COMPARE_NEEDS_NATIVE);
            if compare.clicked() {
                *request = Some(EditorRequest::CompareRuns);
            }
        }
        let show = egui::Button::selectable(showing_changes, SHOW_CHANGES).small();
        if banner.shows_changes() && ui.add(show).clicked() {
            *request = Some(EditorRequest::ShowChanges);
        }
    });
}

/// Returns the changes of the copy in the Editor that the side panel lists and the canvas marks: the banner's changes
/// while Show changes is on, and none otherwise.
pub fn shown_changes(panel: &mut EditorPanel) -> CopyChanges {
    if !panel.show_changes {
        return CopyChanges::default();
    }
    copy_banner(panel).map(|banner| banner.changes).unwrap_or_default()
}

/// Returns the name of the copy the counterpart holds when it was converted from the sheet in the Editor, `None`
/// otherwise.
pub fn counterpart_copy(panel: &EditorPanel) -> Option<String> {
    let sheet = panel.model.sheet()?;
    let graph = panel.counterpart.as_ref()?.model.graph()?;
    let origin = graph.editor_region().model_origin()?;
    (origin.kind == KIND_SHEET && origin.model == sheet.document().sheet.id).then(|| graph.document().name.clone())
}

/// Swaps the model the Editor shows with its counterpart, file name and path included.
pub fn swap_counterpart(panel: &mut EditorPanel) {
    let Some(counterpart) = panel.counterpart.as_mut() else {
        return;
    };
    std::mem::swap(&mut panel.model, &mut counterpart.model);
    std::mem::swap(&mut panel.file_name, &mut counterpart.file_name);
    std::mem::swap(&mut panel.file_path, &mut counterpart.file_path);
    std::mem::swap(&mut panel.saved_mark, &mut counterpart.saved_mark);
    std::mem::swap(&mut panel.saving, &mut counterpart.saving);
    panel.settle_swap();
}

/// Opens the original of the graph in the Editor: the counterpart, or the sheet read from beside the copy, which then
/// takes the copy as its counterpart.
pub fn open_original_of(panel: &mut EditorPanel) {
    let Some(origin) = panel
        .model
        .graph()
        .and_then(|graph| graph.editor_region().model_origin())
    else {
        return;
    };
    if counterpart_is_original(panel, &origin) {
        swap_counterpart(panel);
        return;
    }
    match original(panel, &origin) {
        Original::Sheet(file) => {
            let document = file.document().clone();
            let Some((path, _)) = panel.beside.take() else {
                return;
            };
            let counterpart = Counterpart {
                model: std::mem::take(&mut panel.model),
                file_name: panel.file_name.take(),
                file_path: panel.file_path.take(),
                saved_mark: panel.saved_mark,
                saving: panel.saving.take(),
            };
            panel.load_sheet(SheetEditor::new(document), Some(origin.file.clone()));
            panel.file_path = Some(path);
            panel.counterpart = Some(Box::new(counterpart));
        }
        Original::Broken | Original::Missing => {}
    }
}

/// Draws the side panel's section about the source of a copy recorded in `origin`, then each of `notes`, the prose of
/// the source that has no place in the code.
pub fn origin_ui(ui: &mut egui::Ui, origin: &ModelOrigin, notes: &[NoteLayout]) {
    ui.strong(FROM_AUTHOR_MODEL);
    crate::ui::kv_grid(ui, "henad_expert_origin").show(ui, |ui, rows| {
        for (label, value) in [
            ("Model", origin.name.clone()),
            ("File", origin.file.clone()),
            ("Converted", converted_date(&origin.converted)),
        ] {
            rows.row(ui, label, value);
        }
    });
    for note in notes {
        ui.add_space(4.0);
        if !note.title.is_empty() {
            ui.label(RichText::new(&note.title).strong());
        }
        ui.label(&note.text);
    }
}

#[cfg(test)]
mod tests {
    use super::civil_date;

    #[test]
    fn days_since_the_epoch_read_as_dates() {
        assert_eq!(civil_date(0), (1970, 1, 1));
        assert_eq!(civil_date(20_399), (2025, 11, 7));
        assert_eq!(civil_date(20_764), (2026, 11, 7));
        assert_eq!(civil_date(20_765), (2026, 11, 8));
    }
}
