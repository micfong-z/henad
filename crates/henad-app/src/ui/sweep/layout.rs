//! Layout of the Sweep tab: the width flags, the form's label column, the section headers and the named buttons.
//!
//! The tab's width decides three things. The Plan panel shows beside the form only while the tab is wide enough, the
//! form stacks each label over its control once its column is narrow, and the footer's buttons drop their text on a
//! narrow tab. The first two switch at a pair of widths each ([`Breakpoint`]), so a width between the pair keeps the
//! layout it had.

use std::time::Duration;

use egui::collapsing_header::{CollapsingState, paint_default_icon};
use egui::text::{CCursor, CCursorRange, LayoutJob, TextWrapping};
use egui::widgets::text_edit::TextEditState;
use egui::{
    Align, Button, Checkbox, Color32, FontSelection, Id, Label, Layout, Response, RichText, Sense, TextFormat,
    TextStyle, TextWrapMode, Ui, WidgetInfo, WidgetText, WidgetType, vec2,
};

use crate::icons::material_design_icons::{MDI_ALERT, MDI_PENCIL_OUTLINE};
use crate::ui::mcs;
use crate::ui::plural;
use crate::ui::sweep::draft::{DraftMode, DraftSite, IssueKind};

/// Widest the form's column grows, in points. The rest of a wide tab goes to the Plan panel or stays empty.
pub const FORM_MAX_WIDTH: f32 = 680.0;

/// Narrowest the label column gets, in points.
pub const MIN_LABEL_WIDTH: f32 = 96.0;

/// Widest the label column gets, as a share of the form's width.
pub const MAX_LABEL_SHARE: f32 = 0.4;

/// Width of the form from which the parameter rows show the Parameters tab value in a column of its own, in points.
pub const BASELINE_COLUMN_FROM: f32 = 520.0;

/// Widest a combo box or a text field alone in a row's cell grows, in points.
pub const CONTROL_MAX_WIDTH: f32 = 320.0;

/// Width of the footer below which its secondary buttons show only their icon, in points.
pub const FOOTER_ICONS_ONLY_BELOW: f32 = 360.0;

/// Widths of the tab at which the Plan panel starts and stops fitting beside the form.
pub const PLAN_PANEL_BREAKPOINT: Breakpoint = Breakpoint {
    on_from: 776.0,
    off_below: 744.0,
};

/// Widths of the form at which its rows stop and start stacking each label over its control.
pub const SIDE_BY_SIDE_BREAKPOINT: Breakpoint = Breakpoint {
    on_from: 416.0,
    off_below: 400.0,
};

/// Narrows `ui` to the form's width, [`FORM_MAX_WIDTH`] at most, keeping it to the left like the other tabs.
pub fn cap_form_width(ui: &mut Ui) {
    let width = ui.available_width().min(FORM_MAX_WIDTH);
    ui.set_max_width(width);
}

/// Widths at which a layout flag turns on and off.
///
/// A width between the two keeps the flag as it was. Otherwise a width hovering at one edge would flip the layout on
/// every frame.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Breakpoint {
    /// Width from which an off flag turns on.
    pub on_from: f32,
    /// Width below which an on flag turns off.
    pub off_below: f32,
}

impl Breakpoint {
    /// Returns the flag at `width`, given it was `on` before.
    pub fn update(self, on: bool, width: f32) -> bool {
        if on {
            width >= self.off_below
        } else {
            width >= self.on_from
        }
    }
}

/// Layout flags of the Sweep tab, kept from one frame to the next for their hysteresis.
#[derive(Debug, Clone, Default)]
pub struct FormState {
    /// Whether the Plan panel fits beside the form.
    pub plan_fits: bool,
    /// Whether the form stacks each label over its control.
    pub stacked: bool,
    /// Section and row to open and scroll into view.
    pub reveal: Option<Reveal>,
    /// Last edit of a text field. The field holds back the issue of its text for a moment.
    pub typing: Option<Typing>,
    /// Text field to focus on the first frame with no press or release of the pointer.
    pub focus_field: Option<Id>,
}

impl FormState {
    /// Updates [`Self::plan_fits`] for a tab `tab_width` wide.
    pub fn fit_tab(&mut self, tab_width: f32) {
        self.plan_fits = PLAN_PANEL_BREAKPOINT.update(self.plan_fits, tab_width);
    }

    /// Updates [`Self::stacked`] for a form `form_width` wide.
    pub fn fit_form(&mut self, form_width: f32) {
        self.stacked = !SIDE_BY_SIDE_BREAKPOINT.update(!self.stacked, form_width);
    }
}

/// A top-level section of the Sweep tab's form.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SweepSection {
    Design,
    Search,
    Parameters,
    Actions,
    Seeds,
    RunLength,
    Outputs,
    Execution,
    /// The plan, as a section of its own while the Plan panel does not fit.
    Plan,
}

