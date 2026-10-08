//! Checks of a model entry's contracts, for a model's own tests.
//!
//! [`check_model`] runs every [`ModelCheck`] that applies to an entry and returns a [`ModelReport`] of the failures
//! and the skipped checks. [`assert_set_conforms`] checks a whole [`ModelSet`] and panics with every failure. A
//! check never panics on a model's behalf. A model's panic, a device error and a broken contract each come back as a
//! [`CheckFailure`].
//!
//! Each check that builds the model sets `grid_width`, `grid_height`, `num_agents`, `world_width` and
//! `world_height` small, within their bounds and never above their defaults. [`ModelCheck::ThreadCount`] sets the
//! size the work splits at, and the GPU checks and a GPU model's [`ModelCheck::Actions`] build at the declared
//! defaults. [`CheckSettings::set_text`] overrides any of these.
//!
//! A GPU model is built on the device passed to [`CheckSettings::gpu`]. Without a device, or on wasm32, every check
//! that builds it is skipped, apart from the failures [`check_model`] lists. The GPU checks and
//! `headless_test_device` are native only.
//!
//! Note that a caller of [`check_model`] or [`check_model_set`] installs the panic hook first, through
//! [`henad_compute::fault::install_panic_hook`]. Without it the failure of a kernel panic has no `file:line`.

mod built;
mod declared;
mod determinism;
mod device;
#[cfg(not(target_arch = "wasm32"))]
mod gpu;
mod report;
mod settings;

use std::error::Error;
use std::fmt;

use henad_compute::entry::{ModelEntry, ModelSet};
use henad_compute::fault::{Fault, catching, install_panic_hook};
use henad_compute::gpu::GpuContext;
use henad_compute::gpu::fault::catching_on;
use henad_core::metadata::Backend;
use henad_core::params::ParamValue;
use henad_core::view::{StatEntry, StatValue};

pub use device::TestDeviceRequest;
#[cfg(not(target_arch = "wasm32"))]
pub use device::headless_test_device;
pub use report::{CheckFailure, ModelReport, SetReport, SkipReason, SkippedCheck};
pub use settings::{CheckSettings, MIN_TICKS};

/// One contract of a model entry that the kit checks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum ModelCheck {
    // Declarations, with no build.
    /// The id meets the grammar of model ids.
    ModelId,
    /// Parameter ids are unique, and none repeats an id the engine prepends. Each is used on the command line and in
    /// a design table, so it is not empty, holds no whitespace and no `=`, and does not start with `action.`.
    ParamIds,
    /// Stat labels are unique.
    StatLabels,
    /// Action ids are unique. Each is used on the command line and in a design table, so it is not empty, and holds
    /// no whitespace and no `=`.
    ActionIds,
    /// A declared palette has colours.
    Palette,
    /// The backend, the structure, the topology hint and the device demand agree.
    Metadata,
    /// The declared defaults pass [`henad_compute::simulation::RunSetup::from_parts`], as the app's Build checks them.
    DefaultSetup,
    /// A GPU model's declared defaults fit `wgpu::Limits::default()`, the WebGPU baseline a browser offers.
    DefaultsFit,
    // Built at the defaults.
    /// Each declared apply mode matches what `set_param` accepts.
    ApplyModes,
    /// The factory returns the declared backend, and its views or GPU layers match the topology hint.
    Views,
    /// Only a CPU model reports `parallel_jobs`, and never zero jobs.
    ParallelJobs,
    /// Every declared action is accepted, and the index after the last action is rejected.
    Actions,
    /// Every declared stat gets a value. A model that returns more values than it declares passes, since the engine
    /// drops the extra values before the check sees them.
    StatCount,
    // Determinism.
    /// A CPU model's stats and exported state are the same at the low and the high thread count.
    ThreadCount,
    /// Two builds on one seed agree.
    SameSeed,
    /// Two seeds differ in some stat.
    SeedSensitivity,
    /// A run sampled every tick ends where a run sampled every seventh tick ends.
    SamplingCadence,
    // GPU, given a device.
    /// The model builds on the device at its declared defaults, and its declared demand fits the device.
    BaselineBuild,
    /// One submission of `MAX_STEPS_PER_SUBMISSION` steps executes every step. A model that replays exactly reads back
    /// the same stats as single steps, and any other model that declares stats reads back some stat that is not zero.
    FullSubmission,
    /// A sampled slice reads back the same stats as a snapshot.
    SampledSlice,
}

