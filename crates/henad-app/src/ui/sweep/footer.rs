//! Footer of the Sweep tab, and the two modals its buttons open.
//!
//! While the tab builds a sweep, the footer holds the plan's total or its first problem over the destination and
//! Start. While a sweep runs, it holds the progress bar over Show results, Abort and Pause. Once the sweep ends, it
//! holds the result over Edit sweep, Save results and Show results. The last slot of the button row always holds the
//! next step: Start, then Pause or Resume, then Show results.

use std::path::Path;

use egui::containers::Sides;
use egui::{
    Button, Id, Key, KeyboardShortcut, Margin, Modal, Modifiers, Popup, PopupCloseBehavior, ProgressBar, RichText, vec2,
};
use henad_explore::handle::SweepProgress;

use crate::icons::material_design_icons::{
    MDI_ALERT, MDI_CHART_BOX_OUTLINE, MDI_FOLDER_OUTLINE, MDI_MEMORY, MDI_PAUSE, MDI_PENCIL_OUTLINE, MDI_PLAY,
    MDI_STOP, MDI_TRAY_ARROW_DOWN,
};
use crate::ui::sweep::draft::{DraftMode, DraftSite, IssueKind, MAX_DRAFT_RUNS, MAX_MEMORY_SERIES_BYTES};
use crate::ui::sweep::header::{edit_label, noun, separator_width};
use crate::ui::sweep::layout::{
    FOOTER_ICONS_ONLY_BELOW, IssueCount, Reveal, add_button, destructive_button, filled_button, issue_color,
    issue_icon, text_width, truncated_label,
};
use crate::ui::sweep::plan::folder_name;
use crate::ui::sweep::progress::{SessionResult, abort_text, bar_text, finished_fraction, resume_text};
use crate::ui::sweep::session::SessionState;
use crate::ui::sweep::{CheckSummary, IssueLine, SweepRequest};
use crate::ui::{add_progress_bar, mcs, plural};

/// Id of the footer panel.
pub const FOOTER_ID: &str = "henad_sweep_footer";

/// Space above and below the footer's two rows, in points.
const FOOTER_MARGIN_Y: i8 = 6;

/// Most problems the disabled Start button lists on hover.
const MAX_START_REASONS: usize = 3;

/// Shortcut that presses Start: Cmd+Enter on a Mac, Ctrl+Enter elsewhere.
pub const START_SHORTCUT: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::Enter);

/// Returns the height of the footer panel in `style`: two rows of controls, the space between and around them, and
/// the separator line over them.
pub fn footer_height(style: &egui::Style) -> f32 {
    let spacing = &style.spacing;
    2.0 * spacing.interact_size.y + spacing.item_spacing.y + 2.0 * f32::from(FOOTER_MARGIN_Y) + separator_width(style)
}

/// Frame of the footer panel.
pub fn footer_frame() -> egui::Frame {
    egui::Frame::NONE.inner_margin(Margin::symmetric(0, FOOTER_MARGIN_Y))
}

/// Colour of a line of the footer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineTone {
    Normal,
    Warn,
    Error,
}

/// First line of the builder's footer: the plan's total, or the reason it cannot start.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatusLine {
    pub text: String,
    pub tone: LineTone,
    pub tooltip: Option<String>,
}

/// Contents of the builder's footer.
pub struct BuilderFooter<'a> {
    pub mode: DraftMode,
    pub summary: &'a CheckSummary,
    /// Reason the last press of Start failed.
    pub start_failure: Option<&'a str>,
    /// Ticks each run steps, warm-up included.
    pub steps_per_run: u64,
    /// Folder the results go to, empty while none is chosen, `None` for results held in memory.
    pub folder: Option<&'a Path>,
    /// Whether the Results tab holds results in memory that are not saved.
    pub unsaved_results: bool,
    /// Whether those results come from a search.
    pub search_files: bool,
    /// Whether a modal is open over the tab.
    pub modal_open: bool,
}

