//! Probe builds of a model, made before a sweep to fix its stat columns and measure the footprint of a run.

use std::fmt;

use web_time::Instant;

use henad_compute::entry::{ModelEntry, ModelState};
use henad_compute::fault::{Fault, STEPPING, catching};
use henad_compute::gpu::{Demand, GpuContext};
#[cfg(not(target_arch = "wasm32"))]
use henad_compute::gpu::{GpuSimState, fault::catching_on, stepping};
use henad_core::explore::plan::Plan;
use henad_core::export::StatColumns;
use henad_core::params::ParamValue;

use crate::output::manifest::now_unix_ms;

/// Most refused configs a [`CapacityError`] lists.
pub const MAX_LISTED_CONFIGS: usize = 5;

/// Most configs [`ProbeReport::for_plan`] builds before it gives up.
pub const MAX_PROBED_CONFIGS: usize = 8;

/// Stat columns and footprint of one build of a model, sampled at tick 0.
#[derive(Debug)]
pub struct ProbeReport {
    /// Values of the config the probe built, one per parameter.
    pub params: Vec<ParamValue>,
    /// Seed the probe built with.
    pub seed: Option<u64>,
    /// Columns of the sample at tick 0. Every later sample of the sweep must fit them.
    pub columns: StatColumns,
    /// Bytes the state holds on the host.
    pub heap_bytes: u64,
    pub population: u64,
    /// Jobs one step splits into, `None` when the backend does not say.
    pub parallel_jobs: Option<usize>,
    /// Device resources of the build, `None` for a CPU model.
    pub demand: Option<Demand>,
}

impl ProbeReport {
    /// Builds `entry` at `params` with `seed`, and samples tick 0 as a run of a sweep does.
    ///
    /// # Errors
    ///
    /// Returns [`ProbeError::NoDevice`] for a GPU model with no device in `gpu`, and [`ProbeError::Fault`] for a
    /// build or a sample that faults.
    pub fn build(
        entry: &ModelEntry,
        gpu: Option<&GpuContext>,
        params: &[ParamValue],
        seed: Option<u64>,
    ) -> Result<Self, ProbeError> {
        if entry.gpu_needs().is_some() && gpu.is_none() {
            return Err(ProbeError::NoDevice);
        }
        match entry.build(params, seed, gpu).map_err(ProbeError::Fault)? {
            ModelState::Cpu(mut state) => {
                let stats = catching(STEPPING, || {
                    state.prepare_view();
                    state.stats()
                })
                .map_err(ProbeError::Fault)?;
                Ok(Self {
                    params: params.to_vec(),
                    seed,
                    columns: StatColumns::plan(&stats),
                    heap_bytes: state.heap_bytes() as u64,
                    population: state.population(),
                    parallel_jobs: state.parallel_jobs(),
                    demand: None,
                })
            }
            ModelState::Gpu(state) => probe_gpu(entry, state, gpu, params, seed),
        }
    }

    /// Builds the configs of `plan` in order, each with the seed of its first run, and returns the first report
    /// without a fault.
    ///
    /// A config that faults is left for its runs to record. Note that only the first [`MAX_PROBED_CONFIGS`] configs
    /// are tried.
    ///
    /// # Errors
    ///
    /// Returns [`ProbeError::NoDevice`] for a GPU model with no device in `gpu`, and
    /// [`ProbeError::EveryConfigFaulted`] when every config tried faults.
    pub fn for_plan(entry: &ModelEntry, gpu: Option<&GpuContext>, plan: &Plan) -> Result<Self, ProbeError> {
        let mut probe = PlanProbe::new();
        loop {
            if let Some(timed) = probe.step(entry, gpu, plan)? {
                return Ok(timed.report);
            }
        }
    }

