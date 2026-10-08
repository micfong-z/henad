//! Drawing helpers the result views share.
//!
//! They cover colours, decimation, level labels, a colour bar, the tiles of a heatmap, the labeled combo boxes of the
//! views' controls, and the pace at which a view computes again while runs arrive.

use std::ops::RangeInclusive;
use std::time::Duration;

use egui::{Color32, Mesh, Rect, Shape, TextStyle};
use egui_plot::{GridMark, PlotBounds, PlotGeometry, PlotItem, PlotItemBase, PlotPoint, PlotTransform};
use web_time::Instant;

/// Colours of the configurations a view draws, in order. A config's band and mean line share one colour.
pub const CONFIG_COLORS: [Color32; 10] = [
    Color32::from_rgb(0x4e, 0x79, 0xa7),
    Color32::from_rgb(0xf2, 0x8e, 0x2b),
    Color32::from_rgb(0xe1, 0x57, 0x59),
    Color32::from_rgb(0x76, 0xb7, 0xb2),
    Color32::from_rgb(0x59, 0xa1, 0x4f),
    Color32::from_rgb(0xed, 0xc9, 0x48),
    Color32::from_rgb(0xb0, 0x7a, 0xa1),
    Color32::from_rgb(0xff, 0x9d, 0xa7),
    Color32::from_rgb(0x9c, 0x75, 0x5f),
    Color32::from_rgb(0xba, 0xb0, 0xac),
];

/// Stops of the heatmap scale, low to high, sampled from viridis.
const HEAT_STOPS: [Color32; 5] = [
    Color32::from_rgb(0x44, 0x01, 0x54),
    Color32::from_rgb(0x3b, 0x52, 0x8b),
    Color32::from_rgb(0x21, 0x91, 0x8c),
    Color32::from_rgb(0x5e, 0xc9, 0x62),
    Color32::from_rgb(0xfd, 0xe7, 0x25),
];

/// Colour of a heatmap cell that holds no value.
pub const NO_DATA_COLOR: Color32 = Color32::from_gray(110);

/// Luminance, out of 255, of a tile fill below which the tile's text is white.
const WHITE_TEXT_MAX_LUMINANCE: f32 = 140.0;

/// Maximum number of points a line keeps once decimated.
pub const MAX_PLOT_POINTS: usize = 1000;

/// Shortest time between two computations of a view while runs keep arriving.
pub const REFRESH_INTERVAL: Duration = Duration::from_millis(500);

/// Returns whether a view computed at `computed_at`, from a store that has changed since, is due to compute again.
///
/// Before [`REFRESH_INTERVAL`] has passed, `ctx` repaints once the interval ends.
pub fn refresh_due(ctx: &egui::Context, computed_at: Instant) -> bool {
    match REFRESH_INTERVAL.checked_sub(computed_at.elapsed()) {
        Some(wait) if !wait.is_zero() => {
            ctx.request_repaint_after(wait);
            false
        }
        _ => true,
    }
}

/// Returns the colour of config number `position` in a view.
pub fn config_color(position: usize) -> Color32 {
    CONFIG_COLORS[position % CONFIG_COLORS.len()]
}

/// Returns the colour of `value` on the heatmap scale from `min` to `max`, or [`NO_DATA_COLOR`] for a value that is
/// not finite.
///
/// A scale whose ends meet maps every finite value to its middle.
pub fn heat_color(value: f64, min: f64, max: f64) -> Color32 {
    if !value.is_finite() {
        return NO_DATA_COLOR;
    }
    let fraction = if max > min {
        ((value - min) / (max - min)).clamp(0.0, 1.0)
    } else {
        0.5
    };
    let scaled = fraction * (HEAT_STOPS.len() - 1) as f64;
    let lower = (scaled.floor() as usize).min(HEAT_STOPS.len() - 2);
    let blend = (scaled - lower as f64) as f32;
    blend_color(HEAT_STOPS[lower], HEAT_STOPS[lower + 1], blend)
}