impl BuilderFooter<'_> {
    /// Returns the first line of the footer.
    pub fn status_line(&self) -> StatusLine {
        let noun = noun(self.mode);
        if let Some(failure) = self.start_failure {
            return StatusLine {
                text: format!("{MDI_ALERT} {failure}"),
                tone: LineTone::Error,
                tooltip: None,
            };
        }
        if let Some(first) = self.summary.lines.first() {
            let lead = if noun == "search" {
                "Search not ready."
            } else {
                "Sweep not ready."
            };
            return match first.kind {
                IssueKind::Invalid => StatusLine {
                    text: format!("{MDI_ALERT} {lead} {}", first.text),
                    tone: LineTone::Error,
                    tooltip: None,
                },
                IssueKind::Missing => StatusLine {
                    text: format!("{lead} {}", first.text),
                    tone: LineTone::Normal,
                    tooltip: None,
                },
            };
        }
        let Some(counts) = self.summary.counts else {
            return StatusLine {
                text: "Runs not counted yet".to_owned(),
                tone: LineTone::Normal,
                tooltip: None,
            };
        };
        let near_runs = counts.2 >= MAX_DRAFT_RUNS / 2;
        let near_memory = self.folder.is_none()
            && self
                .summary
                .series_bytes
                .is_some_and(|bytes| bytes >= MAX_MEMORY_SERIES_BYTES / 2);
        let tooltip = if near_runs {
            Some(format!("Near the app's limit of {MAX_DRAFT_RUNS} runs"))
        } else if near_memory {
            Some(format!(
                "Series near the memory limit of {}",
                henad_core::helpers::fmt_bytes(MAX_MEMORY_SERIES_BYTES)
            ))
        } else {
            None
        };
        StatusLine {
            text: ready_sentence(self.mode, counts, self.steps_per_run),
            tone: if tooltip.is_some() {
                LineTone::Warn
            } else {
                LineTone::Normal
            },
            tooltip,
        }
    }
}

/// Returns the total of a plan in `mode` with `counts` of configurations or evaluations, replicates and runs, as in
/// "45 runs: 15 configurations × 3 replicates, 1100 steps each".
pub fn ready_sentence(mode: DraftMode, (configs, replicates, runs): (u64, u64, u64), steps_per_run: u64) -> String {
    let configs_noun = match mode {
        DraftMode::Sweep => "configuration",
        DraftMode::Search => "evaluation",
    };
    format!(
        "{runs} {}: {configs} {} × {replicates} {}, {steps_per_run} {} each",
        plural(runs, "run"),
        plural(configs, configs_noun),
        plural(replicates, "replicate"),
        plural(steps_per_run, "step"),
    )
}

/// Returns the text of the chip that counts `lines`: "2 problems" while any is invalid, "2 missing" while every one is
/// missing input, and `None` for none.
pub fn problems_chip(lines: &[IssueLine]) -> Option<(IssueKind, String)> {
    let mut count = IssueCount::default();
    for line in lines {
        count.add(line.kind);
    }
    Some((count.kind()?, count.phrase()?))
}

/// Returns the hover text of a disabled Start: up to [`MAX_START_REASONS`] problems of `lines`, and a count of the
/// rest.
pub fn start_refusal(lines: &[IssueLine]) -> String {
    let mut text = "Fix these first:".to_owned();
    for line in lines.iter().take(MAX_START_REASONS) {
        text.push('\n');
        text.push_str(&line.text);
    }
    if lines.len() > MAX_START_REASONS {
        text.push_str(&format!("\nand {} more", lines.len() - MAX_START_REASONS));
    }
    text
}

/// Draws the footer of the builder.
pub fn builder_footer(ui: &mut egui::Ui, footer: &BuilderFooter<'_>, request: &mut Option<SweepRequest>) {
    let compact = ui.available_width() < FOOTER_ICONS_ONLY_BELOW;
    let noun = noun(footer.mode);
    let ready = footer.summary.counts.is_some() && footer.summary.lines.is_empty();
    let line = footer.status_line();
    ui.add_enabled_ui(!footer.modal_open, |ui| {
        let (_, reveal) = Sides::new().shrink_left().truncate().show(
            ui,
            |ui| {
                let color = match line.tone {
                    LineTone::Normal => None,
                    LineTone::Warn => Some(ui.visuals().warn_fg_color),
                    LineTone::Error => Some(ui.visuals().error_fg_color),
                };
                let label = truncated_label(ui, line.text.clone(), color);
                if let Some(tooltip) = &line.tooltip {
                    label.on_hover_text(tooltip);
                }
            },
            |ui| chips(ui, footer, noun),
        );
        if let Some(reveal) = reveal {
            *request = Some(SweepRequest::Reveal(reveal));
        }

        let (change, pressed) = Sides::new().shrink_left().truncate().show(
            ui,
            |ui| destination(ui, footer),
            |ui| {
                let shortcut = ui.ctx().format_shortcut(&START_SHORTCUT);
                let mut start = filled_button(format!("{MDI_PLAY} Start"));
                if !compact {
                    start = start.shortcut_text(RichText::new(shortcut).color(mcs::GRAY_50));
                }
                let start = add_button(ui, ready, start, "Start")
                    .on_hover_text(format!("Run {noun}. Live simulation will be paused."))
                    .on_disabled_hover_text(start_refusal(&footer.summary.lines));
                let save = footer.unsaved_results && save_results_button(ui, compact, footer.search_files).clicked();
                if save {
                    Some(SweepRequest::SaveResults)
                } else {
                    start.clicked().then_some(SweepRequest::Start)
                }
            },
        );
        if change {
            *request = Some(SweepRequest::Reveal(Reveal::site(DraftSite::Execution, footer.mode)));
        }
        if pressed.is_some() {
            *request = pressed;
        }
    });

    let ctx = ui.ctx().clone();
    let covered =
        footer.modal_open || Popup::is_any_open(&ctx) || ctx.memory(|memory| memory.top_modal_layer()).is_some();
    if ready && !covered && ui.input_mut(|input| input.consume_shortcut(&START_SHORTCUT)) {
        *request = Some(SweepRequest::Start);
    }
}

