//! Search view: the best objective against evaluations, the generations of a genetic algorithm, or the grid of a
//! Pattern Space Exploration as it fills.

use std::collections::BTreeMap;

use egui::Color32;
use egui_plot::{Heatmap, Legend, Line, Plot, PlotPoint};
use henad_core::explore::search::pse::{ArchiveEntry, PatternAxis, PatternCell, PatternSpaceSettings};
use henad_core::explore::search::{Aggregate, GenerationSummary, Goal, SearchAlgorithm};
use henad_explore::output::search_tables::SearchHistory;

use crate::icons::material_design_icons::MDI_ALERT;
use crate::ui::results::ResultsRequest;
use crate::ui::results::plot::{CONFIG_COLORS, color_bar, format_precise, format_significant, heat_color};
use crate::ui::results::store::{ResultsStore, SearchLog, output_label};
use crate::ui::sweep::draft::DraftAlgorithm;
use crate::ui::sweep::search::algorithm_label;
use crate::ui::{plural, show_plot};

/// Most cells a grid can have for the view to draw it.
const MAX_DRAWN_CELLS: u64 = 1 << 16;

/// State the Search view keeps between frames.
#[derive(Debug, Default)]
pub struct SearchView {
    /// Grid of the last Pattern Space Exploration drawn, with the store revision it was built from.
    grid_cache: Option<(u64, PatternGrid)>,
}

/// Cells of a Pattern Space Exploration's grid, row by row from the lowest cell of the y axis.
#[derive(Debug, Clone, PartialEq)]
pub struct PatternGrid {
    /// Number of cells along the x axis.
    pub columns: usize,
    /// Number of cells along the y axis.
    pub rows: usize,
    /// Candidates that landed in each cell, not finite for a cell none landed in.
    pub hits: Vec<f64>,
    /// Exemplar of each cell, the first candidate to land in it.
    pub exemplars: Vec<Option<u64>>,
    /// Number of cells some candidate landed in.
    pub filled: usize,
}

impl PatternGrid {
    /// Returns the position in the grid's vectors of the cell at x index `column` and y index `row`.
    pub fn index(&self, column: usize, row: usize) -> usize {
        row * self.columns + column
    }
}

/// Returns the grid of `settings` with the cells of `archive` in it.
///
/// A cell past the grid's axes is left out.
pub fn pattern_grid(settings: &PatternSpaceSettings, archive: &BTreeMap<PatternCell, ArchiveEntry>) -> PatternGrid {
    let (columns, rows) = (settings.x_axis.cells as usize, settings.y_axis.cells as usize);
    let mut grid = PatternGrid {
        columns,
        rows,
        hits: vec![f64::NAN; columns * rows],
        exemplars: vec![None; columns * rows],
        filled: 0,
    };
    for (cell, entry) in archive {
        let (column, row) = (cell.x_index as usize, cell.y_index as usize);
        if column >= columns || row >= rows {
            continue;
        }
        let index = grid.index(column, row);
        grid.hits[index] = entry.hits as f64;
        grid.exemplars[index] = Some(entry.candidate_id);
        grid.filled += 1;
    }
    grid
}

/// Returns the best objective after each batch against the evaluations told by then, as the steps of a line.
///
/// The best holds from one batch to the next, so a change draws as a vertical step at the batch that made it. A
/// batch with no finite best is left out.
pub fn best_so_far_points(history: &SearchHistory) -> Vec<[f64; 2]> {
    let mut points: Vec<[f64; 2]> = Vec::with_capacity(history.batches.len() * 2);
    for standing in &history.batches {
        let Some(best) = standing.best_objective else {
            continue;
        };
        let evaluations = standing.evaluations as f64;
        if let Some(&[_, previous]) = points.last()
            && previous != best
        {
            points.push([evaluations, previous]);
        }
        points.push([evaluations, best]);
    }
    points
}

/// Returns the best, the median and the worst objective of each finished generation against its index, finite
/// values alone.
pub fn generation_lines(generations: &[GenerationSummary]) -> [Vec<[f64; 2]>; 3] {
    let line = |value: fn(&GenerationSummary) -> f64| -> Vec<[f64; 2]> {
        generations
            .iter()
            .filter(|generation| value(generation).is_finite())
            .map(|generation| [generation.generation as f64, value(generation)])
            .collect()
    };
    [
        line(|generation| generation.best),
        line(|generation| generation.median),
        line(|generation| generation.worst),
    ]
}