    /// Builds the last config of `plan` with the seed of its first run.
    ///
    /// Returns `None` for a plan of one config or a last config `probed` already built. A last config that faults
    /// gives `None` too, and is left for its runs to record.
    pub fn for_last_config(entry: &ModelEntry, gpu: Option<&GpuContext>, plan: &Plan, probed: &Self) -> Option<Self> {
        let config_id = plan.configs().len().checked_sub(1)? as u64;
        let config = plan.config(config_id)?;
        let run = plan.run(config_id * plan.replicates())?;
        if config_id == 0 || (config.params == probed.params && Some(run.seed) == probed.seed) {
            return None;
        }
        Self::build(entry, gpu, &config.params, Some(run.seed)).ok()
    }

    /// Bytes the build holds, on the host and on the device together.
    pub fn footprint(&self) -> u64 {
        self.heap_bytes + self.demand.as_ref().map_or(0, Demand::bytes)
    }

    /// Returns the report of the same CPU build made on a thread pool of `threads` workers.
    ///
    /// Note that a buffer sized to the pool, such as a scatter grid's scratch, can change the host bytes.
    ///
    /// # Errors
    ///
    /// Returns [`ProbeError::Pool`] when the pool cannot be built, and the error [`Self::build`] gives.
    pub fn rebuilt_on(&self, entry: &ModelEntry, threads: usize) -> Result<Self, ProbeError> {
        let pool = rayon::ThreadPoolBuilder::new()
            .num_threads(threads)
            .build()
            .map_err(ProbeError::Pool)?;
        crate::exec::run_in_pool(&pool, || Self::build(entry, None, &self.params, self.seed))
    }
}

/// Probe of a plan's configs in progress, one build per call to [`PlanProbe::step`].
///
/// The configs are tried as [`ProbeReport::for_plan`] tries them.
#[derive(Debug)]
pub(crate) struct PlanProbe {
    /// Config the next step builds.
    next_config: u64,
    /// Faults of the configs tried so far.
    faults: Vec<ConfigFault>,
    /// Clock reading when the probe was created.
    started: Instant,
    /// Wall clock time when the probe was created, in milliseconds since the Unix epoch.
    started_unix_ms: u64,
}

/// Report of the first config of a plan that builds, with the clock readings taken when its probe was created.
#[derive(Debug)]
pub(crate) struct TimedProbe {
    pub(crate) report: ProbeReport,
    pub(crate) started: Instant,
    pub(crate) started_unix_ms: u64,
}

impl PlanProbe {
    /// Returns a probe that has tried no config yet, timed from now.
    pub(crate) fn new() -> Self {
        Self {
            next_config: 0,
            faults: Vec::new(),
            started: Instant::now(),
            started_unix_ms: now_unix_ms(),
        }
    }

