//! Heatmap view: an output over the levels of two varied parameters or action ticks.

use egui_plot::{Heatmap, Plot, PlotPoint, uniform_grid_spacer};
use web_time::Instant;

use crate::ui::results::ResultsRequest;
use crate::ui::results::plot::{
    color_bar, format_significant, heat_color, labeled_combo, level_formatter, refresh_due,
};
use crate::ui::results::response::{output_combo, pins_row};
use crate::ui::results::store::{HeatColor, HeatGrid, HeatQuery, ResultsStore};
use crate::ui::show_plot;

/// Most levels an axis can have to be a side of the heatmap.
pub const MAX_HEATMAP_LEVELS: usize = 64;

/// Most cells a heatmap writes its values on.
const MAX_LABELED_CELLS: usize = 100;

impl HeatColor {
    fn label(self) -> &'static str {
        match self {
            Self::Mean => "Mean",
            Self::StandardDeviation => "Standard deviation",
            Self::CoefficientOfVariation => "Coefficient of variation",
        }
    }
}

/// Settings of the Heatmap view.
#[derive(Debug, Default)]
pub struct HeatmapView {
    x_axis: usize,
    y_axis: usize,
    /// Index of the reducer column.
    output: usize,
    color_by: HeatColor,
    /// Level each axis is held at, `None` for any level.
    pins: Vec<Option<usize>>,
    cache: Option<HeatmapCache>,
}

/// Grid of one heatmap, with the query and the store revision it was computed from.
#[derive(Debug)]
struct HeatmapCache {
    revision: u64,
    computed_at: Instant,
    query: HeatQuery,
    grid: Option<HeatGrid>,
}

pub fn heatmap_ui(
    ui: &mut egui::Ui,
    store: &ResultsStore,
    view: &mut HeatmapView,
    request: &mut Option<ResultsRequest>,
) {
    let sides: Vec<usize> = (0..store.axes().len())
        .filter(|&axis| store.axes()[axis].levels.len() <= MAX_HEATMAP_LEVELS)
        .collect();
    if sides.len() < 2 {
        ui.weak("Vary two parameters over values or ranges to draw a heatmap.");
        return;
    }
    if store.reducer_columns().is_empty() {
        ui.weak("Heatmap will appear once the first run finishes.");
        return;
    }
    view.fit(store, &sides);
    controls(ui, store, view, &sides);
    let query = HeatQuery {
        x_axis: view.x_axis,
        y_axis: view.y_axis,
        output: view.output,
        color_by: view.color_by,
        pins: view.pins.clone(),
    };
    let (x_axis, y_axis) = (&store.axes()[view.x_axis], &store.axes()[view.y_axis]);
    let Some(grid) = view.grid(ui.ctx(), store, &query) else {
        return;
    };
    let (min, max) = grid.range().unwrap_or((0.0, 1.0));
    color_bar(ui, min, max);
    let heatmap = Heatmap::new(grid.values.clone(), grid.columns)
        .at(PlotPoint::new(-0.5, -0.5))
        .custom_mapping(Box::new(move |value| heat_color(value, min, max)))
        .formatter(Box::new(format_significant))
        .show_labels(grid.values.len() <= MAX_LABELED_CELLS);
    let plot = Plot::new("henad_results_heatmap")
        .x_axis_label(x_axis.label.as_str())
        .y_axis_label(y_axis.label.as_str())
        .x_axis_formatter(level_formatter(&x_axis.levels))
        .y_axis_formatter(level_formatter(&y_axis.levels))
        .x_grid_spacer(uniform_grid_spacer(|_| [1.0, 5.0, 10.0]))
        .y_grid_spacer(uniform_grid_spacer(|_| [1.0, 5.0, 10.0]))
        .show_crosshair(false);
    let response = show_plot(ui, plot, |plot_ui| {
        plot_ui.heatmap(heatmap);
        plot_ui.pointer_coordinate()
    });
    let Some((column, row)) = response.inner.and_then(|position| cell_at(grid, position)) else {
        return;
    };
    let cell = grid.cell(column, row);
    if response.response.clicked() && !grid.configs[cell].is_empty() {
        *request = Some(ResultsRequest::FilterConfigs(grid.configs[cell].clone()));
    }
    response.response.on_hover_text_at_pointer(format!(
        "{}, {}: {} (n = {})",
        x_axis.levels[column],
        y_axis.levels[row],
        format_significant(grid.values[cell]),
        grid.counts[cell]
    ));
}

