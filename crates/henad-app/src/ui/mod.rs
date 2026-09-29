pub mod about;
pub mod agent_layer;
pub mod charts;
pub mod dock;
pub mod edge_layer;
pub mod export;
pub mod fault;
pub mod files;
pub mod mcs;
pub mod menu_bar;
pub mod model;
pub mod pacing;
pub mod painted;
pub mod params;
pub mod performance;
pub mod playback;
pub mod results;
pub mod stats;
pub mod sweep;
pub mod system;
pub mod viewport;

pub fn banner(ui: &mut egui::Ui, icon: &str, color: egui::Color32, title: &str, detail: &str) {
    ui.horizontal_wrapped(|ui| {
        ui.colored_label(color, icon);
        ui.colored_label(color, title);
    });
    ui.label(detail);
    ui.separator();
}

/// Adds `bar` filled with `fill`, its text in [`mcs::GRAY_50`].
///
/// egui draws the text of a bar in the selection's text colour unless the text colour is overridden.
pub fn add_progress_bar(ui: &mut egui::Ui, bar: egui::ProgressBar, fill: egui::Color32) -> egui::Response {
    ui.scope(|ui| {
        ui.visuals_mut().override_text_color = Some(mcs::GRAY_50);
        ui.add(bar.fill(fill))
    })
    .inner
}

/// Returns `noun` as written after a count of `count`, as in "1 run" and "2 runs".
pub(crate) fn plural(count: u64, noun: &str) -> String {
    if count == 1 {
        noun.to_owned()
    } else {
        format!("{noun}s")
    }
}

const KV_GRID_SPACING: [f32; 2] = [16.0, 4.0];

/// Width in points that a stripe of a [`KvGrid`] reaches past each side of the grid, as egui's own stripes do.
const KV_STRIPE_OVERHANG: f32 = 2.0;

/// Returns a two-column key/value grid across the full panel width, each column pinned to half of it.
///
/// Left to itself, a `Grid` sizes to its content.
pub fn kv_grid(ui: &egui::Ui, id: &str) -> KvGrid {
    let col_width = ((ui.available_width() - KV_GRID_SPACING[0]) / 2.0).max(0.0);
    KvGrid {
        grid: egui::Grid::new(id)
            .num_columns(2)
            .striped(false)
            .spacing(KV_GRID_SPACING)
            .min_col_width(col_width),
    }
}

/// Two-column key/value grid whose every other row is striped with square corners.
///
/// egui paints the stripes of a `Grid` with a fixed corner radius of 2. The grid is therefore shown without them,
/// and [`KvGridRows::end_row`] paints its own.
pub struct KvGrid {
    grid: egui::Grid,
}

impl KvGrid {
    /// Shows the grid. `add_rows` draws the rows and ends each through [`KvGridRows::end_row`], never
    /// [`egui::Ui::end_row`].
    pub fn show<R>(
        self,
        ui: &mut egui::Ui,
        add_rows: impl FnOnce(&mut egui::Ui, &mut KvGridRows) -> R,
    ) -> egui::InnerResponse<R> {
        self.grid.show(ui, |ui| {
            let mut rows = KvGridRows::new(ui);
            add_rows(ui, &mut rows)
        })
    }
}

/// Rows of a [`KvGrid`] drawn so far, with a slot for the stripe under the row being drawn.
pub struct KvGridRows {
    count: usize,
    stripe: egui::layers::ShapeIdx,
}

impl KvGridRows {
    fn new(ui: &egui::Ui) -> Self {
        Self {
            count: 0,
            stripe: ui.painter().add(egui::Shape::Noop),
        }
    }

    /// Ends the row being drawn, and paints its stripe when it is an odd row.
    ///
    /// The slot for the next row's stripe is added before any of its cells, so the stripe lies under them.
    pub fn end_row(&mut self, ui: &mut egui::Ui) {
        if self.count % 2 == 1 {
            // Within a row, the grid's cursor stays at the row's top, and the cells drawn so far reach its bottom.
            let rect = egui::Rect::from_x_y_ranges(
                ui.max_rect().left()..=ui.max_rect().right().max(ui.min_rect().right()),
                ui.cursor().top()..=ui.min_rect().bottom(),
            )
            .expand2(egui::vec2(KV_STRIPE_OVERHANG, 0.5 * KV_GRID_SPACING[1]));
            let stripe = egui::epaint::RectShape::filled(rect, egui::CornerRadius::ZERO, ui.visuals().faint_bg_color);
            ui.painter().set(self.stripe, stripe);
        }
        ui.end_row();
        self.count += 1;
        self.stripe = ui.painter().add(egui::Shape::Noop);
    }
}

/// Shows `plot` over a background with square corners.
///
/// `egui_plot` paints its own background with a fixed corner radius of 2, so the plot shows without it. The
/// background is filled in under the plot once the plot has placed its frame.
pub fn show_plot<'a, R>(
    ui: &mut egui::Ui,
    plot: egui_plot::Plot<'a>,
    build: impl FnOnce(&mut egui_plot::PlotUi<'a>) -> R + 'a,
) -> egui_plot::PlotResponse<R> {
    let background = ui.painter().add(egui::Shape::Noop);
    let response = plot.show_background(false).show(ui, build);
    let visuals = ui.visuals();
    ui.painter().set(
        background,
        egui::epaint::RectShape::new(
            *response.transform.frame(),
            egui::CornerRadius::ZERO,
            visuals.extreme_bg_color,
            visuals.widgets.noninteractive.bg_stroke,
            egui::StrokeKind::Inside,
        ),
    );
    response
}