    /// Builds the next config of `plan` with the seed of its first run, and returns its report with the probe's clock
    /// readings when it builds without a fault.
    ///
    /// Returns `Ok(None)` for a config that faults while configs are left to try. The fault is left for the config's
    /// runs to record.
    ///
    /// # Errors
    ///
    /// Returns [`ProbeError::NoDevice`] for a GPU model with no device in `gpu`, and
    /// [`ProbeError::EveryConfigFaulted`] once every config tried faults.
    pub(crate) fn step(
        &mut self,
        entry: &ModelEntry,
        gpu: Option<&GpuContext>,
        plan: &Plan,
    ) -> Result<Option<TimedProbe>, ProbeError> {
        let config_limit = plan.configs().len().min(MAX_PROBED_CONFIGS) as u64;
        let config_id = self.next_config;
        let Some(config) = plan.config(config_id).filter(|_| config_id < config_limit) else {
            return Err(ProbeError::EveryConfigFaulted(std::mem::take(&mut self.faults)));
        };
        self.next_config += 1;
        let run = plan
            .run(config_id * plan.replicates())
            .expect("every config has a first run");
        match ProbeReport::build(entry, gpu, &config.params, Some(run.seed)) {
            Ok(report) => Ok(Some(TimedProbe {
                report,
                started: self.started,
                started_unix_ms: self.started_unix_ms,
            })),
            Err(ProbeError::Fault(fault)) => {
                self.faults.push(ConfigFault { config_id, fault });
                if self.next_config < config_limit {
                    Ok(None)
                } else {
                    Err(ProbeError::EveryConfigFaulted(std::mem::take(&mut self.faults)))
                }
            }
            Err(error) => Err(error),
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn probe_gpu(
    entry: &ModelEntry,
    mut state: Box<dyn GpuSimState>,
    gpu: Option<&GpuContext>,
    params: &[ParamValue],
    seed: Option<u64>,
) -> Result<ProbeReport, ProbeError> {
    let ctx = gpu.ok_or(ProbeError::NoDevice)?;
    let stats = catching_on(ctx, STEPPING, || stepping::sample_stats(&mut *state, ctx))
        .flatten()
        .map_err(ProbeError::Fault)?;
    stepping::wait(ctx).map_err(ProbeError::Fault)?;
    Ok(ProbeReport {
        params: params.to_vec(),
        seed,
        columns: StatColumns::plan(&stats),
        heap_bytes: state.heap_bytes() as u64,
        population: state.population(),
        parallel_jobs: state.parallel_jobs(),
        demand: entry.demand(params, &ctx.device.limits()),
    })
}

/// Refuses a GPU model. A browser cannot block on the device, and a sample blocks.
#[cfg(target_arch = "wasm32")]
fn probe_gpu(
    _entry: &ModelEntry,
    _state: Box<dyn henad_compute::gpu::GpuSimState>,
    _gpu: Option<&GpuContext>,
    _params: &[ParamValue],
    _seed: Option<u64>,
) -> Result<ProbeReport, ProbeError> {
    Err(ProbeError::NoDevice)
}

/// Fault of the probe build of one config.
#[derive(Debug)]
pub struct ConfigFault {
    pub config_id: u64,
    pub fault: Fault,
}

/// A probe build that cannot run.
#[derive(Debug)]
pub enum ProbeError {
    /// A GPU model with no device to sample it on. A browser never has one.
    NoDevice,
    /// The build or its sample at tick 0 faulted.
    Fault(Fault),
    /// Every config [`ProbeReport::for_plan`] tried faulted, each with its fault.
    EveryConfigFaulted(Vec<ConfigFault>),
    /// Building the thread pool of a probe build failed.
    Pool(rayon::ThreadPoolBuildError),
}

impl fmt::Display for ProbeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoDevice => f.write_str("a GPU sweep needs a GPU device and a native build"),
            Self::Fault(_) => f.write_str("the probe build failed"),
            Self::EveryConfigFaulted(faults) => {
                write!(
                    f,
                    "the probe build faulted on each of the {} configs tried",
                    faults.len()
                )?;
                for config in faults {
                    write!(f, "\n  config {}: {}", config.config_id, config.fault)?;
                }
                Ok(())
            }
            Self::Pool(_) => f.write_str("cannot build the thread pool of the probe build"),
        }
    }
}

impl std::error::Error for ProbeError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::NoDevice | Self::EveryConfigFaulted(_) => None,
            Self::Fault(fault) => Some(fault),
            Self::Pool(error) => Some(error),
        }
    }
}

/// A config the device cannot host.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RefusedConfig {
    pub config_id: u64,
    /// Reasons the device refuses the config, from [`ModelEntry::shortfalls`].
    pub reasons: Vec<String>,
}

/// Configs of a plan the device cannot host, found before any run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CapacityError {
    /// First refused configs, up to [`MAX_LISTED_CONFIGS`] of them.
    pub refused: Vec<RefusedConfig>,
    /// Number of refused configs.
    pub count: u64,
}

impl fmt::Display for CapacityError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.count == 1 {
            f.write_str("1 config does not fit this GPU")?;
        } else {
            write!(f, "{} configs do not fit this GPU", self.count)?;
        }
        for config in &self.refused {
            for reason in &config.reasons {
                write!(f, "\n  config {}: {reason}", config.config_id)?;
            }
        }
        Ok(())
    }
}

