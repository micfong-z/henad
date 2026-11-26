//! Time axis of a run: the x of a plot, the label under it and the tick counter, in ticks or in a unit of time.
//!
//! A model with a time scale names a unit and a hidden parameter holding the length of one tick. The time at a tick
//! is the tick times that length, as the `time` column of a stats export writes it. The app shows a time in the unit
//! that reads most naturally for it ([`natural_unit`]), such as "3.29 months" for 100 ticks of a day, and the export
//! keeps the unit of the tick.

use henad_core::metadata::{TimeScale, TimeUnit};

use crate::ui::text::capitalize;

/// Unit and tick length of one run of a model with a time scale.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RunTime {
    /// Unit that the tick length counts in.
    pub unit: TimeUnit,
    /// Length of one tick of the run, in [`Self::unit`]s.
    pub tick_length: f32,
}

impl RunTime {
    /// Returns the time at `tick` in [`Self::unit`]s, as [`TimeScale::time_at`] computes it.
    pub fn at(self, tick: u64) -> f64 {
        TimeScale::time_at(tick, self.tick_length)
    }
}

/// Time axis of a plot of runs with a time scale: the run's time and the unit the axis reads in.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PlotTime {
    /// Unit and tick length of the run.
    pub run: RunTime,
    /// Unit that the axis reads in.
    pub unit: TimeUnit,
}

impl PlotTime {
    /// Returns the axis of `run` from tick 0 to tick `last`, in the unit that [`axis_unit`] chooses for the time at
    /// `last`.
    pub fn up_to(run: RunTime, last: u64) -> Self {
        Self {
            run,
            unit: axis_unit(run.at(last), run.unit),
        }
    }
}

/// Returns `time`, a length of time in `unit`s, in `target`s.
pub fn convert(time: f64, unit: TimeUnit, target: TimeUnit) -> f64 {
    if unit == target {
        return time;
    }
    time * unit.days() / target.days()
}

/// Returns the unit that `time`, a length of time in `unit`s, reads in.
///
/// That is the longest unit of [`TimeUnit::ALL`] in which the time, rounded to three significant digits, is at least 1.
/// A tenth of a day then reads as "2.4 hours", 6 days as "6 days", 24 days as "3.43 weeks" and 100 days as "3.29
/// months". A time of 0 reads in `unit`, and a time under a second in seconds.
pub fn natural_unit(time: f64, unit: TimeUnit) -> TimeUnit {
    unit_reaching(time, unit, 1.0)
}

/// Returns the unit that an axis from 0 to `time`, a length of time in `unit`s, reads in.
///
/// That is the longest unit in which the time is at least 2, so the axis spans whole units. 10 days then read in days,
/// 60 days in weeks and 400 days in months.
pub fn axis_unit(time: f64, unit: TimeUnit) -> TimeUnit {
    unit_reaching(time, unit, 2.0)
}

/// Returns the longest unit in which `time`, a length of time in `unit`s, rounded to three significant digits, is at
/// least `least`. A time of 0 takes `unit`, and a time too short for any unit seconds.
fn unit_reaching(time: f64, unit: TimeUnit, least: f64) -> TimeUnit {
    if time.is_nan() || time <= 0.0 {
        return unit;
    }
    TimeUnit::ALL
        .into_iter()
        .rev()
        .find(|candidate| rounded(convert(time, unit, *candidate)) >= least)
        .unwrap_or(TimeUnit::Second)
}

/// Returns the number of decimals that keep three significant digits of `value`, and none from 1000 up.
fn decimals(value: f64) -> usize {
    if value == 0.0 || !value.is_finite() {
        return 0;
    }
    // The magnitude of a finite value other than 0 lies within the exponent range of an `f64`.
    let magnitude = value.abs().log10().floor() as i32;
    usize::try_from(2 - magnitude).unwrap_or(0)
}

/// Returns `value` rounded to three significant digits, or to a whole number from 1000 up.
fn rounded(value: f64) -> f64 {
    let scale = 10_f64.powi(i32::try_from(decimals(value)).unwrap_or(0));
    (value * scale).round() / scale
}

