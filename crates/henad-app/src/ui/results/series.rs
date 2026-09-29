//! Series view: one stat over time, with a band over the replicates of each selected config, and optionally every
//! run as a thin line.

use std::collections::BTreeSet;

use egui::Id;
use egui_plot::{FilledArea, Legend, LegendGrouping, Line, Plot, PlotPoint, PlotResponse};
use web_time::Instant;

use crate::ui::results::ResultsRequest;
use crate::ui::results::plot::{MAX_PLOT_POINTS, config_color, decimate, labeled_combo, refresh_due};
use crate::ui::results::store::{Band, BandKind, ResultsStore};
use crate::ui::{plural, show_plot};

/// Most configs the view draws at once.
pub const MAX_DRAWN_CONFIGS: usize = 10;

/// Most configs the Configurations menu lists.
const MAX_LISTED_CONFIGS: usize = 2000;

/// Settings of the Series view.
#[derive(Debug, Default)]
pub struct SeriesView {
    /// Index of the stat column drawn.
    stat: usize,
    band: BandKind,
    /// Whether every replicate is drawn as a thin line.
    show_runs: bool,
    cache: Option<SeriesPlot>,
}

/// Bands and run lines of one drawing, with the settings and the run counts they were computed from.
#[derive(Debug)]
struct SeriesPlot {
    /// Number of runs, and of runs with their series held, of each drawn config.
    run_counts: Vec<(usize, usize)>,
    computed_at: Instant,
    stat: usize,
    band: BandKind,
    show_runs: bool,
    configs: Vec<u64>,
    /// Band of each drawn config, `None` for a config with no series held.
    bands: Vec<Option<ConfigBand>>,
    /// Line of each run of the drawn configs.
    run_lines: Vec<RunLine>,
}

/// Band of one config, thinned for drawing.
#[derive(Debug)]
struct ConfigBand {
    config_id: u64,
    /// Band whose low and high ends the fill spans.
    band: Band,
    /// Centre line, decimated so its spikes survive.
    center: Vec<PlotPoint>,
}

/// Line of one run, thinned for drawing.
#[derive(Debug)]
struct RunLine {
    run_id: u64,
    /// Position of the run's config among the drawn configs.
    position: usize,
    points: Vec<PlotPoint>,
}

/// Returns the name of `kind` in the Band menu.
fn band_label(kind: BandKind) -> &'static str {
    match kind {
        BandKind::StandardDeviation => "Mean ± SD",
        BandKind::ConfidenceInterval => "Mean ± 95% CI",
        BandKind::Percentiles => "Median, 10–90%",
    }
}

/// Returns the tooltip of `kind` in the Band menu.
fn band_tooltip(kind: BandKind) -> &'static str {
    match kind {
        BandKind::StandardDeviation => "Mean ± one standard deviation (SD) of replicates",
        BandKind::ConfidenceInterval => "Mean with 95% confidence interval (CI)",
        BandKind::Percentiles => "Median with 10th to 90th percentile of replicates",
    }
}

pub fn series_ui(
    ui: &mut egui::Ui,
    store: &ResultsStore,
    view: &mut SeriesView,
    selected_configs: &mut BTreeSet<u64>,
    request: &mut Option<ResultsRequest>,
) {
    if store.stat_columns().is_empty() {
        ui.weak("Series will appear once the first run finishes.");
        return;
    }
    if !store.runs().is_empty() && store.empty_series_count() == store.runs().len() {
        ui.weak("No run recorded a series.");
        return;
    }
    view.stat = view.stat.min(store.stat_columns().len() - 1);
    controls(ui, store, view, selected_configs);
    held_line(ui, store, selected_configs, request);

    let drawn: Vec<u64> = selected_configs.iter().take(MAX_DRAWN_CONFIGS).copied().collect();
    if selected_configs.len() > MAX_DRAWN_CONFIGS {
        ui.weak(format!(
            "Showing the first {MAX_DRAWN_CONFIGS} of {} selected configurations",
            selected_configs.len()
        ));
    }
    if drawn.is_empty() {
        ui.weak("Select configurations to draw.");
        return;
    }
    let plotted = view.cached_plot(ui.ctx(), store, drawn);
    let labels: Vec<String> = plotted
        .configs
        .iter()
        .map(|&config_id| store.config_label(config_id))
        .collect();
    let response = draw_plot(ui, plotted, &labels, store.stat_columns()[plotted.stat].as_str());
    if response.response.clicked()
        && let Some(hovered) = response.hovered_plot_item
        && let Some(line) = plotted
            .run_lines
            .iter()
            .find(|line| run_line_id(line.run_id) == hovered)
    {
        *request = Some(ResultsRequest::SelectRun(line.run_id));
    }
}