impl std::error::Error for CapacityError {}

/// Checks every config of `plan` against `limits` before any run. A CPU model always passes.
///
/// # Errors
///
/// Returns [`CapacityError`] when some config needs more than the device allows.
pub(crate) fn check_capacity(entry: &ModelEntry, plan: &Plan, limits: &wgpu::Limits) -> Result<(), CapacityError> {
    if entry.gpu_needs().is_none() {
        return Ok(());
    }
    let mut error = CapacityError {
        refused: Vec::new(),
        count: 0,
    };
    for (config_id, config) in (0_u64..).zip(plan.configs()) {
        let reasons = entry.shortfalls(&config.params, limits);
        if reasons.is_empty() {
            continue;
        }
        error.count += 1;
        if error.refused.len() < MAX_LISTED_CONFIGS {
            error.refused.push(RefusedConfig { config_id, reasons });
        }
    }
    if error.count == 0 { Ok(()) } else { Err(error) }
}

#[cfg(test)]
mod tests {
    use henad_compute::entry::{ModelEntry, register_grid_model};
    use henad_compute::fault::install_panic_hook;
    use henad_core::explore::design::DesignKind;
    use henad_core::explore::factor::{FactorSpec, LevelSpec};
    use henad_core::explore::spec::{BlockSpec, SweepSpec};
    use henad_core::params::ParamValue;
    use henad_models::example_models;

    use super::{MAX_LISTED_CONFIGS, MAX_PROBED_CONFIGS, ProbeError, ProbeReport, check_capacity};
    use crate::tests::broken::DividesByParam;

    fn entry(id: &str) -> ModelEntry {
        example_models().get(id).cloned().expect("the model is registered")
    }

    #[test]
    fn a_probe_reads_the_columns_and_footprint_of_config_zero() {
        let entry = entry("boids");
        let mut spec = SweepSpec::new("boids");
        spec.fixed = vec![("num_agents".to_owned(), "300".to_owned())];
        let plan = spec.plan(&entry.schema()).expect("a valid spec");
        let probe = ProbeReport::for_plan(&entry, None, &plan).expect("boids builds");
        assert_eq!(probe.population, 300);
        assert!(probe.heap_bytes > 0);
        assert!(probe.parallel_jobs.is_some());
        assert!(probe.demand.is_none(), "boids runs on the CPU");
        assert!(!probe.columns.is_empty());
    }