/// Returns the colour at fraction `blend` of the way from `low` to `high`.
fn blend_color(low: Color32, high: Color32, blend: f32) -> Color32 {
    let channel = |low_channel: u8, high_channel: u8| {
        let (low_channel, high_channel) = (f32::from(low_channel), f32::from(high_channel));
        (low_channel + (high_channel - low_channel) * blend).round() as u8
    };
    Color32::from_rgb(
        channel(low.r(), high.r()),
        channel(low.g(), high.g()),
        channel(low.b(), high.b()),
    )
}

/// Returns `points` thinned to at most `max_points`, keeping the first and last point and the lowest and highest
/// point of every bucket in between.
///
/// Points keep their order, so a spike survives however far a long series is thinned. Note that a `max_points` below
/// 4 leaves `points` as they are.
pub fn decimate(points: &[[f64; 2]], max_points: usize) -> Vec<[f64; 2]> {
    let [first, interior @ .., last] = points else {
        return points.to_vec();
    };
    if points.len() <= max_points || max_points < 4 {
        return points.to_vec();
    }
    let buckets = (max_points - 2) / 2;
    let bucket_size = interior.len().div_ceil(buckets);
    let mut kept = Vec::with_capacity(max_points);
    kept.push(*first);
    for bucket in interior.chunks(bucket_size) {
        let lowest = extreme_position(bucket, |a, b| a < b);
        let highest = extreme_position(bucket, |a, b| a > b);
        let (earlier, later) = (lowest.min(highest), lowest.max(highest));
        kept.push(bucket[earlier]);
        if later != earlier {
            kept.push(bucket[later]);
        }
    }
    kept.push(*last);
    kept
}

/// Returns the position in `bucket` of the point whose value `beats` every other value, the first on a tie. A value
/// that is not finite never wins over a finite value.
fn extreme_position(bucket: &[[f64; 2]], beats: impl Fn(f64, f64) -> bool) -> usize {
    let mut best = 0;
    for (position, point) in bucket.iter().enumerate().skip(1) {
        let current = bucket[best][1];
        if point[1].is_finite() && (!current.is_finite() || beats(point[1], current)) {
            best = position;
        }
    }
    best
}

/// Returns an axis formatter that labels the mark at index `i` with `levels[i]`, and leaves every other mark blank.
pub fn level_formatter(levels: &[String]) -> impl Fn(GridMark, &RangeInclusive<f64>) -> String + '_ {
    move |mark, _| {
        let index = mark.value.round();
        if (mark.value - index).abs() > 1e-6 || index < 0.0 {
            return String::new();
        }
        levels.get(index as usize).cloned().unwrap_or_default()
    }
}

/// Draws a horizontal bar of the heatmap scale from `min` to `max`, labelled at both ends.
pub fn color_bar(ui: &mut egui::Ui, min: f64, max: f64) {
    const STEPS: usize = 32;
    ui.horizontal(|ui| {
        ui.label(format_significant(min));
        let (rect, _) = ui.allocate_exact_size(egui::vec2(160.0, 12.0), egui::Sense::hover());
        let painter = ui.painter_at(rect);
        let step_width = rect.width() / STEPS as f32;
        for step in 0..STEPS {
            let fraction = (step as f64 + 0.5) / STEPS as f64;
            let left = rect.left() + step as f32 * step_width;
            let cell = egui::Rect::from_min_size(
                egui::pos2(left, rect.top()),
                egui::vec2(step_width + 0.5, rect.height()),
            );
            painter.rect_filled(cell, 0.0, heat_color(fraction, 0.0, 1.0));
        }
        ui.label(format_significant(max));
    });
}