/// Draws the bands and run lines of `plotted`, with `stat` on the y axis.
///
/// `labels` holds the name of each drawn config, in the order of `plotted.configs`. The legend lists the configs in
/// that order, one entry each.
fn draw_plot(ui: &mut egui::Ui, plotted: &SeriesPlot, labels: &[String], stat: &str) -> PlotResponse<()> {
    let plot = Plot::new("henad_results_series")
        .legend(
            Legend::default()
                .follow_insertion_order(true)
                .grouping(LegendGrouping::ById),
        )
        .x_axis_label("Tick")
        .y_axis_label(stat);
    show_plot(ui, plot, |plot_ui| {
        for line in &plotted.run_lines {
            plot_ui.line(
                Line::new("", line.points.as_slice())
                    .color(config_color(line.position).gamma_multiply(0.45))
                    .width(1.0)
                    .id(run_line_id(line.run_id)),
            );
        }
        for (position, (drawn, label)) in plotted.bands.iter().zip(labels).enumerate() {
            let Some(drawn) = drawn else {
                continue;
            };
            let color = config_color(position);
            let id = config_item_id(drawn.config_id);
            let band = &drawn.band;
            // Unnamed, the band stays out of the legend. The shared id hides it with its line.
            plot_ui.add(
                FilledArea::new("", &band.ticks, &band.low, &band.high)
                    .fill_color(color.gamma_multiply(0.25))
                    .allow_hover(false)
                    .id(id),
            );
            plot_ui.line(
                Line::new(label.as_str(), drawn.center.as_slice())
                    .color(color)
                    .width(2.0)
                    .id(id),
            );
        }
    })
}

fn run_line_id(run_id: u64) -> Id {
    Id::new(("henad_results_run", run_id))
}

fn config_item_id(config_id: u64) -> Id {
    Id::new(("henad_results_config", config_id))
}

fn controls(ui: &mut egui::Ui, store: &ResultsStore, view: &mut SeriesView, selected_configs: &mut BTreeSet<u64>) {
    ui.horizontal_wrapped(|ui| {
        let stat_text = store.stat_columns()[view.stat].as_str();
        labeled_combo(ui, "Stat", "henad_results_series_stat", stat_text, |ui| {
            for (index, name) in store.stat_columns().iter().enumerate() {
                ui.selectable_value(&mut view.stat, index, name.as_str());
            }
        });
        configs_menu(ui, store, selected_configs);
        labeled_combo(ui, "Band", "henad_results_series_band", band_label(view.band), |ui| {
            for kind in [
                BandKind::StandardDeviation,
                BandKind::ConfidenceInterval,
                BandKind::Percentiles,
            ] {
                ui.selectable_value(&mut view.band, kind, band_label(kind))
                    .on_hover_text(band_tooltip(kind));
            }
        })
        .on_hover_text(band_tooltip(view.band));
        ui.checkbox(&mut view.show_runs, "Show runs")
            .on_hover_text("Draw every replicate as a thin line. Click one to select its run.");
    });
}

/// Draws the menu that picks the configs the view draws.
fn configs_menu(ui: &mut egui::Ui, store: &ResultsStore, selected_configs: &mut BTreeSet<u64>) {
    ui.menu_button(format!("Configurations ({})", selected_configs.len()), |ui| {
        ui.horizontal(|ui| {
            if ui.button("All").clicked() {
                selected_configs.extend(store.config_ids());
            }
            if ui.button("None").clicked() {
                selected_configs.clear();
            }
        });
        ui.separator();
        let listed: Vec<u64> = store.config_ids().take(MAX_LISTED_CONFIGS).collect();
        egui::ScrollArea::vertical().max_height(320.0).show_rows(
            ui,
            ui.spacing().interact_size.y,
            listed.len(),
            |ui, rows| {
                for &config_id in &listed[rows] {
                    let mut selected = selected_configs.contains(&config_id);
                    if ui.checkbox(&mut selected, store.config_label(config_id)).changed() {
                        if selected {
                            selected_configs.insert(config_id);
                        } else {
                            selected_configs.remove(&config_id);
                        }
                    }
                }
            },
        );
        if store.config_count() > MAX_LISTED_CONFIGS {
            ui.weak(format!(
                "First {MAX_LISTED_CONFIGS} of {} configurations",
                store.config_count()
            ));
        }
    });
}