impl ModelCheck {
    /// Every check, in the order [`check_model`] runs them.
    pub const ALL: &'static [Self] = &[
        Self::ModelId,
        Self::ParamIds,
        Self::StatLabels,
        Self::ActionIds,
        Self::Palette,
        Self::Metadata,
        Self::DefaultSetup,
        Self::DefaultsFit,
        Self::ApplyModes,
        Self::Views,
        Self::ParallelJobs,
        Self::Actions,
        Self::StatCount,
        Self::ThreadCount,
        Self::SameSeed,
        Self::SeedSensitivity,
        Self::SamplingCadence,
        Self::BaselineBuild,
        Self::FullSubmission,
        Self::SampledSlice,
    ];

    /// Returns whether the check builds the model.
    fn builds(self) -> bool {
        !matches!(
            self,
            Self::ModelId
                | Self::ParamIds
                | Self::StatLabels
                | Self::ActionIds
                | Self::Palette
                | Self::Metadata
                | Self::DefaultSetup
                | Self::DefaultsFit
        )
    }

    /// Returns whether the check applies to a model on `backend`.
    fn applies_to(self, backend: Backend) -> bool {
        match self {
            Self::ThreadCount => backend == Backend::Cpu,
            Self::DefaultsFit | Self::BaselineBuild | Self::FullSubmission | Self::SampledSlice => {
                backend == Backend::Gpu
            }
            _ => true,
        }
    }

    /// Returns whether the check compares one run with another and needs a model that replays exactly.
    fn compares_runs(self) -> bool {
        matches!(self, Self::SameSeed | Self::SeedSensitivity | Self::SamplingCadence)
    }
}

impl fmt::Display for ModelCheck {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(self, f)
    }
}

/// Checks every contract of `entry` that `settings` allows, and reports every failure.
///
/// An exemption of a check that does not apply to the model fails that check. An override that the model rejects fails
/// every check that builds the model, including a check skipped for want of a device. With `HENAD_REQUIRE_GPU` set,
/// a check that needs a device that the settings do not provide fails as well.
#[must_use]
pub fn check_model(entry: &ModelEntry, settings: &CheckSettings) -> ModelReport {
    check_model_requiring(entry, settings, device::gpu_required())
}

/// Checks `entry` as [`check_model`] does, with `gpu_required` instead of reading `HENAD_REQUIRE_GPU`.
pub(crate) fn check_model_requiring(entry: &ModelEntry, settings: &CheckSettings, gpu_required: bool) -> ModelReport {
    let mut report = ModelReport::new(entry.id());
    let backend = entry.metadata().backend;
    let gpu = if backend == Backend::Gpu {
        settings.device()
    } else {
        None
    };
    let refused_override = settings.default_values(entry).err();
    for &check in ModelCheck::ALL {
        if let Some(reason) = skip_reason(entry, settings, check, gpu.is_some()) {
            let exempt = settings.exemption(entry.id(), check).is_some();
            match skipped_failure(check, &reason, exempt, refused_override.as_deref(), gpu_required) {
                Some(message) => report.fail(check, message),
                None => report.skip(check, reason),
            }
            continue;
        }
        let outcome = guarded(gpu, || run_check(entry, settings, check, gpu));
        match outcome {
            Ok(Ran::Passed) => {}
            Ok(Ran::PassedAtJobs(jobs)) => report.set_thread_count_jobs(jobs),
            Ok(Ran::Skipped(reason)) => report.skip(check, reason),
            Err(message) => report.fail(check, message),
        }
    }
    report
}

/// Checks every model of `models`, in the set's order.
///
/// The report also lists every model id that an override or an exemption in `settings` specifies and that is
/// missing from the set.
#[must_use]
pub fn check_model_set(models: &ModelSet, settings: &CheckSettings) -> SetReport {
    let reports = models.iter().map(|entry| check_model(entry, settings)).collect();
    let unknown_models = settings
        .named_models()
        .filter(|id| models.get(id).is_none())
        .map(str::to_owned)
        .collect();
    SetReport::new(reports, unknown_models)
}

/// Installs the panic hook, then checks every model of `models`.
///
/// A set that passes prints its report, the checks each model skipped included. libtest shows it under
/// `--nocapture`.
///
/// # Panics
///
/// Panics with every failure of every model, and every model id that the settings specify and that is missing from
/// the set, listed together.
pub fn assert_set_conforms(models: &ModelSet, settings: &CheckSettings) {
    install_panic_hook();
    let report = check_model_set(models, settings);
    report.assert_passed();
    #[expect(clippy::print_stdout, reason = "libtest captures a test's standard output")]
    {
        println!("{report}");
    }
}