impl SweepSection {
    pub fn title(self) -> &'static str {
        match self {
            Self::Design => "Design",
            Self::Search => "Search",
            Self::Parameters => "Parameters",
            Self::Actions => "Actions",
            Self::Seeds => "Replicates and seeds",
            Self::RunLength => "Run length",
            Self::Outputs => "Outputs",
            Self::Execution => "Execution",
            Self::Plan => "Plan",
        }
    }

    /// Salt of the section header's id. It never changes with the header's text.
    pub fn id_salt(self) -> &'static str {
        match self {
            Self::Design => "henad_sweep_design",
            Self::Search => "henad_sweep_search",
            Self::Parameters => "henad_sweep_parameters",
            Self::Actions => "henad_sweep_actions",
            Self::Seeds => "henad_sweep_seeds",
            Self::RunLength => "henad_sweep_run_length",
            Self::Outputs => "henad_sweep_outputs",
            Self::Execution => "henad_sweep_execution",
            Self::Plan => "henad_sweep_plan_section",
        }
    }

    /// Returns whether the section starts open.
    pub fn default_open(self) -> bool {
        !matches!(self, Self::Outputs | Self::Execution | Self::Plan)
    }

    /// Returns the section holding the row an issue of `site` belongs to, in a draft of `mode`.
    ///
    /// An issue of the whole sweep belongs to the Design or Search section.
    pub fn of_site(site: DraftSite, mode: DraftMode) -> Self {
        match site {
            DraftSite::Sweep => match mode {
                DraftMode::Sweep => Self::Design,
                DraftMode::Search => Self::Search,
            },
            DraftSite::Factor(_) | DraftSite::Parameters => Self::Parameters,
            DraftSite::Design => Self::Design,
            DraftSite::Search | DraftSite::Objective | DraftSite::Axis(_) | DraftSite::MethodSetting(_) => Self::Search,
            DraftSite::Action(_) => Self::Actions,
            DraftSite::Replicates | DraftSite::Seed | DraftSite::DesignSeed => Self::Seeds,
            DraftSite::RunLength | DraftSite::Stop => Self::RunLength,
            DraftSite::Sampling | DraftSite::Output(_) => Self::Outputs,
            DraftSite::Execution => Self::Execution,
        }
    }
}

/// Section to open and scroll into view, and the row in it to reveal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Reveal {
    pub section: SweepSection,
    /// Row to scroll to and focus, `None` for the section's header.
    pub site: Option<DraftSite>,
}

impl Reveal {
    /// Returns a reveal of `section`'s header.
    pub fn section(section: SweepSection) -> Self {
        Self { section, site: None }
    }

    /// Returns a reveal of the row an issue of `site` belongs to, in a draft of `mode`.
    ///
    /// An issue of the whole sweep reveals the Design or Search section's first row.
    pub fn site(site: DraftSite, mode: DraftMode) -> Self {
        let row = match (site, mode) {
            (DraftSite::Sweep, DraftMode::Sweep) => DraftSite::Design,
            (DraftSite::Sweep, DraftMode::Search) => DraftSite::Search,
            (site, _) => site,
        };
        Self {
            section: SweepSection::of_site(site, mode),
            site: Some(row),
        }
    }

    /// Returns the reveal as a draft in `mode` draws it. The Design and Search sections swap with the mode, and a
    /// reveal of either lands on the one drawn.
    pub fn in_mode(self, mode: DraftMode) -> Self {
        let section = match (self.section, mode) {
            (SweepSection::Design, DraftMode::Search) => SweepSection::Search,
            (SweepSection::Search, DraftMode::Sweep) => SweepSection::Design,
            (section, _) => section,
        };
        Self { section, ..self }
    }
}

/// Time after the last keystroke from which a focused text field shows the issue of its text.
pub const ISSUE_DELAY: Duration = Duration::from_millis(800);

/// Last edit of a text field of the form.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Typing {
    pub field: Id,
    /// Time of the edit, in seconds of egui's clock.
    pub time: f64,
}

/// Returns whether a text field holds back the issue of its text.
///
/// It does while it has focus, `focused`, and less than [`ISSUE_DELAY`] passed since its last edit, `since_edit`
/// seconds ago, `None` for a field not edited.
pub fn holds_back_issue(focused: bool, since_edit: Option<f64>) -> bool {
    focused && since_edit.is_some_and(|elapsed| elapsed < ISSUE_DELAY.as_secs_f64())
}

/// Layout of the form and the per-frame state its rows share: the row to reveal, and the typing that holds back an
/// issue.
#[derive(Debug, Clone, Copy)]
pub struct FormRows {
    pub layout: FormLayout,
    /// Row to reveal in the section being drawn, set once the section is fully open.
    pub reveal: Option<DraftSite>,
    /// Whether the row to reveal was drawn and revealed.
    pub revealed: bool,
    pub typing: Option<Typing>,
    /// Text field to focus on the first frame with no press or release of the pointer.
    pub focus_field: Option<Id>,
    /// Time of the frame, in seconds of egui's clock.
    pub time: f64,
    /// Whether the row to reveal sits in a nested section still opening, so the reveal waits another frame.
    pub pending: bool,
}

impl FormRows {
    /// Reveals the row of `site` through its widget `response` when it is the row to reveal: scrolls it to the middle
    /// of the view and focuses it.
    ///
    /// A text field of `text_len` characters gets all of its text selected.
    pub fn reveal_row(&mut self, ui: &Ui, site: DraftSite, response: &Response, text_len: Option<usize>) {
        if self.revealed || self.reveal != Some(site) {
            return;
        }
        response.scroll_to_me(Some(Align::Center));
        if let Some(len) = text_len {
            select_all(ui, response, len);
        }
        self.revealed = true;
    }

    /// Asks for the text field `field` to take focus once the pointer is still.
    ///
    /// A text field gives up its focus on any press or release of the pointer outside it, as on the click that asks.
    pub fn focus_later(&mut self, ui: &Ui, field: Id) {
        self.focus_field = Some(field);
        ui.ctx().request_repaint();
    }

    /// Focuses the text field `response` when it was asked to take focus and the pointer is still.
    pub fn take_focus(&mut self, ui: &Ui, response: &Response) {
        if self.focus_field != Some(response.id) {
            return;
        }
        if ui.input(|input| input.pointer.any_pressed() || input.pointer.any_released()) {
            ui.ctx().request_repaint();
            return;
        }
        response.request_focus();
        self.focus_field = None;
    }