/// Returns the id of the first run of candidate `candidate_id` that the store holds.
fn first_run(store: &ResultsStore, candidate_id: u64) -> Option<u64> {
    store.config_runs(candidate_id).next().map(|outcome| outcome.run.run_id)
}

pub fn search_ui(ui: &mut egui::Ui, store: &ResultsStore, view: &mut SearchView, request: &mut Option<ResultsRequest>) {
    let Some(search) = store.search_log() else {
        ui.weak("Search results will appear here while a search runs.");
        return;
    };
    ui.label(heading_text(search));
    if let Some(error) = &search.table_error {
        ui.colored_label(
            ui.visuals().warn_fg_color,
            format!("{MDI_ALERT} Search tables unreadable: {error}"),
        );
    }
    match &search.spec.algorithm {
        SearchAlgorithm::PatternSpaceExploration(settings) => {
            // An automatic range is known once the initial samples are told.
            let settings = search.history.pattern_settings.as_ref().unwrap_or(settings);
            grid_ui(ui, store, view, settings, request);
        }
        algorithm => {
            best_line(ui, store, search, request);
            if matches!(algorithm, SearchAlgorithm::Genetic(_)) {
                generations_plot(ui, search);
            } else {
                best_so_far_plot(ui, search);
            }
        }
    }
}

/// Returns the line naming the method and its objective or its two axes.
fn heading_text(search: &SearchLog) -> String {
    let aggregate = |aggregate: Aggregate| match aggregate {
        Aggregate::Median => "median",
        Aggregate::Mean => "mean",
    };
    let method = algorithm_label(DraftAlgorithm::from(&search.spec.algorithm));
    if let SearchAlgorithm::PatternSpaceExploration(settings) = &search.spec.algorithm {
        return format!(
            "{method} · {} against {} · {} of replicates",
            output_label(&settings.y_axis.column),
            output_label(&settings.x_axis.column),
            aggregate(settings.aggregate)
        );
    }
    match &search.spec.objective {
        Some(objective) => {
            let goal = match objective.goal {
                Goal::Maximize => "maximize",
                Goal::Minimize => "minimize",
            };
            format!(
                "{method} · {goal} {} · {} of replicates",
                output_label(&objective.column),
                aggregate(objective.aggregate)
            )
        }
        None => method.to_owned(),
    }
}

/// Draws the evaluations told of the budget, and the best candidate with a button that selects its first run.
fn best_line(ui: &mut egui::Ui, store: &ResultsStore, search: &SearchLog, request: &mut Option<ResultsRequest>) {
    let last = search.history.batches.last();
    let evaluations = last.map_or(0, |standing| standing.evaluations);
    ui.horizontal_wrapped(|ui| {
        ui.label(format!("{evaluations} of {} evaluations", search.spec.max_evaluations));
        let Some(standing) = last else {
            return;
        };
        let (Some(candidate_id), Some(objective)) = (standing.best_candidate_id, standing.best_objective) else {
            return;
        };
        ui.label(format!(
            "· best {}, candidate {candidate_id}",
            format_precise(objective)
        ));
        if let Some(run_id) = first_run(store, candidate_id)
            && ui
                .button("Select run")
                .on_hover_text("Select first run of best candidate in the table below")
                .clicked()
        {
            *request = Some(ResultsRequest::SelectRun(run_id));
        }
    });
}

/// Returns the y axis label of a plot of the search's objective.
fn objective_label(search: &SearchLog) -> String {
    search
        .spec
        .objective
        .as_ref()
        .map_or_else(|| "Objective".to_owned(), |objective| output_label(&objective.column))
}

fn best_so_far_plot(ui: &mut egui::Ui, search: &SearchLog) {
    let points = best_so_far_points(&search.history);
    if points.is_empty() {
        ui.weak("Best so far will appear once the first batch finishes.");
        return;
    }
    let plot = Plot::new("henad_results_best_so_far")
        .x_axis_label("Evaluations")
        .y_axis_label(objective_label(search));
    show_plot(ui, plot, |plot_ui| {
        plot_ui.line(Line::new("Best so far", points).color(CONFIG_COLORS[0]).width(2.0));
    });
}

fn generations_plot(ui: &mut egui::Ui, search: &SearchLog) {
    if search.history.generations.is_empty() {
        ui.weak("First generation is still running.");
        return;
    }
    let [best, median, worst] = generation_lines(&search.history.generations);
    let plot = Plot::new("henad_results_generations")
        .legend(Legend::default())
        .x_axis_label("Generation")
        .y_axis_label(objective_label(search));
    show_plot(ui, plot, |plot_ui| {
        let lines = [("Best", best), ("Median", median), ("Worst", worst)];
        for (position, (name, points)) in lines.into_iter().enumerate() {
            plot_ui.line(Line::new(name, points).color(CONFIG_COLORS[position]).width(2.0));
        }
    });
}

