//! The sheet editor of the Editor tab: a sheet model shown as its sheet and its cards, read-only.
//!
//! A *sheet* is a model file of `kind = "sheet"`, the author document of [`henad_graph::sheet`]. The *Sheet* pane
//! lists what the model holds (its purpose, world, properties, parameters, measures, colours and buttons), and the
//! *Card* pane lists each card's numbered lines as sentences. The world itself stays in the dock's Viewport, Playback,
//! Pacing, Parameters and Charts tabs.
//!
//! A sheet opens through the sheet reader alone, so a sheet with checker errors still opens here. Its preview builds
//! only while the sheet reads, checks and lowers, and the preview waits paused at tick 0 until Play.

pub mod card_pane;
pub mod help;
pub mod new_model;
pub mod sheet_pane;

use std::sync::atomic::Ordering;

use egui::{CentralPanel, Frame, Margin, Panel, ScrollArea};
use henad_core::diagnostic::CodeDiagnostic;
use henad_graph::sheet::check::{SheetReport, check};
use henad_graph::sheet::document::SheetDocument;
use henad_graph::sheet::{ParsedSheet, SheetFile};

use super::graph::{CompiledProgram, GraphProgram, LOADS};

/// Heading of the pane that lists what the model holds.
pub const SHEET_PANE: &str = "Sheet";

/// Heading of the pane that lists the cards' lines.
pub const CARD_PANE: &str = "Card";

/// Default share of the Editor's width the Sheet pane takes. The Card pane takes the rest.
const SHEET_PANE_SHARE: f32 = 0.4;

/// Narrowest width in points of each pane.
const PANE_MIN_WIDTH: f32 = 160.0;

/// A sheet model in the Editor.
pub struct SheetEditor {
    /// Document the sheet holds.
    document: SheetDocument,
    /// Canonical text of the document. Save model writes it and Apply reads it.
    text: String,
    /// Report of the sheet's check, each problem placed in [`Self::text`].
    report: SheetReport,
    /// Sheet read, checked and lowered from [`Self::text`], or the problems that stop it lowering.
    file: Result<SheetFile, Vec<CodeDiagnostic>>,
    /// Number of the load. It keys the preview.
    load: u64,
    /// Program the preview runs, compiled on first use.
    program: Option<CompiledProgram>,
}

/// Prints the sheet's id, the load and whether the sheet lowers.
impl std::fmt::Debug for SheetEditor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SheetEditor")
            .field("id", &self.document.sheet.id)
            .field("load", &self.load)
            .field("lowers", &self.file.is_ok())
            .finish_non_exhaustive()
    }
}

impl SheetEditor {
    /// Returns an editor of `document`, checked and lowered from its canonical text.
    ///
    /// Note that a document whose canonical text does not write, as with a number that is not finite, keeps an empty
    /// text and the writer's error as the problem that stops it lowering. The sheet reader refuses such a number.
    pub fn new(document: SheetDocument) -> Self {
        let (text, report, file) = match document.to_text() {
            Ok(text) => {
                let report = ParsedSheet::parse(&text).map_or_else(|_| check(&document), |parsed| parsed.check());
                let file = SheetFile::parse(&text).map_err(|error| error.diagnostics().to_vec());
                (text, report, file)
            }
            Err(error) => (
                String::new(),
                check(&document),
                Err(vec![CodeDiagnostic::error(error.to_string())]),
            ),
        };
        Self {
            document,
            text,
            report,
            file,
            load: LOADS.fetch_add(1, Ordering::Relaxed),
            program: None,
        }
    }

    /// Document the sheet holds.
    pub fn document(&self) -> &SheetDocument {
        &self.document
    }

    /// Canonical text of the sheet. Save model writes it and Apply reads it.
    pub fn text(&self) -> &str {
        &self.text
    }

    /// Report of the sheet's check, each problem placed in [`Self::text`].
    pub fn report(&self) -> &SheetReport {
        &self.report
    }

    /// Returns whether the sheet holds a problem that stops it lowering. The preview is then left unbuilt.
    pub fn has_errors(&self) -> bool {
        self.file.is_err()
    }

