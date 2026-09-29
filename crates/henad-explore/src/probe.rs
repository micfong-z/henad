//! Probe builds of a model, made before a sweep to fix its stat columns and measure the footprint of a run.

use std::fmt;

use henad_compute::fault::{Fault, STEPPING, catching};
use henad_compute::gpu::{Demand, GpuContext};
#[cfg(not(target_arch = "wasm32"))]
use henad_compute::gpu::{GpuSimState, fault::catching_on, stepping};
use henad_core::explore::plan::Plan;
use henad_core::export::StatColumns;
use henad_core::params::ParamValue;
use henad_models::registry::{ModelEntry, ModelState};

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
        match (entry.create)(params, seed).map_err(ProbeError::Fault)? {
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
        let mut faults = Vec::new();
        for (config_id, config) in (0_u64..).zip(plan.configs()).take(MAX_PROBED_CONFIGS) {
            let run = plan
                .run(config_id * plan.replicates())
                .expect("every config has a first run");
            match Self::build(entry, gpu, &config.params, Some(run.seed)) {
                Ok(report) => return Ok(report),
                Err(ProbeError::Fault(fault)) => faults.push(ConfigFault { config_id, fault }),
                Err(error) => return Err(error),
            }
        }
        Err(ProbeError::EveryConfigFaulted(faults))
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

#[cfg(not(target_arch = "wasm32"))]
fn probe_gpu(
    entry: &ModelEntry,
    mut state: Box<dyn GpuSimState>,
    gpu: Option<&GpuContext>,
    params: &[ParamValue],
    seed: Option<u64>,
) -> Result<ProbeReport, ProbeError> {
    let ctx = gpu.ok_or(ProbeError::NoDevice)?;
    let stats = catching_on(ctx, STEPPING, || stepping::sample_stats(&mut *state, ctx)).map_err(ProbeError::Fault)?;
    stepping::wait(ctx).map_err(ProbeError::Fault)?;
    Ok(ProbeReport {
        params: params.to_vec(),
        seed,
        columns: StatColumns::plan(&stats),
        heap_bytes: state.heap_bytes() as u64,
        population: state.population(),
        parallel_jobs: state.parallel_jobs(),
        demand: entry.demand(params),
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
pub fn check_capacity(entry: &ModelEntry, plan: &Plan, limits: &wgpu::Limits) -> Result<(), CapacityError> {
    if entry.capacity.is_none() {
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
    use henad_compute::fault::install_panic_hook;
    use henad_core::explore::design::DesignKind;
    use henad_core::explore::factor::{FactorSpec, LevelSpec};
    use henad_core::explore::spec::{BlockSpec, SweepSpec};
    use henad_core::params::ParamValue;
    use henad_models::registry::{ModelEntry, model_registry, register_grid_model};

    use super::{MAX_PROBED_CONFIGS, ProbeError, ProbeReport, check_capacity};
    use crate::schema::model_schema;
    use crate::tests::broken::DividesByParam;

    fn entry(id: &str) -> ModelEntry {
        model_registry(None)
            .into_iter()
            .find(|entry| entry.id == id)
            .expect("the model is registered")
    }

    #[test]
    fn a_probe_reads_the_columns_and_footprint_of_config_zero() {
        let entry = entry("boids");
        let mut spec = SweepSpec::new("boids");
        spec.fixed = vec![("num_agents".to_owned(), "300".to_owned())];
        let plan = spec.plan(&model_schema(&entry)).expect("a valid spec");
        let probe = ProbeReport::for_plan(&entry, None, &plan).expect("boids builds");
        assert_eq!(probe.population, 300);
        assert!(probe.heap_bytes > 0);
        assert!(probe.parallel_jobs.is_some());
        assert!(probe.demand.is_none(), "boids runs on the CPU");
        assert!(!probe.columns.is_empty());
        assert!(check_capacity(&entry, &plan, &wgpu::Limits::default()).is_ok());
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
            .plan(&model_schema(&entry))
            .expect("a valid spec");
        let probe = ProbeReport::for_plan(&entry, None, &plan).expect("config 2 builds");
        assert_eq!(probe.params[3], ParamValue::U32(1), "init_divisor of config 2");
        assert_eq!(
            probe.seed,
            plan.run(4).map(|run| run.seed),
            "the seed of config 2's first run"
        );

        let zeros = vec!["0"; MAX_PROBED_CONFIGS + 1];
        let plan = init_divisors(&zeros).plan(&model_schema(&entry)).expect("a valid spec");
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
        let plan = spec.plan(&model_schema(&entry)).expect("a valid spec");
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