/// Seed of every build a check makes, and the first seed [`ModelCheck::SeedSensitivity`] compares.
const SEED: u64 = 1;

/// Phase recorded in a fault that a check raises.
const CHECKING: &str = "checking the model";

/// Outcome of a check that ran to its end.
enum Ran {
    Passed,
    /// [`ModelCheck::ThreadCount`] passed with a step split into this many jobs.
    PassedAtJobs(usize),
    /// The check found nothing to compare and skipped itself.
    Skipped(SkipReason),
}

/// Returns the reason `check` is skipped for `entry`, or `None` to run it.
fn skip_reason(
    entry: &ModelEntry,
    settings: &CheckSettings,
    check: ModelCheck,
    has_device: bool,
) -> Option<SkipReason> {
    let metadata = entry.metadata();
    if !check.applies_to(metadata.backend) {
        return Some(SkipReason::OtherBackend);
    }
    if check.compares_runs() && !metadata.replays_exactly {
        return Some(SkipReason::InexactReplay);
    }
    if let Some(reason) = settings.exemption(entry.id(), check) {
        return Some(SkipReason::Exempt(reason.to_owned()));
    }
    if cfg!(target_arch = "wasm32") && check == ModelCheck::ThreadCount {
        return Some(SkipReason::NativeOnly);
    }
    if metadata.backend == Backend::Gpu && check.builds() {
        if cfg!(target_arch = "wasm32") {
            return Some(SkipReason::NativeOnly);
        }
        if !has_device {
            return Some(SkipReason::NoDevice);
        }
    }
    None
}

/// Returns the failure of `check`, skipped for `reason`, or `None` when the skip stands.
///
/// An exemption of a check that does not apply fails that check. A check that builds the model and cannot run here
/// fails on `refused_override`, the message of an override that the model rejects, as it fails where it runs.
/// Otherwise a stale override of a GPU model passes on a machine without a device. A check skipped for want of a
/// device fails when `gpu_required` is set.
fn skipped_failure(
    check: ModelCheck,
    reason: &SkipReason,
    exempt: bool,
    refused_override: Option<&str>,
    gpu_required: bool,
) -> Option<String> {
    match (reason, refused_override) {
        (SkipReason::OtherBackend | SkipReason::InexactReplay, _) if exempt => Some(format!(
            "The settings exempt this check, and it does not apply to the model: {reason}."
        )),
        (SkipReason::NoDevice | SkipReason::NativeOnly, Some(message)) if check.builds() => Some(message.to_owned()),
        (SkipReason::NoDevice, _) if gpu_required => {
            Some("HENAD_REQUIRE_GPU is set, but the settings give no GPU device.".to_owned())
        }
        _ => None,
    }
}

/// Runs `check` on `entry`, on `gpu` for a GPU model.
fn run_check(
    entry: &ModelEntry,
    settings: &CheckSettings,
    check: ModelCheck,
    gpu: Option<&GpuContext>,
) -> Result<Ran, String> {
    match check {
        ModelCheck::ModelId => declared::model_id(entry),
        ModelCheck::ParamIds => declared::param_ids(entry),
        ModelCheck::StatLabels => declared::stat_labels(entry),
        ModelCheck::ActionIds => declared::action_ids(entry),
        ModelCheck::Palette => declared::palette(entry),
        ModelCheck::Metadata => declared::metadata(entry),
        ModelCheck::DefaultSetup => declared::default_setup(entry),
        ModelCheck::DefaultsFit => declared::defaults_fit(entry),
        ModelCheck::ApplyModes => built::apply_modes(entry, &settings.check_values(entry)?, gpu),
        ModelCheck::Views => built::views(entry, &settings.check_values(entry)?, gpu),
        ModelCheck::ParallelJobs => built::parallel_jobs(entry, &settings.check_values(entry)?, gpu),
        // A GPU model's actions run at the size a host builds first.
        ModelCheck::Actions if gpu.is_some() => built::actions(entry, &settings.default_values(entry)?, gpu),
        ModelCheck::Actions => built::actions(entry, &settings.check_values(entry)?, gpu),
        ModelCheck::StatCount => built::stat_count(entry, &settings.check_values(entry)?, gpu),
        ModelCheck::ThreadCount => return determinism::thread_count(entry, settings),
        ModelCheck::SameSeed => determinism::same_seed(entry, settings, gpu),
        ModelCheck::SeedSensitivity => determinism::seed_sensitivity(entry, settings, gpu),
        ModelCheck::SamplingCadence => determinism::sampling_cadence(entry, settings, gpu),
        #[cfg(not(target_arch = "wasm32"))]
        ModelCheck::BaselineBuild => gpu::baseline_build(entry, settings, device(gpu)?),
        #[cfg(not(target_arch = "wasm32"))]
        ModelCheck::FullSubmission => gpu::full_submission(entry, settings, device(gpu)?),
        #[cfg(not(target_arch = "wasm32"))]
        ModelCheck::SampledSlice => gpu::sampled_slice(entry, settings, device(gpu)?),
        #[cfg(target_arch = "wasm32")]
        ModelCheck::BaselineBuild | ModelCheck::FullSubmission | ModelCheck::SampledSlice => {
            return Ok(Ran::Skipped(SkipReason::NativeOnly));
        }
    }
    .map(|()| Ran::Passed)
}