    /// Records an edit of the text field `response`, and returns whether the field holds back the issue of its text.
    pub fn holds_back_issue(&mut self, ui: &Ui, response: &Response) -> bool {
        if response.changed() {
            self.typing = Some(Typing {
                field: response.id,
                time: self.time,
            });
            ui.ctx().request_repaint_after(ISSUE_DELAY);
        }
        let since_edit = self
            .typing
            .filter(|typing| typing.field == response.id)
            .map(|typing| self.time - typing.time);
        holds_back_issue(response.has_focus(), since_edit)
    }
}

/// Focuses the text field `response` of `len` characters, with all of its text selected.
fn select_all(ui: &Ui, response: &Response, len: usize) {
    let mut state = TextEditState::load(ui.ctx(), response.id).unwrap_or_default();
    state
        .cursor
        .set_char_range(Some(CCursorRange::two(CCursor::new(0), CCursor::new(len))));
    state.store(ui.ctx(), response.id);
    response.request_focus();
}

/// Returns the icon an issue of `kind` shows.
pub fn issue_icon(kind: IssueKind) -> &'static str {
    match kind {
        IssueKind::Missing => MDI_PENCIL_OUTLINE,
        IssueKind::Invalid => MDI_ALERT,
    }
}

/// Returns the colour an issue of `kind` is drawn in: the text colour for input still to give, the error colour for
/// input the plan refuses.
pub fn issue_color(ui: &Ui, kind: IssueKind) -> Color32 {
    match kind {
        IssueKind::Missing => ui.visuals().text_color(),
        IssueKind::Invalid => ui.visuals().error_fg_color,
    }
}

/// Issues of one section, as its header counts them.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct IssueCount {
    pub missing: usize,
    pub invalid: usize,
}

impl IssueCount {
    /// Counts one more issue of `kind`.
    pub fn add(&mut self, kind: IssueKind) {
        match kind {
            IssueKind::Missing => self.missing += 1,
            IssueKind::Invalid => self.invalid += 1,
        }
    }

    pub fn total(self) -> usize {
        self.missing + self.invalid
    }

    /// Kind the count is drawn as: invalid while any issue is, `None` for no issue.
    pub fn kind(self) -> Option<IssueKind> {
        if self.invalid > 0 {
            Some(IssueKind::Invalid)
        } else if self.missing > 0 {
            Some(IssueKind::Missing)
        } else {
            None
        }
    }

    /// Returns the count in words: "2 problems" while any issue is invalid, "2 missing" while every one is missing input,
    /// and `None` for no issue.
    pub fn phrase(self) -> Option<String> {
        let total = self.total() as u64;
        match self.kind()? {
            IssueKind::Invalid => Some(format!("{total} {}", plural(total, "problem"))),
            IssueKind::Missing => Some(format!("{total} missing")),
        }
    }
}

/// Header of a section: its title, the issues of its rows and a one-line summary of its settings.
#[derive(Debug, Clone, Copy)]
pub struct SectionHeading<'a> {
    pub title: &'a str,
    pub issues: IssueCount,
    pub summary: &'a str,
}

/// State a section's body takes from its header.
#[derive(Debug, Clone, Copy)]
pub struct SectionBody {
    /// Id of the header, for the rows that the section title labels.
    pub heading: Id,
    /// Whether a reveal of the section has opened it fully, so its rows sit where they stay.
    pub settled: bool,
}

/// Result of drawing a section.
#[derive(Debug, Clone)]
pub struct SectionOutcome {
    /// Whether a reveal of the section has opened it fully.
    pub settled: bool,
    pub header: Response,
}

/// Draws a collapsible section with `heading`, and its body through `add_body` while open.
///
/// The whole header toggles the section, and its state is kept under `id_salt`. The header reads the strong title,
/// then the issue count, then the weak summary, truncated in that order from the end.
///
/// While `reveal` is set, the section opens. Once it is fully open, the body and the outcome say so, and the caller
/// scrolls the row or the header into view.
pub fn section(
    ui: &mut Ui,
    id_salt: &str,
    heading: SectionHeading<'_>,
    default_open: bool,
    reveal: bool,
    add_body: impl FnOnce(&mut Ui, SectionBody),
) -> SectionOutcome {
    section_with(ui, id_salt, heading, default_open, reveal, false, add_body)
}

/// Draws a collapsible section inside another, as [`section`] does, with its body indented under its header.
pub fn nested_section(
    ui: &mut Ui,
    id_salt: &str,
    heading: SectionHeading<'_>,
    default_open: bool,
    reveal: bool,
    add_body: impl FnOnce(&mut Ui, SectionBody),
) -> SectionOutcome {
    section_with(ui, id_salt, heading, default_open, reveal, true, add_body)
}

