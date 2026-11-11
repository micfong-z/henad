//! Time axis of a run: the x of a plot, the label under it and the tick counter, in ticks or in the unit of the
//! model's time scale.
//!
//! A model with a time scale names a unit and a hidden parameter holding the length of one tick. The time at a tick
//! is the tick times that length, as the `time` column of a stats export writes it.

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
    /// Returns the time at `tick`, as [`TimeScale::time_at`] computes it.
    pub fn at(self, tick: u64) -> f64 {
        TimeScale::time_at(tick, self.tick_length)
    }
}

/// Returns the x of `tick` on a plot: the tick itself, or its time in the run's unit when `time` is set.
pub fn plot_x(tick: u64, time: Option<RunTime>) -> f64 {
    time.map_or(tick as f64, |time| time.at(tick))
}

/// Returns the label of a plot's x axis: "Tick", or the unit with a capital letter, as in "Day", when `unit` is set.
pub fn axis_label(unit: Option<TimeUnit>) -> String {
    unit.map_or_else(|| "Tick".to_owned(), |unit| capitalize(unit.as_str()))
}

/// Returns the text of the tick counter: "Tick 24", or "Tick 24 (12 days)" when `time` is set.
pub fn counter_text(tick: u64, time: Option<RunTime>) -> String {
    match time {
        Some(time) => format!("Tick {tick} ({})", elapsed_text(tick, time)),
        None => format!("Tick {tick}"),
    }
}

/// Returns the time at `tick` with its unit, such as "12 days", or "1 day" at exactly one unit.
pub fn elapsed_text(tick: u64, time: RunTime) -> String {
    let elapsed = time.at(tick);
    let unit = if elapsed == 1.0 {
        time.unit.as_str()
    } else {
        time.unit.plural()
    };
    format!("{elapsed} {unit}")
}

#[cfg(test)]
mod tests {
    use henad_core::metadata::TimeUnit;

    use super::{RunTime, axis_label, counter_text, plot_x};

    #[test]
    fn the_counter_names_the_time_only_for_a_model_with_a_time_scale() {
        let half_day = RunTime {
            unit: TimeUnit::Day,
            tick_length: 0.5,
        };
        assert_eq!(counter_text(24, None), "Tick 24");
        assert_eq!(counter_text(24, Some(half_day)), "Tick 24 (12 days)");
        assert_eq!(counter_text(10, Some(half_day)), "Tick 10 (5 days)");
        assert_eq!(counter_text(2, Some(half_day)), "Tick 2 (1 day)");
        assert_eq!(counter_text(1, Some(half_day)), "Tick 1 (0.5 days)");
        assert_eq!(counter_text(0, Some(half_day)), "Tick 0 (0 days)");
        let tenth_hour = RunTime {
            unit: TimeUnit::Hour,
            tick_length: 0.1,
        };
        assert_eq!(counter_text(3, Some(tenth_hour)), "Tick 3 (0.3 hours)");
    }

    #[test]
    fn the_axis_reads_in_ticks_or_in_the_unit() {
        assert_eq!(axis_label(None), "Tick");
        assert_eq!(axis_label(Some(TimeUnit::Day)), "Day");
        assert_eq!(axis_label(Some(TimeUnit::Week)), "Week");
        let quarter_day = RunTime {
            unit: TimeUnit::Day,
            tick_length: 0.25,
        };
        assert_eq!(plot_x(10, None), 10.0);
        assert_eq!(plot_x(10, Some(quarter_day)), 2.5);
    }
}