/// Returns the device a GPU check runs on.
#[cfg(not(target_arch = "wasm32"))]
fn device(gpu: Option<&GpuContext>) -> Result<&GpuContext, String> {
    gpu.ok_or_else(|| "A GPU check ran with no device.".to_owned())
}

/// Runs `body` inside the fault scopes, on `gpu` for a GPU model.
///
/// A panic or a device error out of `body` comes back as the check's failure. On a GPU the check then waits for the
/// device, and a fault that its work left in the sink is the check's failure too. No fault outlives the check that
/// raised it.
fn guarded<T>(gpu: Option<&GpuContext>, body: impl FnOnce() -> Result<T, String>) -> Result<T, String> {
    let Some(ctx) = gpu else {
        return catching(CHECKING, body).map_err(|fault| fault_text(&fault))?;
    };
    // A fault left by earlier work on the device belongs to no check.
    drop(ctx.faults.take());
    let result = catching_on(ctx, CHECKING, body).map_err(|fault| fault_text(&fault))?;
    #[cfg(not(target_arch = "wasm32"))]
    henad_compute::gpu::stepping::wait(ctx).map_err(|fault| fault_text(&fault))?;
    if let Some(fault) = ctx.faults.take() {
        return Err(fault_text(&fault));
    }
    result
}

/// Returns `fault` as a failure message.
fn fault_text(fault: &Fault) -> String {
    format!("Fault {fault}.")
}

/// Returns `error` and each of its causes, joined by colons.
///
/// The chain ends at a [`Fault`], whose own text already includes its cause.
fn error_text(error: &(dyn Error + 'static)) -> String {
    let mut text = error.to_string();
    let mut current = error;
    while !current.is::<Fault>()
        && let Some(source) = current.source()
    {
        text.push_str(": ");
        text.push_str(&source.to_string());
        current = source;
    }
    text
}

/// Returns the declared default of every parameter of `entry`.
fn declared_defaults(entry: &ModelEntry) -> Vec<ParamValue> {
    entry
        .param_descriptors()
        .iter()
        .map(|descriptor| descriptor.kind.default_value())
        .collect()
}

/// Returns the bits of every value of `entries`, each under its label.
fn stat_bits(entries: &[StatEntry]) -> Vec<(&'static str, Vec<u64>)> {
    entries
        .iter()
        .map(|entry| {
            let bits = match &entry.value {
                StatValue::Scalar(value) => vec![value.to_bits()],
                StatValue::Vector2D { x, y } => vec![x.to_bits(), y.to_bits()],
                StatValue::Histogram { edges, counts } => edges
                    .iter()
                    .map(|edge| edge.to_bits())
                    .chain(counts.iter().copied())
                    .collect(),
            };
            (entry.label, bits)
        })
        .collect()
}

/// Returns the first stat whose bits differ between `first` and `second`, or `None` when they agree.
fn first_stat_difference(first: &[StatEntry], second: &[StatEntry]) -> Option<String> {
    let (first, second) = (stat_bits(first), stat_bits(second));
    if first.len() != second.len() {
        return Some(format!("{} stats against {}", first.len(), second.len()));
    }
    first
        .iter()
        .zip(&second)
        .find(|(left, right)| left != right)
        .map(|((label, _), _)| format!("stat '{label}'"))
}