/// Draws the count of runs whose series is held, with Load series where a folder holds the rest.
///
/// Runs that recorded no series count neither way.
fn held_line(
    ui: &mut egui::Ui,
    store: &ResultsStore,
    selected_configs: &BTreeSet<u64>,
    request: &mut Option<ResultsRequest>,
) {
    let held = store.series_cache().len() as u64;
    let recorded = (store.runs().len() - store.empty_series_count()) as u64;
    if held == recorded {
        return;
    }
    ui.horizontal(|ui| {
        ui.weak(format!(
            "Series loaded for {held} of {recorded} {}",
            plural(recorded, "run")
        ));
        let drawn: BTreeSet<u64> = selected_configs.iter().take(MAX_DRAWN_CONFIGS).copied().collect();
        if cfg!(not(target_arch = "wasm32"))
            && store.folder().is_some()
            && !store.runs_without_series(&drawn).is_empty()
            && ui
                .button("Load series")
                .on_hover_text("Load remaining series from results folder")
                .clicked()
        {
            *request = Some(ResultsRequest::LoadSeries);
        }
    });
}

impl SeriesView {
    /// Returns the bands and run lines of `configs`, computed again when the settings change, and at most once per
    /// [`REFRESH_INTERVAL`] while runs land in `configs`.
    ///
    /// Runs landing in other configs leave the drawing alone. `ctx` repaints once a pending refresh is due.
    ///
    /// [`REFRESH_INTERVAL`]: crate::ui::results::plot::REFRESH_INTERVAL
    fn cached_plot(&mut self, ctx: &egui::Context, store: &ResultsStore, configs: Vec<u64>) -> &SeriesPlot {
        let (stat, band, show_runs) = (self.stat, self.band, self.show_runs);
        let run_counts = run_counts(store, &configs);
        let stale = self.cache.as_ref().is_some_and(|cache| {
            let settings_changed =
                cache.stat != stat || cache.band != band || cache.show_runs != show_runs || cache.configs != configs;
            if settings_changed || cache.run_counts == run_counts {
                return settings_changed;
            }
            refresh_due(ctx, cache.computed_at)
        });
        if stale {
            self.cache = None;
        }
        self.cache.get_or_insert_with(|| {
            let bands = configs
                .iter()
                .map(|&config_id| {
                    let band = store.band(config_id, stat, band)?;
                    Some(ConfigBand {
                        config_id,
                        center: center_line(&band),
                        band: thin_band(band),
                    })
                })
                .collect();
            let run_lines = if show_runs {
                run_lines(store, &configs, stat)
            } else {
                Vec::new()
            };
            SeriesPlot {
                run_counts,
                computed_at: Instant::now(),
                stat,
                band,
                show_runs,
                configs,
                bands,
                run_lines,
            }
        })
    }
}

/// Returns the number of runs, and of runs with their series held, of each config of `configs`.
fn run_counts(store: &ResultsStore, configs: &[u64]) -> Vec<(usize, usize)> {
    configs
        .iter()
        .map(|&config_id| {
            let (mut runs, mut held) = (0, 0);
            for outcome in store.config_runs(config_id) {
                runs += 1;
                held += usize::from(store.series(outcome.run.run_id).is_some());
            }
            (runs, held)
        })
        .collect()
}

/// Returns the line of stat `stat` for every run of `configs` whose series is held, finite values only.
fn run_lines(store: &ResultsStore, configs: &[u64], stat: usize) -> Vec<RunLine> {
    let mut lines = Vec::new();
    for (position, &config_id) in configs.iter().enumerate() {
        for outcome in store.config_runs(config_id) {
            let Some(series) = store.series(outcome.run.run_id) else {
                continue;
            };
            if stat >= series.width() {
                continue;
            }
            let points: Vec<[f64; 2]> = series
                .rows()
                .filter(|(_, row)| row[stat].is_finite())
                .map(|(tick, row)| [tick as f64, row[stat]])
                .collect();
            let points = decimate(&points, MAX_PLOT_POINTS);
            lines.push(RunLine {
                run_id: outcome.run.run_id,
                position,
                points: points.into_iter().map(|[x, y]| PlotPoint::new(x, y)).collect(),
            });
        }
    }
    lines
}

/// Returns the centre line of `band`, decimated to at most [`MAX_PLOT_POINTS`] points.
fn center_line(band: &Band) -> Vec<PlotPoint> {
    let points: Vec<[f64; 2]> = band
        .ticks
        .iter()
        .zip(&band.center)
        .map(|(&tick, &center)| [tick, center])
        .collect();
    decimate(&points, MAX_PLOT_POINTS)
        .into_iter()
        .map(|[tick, center]| PlotPoint::new(tick, center))
        .collect()
}