/// Draws a section as [`section`] does, its body indented under the header when `indented` is set.
fn section_with(
    ui: &mut Ui,
    id_salt: &str,
    heading: SectionHeading<'_>,
    default_open: bool,
    reveal: bool,
    indented: bool,
    add_body: impl FnOnce(&mut Ui, SectionBody),
) -> SectionOutcome {
    let id = ui.make_persistent_id(id_salt);
    let mut state = CollapsingState::load_with_default_open(ui.ctx(), id, default_open);
    if reveal && !state.is_open() {
        state.set_open(true);
        ui.ctx().request_repaint();
    }

    let spacing = ui.spacing().clone();
    let available_width = ui.available_width();
    let text_width = (available_width - spacing.indent - spacing.button_padding.x).max(0.0);
    let galley = heading_galley(ui, heading, text_width);
    let height = (galley.size().y + 2.0 * spacing.button_padding.y).max(spacing.interact_size.y);
    let (_, rect) = ui.allocate_space(vec2(available_width, height));
    let mut response = ui.interact(rect, id, Sense::click());
    if response.clicked() {
        state.toggle(ui);
        response.mark_changed();
    }
    response.widget_info(|| WidgetInfo::labeled(WidgetType::CollapsingHeader, ui.is_enabled(), heading_name(heading)));

    let openness = state.openness(ui.ctx());
    if ui.is_rect_visible(rect) {
        let visuals = *ui.style().interact(&response);
        if ui.visuals().collapsing_header_frame {
            ui.painter().rect(
                rect.expand(visuals.expansion),
                visuals.corner_radius,
                visuals.weak_bg_fill,
                visuals.bg_stroke,
                egui::StrokeKind::Inside,
            );
        }
        let (mut icon_rect, _) = ui.spacing().icon_rectangles(rect);
        icon_rect.set_center(egui::pos2(rect.left() + spacing.indent / 2.0, rect.center().y));
        paint_default_icon(ui, openness, &response.clone().with_new_rect(icon_rect));
        let text_pos = egui::pos2(rect.left() + spacing.indent, rect.center().y - galley.size().y / 2.0);
        ui.painter().galley(text_pos, galley, visuals.text_color());
    }

    let settled = reveal && openness >= 1.0;
    let body = SectionBody { heading: id, settled };
    if indented {
        state.show_body_indented(&response, ui, |ui| add_body(ui, body));
    } else {
        state.show_body_unindented(ui, |ui| add_body(ui, body));
    }
    SectionOutcome {
        settled,
        header: response,
    }
}

/// Returns the accessible name of a section header: its title, its issue count in words and its summary, as in
/// "Parameters, 1 problem, 2 of 5 varied".
fn heading_name(heading: SectionHeading<'_>) -> String {
    let mut name = heading.title.to_owned();
    for part in heading
        .issues
        .phrase()
        .iter()
        .map(String::as_str)
        .chain([heading.summary])
    {
        if !part.is_empty() {
            name.push_str(", ");
            name.push_str(part);
        }
    }
    name
}

/// Lays out the text of a section header, truncated at `width`.
fn heading_galley(ui: &Ui, heading: SectionHeading<'_>, width: f32) -> std::sync::Arc<egui::Galley> {
    let visuals = ui.visuals();
    let font = FontSelection::Default.resolve(ui.style());
    let format = |color: Color32| TextFormat {
        font_id: font.clone(),
        color,
        valign: Align::Center,
        ..TextFormat::default()
    };
    let mut job = LayoutJob::default();
    job.append(heading.title, 0.0, format(visuals.strong_text_color()));
    if let Some(kind) = heading.issues.kind() {
        let color = issue_color(ui, kind);
        let text = format!("{} {}", issue_icon(kind), heading.issues.total());
        job.append(&text, spacing_before(ui), format(color));
    }
    if !heading.summary.is_empty() {
        job.append(heading.summary, spacing_before(ui), format(visuals.weak_text_color()));
    }
    job.wrap = TextWrapping::truncate_at_width(width);
    ui.fonts_mut(|fonts| fonts.layout_job(job))
}

/// Space before each part of a header after the title.
fn spacing_before(ui: &Ui) -> f32 {
    ui.spacing().item_spacing.x
}

/// Label column, control column and stacking of the form, computed once per frame from its width.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FormLayout {
    /// Width of the label column. A stacked form gives the label the whole width.
    pub label_width: f32,
    /// Width of the control cell beside or below the label.
    pub cell_width: f32,
    /// Width of the column of Parameters tab values, `None` for a form too narrow for it.
    pub baseline_width: Option<f32>,
    /// Whether each label sits over its control.
    pub stacked: bool,
    /// Gap between two columns.
    pub gap: f32,
}

impl FormLayout {
    /// Returns the layout of a form `form_width` wide, whose widest label is `widest_label` wide and widest
    /// Parameters tab value `widest_baseline` wide, with `gap` between two columns.
    ///
    /// The label column fits the widest label within [`MIN_LABEL_WIDTH`] and [`MAX_LABEL_SHARE`] of the form. A
    /// longer label truncates.
    pub fn new(form_width: f32, widest_label: f32, widest_baseline: f32, stacked: bool, gap: f32) -> Self {
        let form_width = form_width.max(0.0);
        if stacked {
            return Self {
                label_width: form_width,
                cell_width: form_width,
                baseline_width: None,
                stacked,
                gap,
            };
        }
        let label_width = widest_label.clamp(MIN_LABEL_WIDTH, (MAX_LABEL_SHARE * form_width).max(MIN_LABEL_WIDTH));
        Self {
            label_width,
            cell_width: (form_width - label_width - gap).max(0.0),
            baseline_width: (form_width >= BASELINE_COLUMN_FROM).then_some(widest_baseline),
            stacked,
            gap,
        }
    }

    /// Returns the layout of rows indented `indent` points, as under a nested section's header.
    ///
    /// Beside a label column, the label column narrows and the cell keeps its width and place. A stacked form
    /// narrows the cell.
    pub fn indented(&self, indent: f32) -> Self {
        if self.stacked {
            let width = (self.cell_width - indent).max(0.0);
            return Self {
                label_width: width,
                cell_width: width,
                ..*self
            };
        }
        Self {
            label_width: (self.label_width - indent).max(0.0),
            ..*self
        }
    }

    /// Width of a parameter row's values, after the column of Parameters tab values.
    pub fn values_width(&self) -> f32 {
        match self.baseline_width {
            Some(baseline) => (self.cell_width - baseline - self.gap).max(0.0),
            None => self.cell_width,
        }
    }

