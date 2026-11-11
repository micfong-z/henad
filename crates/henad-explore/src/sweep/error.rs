//! Error of a sweep or a search that cannot run to its end.

use std::fmt;

use henad_core::explore::measure::MeasureError;
use henad_core::explore::plan::PlanError;

use crate::exec::ExecutionError;
use crate::output::OutputError;
use crate::output::resume::ResumeError;
use crate::probe::{CapacityError, ProbeError};
use crate::search_run::SearchPlanError;

/// A sweep that cannot run to its end.
///
/// The variants differ between targets, and a match outside this crate ends in a wildcard arm.
#[derive(Debug)]
#[non_exhaustive]
pub enum ExploreError {
    /// A spec that the model rejects.
    Plan(PlanError),
    /// Configs the device cannot host.
    Capacity(CapacityError),
    /// A probe build that failed.
    Probe(ProbeError),
    /// Reducers that do not bind to the columns of the probe build.
    Measure(MeasureError),
    /// A directory the sweep cannot resume into.
    Resume(ResumeError),
    /// Results that cannot be written.
    Output(OutputError),
    /// A batch of runs that cannot run.
    Execution(ExecutionError),
    /// A search spec that the model or the options reject.
    Search(SearchPlanError),
    /// A GPU model with no device, on a machine where no device can be acquired.
    #[cfg(not(target_arch = "wasm32"))]
    Device(crate::device::DeviceError),
}

impl fmt::Display for ExploreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Plan(_) => f.write_str("cannot plan the sweep"),
            Self::Capacity(_) => f.write_str("the sweep does not fit this GPU"),
            Self::Probe(_) => f.write_str("cannot probe the model"),
            Self::Measure(_) => f.write_str("cannot bind the reducers to the model's stat columns"),
            Self::Resume(_) => f.write_str("cannot resume the sweep"),
            Self::Output(_) => f.write_str("cannot write the results"),
            Self::Execution(_) => f.write_str("cannot run the sweep"),
            Self::Search(_) => f.write_str("cannot plan the search"),
            #[cfg(not(target_arch = "wasm32"))]
            Self::Device(_) => f.write_str("cannot acquire a GPU device for the sweep"),
        }
    }
}

impl std::error::Error for ExploreError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Plan(error) => Some(error),
            Self::Capacity(error) => Some(error),
            Self::Probe(error) => Some(error),
            Self::Measure(error) => Some(error),
            Self::Resume(error) => Some(error),
            Self::Output(error) => Some(error),
            Self::Execution(error) => Some(error),
            Self::Search(error) => Some(error),
            #[cfg(not(target_arch = "wasm32"))]
            Self::Device(error) => Some(error),
        }
    }
}

impl From<PlanError> for ExploreError {
    fn from(error: PlanError) -> Self {
        Self::Plan(error)
    }
}

impl From<CapacityError> for ExploreError {
    fn from(error: CapacityError) -> Self {
        Self::Capacity(error)
    }
}

impl From<ProbeError> for ExploreError {
    fn from(error: ProbeError) -> Self {
        Self::Probe(error)
    }
}

impl From<MeasureError> for ExploreError {
    fn from(error: MeasureError) -> Self {
        Self::Measure(error)
    }
}

impl From<ResumeError> for ExploreError {
    fn from(error: ResumeError) -> Self {
        Self::Resume(error)
    }
}

impl From<OutputError> for ExploreError {
    fn from(error: OutputError) -> Self {
        Self::Output(error)
    }
}

impl From<ExecutionError> for ExploreError {
    fn from(error: ExecutionError) -> Self {
        Self::Execution(error)
    }
}

impl From<SearchPlanError> for ExploreError {
    fn from(error: SearchPlanError) -> Self {
        Self::Search(error)
    }
}