/// Draws the chips that count the warnings and the problems, each opening a list of them, from right to left.
///
/// Returns the reveal of the row of a problem picked in a list.
fn chips(ui: &mut egui::Ui, footer: &BuilderFooter<'_>, noun: &str) -> Option<Reveal> {
    let mut reveal = None;
    if let Some((kind, count)) = problems_chip(&footer.summary.lines) {
        let color = issue_color(ui, kind);
        let chip = add_button(
            ui,
            true,
            Button::new(RichText::new(format!("{} {count}", issue_icon(kind))).color(color)).frame(false),
            &count,
        )
        .on_hover_text("Show problems");
        let title = if noun == "search" {
            "Search not ready"
        } else {
            "Sweep not ready"
        };
        Popup::from_toggle_button_response(&chip)
            .id(Id::new("henad_sweep_problems_popup"))
            .close_behavior(PopupCloseBehavior::CloseOnClickOutside)
            .width(360.0)
            .show(|ui| {
                ui.strong(title);
                for line in &footer.summary.lines {
                    let text = RichText::new(format!("{} {}", issue_icon(line.kind), line.text))
                        .color(issue_color(ui, line.kind));
                    if ui.selectable_label(false, text).clicked() {
                        reveal = Some(line.reveal());
                        ui.close();
                    }
                }
            });
    }
    let warnings = footer.summary.warnings.len() as u64;
    if warnings > 0 {
        let count = format!("{warnings} {}", plural(warnings, "warning"));
        let color = ui.visuals().warn_fg_color;
        let chip = add_button(
            ui,
            true,
            Button::new(RichText::new(format!("{MDI_ALERT} {count}")).color(color)).frame(false),
            &count,
        )
        .on_hover_text("Show warnings");
        Popup::from_toggle_button_response(&chip)
            .id(Id::new("henad_sweep_warnings_popup"))
            .close_behavior(PopupCloseBehavior::CloseOnClickOutside)
            .width(360.0)
            .show(|ui| {
                ui.strong("Warnings");
                for warning in &footer.summary.warnings {
                    ui.colored_label(color, format!("{MDI_ALERT} {warning}"));
                }
            });
    }
    reveal
}

/// Draws where the results go, with a link to the Execution section, or a warning of results not saved.
///
/// Returns whether the link was clicked.
fn destination(ui: &mut egui::Ui, footer: &BuilderFooter<'_>) -> bool {
    if footer.unsaved_results {
        truncated_label(ui, "Last results not saved", Some(ui.visuals().warn_fg_color));
        return false;
    }
    // The text truncates to the room the link leaves it.
    let link_width = text_width(ui, "Change") + ui.spacing().item_spacing.x;
    let text_room = vec2(
        (ui.available_width() - link_width).max(0.0),
        ui.spacing().interact_size.y,
    );
    ui.allocate_ui(text_room, |ui| match footer.folder {
        Some(folder) if folder.as_os_str().is_empty() => {
            truncated_label(ui, format!("{MDI_FOLDER_OUTLINE} No folder selected"), None);
        }
        Some(folder) => {
            truncated_label(
                ui,
                format!("{MDI_FOLDER_OUTLINE} Results will go to {}", folder_name(folder)),
                None,
            )
            .on_hover_text(folder.display().to_string());
        }
        None => {
            truncated_label(ui, format!("{MDI_MEMORY} Results will be kept in memory"), None);
        }
    });
    ui.link("Change").on_hover_text("Open Execution section").clicked()
}