    /// Draws `label` in the label column and the controls `add_control` adds in the cell beside it, or below it in a
    /// stacked form.
    ///
    /// The label truncates to the column and shows its whole text on hover. The cell lays out top to bottom.
    ///
    /// Returns the label's response, for the controls it names, and what `add_control` returns.
    pub fn row<R>(
        &self,
        ui: &mut Ui,
        label: impl Into<WidgetText>,
        add_control: impl FnOnce(&mut Ui) -> R,
    ) -> (Response, R) {
        self.row_with(ui, 0.0, |ui| ui.add(Label::new(label).truncate()), add_control)
    }

    /// Draws `label` indented to line up under a checkbox's text, as a row that belongs to the checkbox above it.
    ///
    /// A stacked form indents the controls under the label too. `add_control` gets the layout of the row, whose cell
    /// is narrower by the indent in a stacked form.
    pub fn sub_row<R>(
        &self,
        ui: &mut Ui,
        label: impl Into<WidgetText>,
        add_control: impl FnOnce(&mut Ui, &Self) -> R,
    ) -> (Response, R) {
        let indent = checkbox_indent(ui);
        let add_label = |ui: &mut Ui| ui.add(Label::new(label).truncate());
        if !self.stacked {
            return self.row_with(ui, indent, add_label, |ui| add_control(ui, self));
        }
        let inner = self.indented(indent);
        ui.horizontal(|ui| {
            ui.add_space(indent);
            ui.vertical(|ui| inner.row_with(ui, 0.0, add_label, |ui| add_control(ui, &inner)))
                .inner
        })
        .inner
    }

    /// Draws a checkbox for `checked` reading `text` in the label column, and the controls `add_control` adds in the
    /// cell.
    pub fn checkbox_row<R>(
        &self,
        ui: &mut Ui,
        checked: &mut bool,
        text: impl Into<WidgetText>,
        add_control: impl FnOnce(&mut Ui) -> R,
    ) -> (Response, R) {
        self.row_with(ui, 0.0, |ui| ui.add(Checkbox::new(checked, text)), add_control)
    }

    /// Draws a row with nothing in the label column, and the controls `add_control` adds in the cell.
    pub fn blank_row<R>(&self, ui: &mut Ui, add_control: impl FnOnce(&mut Ui) -> R) -> R {
        if self.stacked {
            return self.cell(ui, add_control);
        }
        self.row_with(
            ui,
            0.0,
            |ui| ui.allocate_response(vec2(0.0, 0.0), Sense::hover()),
            add_control,
        )
        .1
    }

    /// Draws the widget `add_label` adds in the label column, `indent` points in, and the controls `add_control` adds
    /// in the cell.
    ///
    /// The label column truncates its text. The cell lays out top to bottom from the top of the row.
    pub fn row_with<R>(
        &self,
        ui: &mut Ui,
        indent: f32,
        add_label: impl FnOnce(&mut Ui) -> Response,
        add_control: impl FnOnce(&mut Ui) -> R,
    ) -> (Response, R) {
        let add_label = |ui: &mut Ui, width: f32| {
            slot(ui, width, |ui| {
                ui.add_space(indent);
                add_label(ui)
            })
        };
        if self.stacked {
            let label = add_label(ui, self.cell_width);
            let inner = self.cell(ui, add_control);
            return (label, inner);
        }
        ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
            let label = add_label(ui, self.label_width);
            let inner = self.cell(ui, add_control);
            (label, inner)
        })
        .inner
    }

    /// Draws the controls `add_control` adds in a cell of [`Self::cell_width`], top to bottom.
    pub fn cell<R>(&self, ui: &mut Ui, add_control: impl FnOnce(&mut Ui) -> R) -> R {
        column(ui, self.cell_width, add_control)
    }

    /// Width of a combo box or a text field alone in the cell, [`CONTROL_MAX_WIDTH`] at most.
    pub fn control_width(&self) -> f32 {
        self.cell_width.min(CONTROL_MAX_WIDTH)
    }

    /// Draws the control `add_control` adds on a line of its own, with `feedback` after it, or on a reserved line
    /// below it in a stacked form.
    pub fn with_feedback<R>(&self, ui: &mut Ui, feedback: &Note, add_control: impl FnOnce(&mut Ui) -> R) -> R {
        let inner = ui
            .horizontal(|ui| {
                let inner = add_control(ui);
                if !self.stacked {
                    note_label(ui, feedback);
                }
                inner
            })
            .inner;
        if self.stacked {
            note_line(ui, self.cell_width, feedback);
        }
        inner
    }
}

/// Draws what `add_contents` adds in a slot `width` wide and one control high, left to right, truncating its text.
pub fn slot<R>(ui: &mut Ui, width: f32, add_contents: impl FnOnce(&mut Ui) -> R) -> R {
    let line_height = ui.spacing().interact_size.y;
    ui.allocate_ui_with_layout(vec2(width, line_height), Layout::left_to_right(Align::Center), |ui| {
        ui.set_width(width);
        ui.style_mut().wrap_mode = Some(TextWrapMode::Truncate);
        add_contents(ui)
    })
    .inner
}

/// Draws what `add_contents` adds in a column `width` wide, top to bottom.
pub fn column<R>(ui: &mut Ui, width: f32, add_contents: impl FnOnce(&mut Ui) -> R) -> R {
    let line_height = ui.spacing().interact_size.y;
    ui.allocate_ui_with_layout(vec2(width, line_height), Layout::top_down(Align::Min), add_contents)
        .inner
}

/// Returns the width of a checkbox's box and the space after it, by which a row under a checkbox is indented.
pub fn checkbox_indent(ui: &Ui) -> f32 {
    ui.spacing().icon_width + ui.spacing().icon_spacing
}