fn grid_ui(
    ui: &mut egui::Ui,
    store: &ResultsStore,
    view: &mut SearchView,
    settings: &PatternSpaceSettings,
    request: &mut Option<ResultsRequest>,
) {
    let Some(search) = store.search_log() else {
        return;
    };
    let (x_axis, y_axis) = (&settings.x_axis, &settings.y_axis);
    let total = u64::from(x_axis.cells) * u64::from(y_axis.cells);
    if total > MAX_DRAWN_CELLS {
        ui.weak(format!("Grid of {total} cells is too large to draw."));
        return;
    }
    let (Some((x_min, x_max)), Some((y_min, y_max))) = (x_axis.range(), y_axis.range()) else {
        ui.weak("Grid will appear once the initial samples finish.");
        return;
    };
    let stale = view
        .grid_cache
        .as_ref()
        .is_none_or(|(revision, _)| *revision != store.revision());
    if stale {
        view.grid_cache = Some((store.revision(), pattern_grid(settings, &search.history.archive)));
    }
    let Some((_, grid)) = &view.grid_cache else {
        return;
    };
    let evaluations = search.history.batches.last().map_or(0, |standing| standing.evaluations);
    ui.horizontal_wrapped(|ui| {
        ui.label(format!(
            "{} of {total} cells filled, {evaluations} of {} evaluations",
            grid.filled, search.spec.max_evaluations
        ));
        ui.weak("Click a cell to select the run of its first candidate.");
    });
    let outside = search.history.outside_count;
    if outside > 0 {
        // An automatic axis shows no bounds to widen in the Sweep tab.
        let automatic = matches!(
            &search.spec.algorithm,
            SearchAlgorithm::PatternSpaceExploration(spec) if spec.x_axis.is_automatic() || spec.y_axis.is_automatic()
        );
        let advice = if automatic {
            "Press Use range from results in the Sweep tab to include these evaluations"
        } else {
            "Widen axes in the Sweep tab to include these evaluations"
        };
        ui.colored_label(
            ui.visuals().warn_fg_color,
            format!(
                "{MDI_ALERT} {outside} {} outside the axes, counted in the edge cells",
                plural(outside, "evaluation")
            ),
        )
        .on_hover_text(advice);
    }
    let max_hits = grid
        .hits
        .iter()
        .copied()
        .filter(|hits| hits.is_finite())
        .fold(1.0, f64::max);
    color_bar(ui, 1.0, max_hits);
    let empty_color = ui.visuals().faint_bg_color;
    let heatmap = Heatmap::new(grid.hits.clone(), grid.columns)
        .at(PlotPoint::new(x_min, y_min))
        .tile_size(
            ((x_max - x_min) / f64::from(x_axis.cells)) as f32,
            ((y_max - y_min) / f64::from(y_axis.cells)) as f32,
        )
        .custom_mapping(Box::new(move |hits| hits_color(hits, max_hits, empty_color)))
        // The heatmap lays out a label for every cell even when it shows none.
        .formatter(Box::new(|_| String::new()))
        .show_labels(false);
    let plot = Plot::new("henad_results_pattern_grid")
        .x_axis_label(output_label(&x_axis.column))
        .y_axis_label(output_label(&y_axis.column))
        .show_crosshair(false);
    let response = show_plot(ui, plot, |plot_ui| {
        plot_ui.heatmap(heatmap);
        plot_ui.pointer_coordinate()
    });
    let Some(position) = response.inner else {
        return;
    };
    let (Some(column), Some(row)) = (axis_cell_index(position.x, x_axis), axis_cell_index(position.y, y_axis)) else {
        return;
    };
    let index = grid.index(column, row);
    let Some(mut hover) = cell_ranges_text(settings, column, row) else {
        return;
    };
    match grid.exemplars[index] {
        Some(candidate_id) => {
            let hits = grid.hits[index] as u64;
            let noun = if hits == 1 { "hit" } else { "hits" };
            hover.push_str(&format!("\n{hits} {noun}, first candidate {candidate_id}"));
            if response.response.clicked()
                && let Some(run_id) = first_run(store, candidate_id)
            {
                *request = Some(ResultsRequest::SelectRun(run_id));
            }
        }
        None => hover.push_str("\nNo candidate yet"),
    }
    response.response.on_hover_text_at_pointer(hover);
}