/// Returns `band` with at most [`MAX_PLOT_POINTS`] ticks, each kept tick spanning the lowest low and highest high of
/// the ticks it stands for.
///
/// Note that the thinned band's centre is the middle sample of each bucket, and [`center_line`] draws the centre.
fn thin_band(band: Band) -> Band {
    let count = band.ticks.len();
    if count <= MAX_PLOT_POINTS {
        return band;
    }
    let bucket = count.div_ceil(MAX_PLOT_POINTS);
    let mut thinned_band = Band {
        runs: band.runs,
        ..Band::default()
    };
    for start in (0..count).step_by(bucket) {
        let range = start..(start + bucket).min(count);
        let middle = (range.start + range.end - 1) / 2;
        thinned_band.ticks.push(band.ticks[middle]);
        thinned_band.center.push(band.center[middle]);
        thinned_band
            .low
            .push(band.low[range.clone()].iter().copied().fold(f64::INFINITY, f64::min));
        thinned_band
            .high
            .push(band.high[range].iter().copied().fold(f64::NEG_INFINITY, f64::max));
    }
    thinned_band
}

#[cfg(test)]
mod tests {
    use egui::{Pos2, Rect, Shape, vec2};
    use egui_plot::PlotPoint;
    use web_time::Instant;

    use super::{ConfigBand, SeriesPlot, center_line, draw_plot, thin_band};
    use crate::ui::results::plot::MAX_PLOT_POINTS;
    use crate::ui::results::store::{Band, BandKind};

    /// Adds the text of every text shape in `shape` to `texts`, in paint order.
    fn collect_texts(shape: &Shape, texts: &mut Vec<String>) {
        match shape {
            Shape::Text(text) => texts.push(text.galley.text().to_owned()),
            Shape::Vec(shapes) => {
                for shape in shapes {
                    collect_texts(shape, texts);
                }
            }
            _ => {}
        }
    }

    #[test]
    fn the_legend_lists_each_config_once_in_id_order() {
        let configs = vec![1, 2, 10];
        let config_band = |config_id| ConfigBand {
            config_id,
            band: Band {
                ticks: vec![0.0, 1.0],
                center: vec![1.0, 2.0],
                low: vec![0.5, 1.5],
                high: vec![1.5, 2.5],
                runs: 2,
            },
            center: vec![PlotPoint::new(0.0, 1.0), PlotPoint::new(1.0, 2.0)],
        };
        let plotted = SeriesPlot {
            run_counts: vec![(2, 2); configs.len()],
            computed_at: Instant::now(),
            stat: 0,
            band: BandKind::default(),
            show_runs: false,
            bands: configs.iter().map(|&config_id| Some(config_band(config_id))).collect(),
            configs,
            run_lines: Vec::new(),
        };
        let labels: Vec<String> = plotted
            .configs
            .iter()
            .map(|config_id| format!("Configuration {config_id}"))
            .collect();
        let context = egui::Context::default();
        let input = egui::RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(800.0, 600.0))),
            ..egui::RawInput::default()
        };
        let output = context.run_ui(input, |ui| {
            draw_plot(ui, &plotted, &labels, "Infected");
        });
        let mut texts = Vec::new();
        for clipped in &output.shapes {
            collect_texts(&clipped.shape, &mut texts);
        }
        output.drop_without_applying_deltas();
        let legend: Vec<&str> = texts
            .iter()
            .map(String::as_str)
            .filter(|text| text.starts_with("Configuration"))
            .collect();
        assert_eq!(legend, ["Configuration 1", "Configuration 2", "Configuration 10"]);
    }

    #[test]
    fn a_long_band_keeps_the_spikes_of_its_center_line() {
        let ticks: Vec<f64> = (0..100_000).map(f64::from).collect();
        let mut center: Vec<f64> = ticks.iter().map(|tick| (tick * 0.001).sin()).collect();
        center[54_321] = 30.0;
        center[77_777] = -20.0;
        let band = Band {
            low: center.iter().map(|value| value - 1.0).collect(),
            high: center.iter().map(|value| value + 1.0).collect(),
            ticks,
            center,
            runs: 1,
        };

        let line = center_line(&band);
        assert!(line.len() <= MAX_PLOT_POINTS, "{} points", line.len());
        for (tick, value) in [(54_321.0, 30.0), (77_777.0, -20.0)] {
            assert!(
                line.iter().any(|point| *point == PlotPoint::new(tick, value)),
                "the center line drops the spike at tick {tick}"
            );
        }

        let thinned_band = thin_band(band);
        assert!(thinned_band.ticks.len() <= MAX_PLOT_POINTS);
        let highest = thinned_band.high.iter().copied().fold(f64::NEG_INFINITY, f64::max);
        let lowest = thinned_band.low.iter().copied().fold(f64::INFINITY, f64::min);
        assert_eq!((highest, lowest), (31.0, -21.0), "the fill spans both spikes");
    }
}