/// Weak text, a warning or an issue on a note line or in a feedback slot.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum Note {
    #[default]
    Empty,
    Weak(String),
    Warn(String),
    Issue(IssueKind, String),
}

impl Note {
    /// Returns the note of the first of `issues`, or `fallback` when there is none.
    pub fn issue_or<'a>(
        issues: impl IntoIterator<Item = &'a crate::ui::sweep::draft::DraftIssue>,
        fallback: Self,
    ) -> Self {
        issues
            .into_iter()
            .next()
            .map_or(fallback, |issue| Self::Issue(issue.kind, issue.message.clone()))
    }

    /// Returns whether the note reports an issue.
    pub fn is_issue(&self) -> bool {
        matches!(self, Self::Issue(..))
    }
}

/// Draws `note` on one line `width` wide, reserved even for an empty note so nothing below moves when one comes.
///
/// Returns the note's response, `None` for an empty note.
pub fn note_line(ui: &mut Ui, width: f32, note: &Note) -> Option<Response> {
    let height = ui.text_style_height(&TextStyle::Body);
    ui.allocate_ui_with_layout(vec2(width, height), Layout::left_to_right(Align::Center), |ui| {
        ui.set_min_size(vec2(width, height));
        note_label(ui, note)
    })
    .inner
}

/// Small button at the end of a note line: its text and its tooltip.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NoteButton<'a> {
    pub text: &'a str,
    pub tooltip: &'a str,
}

/// Draws `note` on one reserved line `width` wide, as [`note_line`] does, with `button` at its end when set.
///
/// The note truncates to the room the button leaves it. Returns whether the button was clicked.
pub fn note_line_with_button(ui: &mut Ui, width: f32, note: &Note, button: Option<NoteButton<'_>>) -> bool {
    let height = ui.text_style_height(&TextStyle::Body);
    ui.allocate_ui_with_layout(vec2(width, height), Layout::left_to_right(Align::Center), |ui| {
        ui.set_min_size(vec2(width, height));
        let (_, clicked) = egui::containers::Sides::new().shrink_left().truncate().show(
            ui,
            |ui| note_label(ui, note),
            |ui| button.is_some_and(|button| ui.small_button(button.text).on_hover_text(button.tooltip).clicked()),
        );
        clicked
    })
    .inner
}

/// Draws `note` in a block `width` wide and as many lines high as `reserved` takes, wrapped at that width.
///
/// The block keeps its height whatever the note, so nothing below moves when another note takes its place. A note
/// longer than the block truncates on its last line and shows its whole text on hover.
///
/// Returns the note's response, `None` for an empty note.
pub fn note_block(ui: &mut Ui, width: f32, reserved: &str, note: &Note) -> Option<Response> {
    let font = FontSelection::Default.resolve(ui.style());
    let rows = ui.fonts_mut(|fonts| {
        fonts
            .layout(reserved.to_owned(), font.clone(), Color32::PLACEHOLDER, width)
            .rows
            .len()
            .max(1)
    });
    let height = rows as f32 * ui.text_style_height(&TextStyle::Body);
    let galley = note_text(ui, note).map(|(text, color)| {
        let mut job = LayoutJob::single_section(
            text,
            TextFormat {
                font_id: font,
                color,
                ..TextFormat::default()
            },
        );
        job.wrap = TextWrapping {
            max_width: width,
            max_rows: rows,
            ..TextWrapping::default()
        };
        ui.fonts_mut(|fonts| fonts.layout_job(job))
    });
    ui.allocate_ui_with_layout(vec2(width, height), Layout::top_down(Align::Min), |ui| {
        ui.set_min_size(vec2(width, height));
        galley.map(|galley| ui.add(Label::new(galley)))
    })
    .inner
}

/// Returns the text of `note` with its icon, and the colour it is drawn in, `None` for an empty note.
fn note_text(ui: &Ui, note: &Note) -> Option<(String, Color32)> {
    match note {
        Note::Empty => None,
        Note::Weak(text) => Some((text.clone(), ui.visuals().weak_text_color())),
        Note::Warn(text) => Some((format!("{MDI_ALERT} {text}"), ui.visuals().warn_fg_color)),
        Note::Issue(kind, text) => Some((format!("{} {text}", issue_icon(*kind)), issue_color(ui, *kind))),
    }
}

/// Draws `note` in the space left on the line, truncated and whole on hover.
///
/// Returns the note's response, `None` for an empty note.
pub fn note_label(ui: &mut Ui, note: &Note) -> Option<Response> {
    let (text, color) = note_text(ui, note)?;
    Some(ui.add(Label::new(RichText::new(text).color(color)).truncate()))
}

/// Draws a segmented control of `choices`, each a value, its text and its tooltip, writing the one picked into
/// `value`.
///
/// Returns whether the value changed. A choice not `enabled` is drawn disabled, with its reason on hover.
pub fn segmented<T: Copy + PartialEq>(
    ui: &mut Ui,
    value: &mut T,
    choices: &[(T, &str, &str)],
    enabled: impl Fn(T) -> Result<(), &'static str>,
) -> bool {
    let before = *value;
    let responses: Vec<Response> = ui
        .horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 0.0;
            choices
                .iter()
                .map(|&(choice, text, tooltip)| {
                    let refusal = enabled(choice).err();
                    let response = ui
                        .add_enabled(refusal.is_none(), Button::selectable(*value == choice, text))
                        .on_hover_text(tooltip);
                    let response = match refusal {
                        Some(reason) => response.on_disabled_hover_text(reason),
                        None => response,
                    };
                    if response.clicked() {
                        *value = choice;
                    }
                    response
                })
                .collect()
        })
        .inner;
    // A frame would add its stroke to the row's height, so the outline is painted over the buttons instead.
    if let (Some(first), Some(last)) = (responses.first(), responses.last()) {
        ui.painter().rect_stroke(
            first.rect.union(last.rect),
            egui::CornerRadius::ZERO,
            ui.visuals().widgets.noninteractive.bg_stroke,
            egui::StrokeKind::Inside,
        );
    }
    *value != before
}