/// Tiles of a heatmap as one plot item, filled from values it borrows.
///
/// Value `i` fills the tile in column `i % columns` and row `i / columns`, with row 0 lowest.
pub struct HeatmapTiles<'a, F> {
    base: PlotItemBase,
    /// Value of each tile, row by row from the lowest row.
    values: &'a [f64],
    /// Number of tiles in a row.
    columns: usize,
    /// Lower left corner of the first tile.
    origin: PlotPoint,
    /// Width and height of a tile in plot coordinates.
    tile_size: [f64; 2],
    /// Mapping from a tile's value to its fill.
    fill: F,
    /// Formatter of the value each tile writes, `None` for tiles without text.
    label: Option<fn(f64) -> String>,
}

impl<'a, F: Fn(f64) -> Color32> HeatmapTiles<'a, F> {
    /// Returns the tiles of `values`, `columns` to a row, each filled by `fill`.
    ///
    /// The first tile's lower left corner sits at `origin`, and each tile is `tile_size` wide and high in plot
    /// coordinates.
    pub fn new(values: &'a [f64], columns: usize, origin: PlotPoint, tile_size: [f64; 2], fill: F) -> Self {
        Self {
            base: PlotItemBase::new(String::new()),
            values,
            columns,
            origin,
            tile_size,
            fill,
            label: None,
        }
    }

    /// Writes each tile's value in the tile, formatted by `format`.
    pub fn labels(mut self, format: fn(f64) -> String) -> Self {
        self.label = Some(format);
        self
    }

    /// Number of rows, including a short last row.
    fn rows(&self) -> usize {
        if self.columns == 0 {
            0
        } else {
            self.values.len().div_ceil(self.columns)
        }
    }

    /// Returns the rectangle on screen of tile `index` under `transform`.
    fn tile_rect(&self, transform: &PlotTransform, index: usize) -> Rect {
        let (column, row) = ((index % self.columns) as f64, (index / self.columns) as f64);
        let [width, height] = self.tile_size;
        let corner =
            |column: f64, row: f64| PlotPoint::new(self.origin.x + width * column, self.origin.y + height * row);
        transform.rect_from_values(&corner(column, row), &corner(column + 1.0, row + 1.0))
    }
}

impl<F: Fn(f64) -> Color32> PlotItem for HeatmapTiles<'_, F> {
    fn shapes(&self, ui: &egui::Ui, transform: &PlotTransform, shapes: &mut Vec<Shape>) {
        if self.columns == 0 {
            return;
        }
        let mut mesh = Mesh::default();
        mesh.reserve_vertices(4 * self.values.len());
        mesh.reserve_triangles(2 * self.values.len());
        let font = TextStyle::Monospace.resolve(ui.style());
        let mut labels = Vec::new();
        for (index, &value) in self.values.iter().enumerate() {
            let rect = self.tile_rect(transform, index);
            let fill = (self.fill)(value);
            mesh.add_colored_rect(rect, fill);
            if let Some(format) = self.label {
                let color = tile_text_color(fill);
                let galley = ui.painter().layout_no_wrap(format(value), font.clone(), color);
                labels.push(Shape::galley(rect.center() - galley.size() / 2.0, galley, color));
            }
        }
        shapes.push(Shape::mesh(mesh));
        shapes.extend(labels);
    }

    fn initialize(&mut self, _x_range: RangeInclusive<f64>) {}

    fn color(&self) -> Color32 {
        Color32::TRANSPARENT
    }

    // Each view writes its own hover text for the tile under the pointer.
    fn allow_hover(&self) -> bool {
        false
    }

    fn geometry(&self) -> PlotGeometry<'_> {
        PlotGeometry::None
    }

    fn bounds(&self) -> PlotBounds {
        let [width, height] = self.tile_size;
        PlotBounds::from_min_max(
            [self.origin.x, self.origin.y],
            [
                self.origin.x + width * self.columns as f64,
                self.origin.y + height * self.rows() as f64,
            ],
        )
    }

    fn base(&self) -> &PlotItemBase {
        &self.base
    }

    fn base_mut(&mut self) -> &mut PlotItemBase {
        &mut self.base
    }
}