    /// Returns a spec over `model` whose configs are the square grids with the sides `sides`.
    fn square_grids(model: &str, sides: &[&str]) -> SweepSpec {
        let levels = LevelSpec::Values(sides.iter().map(|&side| side.to_owned()).collect());
        let mut spec = SweepSpec::new(model);
        spec.blocks = vec![BlockSpec {
            design: DesignKind::Zip,
            factors: ["grid_width", "grid_height"]
                .map(|id| FactorSpec::param(id, levels.clone()))
                .to_vec(),
            design_seed: None,
        }];
        spec
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn check_capacity_counts_every_config_past_the_limits_and_passes_a_cpu_model() {
        let Some(ctx) = crate::tests::support::headless_device() else {
            return;
        };
        let gpu_sir = crate::tests::support::entry("gpu_sir", Some(&ctx));
        let sides = ["16", "32", "48", "64", "80", "96", "112"];
        let plan = square_grids("gpu_sir", &sides)
            .plan(&gpu_sir.schema())
            .expect("a valid spec");
        let fitting = plan.config(0).expect("the plan has config 0");
        let demand = gpu_sir
            .demand(&fitting.params, &ctx.device.limits())
            .expect("a GPU model has a demand");
        let largest = demand
            .buffers
            .iter()
            .map(|alloc| alloc.bytes)
            .max()
            .expect("gpu_sir allocates buffers");
        // The largest buffer of the smallest grid fills a binding, and every larger grid needs more.
        let limits = wgpu::Limits {
            max_storage_buffer_binding_size: largest,
            ..wgpu::Limits::default()
        };
        let error = check_capacity(&gpu_sir, &plan, &limits).expect_err("the larger grids pass the binding size");
        assert_eq!(error.count, 6);
        let listed: Vec<u64> = error.refused.iter().map(|config| config.config_id).collect();
        assert_eq!(listed, (1..=MAX_LISTED_CONFIGS as u64).collect::<Vec<_>>());
        assert!(error.refused.iter().all(|config| !config.reasons.is_empty()));

        let game_of_life = entry("game_of_life");
        let plan = square_grids("game_of_life", &sides)
            .plan(&game_of_life.schema())
            .expect("a valid spec");
        assert!(
            check_capacity(&game_of_life, &plan, &limits).is_ok(),
            "a CPU model passes limits that refuse a GPU model"
        );
    }

    /// Returns a spec over `DividesByParam` on an 8 by 8 grid, varying `init_divisor` over `levels`.
    fn init_divisors(levels: &[&str]) -> SweepSpec {
        let mut spec = SweepSpec::new("divides_by_param");
        spec.fixed = vec![
            ("grid_width".to_owned(), "8".to_owned()),
            ("grid_height".to_owned(), "8".to_owned()),
        ];
        spec.run.replicates = 2;
        spec.blocks = vec![BlockSpec {
            design: DesignKind::Factorial,
            factors: vec![FactorSpec::param(
                "init_divisor",
                LevelSpec::Values(levels.iter().map(|&level| level.to_owned()).collect()),
            )],
            design_seed: None,
        }];
        spec
    }

    #[test]
    fn a_config_that_faults_is_left_to_its_runs() {
        install_panic_hook();
        let entry = register_grid_model::<DividesByParam>();
        let plan = init_divisors(&["0", "0", "1"])
            .plan(&entry.schema())
            .expect("a valid spec");
        let probe = ProbeReport::for_plan(&entry, None, &plan).expect("config 2 builds");
        assert_eq!(probe.params[3], ParamValue::U32(1), "init_divisor of config 2");
        assert_eq!(
            probe.seed,
            plan.run(4).map(|run| run.seed),
            "the seed of config 2's first run"
        );

        let zeros = vec!["0"; MAX_PROBED_CONFIGS + 1];
        let plan = init_divisors(&zeros).plan(&entry.schema()).expect("a valid spec");
        let error = ProbeReport::for_plan(&entry, None, &plan).expect_err("no config builds");
        let ProbeError::EveryConfigFaulted(faults) = &error else {
            panic!("{error:?}");
        };
        let tried: Vec<u64> = faults.iter().map(|config| config.config_id).collect();
        assert_eq!(tried, (0..MAX_PROBED_CONFIGS as u64).collect::<Vec<_>>());
        let text = error.to_string();
        assert!(text.contains("\n  config 7: while building the model"), "{text}");
    }

    #[test]
    fn a_rebuild_on_a_narrower_pool_holds_less_scratch() {
        let entry = entry("ants");
        let mut spec = SweepSpec::new("ants");
        spec.fixed = vec![
            ("num_agents".to_owned(), "300".to_owned()),
            ("world_width".to_owned(), "64".to_owned()),
            ("world_height".to_owned(), "64".to_owned()),
        ];
        let plan = spec.plan(&entry.schema()).expect("a valid spec");
        let probe = ProbeReport::for_plan(&entry, None, &plan).expect("ants builds");
        let [one, four] = [1, 4].map(|threads| probe.rebuilt_on(&entry, threads).expect("ants builds on a pool"));
        assert!(
            one.heap_bytes < four.heap_bytes,
            "a scatter grid keeps a shadow grid per worker: {} and {}",
            one.heap_bytes,
            four.heap_bytes
        );
        assert_eq!((one.params, one.seed), (probe.params, probe.seed));
        assert_eq!(one.columns, four.columns);
    }
}