/// Returns the width of the widest of `labels` as the form lays them out, each paired with the width a checkbox's box
/// or an indent adds before it.
pub fn widest_label<'a>(ui: &Ui, labels: impl IntoIterator<Item = (&'a str, f32)>) -> f32 {
    labels
        .into_iter()
        .map(|(label, before)| text_width(ui, label) + before)
        .fold(0.0, f32::max)
}

/// Returns the width of `text` in the body font, on one line.
pub fn text_width(ui: &Ui, text: &str) -> f32 {
    let font = FontSelection::Default.resolve(ui.style());
    ui.fonts_mut(|fonts| {
        fonts
            .layout_no_wrap(text.to_owned(), font, Color32::PLACEHOLDER)
            .size()
            .x
    })
}

/// Adds `button`, named `name` for assistive technology, and enabled while `enabled` is set.
///
/// The name replaces the button's text. That text starts with an icon glyph a screen reader cannot read.
pub fn add_button(ui: &mut Ui, enabled: bool, button: Button<'_>, name: &str) -> Response {
    let response = ui.add_enabled(enabled, button);
    response.widget_info(|| WidgetInfo::labeled(WidgetType::Button, enabled, name));
    response
}

/// Returns the width of a small button showing `icon` alone, as [`icon_button`] adds it.
pub fn icon_button_width(ui: &Ui, icon: &str) -> f32 {
    let font = TextStyle::Button.resolve(ui.style());
    let icon_width = ui.fonts_mut(|fonts| {
        fonts
            .layout_no_wrap(icon.to_owned(), font, Color32::PLACEHOLDER)
            .size()
            .x
    });
    icon_width + 2.0 * ui.spacing().button_padding.x
}

/// Adds a small button showing `icon` alone, named `name` and described by `tooltip`.
pub fn icon_button(ui: &mut Ui, icon: &str, name: &str, tooltip: &str) -> Response {
    add_button(ui, true, Button::new(icon).small(), name).on_hover_text(tooltip)
}

/// Returns the tab's primary button, reading `text` in [`mcs::GRAY_50`] on [`mcs::BLUE_700`].
pub fn filled_button<'a>(text: String) -> Button<'a> {
    Button::new(RichText::new(text).color(mcs::GRAY_50)).fill(mcs::BLUE_700)
}

/// Returns a button for an action that discards work, reading `text` in [`mcs::RED_200`] on [`mcs::RED_900`].
pub fn destructive_button<'a>(text: impl Into<String>) -> Button<'a> {
    Button::new(RichText::new(text).color(mcs::RED_200)).fill(mcs::RED_900)
}

/// Adds `text` in `color` on one line, truncated to the space left and whole on hover.
pub fn truncated_label(ui: &mut Ui, text: impl Into<String>, color: Option<Color32>) -> Response {
    let text = egui::RichText::new(text.into());
    let text = match color {
        Some(color) => text.color(color),
        None => text,
    };
    ui.add(Label::new(text).truncate())
}

#[cfg(test)]
mod tests {
    use super::{
        Breakpoint, FormLayout, FormState, ISSUE_DELAY, IssueCount, MIN_LABEL_WIDTH, PLAN_PANEL_BREAKPOINT, Reveal,
        SectionHeading, SweepSection, heading_name, holds_back_issue,
    };
    use crate::ui::sweep::draft::{DraftMode, DraftSite, GridAxis, IssueKind};

    #[test]
    fn a_breakpoint_keeps_its_flag_between_its_two_widths() {
        let breakpoint = Breakpoint {
            on_from: 776.0,
            off_below: 744.0,
        };
        assert!(
            !breakpoint.update(false, 760.0),
            "an off flag waits for the upper width"
        );
        assert!(breakpoint.update(false, 776.0));
        assert!(breakpoint.update(true, 760.0), "an on flag waits for the lower width");
        assert!(breakpoint.update(true, 744.0));
        assert!(!breakpoint.update(true, 743.0));
    }

    #[test]
    fn the_plan_panel_and_the_stacked_form_switch_with_hysteresis() {
        let mut state = FormState::default();
        state.fit_tab(PLAN_PANEL_BREAKPOINT.on_from);
        assert!(state.plan_fits);
        state.fit_tab(750.0);
        assert!(state.plan_fits, "a width between the pair keeps the panel");
        state.fit_tab(740.0);
        assert!(!state.plan_fits);
        state.fit_tab(750.0);
        assert!(!state.plan_fits, "and keeps it away once gone");

        state.fit_form(500.0);
        assert!(!state.stacked);
        state.fit_form(405.0);
        assert!(!state.stacked, "stacking starts below 400");
        state.fit_form(399.0);
        assert!(state.stacked);
        state.fit_form(410.0);
        assert!(state.stacked, "and ends at 416");
        state.fit_form(416.0);
        assert!(!state.stacked);
    }