/// Returns the colour of the text on a tile filled with `fill`, white on a dark fill and black on a light one.
fn tile_text_color(fill: Color32) -> Color32 {
    let luminance = 0.2126 * f32::from(fill.r()) + 0.7152 * f32::from(fill.g()) + 0.0722 * f32::from(fill.b());
    if luminance < WHITE_TEXT_MAX_LUMINANCE {
        Color32::WHITE
    } else {
        Color32::BLACK
    }
}

/// Draws `label` and a combo box on one row of a wrapping layout, and returns the combo box's response, labeled by
/// `label`.
///
/// The row wraps before the label when the label and the combo box do not fit the rest of the row. A combo box's
/// width is known only once it is placed, so the layout cannot wrap it on its own.
pub fn labeled_combo(
    ui: &mut egui::Ui,
    label: &str,
    id_salt: impl egui::AsIdSalt,
    selected_text: &str,
    add_contents: impl FnOnce(&mut egui::Ui),
) -> egui::Response {
    let spacing = ui.spacing();
    let button_extra = spacing.icon_spacing + spacing.icon_width + 2.0 * spacing.button_padding.x;
    let (combo_width, item_spacing) = (spacing.combo_width, spacing.item_spacing.x);
    let selected_width = (text_width(ui, selected_text, egui::TextStyle::Button) + button_extra).max(combo_width);
    let pair_width = text_width(ui, label, egui::TextStyle::Body) + item_spacing + selected_width;
    let row_started = ui.cursor().left() > ui.max_rect().left() + 0.5;
    if row_started && ui.available_size_before_wrap().x < pair_width {
        ui.end_row();
    }
    let label = ui.add(egui::Label::new(label).wrap_mode(egui::TextWrapMode::Extend));
    egui::ComboBox::from_id_salt(id_salt)
        .selected_text(selected_text)
        .truncate()
        .show_ui(ui, add_contents)
        .response
        .labelled_by(label.id)
}

/// Returns the width of `text` in `style` on one line.
pub fn text_width(ui: &egui::Ui, text: &str, style: egui::TextStyle) -> f32 {
    let galley = egui::WidgetText::from(text).into_galley(ui, Some(egui::TextWrapMode::Extend), f32::INFINITY, style);
    galley.size().x
}

/// Returns `value` with four significant digits, or a dash for a value that is not finite.
///
/// A value from 1000 up to a trillion is rounded to a whole number with its digits grouped in threes. Only a value
/// past that range, or below a thousandth, takes an exponent.
pub fn format_significant(value: f64) -> String {
    if !value.is_finite() {
        return "–".to_owned();
    }
    if value == 0.0 {
        return "0".to_owned();
    }
    let magnitude = value.abs().log10().floor();
    if !(-3.0..12.0).contains(&magnitude) {
        return format!("{value:.3e}");
    }
    if magnitude >= 3.0 {
        return group_digits(value.round());
    }
    let decimals = (3.0 - magnitude) as usize;
    let text = format!("{value:.decimals$}");
    if text.contains('.') {
        text.trim_end_matches('0').trim_end_matches('.').to_owned()
    } else {
        text
    }
}

/// Returns `value` with six significant digits and no trailing zeros, or a dash for a value that is not finite.
///
/// A value from a million up keeps every digit of its whole part. The whole part of a value from 1000 up has its
/// digits grouped in threes, as in `3,108.5`. Only a value from a trillion up, or below a thousandth, takes an
/// exponent.
pub fn format_precise(value: f64) -> String {
    if !value.is_finite() {
        return "–".to_owned();
    }
    if value == 0.0 {
        return "0".to_owned();
    }
    let magnitude = value.abs().log10().floor();
    if !(-3.0..12.0).contains(&magnitude) {
        return format!("{value:.5e}");
    }
    let decimals = (5.0 - magnitude).max(0.0) as usize;
    let text = format!("{:.decimals$}", value.abs());
    let text = if text.contains('.') {
        text.trim_end_matches('0').trim_end_matches('.')
    } else {
        text.as_str()
    };
    let (whole, fraction) = text
        .split_once('.')
        .map_or((text, None), |(whole, fraction)| (whole, Some(fraction)));
    let mut formatted = String::new();
    if value < 0.0 {
        formatted.push('-');
    }
    formatted.push_str(&group_whole(whole));
    if let Some(fraction) = fraction {
        formatted.push('.');
        formatted.push_str(fraction);
    }
    formatted
}