/// Adds Save results, only its icon when `compact`, with the tooltip for a search's files when `search`.
fn save_results_button(ui: &mut egui::Ui, compact: bool, search: bool) -> egui::Response {
    let text = if compact {
        MDI_TRAY_ARROW_DOWN.to_owned()
    } else {
        format!("{MDI_TRAY_ARROW_DOWN} Save results")
    };
    let tooltip = if search {
        "Save runs, summary, series, manifest and search tables"
    } else {
        "Save runs, summary, series and manifest files"
    };
    add_button(ui, true, Button::new(text), "Save results").on_hover_text(tooltip)
}

/// Adds Show results, filled once the session has ended, and only its icon when `compact`.
fn show_results_button(ui: &mut egui::Ui, compact: bool, ended: bool) -> egui::Response {
    let text = if compact {
        MDI_CHART_BOX_OUTLINE.to_owned()
    } else {
        format!("{MDI_CHART_BOX_OUTLINE} Show results")
    };
    let (button, tooltip) = if ended {
        (filled_button(text), "Open Results tab")
    } else {
        (
            Button::new(text),
            "Open Results tab. Results will appear as runs finish.",
        )
    };
    add_button(ui, true, button, "Show results").on_hover_text(tooltip)
}

/// Contents of a session's footer.
pub struct SessionFooter<'a> {
    /// "sweep" or "search".
    pub noun: &'static str,
    pub state: SessionState,
    pub progress: &'a SweepProgress,
    /// End of the session, `None` while it runs.
    pub result: Option<SessionResult>,
    /// Whether the Results tab holds results in memory that are not saved.
    pub unsaved_results: bool,
    pub search_files: bool,
    /// Whether the session resumes a folder, and has no draft of its own.
    pub resumed: bool,
    pub modal_open: bool,
}

/// Draws the footer of a running or ended session.
pub fn session_footer(ui: &mut egui::Ui, footer: &SessionFooter<'_>, request: &mut Option<SweepRequest>) {
    let compact = ui.available_width() < FOOTER_ICONS_ONLY_BELOW;
    ui.add_enabled_ui(!footer.modal_open, |ui| match &footer.result {
        None => running_rows(ui, footer, compact, request),
        Some(result) => ended_rows(ui, footer, result, compact, request),
    });
}

fn running_rows(ui: &mut egui::Ui, footer: &SessionFooter<'_>, compact: bool, request: &mut Option<SweepRequest>) {
    let state = footer.state;
    let mut bar = ProgressBar::new(finished_fraction(footer.progress))
        .text(bar_text(footer.progress, state))
        .corner_radius(0);
    if state == SessionState::Planning {
        bar = bar.animate(true);
    }
    let fill = if state.is_paused() {
        mcs::ORANGE_700
    } else {
        mcs::BLUE_700
    };
    add_progress_bar(ui, bar, fill);

    let noun = footer.noun;
    let (show, pressed) = Sides::new().shrink_left().show(
        ui,
        |ui| show_results_button(ui, compact, false).clicked(),
        |ui| {
            let pause_text = format!("{MDI_PAUSE} Pause");
            let resume_text = format!("{MDI_PLAY} Resume");
            let slot =
                text_width(ui, &pause_text).max(text_width(ui, &resume_text)) + 2.0 * ui.spacing().button_padding.x;
            let (text, name, tooltip, next) = if state.is_paused() {
                (resume_text, "Resume", format!("Continue {noun}"), SweepRequest::Resume)
            } else {
                (
                    pause_text,
                    "Pause",
                    "Pause all runs. Press Resume to continue.".to_owned(),
                    SweepRequest::Pause,
                )
            };
            let mut pressed = add_button(ui, true, Button::new(text).min_size(vec2(slot, 0.0)), name)
                .on_hover_text(tooltip)
                .clicked()
                .then_some(next);
            let abort = add_button(ui, true, Button::new(format!("{MDI_STOP} Abort")), "Abort")
                .on_hover_text(format!("End {noun}. Results of finished runs will be kept."));
            if abort.clicked() {
                pressed = Some(SweepRequest::AskAbort);
            }
            pressed
        },
    );
    if show {
        *request = Some(SweepRequest::ShowResults);
    }
    if pressed.is_some() {
        *request = pressed;
    }
}