    #[test]
    fn the_label_column_fits_the_widest_label_within_its_bounds() {
        let layout = FormLayout::new(600.0, 130.0, 60.0, false, 8.0);
        assert_eq!(layout.label_width, 130.0);
        assert_eq!(layout.cell_width, 600.0 - 130.0 - 8.0);
        assert_eq!(layout.baseline_width, Some(60.0));
        assert_eq!(layout.values_width(), 600.0 - 130.0 - 8.0 - 60.0 - 8.0);

        let short = FormLayout::new(600.0, 40.0, 60.0, false, 8.0);
        assert_eq!(
            short.label_width, MIN_LABEL_WIDTH,
            "a short label keeps the column's minimum"
        );
        let long = FormLayout::new(450.0, 300.0, 60.0, false, 8.0);
        assert_eq!(long.label_width, 180.0, "a long label is held to 0.4 of the form");
        assert_eq!(long.baseline_width, None, "a form under 520 has no baseline column");
        assert_eq!(long.values_width(), long.cell_width);

        let narrow = FormLayout::new(200.0, 300.0, 60.0, false, 8.0);
        assert_eq!(narrow.label_width, MIN_LABEL_WIDTH, "the minimum wins over the share");

        let stacked = FormLayout::new(300.0, 130.0, 60.0, true, 8.0);
        assert_eq!((stacked.label_width, stacked.cell_width), (300.0, 300.0));
        assert_eq!(stacked.baseline_width, None);
    }

    #[test]
    fn indented_rows_keep_their_cells_in_place() {
        let layout = FormLayout::new(600.0, 130.0, 60.0, false, 8.0);
        let nested = layout.indented(18.0);
        assert_eq!(nested.label_width, 112.0, "the label column gives up the indent");
        assert_eq!(nested.cell_width, layout.cell_width, "so each cell starts where it did");

        let stacked = FormLayout::new(300.0, 130.0, 60.0, true, 8.0).indented(18.0);
        assert_eq!((stacked.label_width, stacked.cell_width), (282.0, 282.0));
    }

    #[test]
    fn a_header_counts_invalid_issues_over_missing_ones() {
        let mut count = IssueCount::default();
        assert_eq!(count.kind(), None);
        assert_eq!(count.phrase(), None);
        count.add(IssueKind::Missing);
        assert_eq!(count.kind(), Some(IssueKind::Missing));
        assert_eq!(count.phrase().as_deref(), Some("1 missing"));
        count.add(IssueKind::Invalid);
        assert_eq!(count.kind(), Some(IssueKind::Invalid));
        assert_eq!(count.total(), 2);
        assert_eq!(count.phrase().as_deref(), Some("2 problems"));
    }

    #[test]
    fn a_header_names_its_issue_count_between_its_title_and_its_summary() {
        let mut issues = IssueCount::default();
        let heading = |issues, summary| SectionHeading {
            title: "Parameters",
            issues,
            summary,
        };
        assert_eq!(
            heading_name(heading(issues, "2 of 5 varied")),
            "Parameters, 2 of 5 varied"
        );
        assert_eq!(heading_name(heading(issues, "")), "Parameters");
        issues.add(IssueKind::Invalid);
        assert_eq!(
            heading_name(heading(issues, "2 of 5 varied")),
            "Parameters, 1 problem, 2 of 5 varied"
        );
        assert_eq!(heading_name(heading(issues, "")), "Parameters, 1 problem");
    }

    #[test]
    fn an_issue_of_the_whole_sweep_belongs_to_the_design_or_the_search() {
        assert_eq!(
            SweepSection::of_site(DraftSite::Sweep, DraftMode::Sweep),
            SweepSection::Design
        );
        assert_eq!(
            SweepSection::of_site(DraftSite::Sweep, DraftMode::Search),
            SweepSection::Search
        );
        assert_eq!(
            SweepSection::of_site(DraftSite::Seed, DraftMode::Sweep),
            SweepSection::Seeds
        );
        assert_eq!(
            SweepSection::of_site(DraftSite::Stop, DraftMode::Search),
            SweepSection::RunLength
        );
    }

    #[test]
    fn each_new_site_belongs_to_the_section_that_draws_its_row() {
        let section = |site| SweepSection::of_site(site, DraftMode::Sweep);
        assert_eq!(section(DraftSite::Sampling), SweepSection::Outputs);
        assert_eq!(section(DraftSite::DesignSeed), SweepSection::Seeds);
        assert_eq!(section(DraftSite::Parameters), SweepSection::Parameters);
        assert_eq!(section(DraftSite::Execution), SweepSection::Execution);
        assert_eq!(section(DraftSite::Axis(GridAxis::X)), SweepSection::Search);
        assert_eq!(
            section(DraftSite::MethodSetting("genetic.elite_count")),
            SweepSection::Search
        );
    }

    #[test]
    fn a_reveal_of_the_whole_sweep_lands_on_the_first_section_of_its_mode() {
        assert_eq!(
            Reveal::site(DraftSite::Sweep, DraftMode::Search),
            Reveal {
                section: SweepSection::Search,
                site: Some(DraftSite::Search),
            }
        );
        let design = Reveal::site(DraftSite::Sweep, DraftMode::Sweep);
        assert_eq!(design.site, Some(DraftSite::Design));
        assert_eq!(
            design.in_mode(DraftMode::Search).section,
            SweepSection::Search,
            "a mode switch moves the reveal to the section drawn"
        );
        assert_eq!(
            Reveal::section(SweepSection::Outputs).in_mode(DraftMode::Search),
            Reveal::section(SweepSection::Outputs)
        );
    }

    #[test]
    fn a_focused_field_holds_back_its_issue_until_typing_stops() {
        assert!(holds_back_issue(true, Some(0.2)));
        assert!(
            !holds_back_issue(true, Some(ISSUE_DELAY.as_secs_f64())),
            "the delay has passed"
        );
        assert!(!holds_back_issue(false, Some(0.2)), "a field left shows its issue");
        assert!(!holds_back_issue(true, None), "a field not typed in shows its issue");
    }
}