/// Returns the range of each output in the cell at x index `column` and y index `row`, one line per axis, or `None`
/// while an axis has no range.
fn cell_ranges_text(settings: &PatternSpaceSettings, column: usize, row: usize) -> Option<String> {
    let (x_axis, y_axis) = (&settings.x_axis, &settings.y_axis);
    let (x_low, x_high) = x_axis.cell_bounds(column as u32)?;
    let (y_low, y_high) = y_axis.cell_bounds(row as u32)?;
    Some(format!(
        "{}: {} to {}\n{}: {} to {}",
        output_label(&x_axis.column),
        format_significant(x_low),
        format_significant(x_high),
        output_label(&y_axis.column),
        format_significant(y_low),
        format_significant(y_high)
    ))
}

/// Returns the index along `axis` of the cell at `value`, or `None` outside the axis or on an axis with no range.
fn axis_cell_index(value: f64, axis: &PatternAxis) -> Option<usize> {
    let (min, max) = axis.range()?;
    if !(min..=max).contains(&value) {
        return None;
    }
    Some(axis.cell_index(value)?.0 as usize)
}

/// Returns the colour of a cell `hits` candidates landed in, on a log scale up to `max_hits`, or `empty` for a cell
/// none landed in.
fn hits_color(hits: f64, max_hits: f64, empty: Color32) -> Color32 {
    if !hits.is_finite() {
        return empty;
    }
    heat_color(hits.ln(), 0.0, max_hits.ln())
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use henad_core::explore::search::GenerationSummary;
    use henad_core::explore::search::pse::{ArchiveEntry, PatternAxis, PatternCell, PatternSpaceSettings};
    use henad_explore::output::search_tables::{BatchStanding, SearchHistory};

    use super::{best_so_far_points, generation_lines, pattern_grid};

    fn standing(batch: u64, evaluations: u64, best_objective: Option<f64>) -> BatchStanding {
        BatchStanding {
            batch,
            evaluations,
            runs: evaluations * 2,
            best_candidate_id: best_objective.map(|_| batch),
            best_objective,
            filled_cells: 0,
        }
    }

    #[test]
    fn the_best_so_far_steps_at_the_batch_that_changes_it() {
        let history = SearchHistory {
            batches: vec![
                standing(0, 4, None),
                standing(1, 8, Some(3.0)),
                standing(2, 12, Some(3.0)),
                standing(3, 16, Some(5.0)),
            ],
            ..SearchHistory::default()
        };
        assert_eq!(
            best_so_far_points(&history),
            [[8.0, 3.0], [12.0, 3.0], [16.0, 3.0], [16.0, 5.0]],
            "a batch with no best is left out, and a change steps up at its batch"
        );
    }

    #[test]
    fn generation_lines_leave_out_values_that_are_not_finite() {
        let generation = |generation, worst| GenerationSummary {
            generation,
            best: 9.0,
            median: 5.0,
            worst,
        };
        let [best, median, worst] = generation_lines(&[generation(0, f64::NEG_INFINITY), generation(1, 2.0)]);
        assert_eq!(best, [[0.0, 9.0], [1.0, 9.0]]);
        assert_eq!(median, [[0.0, 5.0], [1.0, 5.0]]);
        assert_eq!(worst, [[1.0, 2.0]], "a failed member's worst is left out");
    }

    #[test]
    fn a_pattern_grid_places_each_cell_by_row_from_the_lowest_y() {
        let axis = |cells| PatternAxis::bounded("Infected:max", 0.0, 10.0, cells);
        let settings = PatternSpaceSettings::new(axis(3), axis(2));
        let entry = |x_index, y_index, hits, candidate_id| {
            let cell = PatternCell { x_index, y_index };
            let entry = ArchiveEntry {
                cell,
                hits,
                candidate_id,
                x: 0.0,
                y: 0.0,
            };
            (cell, entry)
        };
        let archive = BTreeMap::from([entry(0, 0, 4, 7), entry(2, 1, 1, 11), entry(5, 0, 2, 3)]);
        let grid = pattern_grid(&settings, &archive);

        assert_eq!((grid.columns, grid.rows), (3, 2));
        assert_eq!(grid.filled, 2, "a cell past the axes is left out");
        assert_eq!(grid.exemplars, [Some(7), None, None, None, None, Some(11)]);
        assert_eq!(grid.hits[grid.index(0, 0)], 4.0);
        assert_eq!(grid.hits[grid.index(2, 1)], 1.0);
        assert!(grid.hits[grid.index(1, 1)].is_nan(), "an empty cell holds no value");
    }
}