fn ended_rows(
    ui: &mut egui::Ui,
    footer: &SessionFooter<'_>,
    result: &SessionResult,
    compact: bool,
    request: &mut Option<SweepRequest>,
) {
    let bar = ProgressBar::new(result.fraction)
        .text(result.text.as_str())
        .corner_radius(0);
    let fill = if result.failed { mcs::RED_700 } else { mcs::BLUE_700 };
    add_progress_bar(ui, bar, fill).on_hover_text(result.text.as_str());

    let label = edit_label(footer.noun, footer.resumed);
    let (edit, pressed) = Sides::new().shrink_left().show(
        ui,
        |ui| {
            let tooltip = if footer.resumed {
                "Return to settings".to_owned()
            } else {
                format!(
                    "Return to this {}'s settings. Results will be kept in the Results tab.",
                    footer.noun
                )
            };
            add_button(ui, true, Button::new(format!("{MDI_PENCIL_OUTLINE} {label}")), &label)
                .on_hover_text(tooltip)
                .clicked()
        },
        |ui| {
            let show = show_results_button(ui, compact, true).clicked();
            let save = footer.unsaved_results && save_results_button(ui, compact, footer.search_files).clicked();
            if save {
                Some(SweepRequest::SaveResults)
            } else {
                show.then_some(SweepRequest::ShowResults)
            }
        },
    );
    if edit {
        *request = Some(SweepRequest::EditSweep);
    }
    if pressed.is_some() {
        *request = pressed;
    }
}

/// Asks before a start of the sweep or search `noun` names. The start drops the unsaved results held in memory.
///
/// Cancel, Escape and a click beside the modal close it and start nothing.
pub fn replace_modal(ctx: &egui::Context, noun: &str, open: &mut bool, request: &mut Option<SweepRequest>) {
    let mut closed = false;
    let response = Modal::new(Id::new("henad_replace_results_modal")).show(ctx, |ui| {
        ui.set_max_width(420.0);
        ui.heading("Replace results?");
        ui.label(format!("Starting a {noun} will clear the unsaved results in memory."));
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            if ui.button("Cancel").clicked() {
                closed = true;
            }
            if ui
                .button("Save results")
                .on_hover_text("Save results without starting")
                .clicked()
            {
                *request = Some(SweepRequest::SaveResults);
                closed = true;
            }
            if ui.add(destructive_button("Start anyway")).clicked() {
                *request = Some(SweepRequest::StartAnyway);
                closed = true;
            }
        });
    });
    if closed || response.should_close() {
        *open = false;
    }
}

/// Inputs of the Abort modal.
pub struct AbortModal<'a> {
    /// "sweep" or "search".
    pub noun: &'static str,
    pub progress: &'a SweepProgress,
    pub paused: bool,
    /// Whether the results go to a folder. An aborted sweep can resume from one.
    pub in_folder: bool,
}

/// Asks before aborting the running sweep or search.
///
/// The safe choice comes first and reads as the state it keeps. It, Escape and a click beside the modal close it and
/// leave the sweep as it is.
pub fn abort_modal(ctx: &egui::Context, modal: &AbortModal<'_>, open: &mut bool, request: &mut Option<SweepRequest>) {
    let mut closed = false;
    let noun = modal.noun;
    let response = Modal::new(Id::new("henad_abort_sweep_modal")).show(ctx, |ui| {
        ui.set_max_width(420.0);
        ui.heading(format!("Abort {noun}?"));
        ui.label(abort_text(modal.progress));
        ui.label(resume_text(noun, modal.in_folder));
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            let keep = if modal.paused { "Keep paused" } else { "Keep running" };
            if ui.button(keep).clicked() {
                closed = true;
            }
            if ui.add(destructive_button(format!("Abort {noun}"))).clicked() {
                *request = Some(SweepRequest::Abort);
                closed = true;
            }
        });
    });
    if closed || response.should_close() {
        *open = false;
    }
}

#[cfg(test)]
mod tests {
    use super::{BuilderFooter, LineTone, footer_height, problems_chip, ready_sentence, start_refusal};
    use crate::icons::material_design_icons::MDI_ALERT;
    use crate::ui::sweep::draft::{DraftMode, DraftSite, IssueKind, MAX_DRAFT_RUNS};
    use crate::ui::sweep::layout::SweepSection;
    use crate::ui::sweep::{CheckSummary, IssueLine};