/// Returns the whole number `value` with its digits grouped in threes by commas.
fn group_digits(value: f64) -> String {
    let mut grouped = String::new();
    if value < 0.0 {
        grouped.push('-');
    }
    grouped.push_str(&group_whole(&format!("{:.0}", value.abs())));
    grouped
}

/// Returns the digits `digits` grouped in threes by commas.
fn group_whole(digits: &str) -> String {
    let mut grouped = String::with_capacity(digits.len() + digits.len() / 3);
    for (position, digit) in digits.chars().enumerate() {
        if position > 0 && (digits.len() - position).is_multiple_of(3) {
            grouped.push(',');
        }
        grouped.push(digit);
    }
    grouped
}

#[cfg(test)]
mod tests {
    use egui::{Color32, Rect, Shape, pos2};
    use egui_plot::{PlotItem as _, PlotPoint, PlotTransform};

    use super::{
        HeatmapTiles, NO_DATA_COLOR, decimate, format_precise, format_significant, heat_color, tile_text_color,
    };

    fn series(values: &[f64]) -> Vec<[f64; 2]> {
        values.iter().enumerate().map(|(x, &y)| [x as f64, y]).collect()
    }

    #[test]
    fn decimation_keeps_the_extremes_and_the_ends() {
        let mut values: Vec<f64> = (0..10_000).map(|x| (f64::from(x) * 0.01).sin()).collect();
        values[1234] = 50.0;
        values[7777] = -40.0;
        let points = series(&values);
        let kept = decimate(&points, 100);

        assert!(kept.len() <= 100, "{} points kept", kept.len());
        assert_eq!(kept.first(), points.first());
        assert_eq!(kept.last(), points.last());
        assert!(kept.contains(&[1234.0, 50.0]), "the spike is dropped");
        assert!(kept.contains(&[7777.0, -40.0]), "the dip is dropped");
        assert!(
            kept.windows(2).all(|pair| pair[0][0] < pair[1][0]),
            "points keep their order"
        );
    }

    #[test]
    fn a_short_series_is_left_whole() {
        let points = series(&[1.0, 3.0, 2.0]);
        assert_eq!(decimate(&points, 100), points);
    }

    #[test]
    fn a_value_that_is_not_finite_never_hides_an_extreme() {
        let mut values = vec![0.0; 1000];
        values[10] = f64::NAN;
        values[11] = 9.0;
        let kept = decimate(&series(&values), 10);
        assert!(kept.contains(&[11.0, 9.0]));
    }

    #[test]
    fn the_heat_scale_marks_missing_values() {
        assert_eq!(heat_color(f64::NAN, 0.0, 1.0), NO_DATA_COLOR);
        assert_ne!(heat_color(0.0, 0.0, 1.0), heat_color(1.0, 0.0, 1.0));
        assert_eq!(
            heat_color(-5.0, 0.0, 1.0),
            heat_color(0.0, 0.0, 1.0),
            "a low value clamps to the scale"
        );
        assert_eq!(heat_color(3.0, 3.0, 3.0), heat_color(0.5, 0.0, 1.0));
    }

