//! Runs a plan asks for, and the record of each finished run.

use std::fmt;
use std::str::FromStr;

use crate::explore::measure::SeriesBuffer;

/// One run of a plan.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlannedRun {
    /// Position of the run in the plan, `config_id * replicates + rep`.
    pub run_id: u64,
    pub config_id: u64,
    /// Replicate index within the config.
    pub rep: u64,
    /// Seed the model is built with.
    pub seed: u64,
}

/// End state of a run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunStatus {
    /// The run ended without a fault, and every sampled value was finite.
    Ok,
    /// The run ended without a fault, and some sampled value was not finite.
    NonFinite,
    /// The model panicked while building or stepping.
    Panicked,
    /// The device reported an error.
    GpuError,
    /// The host refused to build the model.
    Refused,
    /// A sample no longer fit the stat layout the plan fixed.
    ShapeError,
    /// The run passed its wall-clock timeout.
    TimedOut,
}

impl RunStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Ok => "ok",
            Self::NonFinite => "non_finite",
            Self::Panicked => "panicked",
            Self::GpuError => "gpu_error",
            Self::Refused => "refused",
            Self::ShapeError => "shape_error",
            Self::TimedOut => "timed_out",
        }
    }

    /// Returns whether the run ended on a fault or a timeout, so its values cover part of the run at most.
    pub fn is_failure(self) -> bool {
        match self {
            Self::Ok | Self::NonFinite => false,
            Self::Panicked | Self::GpuError | Self::Refused | Self::ShapeError | Self::TimedOut => true,
        }
    }
}

impl FromStr for RunStatus {
    type Err = RunStatusError;

    /// Reads a status as [`RunStatus::as_str`] writes it.
    fn from_str(raw: &str) -> Result<Self, Self::Err> {
        match raw {
            "ok" => Ok(Self::Ok),
            "non_finite" => Ok(Self::NonFinite),
            "panicked" => Ok(Self::Panicked),
            "gpu_error" => Ok(Self::GpuError),
            "refused" => Ok(Self::Refused),
            "shape_error" => Ok(Self::ShapeError),
            "timed_out" => Ok(Self::TimedOut),
            _ => Err(RunStatusError { raw: raw.to_owned() }),
        }
    }
}

/// Text that names no [`RunStatus`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunStatusError {
    pub raw: String,
}

impl fmt::Display for RunStatusError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "unknown run status '{}'", self.raw)
    }
}

impl std::error::Error for RunStatusError {}

/// Cause of the end of a run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StopReason {
    /// The run stepped to its total.
    Steps,
    /// The stop condition held at a sample.
    Condition,
    /// A fault ended the run early.
    Fault,
    /// The run passed its wall-clock timeout.
    Timeout,
}

impl StopReason {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Steps => "steps",
            Self::Condition => "condition",
            Self::Fault => "fault",
            Self::Timeout => "timeout",
        }
    }
}

impl FromStr for StopReason {
    type Err = StopReasonError;

    /// Reads a reason as [`StopReason::as_str`] writes it.
    fn from_str(raw: &str) -> Result<Self, Self::Err> {
        match raw {
            "steps" => Ok(Self::Steps),
            "condition" => Ok(Self::Condition),
            "fault" => Ok(Self::Fault),
            "timeout" => Ok(Self::Timeout),
            _ => Err(StopReasonError { raw: raw.to_owned() }),
        }
    }
}

/// Text that names no [`StopReason`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StopReasonError {
    pub raw: String,
}

impl fmt::Display for StopReasonError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "unknown stop reason '{}'", self.raw)
    }
}

impl std::error::Error for StopReasonError {}

/// Record of one finished run.
#[derive(Debug, Clone, PartialEq)]
pub struct RunOutcome {
    pub run: PlannedRun,
    /// Hash naming the run's results, from [`crate::explore::fingerprint::run_key`].
    pub run_key: u64,
    pub status: RunStatus,
    pub stop_reason: StopReason,
    /// Tick the run ended on.
    ///
    /// A GPU run that faulted or timed out gives the tick of its latest landed sample, 0 before its first.
    pub ticks: u64,
    /// Population at the run's last sample.
    pub population: u64,
    /// Time in milliseconds spent building the model.
    pub build_ms: f64,
    /// Time in milliseconds spent stepping and sampling.
    pub wall_ms: f64,
    /// One value per reducer, `None` for a reducer that saw no finite value.
    pub reducers: Vec<Option<f64>>,
    pub series: SeriesBuffer,
    /// Actions the model refused, then the fault message, the timeout or the first value that was not finite.
    pub note: Option<String>,
}

impl RunOutcome {
    /// Steps per second of wall time. Note that a run with no wall time gives a value that is not finite.
    pub fn steps_per_s(&self) -> f64 {
        self.ticks as f64 * 1000.0 / self.wall_ms
    }
}

#[cfg(test)]
mod tests {
    use super::{RunStatus, StopReason};

    #[test]
    fn statuses_are_written_in_snake_case() {
        let statuses = [
            RunStatus::Ok,
            RunStatus::NonFinite,
            RunStatus::Panicked,
            RunStatus::GpuError,
            RunStatus::Refused,
            RunStatus::ShapeError,
            RunStatus::TimedOut,
        ];
        let names: Vec<&str> = statuses.iter().map(|status| status.as_str()).collect();
        assert_eq!(
            names,
            [
                "ok",
                "non_finite",
                "panicked",
                "gpu_error",
                "refused",
                "shape_error",
                "timed_out"
            ]
        );
        let failures: Vec<bool> = statuses.iter().map(|status| status.is_failure()).collect();
        assert_eq!(failures, [false, false, true, true, true, true, true]);
        for status in statuses {
            assert_eq!(status.as_str().parse(), Ok(status));
        }
        assert!("timed out".parse::<RunStatus>().is_err());
        let reasons = [
            StopReason::Steps,
            StopReason::Condition,
            StopReason::Fault,
            StopReason::Timeout,
        ];
        let names: Vec<&str> = reasons.iter().map(|reason| reason.as_str()).collect();
        assert_eq!(names, ["steps", "condition", "fault", "timeout"]);
        for reason in reasons {
            assert_eq!(reason.as_str().parse(), Ok(reason));
        }
        assert!("stopped".parse::<StopReason>().is_err());
    }
}