    /// Problems that stop the sheet lowering, empty for a sheet that lowers.
    pub fn lowering_problems(&self) -> &[CodeDiagnostic] {
        self.file.as_ref().err().map_or(&[], Vec::as_slice)
    }

    /// Returns the key of what the preview shows. Every load of a sheet takes a key of its own.
    pub fn preview_key(&self) -> [u64; 3] {
        [self.load, 0, 0]
    }

    /// Returns the program the preview runs: the graph the sheet lowers to, compiled on first use.
    ///
    /// # Errors
    ///
    /// Returns the problems that stop the sheet lowering, or the compiler's problems, each placed at the sheet line
    /// whose node it names.
    pub fn program(&mut self) -> CompiledProgram {
        let file = &self.file;
        self.program
            .get_or_insert_with(|| match file {
                Ok(file) => GraphProgram::compile(file.lowered()).map_err(|problems| {
                    problems
                        .into_iter()
                        .map(|problem| file.place_graph_diagnostic(problem))
                        .collect()
                }),
                Err(problems) => Err(problems.clone()),
            })
            .clone()
    }

    /// Returns `diagnostic`, a problem of the graph the sheet lowers to, placed in the sheet: at the line of its node,
    /// at the table of its pass, or at `[sheet]`. A sheet that does not lower returns it as it is.
    pub fn place(&self, diagnostic: CodeDiagnostic) -> CodeDiagnostic {
        match &self.file {
            Ok(file) => file.place_graph_diagnostic(diagnostic),
            Err(_) => diagnostic,
        }
    }
}

/// Draws the sheet editor: the Sheet pane on the left and the Card pane beside it, each scrolling on its own.
///
/// Each pane is a group named by its heading.
pub fn sheet_ui(ui: &mut egui::Ui, sheet: &SheetEditor) {
    let width = ui.available_width();
    Panel::left("henad_sheet_pane")
        .resizable(true)
        .default_size(width * SHEET_PANE_SHARE)
        .min_size(PANE_MIN_WIDTH)
        .max_size((width - PANE_MIN_WIDTH).max(PANE_MIN_WIDTH))
        .frame(Frame::NONE.inner_margin(Margin::symmetric(8, 4)))
        .show(ui, |ui| {
            ui.heading(SHEET_PANE);
            ScrollArea::vertical()
                .id_salt("henad_sheet_pane_scroll")
                .auto_shrink([false; 2])
                .show(ui, |ui| sheet_pane::sheet_pane(ui, sheet.document()));
            pane_group(ui, SHEET_PANE);
        });
    CentralPanel::no_frame().show(ui, |ui| {
        Frame::NONE.inner_margin(Margin::symmetric(8, 4)).show(ui, |ui| {
            ui.heading(CARD_PANE);
            ScrollArea::vertical()
                .id_salt("henad_card_pane_scroll")
                .auto_shrink([false; 2])
                .show(ui, |ui| card_pane::card_pane(ui, sheet.document(), sheet.report()));
            pane_group(ui, CARD_PANE);
        });
    });
}

/// Gives the node of `ui` the role of a group named `name` with the bounds of what `ui` holds, as a pane of the sheet
/// editor takes.
fn pane_group(ui: &egui::Ui, name: &str) {
    accessible_group(
        ui.ctx(),
        ui.unique_id(),
        egui::accesskit::Role::Group,
        name,
        ui.min_rect(),
    );
}

/// Gives the accessible node `id` of a `Ui` the role `role`, the name `name` and the bounds `rect`.
///
/// egui gives the node of a `Ui` no bounds, and an accessibility client places a named group by them.
pub fn accessible_group(ctx: &egui::Context, id: egui::Id, role: egui::accesskit::Role, name: &str, rect: egui::Rect) {
    ctx.accesskit_node_builder(id, |node| {
        node.set_role(role);
        node.set_label(name);
        node.set_bounds(egui::accesskit::Rect {
            x0: rect.min.x.into(),
            y0: rect.min.y.into(),
            x1: rect.max.x.into(),
            y1: rect.max.y.into(),
        });
    });
}
