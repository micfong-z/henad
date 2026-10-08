//! Settings of a check run: the device, the run length, the thread counts, parameter overrides and exemptions.

use henad_compute::entry::ModelEntry;
use henad_compute::gpu::GpuContext;
use henad_core::explore::value::parse_value;
use henad_core::params::{ParamKind, ParamValue};

use super::determinism::COARSE_CADENCE;
use super::{ModelCheck, declared_defaults, error_text};

/// Number of ticks that a run of a determinism check steps by default. The count is not a multiple of the seven-tick
/// cadence, so the last tick is sampled in addition.
const DEFAULT_TICKS: u64 = 20;

/// Minimum number of ticks that [`CheckSettings::ticks`] accepts, one past the coarser sampling cadence.
pub const MIN_TICKS: u64 = COARSE_CADENCE + 1;

/// Small value of each size parameter that the engines prepend, used by every check that builds the model.
const SMALL_SIZES: [(&str, u32); 5] = [
    ("grid_width", 128),
    ("grid_height", 128),
    ("num_agents", 256),
    ("world_width", 128),
    ("world_height", 128),
];

/// Device, run length, thread counts, parameter overrides and exemptions of a check run.
#[derive(Debug, Clone)]
pub struct CheckSettings {
    gpu: Option<GpuContext>,
    ticks: u64,
    thread_counts: (usize, usize),
    /// Model id, parameter id and value text of each override.
    texts: Vec<(String, String, String)>,
    /// Model id, check and reason of each exemption.
    exemptions: Vec<(String, ModelCheck, String)>,
}

/// No device, 20 ticks, 1 and 7 threads, no overrides and no exemptions.
impl Default for CheckSettings {
    fn default() -> Self {
        Self {
            gpu: None,
            ticks: DEFAULT_TICKS,
            thread_counts: (1, 7),
            texts: Vec::new(),
            exemptions: Vec::new(),
        }
    }
}

impl CheckSettings {
    /// Runs the GPU models' checks on `device`. Without a device, they are skipped.
    ///
    /// Note that a check attributes every fault it finds in the device's sink to itself, and clones of a context share
    /// the sink. A test that shares `device` with another test can have its faults dropped, or reported by a check.
    /// Each test takes its own device from `headless_test_device`.
    pub fn gpu(mut self, device: GpuContext) -> Self {
        self.gpu = Some(device);
        self
    }

    /// Steps each run of a determinism check `ticks` ticks.
    ///
    /// # Panics
    ///
    /// Panics when `ticks` is below [`MIN_TICKS`]. A shorter run takes no sample at the coarser cadence of
    /// [`ModelCheck::SamplingCadence`] past tick 0.
    pub fn ticks(mut self, ticks: u64) -> Self {
        assert!(
            ticks >= MIN_TICKS,
            "a check runs at least {MIN_TICKS} ticks, not {ticks}"
        );
        self.ticks = ticks;
        self
    }

    /// Compares runs at `low` and `high` worker threads in [`ModelCheck::ThreadCount`], which sizes the work to split
    /// into twice `high` jobs.
    ///
    /// # Panics
    ///
    /// Panics when `low` is 0 or not below `high`.
    pub fn thread_counts(mut self, low: usize, high: usize) -> Self {
        assert!(
            0 < low && low < high,
            "thread counts {low} and {high} are not two counts in order"
        );
        self.thread_counts = (low, high);
        self
    }

    /// Sets parameter `param_id` of model `model_id` from `text`, as `--set` reads it.
    ///
    /// The value holds in every check that builds the model, and no check changes it. A parameter that the model does
    /// not declare, or a value that the parameter rejects, fails every such check, including a check that cannot run
    /// for want of a device.
    pub fn set_text(mut self, model_id: &str, param_id: &str, text: &str) -> Self {
        self.texts
            .push((model_id.to_owned(), param_id.to_owned(), text.to_owned()));
        self
    }

