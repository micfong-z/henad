//! Header of the Sweep tab: the mode switch or the session's state, the model, the Plan toggle and the Spec menu,
//! over a line for notifications and the mode's description.

use egui::containers::Sides;
use egui::{Button, Color32, CornerRadius, Frame, Margin, RichText, WidgetInfo, WidgetType};

use crate::icons::material_design_icons::{
    MDI_ALERT, MDI_CHECK, MDI_CLOSE, MDI_DOCK_RIGHT, MDI_FILE_COG_OUTLINE, MDI_TRAY_ARROW_DOWN, MDI_TRAY_ARROW_UP,
};
use crate::options::cli_phrase;
use crate::ui::mcs;
use crate::ui::sweep::SweepRequest;
use crate::ui::sweep::draft::DraftMode;
use crate::ui::sweep::layout::{add_button, icon_button, segmented, truncated_label};
use crate::ui::sweep::session::SessionState;

/// Id of the header panel.
pub const HEADER_ID: &str = "henad_sweep_header";

/// Space above and below the header's two rows, in points.
const HEADER_MARGIN_Y: i8 = 2;

/// Returns the height of the header panel in `style`: two rows of controls, the space between and around them, and
/// the separator line under them.
pub fn header_height(style: &egui::Style) -> f32 {
    let spacing = &style.spacing;
    2.0 * spacing.interact_size.y + spacing.item_spacing.y + 2.0 * f32::from(HEADER_MARGIN_Y) + separator_width(style)
}

/// Frame of the header panel.
pub fn header_frame() -> Frame {
    Frame::NONE.inner_margin(Margin::symmetric(0, HEADER_MARGIN_Y))
}

/// Width of a panel's separator line in `style`. The panel reserves it beside its frame.
pub fn separator_width(style: &egui::Style) -> f32 {
    style.visuals.widgets.noninteractive.bg_stroke.width.round()
}

/// Contents of the header while the tab builds a sweep.
pub struct BuilderHeader<'a> {
    pub model_name: &'a str,
    /// Line reporting the last save or load, `None` when there is nothing to report.
    pub notification: Option<&'a str>,
    /// Whether the model cannot be swept in a browser, as for a GPU model.
    pub gpu_refused: bool,
    /// Program that the advice refers to, `None` when the product has no command-line program.
    pub cli_command: Option<&'a str>,
    /// Whether the Plan panel fits beside the form, and its toggle shows.
    pub plan_fits: bool,
}

/// Draws the header of the builder, with the mode switch writing `mode` and the Plan toggle writing `plan_open`.
///
/// Returns whether the mode changed.
pub fn builder_header(
    ui: &mut egui::Ui,
    header: &BuilderHeader<'_>,
    mode: &mut DraftMode,
    plan_open: &mut bool,
    request: &mut Option<SweepRequest>,
) -> bool {
    let spec_menu = SpecMenu {
        noun: noun(*mode),
        save_refusal: None,
        load_refusal: None,
    };
    let ((mode_changed, model_clicked), trailing) = Sides::new().shrink_left().truncate().show(
        ui,
        |ui| {
            let mode_changed = mode_switch(ui, mode);
            let model = ui
                .add(
                    Button::new(RichText::new(header.model_name).strong())
                        .frame(false)
                        .truncate(),
                )
                .on_hover_text("Switch models using the Model tab");
            (mode_changed, model.clicked())
        },
        |ui| {
            let mut trailing = None;
            trailing_controls(ui, &spec_menu, header.plan_fits, plan_open, &mut trailing);
            trailing
        },
    );
    if model_clicked {
        *request = Some(SweepRequest::ShowModel);
    }
    if trailing.is_some() {
        *request = trailing;
    }
    let description = if header.gpu_refused {
        Line {
            text: format!(
                "GPU sweeps are unavailable in a browser. Save spec and run it in the desktop app or {}.",
                cli_phrase(header.cli_command)
            ),
            color: Some(ui.visuals().error_fg_color),
        }
    } else {
        Line {
            text: match mode {
                DraftMode::Sweep => "Runs every configuration of a design, each once per replicate.",
                DraftMode::Search => "Selects configurations in batches, based on earlier results.",
            }
            .to_owned(),
            color: Some(ui.visuals().weak_text_color()),
        }
    };
    notification_row(ui, header.notification, description, request);
    mode_changed
}