    #[test]
    fn heatmap_tiles_fill_every_tile_and_write_values_only_when_labeled() {
        let values = [1.0, 2.0, f64::NAN, 4.0, 5.0, 6.0];
        let fill = |value: f64| heat_color(value, 1.0, 6.0);
        let tiles = HeatmapTiles::new(&values, 3, PlotPoint::new(-0.5, -0.5), [1.0, 2.0], fill);
        let bounds = tiles.bounds();
        assert_eq!((bounds.min(), bounds.max()), ([-0.5, -0.5], [2.5, 3.5]));
        assert!(!tiles.allow_hover());
        let labeled =
            HeatmapTiles::new(&values, 3, PlotPoint::new(-0.5, -0.5), [1.0, 2.0], fill).labels(format_significant);

        let transform = PlotTransform::new(Rect::from_min_max(pos2(0.0, 0.0), pos2(300.0, 400.0)), bounds, false);
        let context = egui::Context::default();
        let output = context.run_ui(egui::RawInput::default(), |ui| {
            let mut shapes = Vec::new();
            tiles.shapes(ui, &transform, &mut shapes);
            let [Shape::Mesh(mesh)] = shapes.as_slice() else {
                panic!("{shapes:?} is not one mesh");
            };
            assert_eq!(mesh.vertices.len(), 4 * values.len());
            for (corners, &value) in mesh.vertices.chunks(4).zip(&values) {
                assert!(
                    corners.iter().all(|vertex| vertex.color == fill(value)),
                    "tile of {value}"
                );
            }
            let last: Vec<_> = mesh.vertices[20..].iter().map(|vertex| vertex.pos).collect();
            assert_eq!(
                Rect::from_points(&last),
                transform.rect_from_values(&PlotPoint::new(1.5, 1.5), &PlotPoint::new(2.5, 3.5)),
                "the last tile sits in the top row, rightmost"
            );

            let mut shapes = Vec::new();
            labeled.shapes(ui, &transform, &mut shapes);
            assert!(matches!(shapes[0], Shape::Mesh(_)));
            let texts: Vec<&str> = shapes[1..]
                .iter()
                .map(|shape| match shape {
                    Shape::Text(text) => text.galley.text(),
                    other => panic!("{other:?} is not a text"),
                })
                .collect();
            assert_eq!(texts, ["1", "2", "–", "4", "5", "6"]);
        });
        output.drop_without_applying_deltas();
    }

    #[test]
    fn tile_text_contrasts_with_its_fill() {
        assert_eq!(tile_text_color(heat_color(0.0, 0.0, 1.0)), Color32::WHITE);
        assert_eq!(tile_text_color(heat_color(1.0, 0.0, 1.0)), Color32::BLACK);
    }

    #[test]
    fn values_are_written_with_four_significant_digits() {
        assert_eq!(format_significant(0.123_456), "0.1235");
        assert_eq!(format_significant(123.456), "123.5");
        assert_eq!(format_significant(2.0), "2");
        assert_eq!(format_significant(f64::NAN), "–");
        assert_eq!(format_significant(0.000_123_4), "1.234e-4");
    }

    #[test]
    fn large_values_are_whole_numbers_with_grouped_digits() {
        assert_eq!(format_significant(1234.56), "1,235");
        assert_eq!(format_significant(1_038_345.0), "1,038,345");
        assert_eq!(format_significant(-15_000_000.4), "-15,000,000");
        assert_eq!(format_significant(999_999.6), "1,000,000");
        assert_eq!(format_significant(2.5e13), "2.500e13");
    }

    #[test]
    fn a_precise_value_keeps_six_significant_digits() {
        assert_eq!(format_precise(3108.5), "3,108.5");
        assert_eq!(format_precise(3109.0), "3,109");
        assert_eq!(format_precise(102.333_333), "102.333");
        assert_eq!(format_precise(-0.123_456_78), "-0.123457");
        assert_eq!(format_precise(1_234_567.8), "1,234,568");
        assert_eq!(format_precise(0.0), "0");
        assert_eq!(format_precise(f64::NEG_INFINITY), "–");
        assert_eq!(format_precise(2.5e13), "2.50000e13");
    }
}