    /// Skips `check` for model `model_id`, recording `reason` in its report.
    pub fn exempt(mut self, model_id: &str, check: ModelCheck, reason: &str) -> Self {
        self.exemptions.push((model_id.to_owned(), check, reason.to_owned()));
        self
    }

    pub(super) fn device(&self) -> Option<&GpuContext> {
        self.gpu.as_ref()
    }

    pub(super) fn run_ticks(&self) -> u64 {
        self.ticks
    }

    pub(super) fn low_and_high_threads(&self) -> (usize, usize) {
        self.thread_counts
    }

    /// Returns the reason that model `model_id` is exempt from `check`, or `None` when the model is not exempt.
    pub(super) fn exemption(&self, model_id: &str, check: ModelCheck) -> Option<&str> {
        self.exemptions
            .iter()
            .find(|(model, exempt, _)| model == model_id && *exempt == check)
            .map(|(_, _, reason)| reason.as_str())
    }

    /// Returns every model id that an override or an exemption specifies, each once.
    pub(super) fn named_models(&self) -> impl Iterator<Item = &str> {
        let mut ids: Vec<&str> = self
            .texts
            .iter()
            .map(|(model, _, _)| model.as_str())
            .chain(self.exemptions.iter().map(|(model, _, _)| model.as_str()))
            .collect();
        ids.sort_unstable();
        ids.dedup();
        ids.into_iter()
    }

    /// Returns whether an override sets parameter `param_id` of `entry`.
    pub(super) fn overrides(&self, entry: &ModelEntry, param_id: &str) -> bool {
        self.texts
            .iter()
            .any(|(model, param, _)| model == entry.id() && param == param_id)
    }

    /// Returns the values that a check building `entry` uses: the declared defaults, the sizes made small, then the
    /// overrides.
    ///
    /// # Errors
    ///
    /// Returns the failure message for an override that refers to no parameter of `entry`, or a value that its
    /// parameter rejects.
    pub(super) fn check_values(&self, entry: &ModelEntry) -> Result<Vec<ParamValue>, String> {
        let mut values = declared_defaults(entry);
        for (param_id, small) in SMALL_SIZES {
            if let Some(index) = entry.param_index(param_id) {
                values[index] = shrunk(&entry.param_descriptors()[index].kind, small);
            }
        }
        self.apply_overrides(entry, values)
    }

    /// Returns the declared defaults of `entry` with the overrides applied, the values the GPU checks build at.
    ///
    /// # Errors
    ///
    /// As [`Self::check_values`].
    pub(super) fn default_values(&self, entry: &ModelEntry) -> Result<Vec<ParamValue>, String> {
        self.apply_overrides(entry, declared_defaults(entry))
    }

    fn apply_overrides(&self, entry: &ModelEntry, mut values: Vec<ParamValue>) -> Result<Vec<ParamValue>, String> {
        for (_, param_id, text) in self.texts.iter().filter(|(model, _, _)| model == entry.id()) {
            let index = entry
                .param_index(param_id)
                .ok_or_else(|| format!("The settings set parameter '{param_id}', which the model does not declare."))?;
            values[index] = parse_value(&entry.param_descriptors()[index].kind, text).map_err(|error| {
                format!(
                    "The settings set parameter '{param_id}' to '{text}': {}.",
                    error_text(&error)
                )
            })?;
        }
        Ok(values)
    }
}

/// Returns `small` as a value of `kind`, at most the default and clamped to the bounds.
///
/// A kind other than a number keeps its default.
fn shrunk(kind: &ParamKind, small: u32) -> ParamValue {
    match *kind {
        ParamKind::U32 { min, max, default } => ParamValue::U32(small.min(default).clamp(min, max)),
        // Every small size is exact in an `f32`.
        #[expect(clippy::cast_precision_loss, reason = "the small sizes are below 2^24")]
        ParamKind::F32 { min, max, default, .. } => ParamValue::F32((small as f32).min(default).clamp(min, max)),
        _ => kind.default_value(),
    }
}