/// Returns the column and row of the cell of `grid` at `position`, or `None` outside the grid.
fn cell_at(grid: &HeatGrid, position: PlotPoint) -> Option<(usize, usize)> {
    let (column, row) = (position.x.round(), position.y.round());
    if column < 0.0 || row < 0.0 {
        return None;
    }
    let (column, row) = (column as usize, row as usize);
    (column < grid.columns && row < grid.rows).then_some((column, row))
}

fn controls(ui: &mut egui::Ui, store: &ResultsStore, view: &mut HeatmapView, sides: &[usize]) {
    let axes = store.axes();
    ui.horizontal_wrapped(|ui| {
        let x_text = axes[view.x_axis].label.as_str();
        labeled_combo(ui, "X axis", "henad_results_heatmap_x", x_text, |ui| {
            for &axis in sides {
                ui.selectable_value(&mut view.x_axis, axis, axes[axis].label.as_str());
            }
        });
        let y_text = axes[view.y_axis].label.as_str();
        labeled_combo(ui, "Y axis", "henad_results_heatmap_y", y_text, |ui| {
            for &axis in sides.iter().filter(|&&axis| axis != view.x_axis) {
                ui.selectable_value(&mut view.y_axis, axis, axes[axis].label.as_str());
            }
        });
        output_combo(ui, "henad_results_heatmap_output", store, &mut view.output);
        let color_text = view.color_by.label();
        labeled_combo(ui, "Color by", "henad_results_heatmap_color", color_text, |ui| {
            for color_by in [
                HeatColor::Mean,
                HeatColor::StandardDeviation,
                HeatColor::CoefficientOfVariation,
            ] {
                ui.selectable_value(&mut view.color_by, color_by, color_by.label());
            }
        });
    });
    let free = [Some(view.x_axis), Some(view.y_axis)];
    pins_row(ui, "henad_results_heatmap_pin", axes, &mut view.pins, &free);
}

impl HeatmapView {
    /// Brings the settings within the axes `sides` and the outputs of `store`, the two axes apart.
    fn fit(&mut self, store: &ResultsStore, sides: &[usize]) {
        if !sides.contains(&self.x_axis) {
            self.x_axis = sides[0];
        }
        if !sides.contains(&self.y_axis) || self.y_axis == self.x_axis {
            self.y_axis = sides
                .iter()
                .copied()
                .find(|&axis| axis != self.x_axis)
                .unwrap_or(sides[1]);
        }
        self.output = self.output.min(store.reducer_columns().len() - 1);
        self.pins.resize(store.axes().len(), None);
        for (pin, axis) in self.pins.iter_mut().zip(store.axes()) {
            *pin = pin.filter(|&level| level < axis.levels.len());
        }
    }

    /// Returns the grid of `query`, computed again when the query changes, and at most once per
    /// [`REFRESH_INTERVAL`] while the store changes.
    ///
    /// `ctx` repaints once a pending refresh is due.
    ///
    /// [`REFRESH_INTERVAL`]: crate::ui::results::plot::REFRESH_INTERVAL
    fn grid(&mut self, ctx: &egui::Context, store: &ResultsStore, query: &HeatQuery) -> Option<&HeatGrid> {
        let stale = self.cache.as_ref().is_some_and(|cache| {
            if cache.query != *query || cache.revision == store.revision() {
                return cache.query != *query;
            }
            refresh_due(ctx, cache.computed_at)
        });
        if stale {
            self.cache = None;
        }
        let cache = self.cache.get_or_insert_with(|| HeatmapCache {
            revision: store.revision(),
            computed_at: Instant::now(),
            query: query.clone(),
            grid: store.heat_grid(query),
        });
        cache.grid.as_ref()
    }
}