/// Returns `time`, a length of time in `unit`s, with the unit it reads in, such as "3.29 months", or "1 day" at exactly
/// one unit.
pub fn time_text(time: f64, unit: TimeUnit) -> String {
    let shown = natural_unit(time, unit);
    let value = convert(time, unit, shown);
    let places = decimals(value);
    let mut number = format!("{value:.places$}");
    if number.contains('.') {
        number = number.trim_end_matches('0').trim_end_matches('.').to_owned();
    }
    let name = if number == "1" { shown.as_str() } else { shown.plural() };
    format!("{number} {name}")
}

/// Returns the x of `tick` on a plot: the tick itself, or its time in the unit of `time`'s axis when `time` is set.
pub fn plot_x(tick: u64, time: Option<PlotTime>) -> f64 {
    time.map_or(tick as f64, |time| convert(time.run.at(tick), time.run.unit, time.unit))
}

/// Returns the label of a plot's x axis: "Tick", or the unit with a capital letter, as in "Day", when `unit` is set.
pub fn axis_label(unit: Option<TimeUnit>) -> String {
    unit.map_or_else(|| "Tick".to_owned(), |unit| capitalize(unit.as_str()))
}

/// Returns the text of the tick counter: "Tick 48", or "Tick 48 (2 days)" when `time` is set.
pub fn counter_text(tick: u64, time: Option<RunTime>) -> String {
    match time {
        Some(time) => format!("Tick {tick} ({})", elapsed_text(tick, time)),
        None => format!("Tick {tick}"),
    }
}

/// Returns the time at `tick` in the unit it reads in, as [`time_text`] writes it.
pub fn elapsed_text(tick: u64, time: RunTime) -> String {
    time_text(time.at(tick), time.unit)
}

#[cfg(test)]
mod tests {
    use henad_core::metadata::TimeUnit;

    use super::{PlotTime, RunTime, axis_label, axis_unit, counter_text, plot_x, time_text};

    /// Returns the run whose ticks are `tick_length` `unit`s long.
    fn run(tick_length: f32, unit: TimeUnit) -> RunTime {
        RunTime { unit, tick_length }
    }

    #[test]
    fn the_counter_names_the_time_only_for_a_model_with_a_time_scale() {
        let half_day = run(0.5, TimeUnit::Day);
        assert_eq!(counter_text(24, None), "Tick 24");
        assert_eq!(counter_text(24, Some(half_day)), "Tick 24 (1.71 weeks)");
        assert_eq!(counter_text(12, Some(half_day)), "Tick 12 (6 days)");
        assert_eq!(counter_text(10, Some(half_day)), "Tick 10 (5 days)");
        assert_eq!(counter_text(2, Some(half_day)), "Tick 2 (1 day)");
        assert_eq!(counter_text(1, Some(half_day)), "Tick 1 (12 hours)");
        assert_eq!(counter_text(0, Some(half_day)), "Tick 0 (0 days)");
        assert_eq!(counter_text(3, Some(run(0.1, TimeUnit::Hour))), "Tick 3 (18 minutes)");
    }