/// Contents of the header while a session exists.
pub struct SessionHeader<'a> {
    pub state: SessionState,
    /// "sweep" or "search".
    pub noun: &'static str,
    pub model_name: &'a str,
    pub notification: Option<&'a str>,
    /// Whether the session resumes a folder. Such a session has no draft.
    pub resumed: bool,
    pub plan_fits: bool,
}

/// Draws the header of a running or ended session, with the Plan toggle writing `plan_open`.
pub fn session_header(
    ui: &mut egui::Ui,
    header: &SessionHeader<'_>,
    plan_open: &mut bool,
    request: &mut Option<SweepRequest>,
) {
    let load_refusal = if header.state.is_running() {
        format!("A {} is running", header.noun)
    } else {
        format!("Press {} to load a spec", edit_label(header.noun, header.resumed))
    };
    let spec_menu = SpecMenu {
        noun: header.noun,
        save_refusal: header
            .resumed
            .then(|| format!("This {}'s spec is already in the folder's manifest.json", header.noun)),
        load_refusal: Some(load_refusal),
    };
    Sides::new().shrink_left().truncate().show(
        ui,
        |ui| {
            status_badge(ui, header.state);
            let noun = if header.noun == "search" { "Search" } else { "Sweep" };
            ui.add(egui::Label::new(RichText::new(format!("{noun} of {}", header.model_name)).strong()).truncate());
        },
        |ui| trailing_controls(ui, &spec_menu, header.plan_fits, plan_open, request),
    );
    let text = match header.state {
        SessionState::Planning => "Building first configuration to plan runs.".to_owned(),
        SessionState::Running => "Results will appear as runs finish. Press Show results to open them.".to_owned(),
        SessionState::Paused => "Runs in progress are paused. Press Resume to continue.".to_owned(),
        SessionState::Finished | SessionState::Aborted | SessionState::Stopped | SessionState::Failed
            if header.resumed =>
        {
            format!("Press New {} to set up another.", header.noun)
        }
        SessionState::Finished | SessionState::Aborted | SessionState::Stopped | SessionState::Failed => format!(
            "Press {} to change the settings and run again.",
            edit_label(header.noun, false)
        ),
    };
    let line = Line {
        text,
        color: Some(ui.visuals().weak_text_color()),
    };
    notification_row(ui, header.notification, line, request);
}

/// Returns the label of the button that leaves an ended session: "Edit sweep", or "New sweep" after a session that
/// resumed a folder.
pub fn edit_label(noun: &str, resumed: bool) -> String {
    if resumed {
        format!("New {noun}")
    } else {
        format!("Edit {noun}")
    }
}

/// Returns "sweep" or "search" for a draft in `mode`.
pub fn noun(mode: DraftMode) -> &'static str {
    match mode {
        DraftMode::Sweep => "sweep",
        DraftMode::Search => "search",
    }
}

/// Draws the Sweep and Search switch writing `mode`, and returns whether it changed.
fn mode_switch(ui: &mut egui::Ui, mode: &mut DraftMode) -> bool {
    let choices = [
        (DraftMode::Sweep, "Sweep", "Run every configuration of a design"),
        (
            DraftMode::Search,
            "Search",
            "Optimize an objective, or explore a grid of two outputs",
        ),
    ];
    segmented(ui, mode, &choices, |_| Ok(()))
}

/// Spec menu of the header, and the reason each item is disabled.
struct SpecMenu {
    noun: &'static str,
    save_refusal: Option<String>,
    load_refusal: Option<String>,
}

/// Draws the Spec menu and, while the Plan panel fits, the Plan toggle writing `plan_open`, from right to left.
fn trailing_controls(
    ui: &mut egui::Ui,
    spec_menu: &SpecMenu,
    plan_fits: bool,
    plan_open: &mut bool,
    request: &mut Option<SweepRequest>,
) {
    let menu = ui.menu_button(format!("{MDI_FILE_COG_OUTLINE} Spec"), |ui| {
        let save = add_button(
            ui,
            spec_menu.save_refusal.is_none(),
            Button::new(format!("{MDI_TRAY_ARROW_DOWN} Save spec")),
            "Save spec",
        )
        .on_hover_text(format!("Save {} to TOML file", spec_menu.noun));
        let save = match &spec_menu.save_refusal {
            Some(reason) => save.on_disabled_hover_text(reason),
            None => save,
        };
        if save.clicked() {
            *request = Some(SweepRequest::SaveSpec);
        }
        let load = add_button(
            ui,
            spec_menu.load_refusal.is_none(),
            Button::new(format!("{MDI_TRAY_ARROW_UP} Load spec")),
            "Load spec",
        )
        .on_hover_text(
            "Load sweep or search from TOML file. Current settings and any Parameters tab values the spec sets will \
             be replaced.",
        );
        let load = match &spec_menu.load_refusal {
            Some(reason) => load.on_disabled_hover_text(reason),
            None => load,
        };
        if load.clicked() {
            *request = Some(SweepRequest::LoadSpec);
        }
    });
    let menu = menu.response.on_hover_text("Save or load spec file");
    menu.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, "Spec"));

    if plan_fits {
        let toggle = ui
            .add(Button::selectable(*plan_open, format!("{MDI_DOCK_RIGHT} Plan")))
            .on_hover_text("Show plan beside settings");
        if toggle.clicked() {
            *plan_open = !*plan_open;
        }
        let selected = *plan_open;
        toggle.widget_info(|| WidgetInfo::selected(WidgetType::Button, true, selected, "Plan"));
    }
}