    fn line(kind: IssueKind, text: &str) -> IssueLine {
        IssueLine {
            kind,
            text: text.to_owned(),
            section: SweepSection::Parameters,
            site: DraftSite::Factor(2),
        }
    }

    fn footer(summary: &CheckSummary) -> BuilderFooter<'_> {
        BuilderFooter {
            mode: DraftMode::Sweep,
            summary,
            start_failure: None,
            steps_per_run: 1100,
            folder: None,
            unsaved_results: false,
            search_files: false,
            modal_open: false,
        }
    }

    #[test]
    fn a_ready_footer_totals_the_runs() {
        assert_eq!(
            ready_sentence(DraftMode::Sweep, (15, 3, 45), 1100),
            "45 runs: 15 configurations × 3 replicates, 1100 steps each"
        );
        assert_eq!(
            ready_sentence(DraftMode::Search, (200, 3, 600), 1000),
            "600 runs: 200 evaluations × 3 replicates, 1000 steps each"
        );
        assert_eq!(
            ready_sentence(DraftMode::Sweep, (1, 1, 1), 1),
            "1 run: 1 configuration × 1 replicate, 1 step each"
        );
    }

    #[test]
    fn a_footer_leads_with_a_start_failure_then_the_first_problem() {
        let mut summary = CheckSummary {
            counts: Some((15, 3, 45)),
            ..CheckSummary::default()
        };
        let ready = footer(&summary).status_line();
        assert_eq!(ready.text, "45 runs: 15 configurations × 3 replicates, 1100 steps each");
        assert_eq!(ready.tone, LineTone::Normal);

        summary.counts = Some((MAX_DRAFT_RUNS / 2, 1, MAX_DRAFT_RUNS / 2));
        let near = footer(&summary).status_line();
        assert_eq!(near.tone, LineTone::Warn, "half the run limit warns");
        assert_eq!(near.tooltip.as_deref(), Some("Near the app's limit of 1048576 runs"));

        summary.counts = None;
        summary.lines = vec![line(IssueKind::Missing, "Infection Rate: Enter values")];
        let missing = footer(&summary).status_line();
        assert_eq!(missing.text, "Sweep not ready. Infection Rate: Enter values");
        assert_eq!(missing.tone, LineTone::Normal, "missing input is not an error");

        summary
            .lines
            .insert(0, line(IssueKind::Invalid, "Recovery Rate: '0.x' is not a number"));
        let invalid = footer(&summary).status_line();
        assert_eq!(
            invalid.text,
            format!("{MDI_ALERT} Sweep not ready. Recovery Rate: '0.x' is not a number")
        );
        assert_eq!(invalid.tone, LineTone::Error);

        let mut failed = footer(&summary);
        failed.start_failure = Some("Sweep start failed: no device");
        assert_eq!(
            failed.status_line().text,
            format!("{MDI_ALERT} Sweep start failed: no device")
        );
    }

    #[test]
    fn the_problems_chip_counts_problems_or_input_to_fill_in() {
        assert_eq!(problems_chip(&[]), None);
        let missing = [line(IssueKind::Missing, "a"), line(IssueKind::Missing, "b")];
        assert_eq!(
            problems_chip(&missing),
            Some((IssueKind::Missing, "2 missing".to_owned()))
        );
        let mixed = [line(IssueKind::Invalid, "a"), line(IssueKind::Missing, "b")];
        assert_eq!(
            problems_chip(&mixed),
            Some((IssueKind::Invalid, "2 problems".to_owned()))
        );
        assert_eq!(
            problems_chip(&mixed[..1]),
            Some((IssueKind::Invalid, "1 problem".to_owned()))
        );
    }

    #[test]
    fn a_disabled_start_lists_three_problems_and_counts_the_rest() {
        let lines: Vec<IssueLine> = ["a", "b", "c", "d", "e"]
            .into_iter()
            .map(|text| line(IssueKind::Invalid, text))
            .collect();
        assert_eq!(start_refusal(&lines), "Fix these first:\na\nb\nc\nand 2 more");
        assert_eq!(start_refusal(&lines[..2]), "Fix these first:\na\nb");
    }

    #[test]
    fn the_footer_holds_two_rows_and_its_separator() {
        let style = egui::Style::default();
        let spacing = &style.spacing;
        let separator = style.visuals.widgets.noninteractive.bg_stroke.width.round();
        assert_eq!(
            footer_height(&style),
            2.0 * spacing.interact_size.y + spacing.item_spacing.y + 12.0 + separator
        );
    }
}