    /// Each time reads in the longest unit it reaches 1 of, rounded to three significant digits.
    #[test]
    fn a_time_reads_in_the_unit_that_fits_it() {
        let table: [(f32, TimeUnit, u64, &str); 36] = [
            (1.0, TimeUnit::Day, 0, "0 days"),
            (1.0, TimeUnit::Day, 1, "1 day"),
            (1.0, TimeUnit::Day, 6, "6 days"),
            (1.0, TimeUnit::Day, 7, "1 week"),
            (1.0, TimeUnit::Day, 10, "1.43 weeks"),
            (1.0, TimeUnit::Day, 24, "3.43 weeks"),
            (1.0, TimeUnit::Day, 30, "4.29 weeks"),
            (1.0, TimeUnit::Day, 31, "1.02 months"),
            (1.0, TimeUnit::Day, 60, "1.97 months"),
            (1.0, TimeUnit::Day, 100, "3.29 months"),
            (1.0, TimeUnit::Day, 365, "12 months"),
            (1.0, TimeUnit::Day, 366, "1 year"),
            (1.0, TimeUnit::Day, 400, "1.1 years"),
            (1.0, TimeUnit::Day, 730, "2 years"),
            (1.0, TimeUnit::Day, 36_525, "100 years"),
            (1.0, TimeUnit::Hour, 23, "23 hours"),
            (1.0, TimeUnit::Hour, 24, "1 day"),
            (1.0, TimeUnit::Hour, 36, "1.5 days"),
            (1.0, TimeUnit::Hour, 60, "2.5 days"),
            (1.0, TimeUnit::Hour, 168, "1 week"),
            (6.0, TimeUnit::Hour, 2, "12 hours"),
            (1.0, TimeUnit::Minute, 90, "1.5 hours"),
            (1.0, TimeUnit::Second, 45, "45 seconds"),
            (1.0, TimeUnit::Second, 90, "1.5 minutes"),
            (1.0, TimeUnit::Second, 3600, "1 hour"),
            (0.001, TimeUnit::Second, 1, "0.001 seconds"),
            (1.0, TimeUnit::Week, 1, "1 week"),
            (1.0, TimeUnit::Week, 4, "4 weeks"),
            (1.0, TimeUnit::Week, 5, "1.15 months"),
            (0.5, TimeUnit::Week, 1, "3.5 days"),
            (1.0, TimeUnit::Month, 6, "6 months"),
            (1.0, TimeUnit::Month, 12, "1 year"),
            (1.0, TimeUnit::Month, 18, "1.5 years"),
            (1.0, TimeUnit::Year, 1, "1 year"),
            (1.0, TimeUnit::Year, 250, "250 years"),
            (1.0, TimeUnit::Year, 12_345, "12345 years"),
        ];
        for (tick_length, unit, tick, expected) in table {
            let time = run(tick_length, unit).at(tick);
            assert_eq!(
                time_text(time, unit),
                expected,
                "{tick} ticks of {tick_length} {unit:?}"
            );
        }
    }

    #[test]
    fn the_axis_reads_in_ticks_or_in_the_unit_of_its_last_time() {
        assert_eq!(axis_label(None), "Tick");
        assert_eq!(axis_label(Some(TimeUnit::Day)), "Day");
        assert_eq!(axis_label(Some(TimeUnit::Month)), "Month");
        let quarter_day = run(0.25, TimeUnit::Day);
        assert_eq!(plot_x(10, None), 10.0);
        let days = PlotTime::up_to(quarter_day, 40);
        assert_eq!(days.unit, TimeUnit::Day, "10 days");
        assert_eq!(plot_x(10, Some(days)), 2.5);
        let hours = PlotTime::up_to(quarter_day, 6);
        assert_eq!(hours.unit, TimeUnit::Hour, "a day and a half");
        assert!((plot_x(1, Some(hours)) - 6.0).abs() < 1e-12);
        let months = PlotTime::up_to(run(1.0, TimeUnit::Day), 100);
        assert_eq!(months.unit, TimeUnit::Month);
        assert!((plot_x(100, Some(months)) - 100.0 / 30.44).abs() < 1e-12);
        assert_eq!(PlotTime::up_to(quarter_day, 0).unit, TimeUnit::Day, "an axis at tick 0");
        // An axis spans at least 2 of its unit, where a single time reads in the longest unit it reaches 1 of.
        for (days, axis, text) in [
            (1.5, TimeUnit::Hour, "1.5 days"),
            (10.0, TimeUnit::Day, "1.43 weeks"),
            (60.0, TimeUnit::Week, "1.97 months"),
            (400.0, TimeUnit::Month, "1.1 years"),
            (800.0, TimeUnit::Year, "2.19 years"),
        ] {
            assert_eq!(axis_unit(days, TimeUnit::Day), axis, "{days} days");
            assert_eq!(time_text(days, TimeUnit::Day), text);
        }
    }
}