/// A line of text in a colour, `None` for the text colour.
struct Line {
    text: String,
    color: Option<Color32>,
}

/// Draws `notification` with a Dismiss button when set, and `fallback` when not.
///
/// A notification of a failure is drawn in the error colour with an alert icon.
fn notification_row(ui: &mut egui::Ui, notification: Option<&str>, fallback: Line, request: &mut Option<SweepRequest>) {
    let line = match notification {
        Some(text) if is_failure(text) => Line {
            text: format!("{MDI_ALERT} {text}"),
            color: Some(ui.visuals().error_fg_color),
        },
        Some(text) => Line {
            text: text.to_owned(),
            color: None,
        },
        None => fallback,
    };
    let (_, dismissed) = Sides::new().shrink_left().truncate().show(
        ui,
        |ui| truncated_label(ui, line.text, line.color),
        |ui| notification.is_some() && icon_button(ui, MDI_CLOSE, "Dismiss", "Dismiss").clicked(),
    );
    if dismissed {
        *request = Some(SweepRequest::DismissNotification);
    }
}

/// Returns whether the notification `text` reports a failure, as in "Spec save failed: ...".
fn is_failure(text: &str) -> bool {
    text.contains("failed:")
}

/// Draws the state of a session as a coloured badge.
fn status_badge(ui: &mut egui::Ui, state: SessionState) {
    let (text, fill, color) = match state {
        SessionState::Planning => ("Planning".to_owned(), mcs::GRAY_800, mcs::GRAY_200),
        SessionState::Running => ("Running".to_owned(), mcs::BLUE_900, mcs::BLUE_200),
        SessionState::Paused => ("Paused".to_owned(), mcs::ORANGE_900, mcs::ORANGE_200),
        SessionState::Finished => (format!("{MDI_CHECK} Finished"), mcs::GREEN_900, mcs::GREEN_200),
        SessionState::Aborted => ("Aborted".to_owned(), mcs::ORANGE_900, mcs::ORANGE_200),
        SessionState::Stopped => ("Stopped".to_owned(), mcs::ORANGE_900, mcs::ORANGE_200),
        SessionState::Failed => ("Failed".to_owned(), mcs::RED_900, mcs::RED_200),
    };
    Frame::new()
        .fill(fill)
        .corner_radius(CornerRadius::ZERO)
        .inner_margin(Margin::symmetric(6, 1))
        .show(ui, |ui| ui.colored_label(color, text));
}

#[cfg(test)]
mod tests {
    use super::{edit_label, header_height, is_failure};

    #[test]
    fn the_header_holds_two_rows_and_its_separator() {
        let style = egui::Style::default();
        let spacing = &style.spacing;
        let separator = style.visuals.widgets.noninteractive.bg_stroke.width.round();
        assert_eq!(
            header_height(&style),
            2.0 * spacing.interact_size.y + spacing.item_spacing.y + 4.0 + separator
        );
    }

    #[test]
    fn an_ended_session_is_edited_unless_it_resumed_a_folder() {
        assert_eq!(edit_label("sweep", false), "Edit sweep");
        assert_eq!(edit_label("search", false), "Edit search");
        assert_eq!(edit_label("sweep", true), "New sweep");
    }

    #[test]
    fn a_failed_save_or_load_reads_as_a_failure() {
        assert!(is_failure("Spec load failed: no such file"));
        assert!(is_failure("Save failed: disk full"));
        assert!(!is_failure("Loaded sir.toml"));
        assert!(!is_failure("Save canceled"));
    }
}
