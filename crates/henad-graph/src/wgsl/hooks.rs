//! Functions the GPU grid engine calls for a graph model: its sizes, its seed, its uniform blocks, its reruns and its
//! stat values, each run by the host evaluator.

use std::sync::Arc;

use henad_compute::gpu::{GridHooks, GridPassCtx, GridPassId, RerunUploads};
use henad_core::authoring::primitives::space::Boundary;
use henad_core::helpers::extract_u32;
use henad_core::params::ParamValue;
use henad_core::view::StatValue;

use super::uniform::UniformBlock;
use crate::host::value::HostValue;
use crate::host::{HostEvaluator, HostLanes, HostWorld, RunContext, run};

/// Number of grid parameters the engine puts in front of a graph's own.
pub(crate) const GRID_PARAMS: usize = 2;

/// Uniform block of one pass and the context its host values are evaluated in.
#[derive(Debug, Clone)]
pub(crate) struct PassUniform {
    /// Pass the block belongs to.
    pub id: GridPassId,
    /// Members of the block.
    pub block: UniformBlock,
}

/// Source of one stat value: a value of a stats pass's post block.
#[derive(Debug, Clone)]
pub(crate) struct StatSource {
    /// Document position of the stats pass, `None` for a stat of a pass that never runs, which reads 0.
    pub position: Option<usize>,
    /// Slot of the value in the pass's program.
    pub slot: usize,
    /// First counter of the pass's tallies.
    pub first_counter: usize,
}

/// One action as the hooks run it.
#[derive(Debug, Clone)]
pub(crate) struct ActionPlan {
    /// Id of the action.
    pub id: String,
    /// Whether the action reruns an init pass on the host.
    pub reruns: bool,
}

/// Plan the hooks of a graph read.
#[derive(Debug)]
pub(crate) struct GraphHooks {
    pub evaluator: Arc<HostEvaluator>,
    pub width: u32,
    pub height: u32,
    pub boundary: Boundary,
    /// Positions of the lanes each lane buffer holds, in buffer order and segment order.
    pub lane_buffers: Vec<Vec<usize>>,
    /// Whether a one-word tick buffer follows the lanes.
    pub tick_buffer: bool,
    pub uniforms: Vec<PassUniform>,
    pub stats: Vec<StatSource>,
    /// Document position and sum number of each float lane the reduce shader sums, in the order of its partials.
    pub sum_lanes: Vec<(usize, usize)>,
    pub actions: Vec<ActionPlan>,
}

impl GraphHooks {
    /// Returns the world of a grid of `width` by `height`.
    pub(crate) fn world(&self, width: u32, height: u32) -> HostWorld {
        HostWorld::Grid {
            width,
            height,
            boundary: self.boundary,
        }
    }

    /// Returns the values of the graph's own parameters in `params`, after the grid's two.
    pub(crate) fn own(&self, params: &[ParamValue]) -> Vec<HostValue> {
        self.evaluator
            .param_values(params.get(GRID_PARAMS..).unwrap_or_default())
    }

    /// Returns the context the host values of pass `id` are evaluated in.
    pub(crate) fn context(id: GridPassId, ctx: GridPassCtx) -> RunContext {
        match id {
            GridPassId::Action { action, .. } => {
                RunContext::action(ctx.seed, action, ctx.press.count, tick32(ctx.tick))
            }
            _ => RunContext::tick(ctx.seed, tick32(ctx.tick)),
        }
    }

    /// Returns the uniform block of pass `id`, or `Err` with the fault of a prelude the host cannot evaluate.
    /// Returns the totals of each of the `count` float sums of the stats pass at `position`, read from the lanes
    /// `sums` holds, with whether each sums `vec2f` values. A sum the readback leaves out is 0.
    pub(crate) fn pass_sums(&self, position: usize, count: usize, sums: &[f32]) -> Vec<([f64; 2], bool)> {
        (0..count)
            .map(|sum| {
                let lanes: Vec<usize> = self
                    .sum_lanes
                    .iter()
                    .enumerate()
                    .filter(|(_, (known, number))| *known == position && *number == sum)
                    .map(|(lane, _)| lane)
                    .collect();
                let value = |k: usize| {
                    lanes
                        .get(k)
                        .and_then(|&lane| sums.get(lane))
                        .map_or(0.0, |&total| f64::from(total))
                };
                ([value(0), value(1)], lanes.len() > 1)
            })
            .collect()
    }

    /// Returns the words of each lane buffer from `lanes`, the segments of its lanes one after another.
    fn lane_words(&self, lanes: &HostLanes) -> Vec<Vec<u32>> {
        self.lane_buffers
            .iter()
            .map(|positions| {
                positions
                    .iter()
                    .filter_map(|&lane| lanes.columns().get(lane))
                    .flat_map(|column| column.words().iter().copied())
                    .collect()
            })
            .collect()
    }

    pub(crate) fn block_bytes(
        &self,
        id: GridPassId,
        ctx: GridPassCtx,
        params: &[ParamValue],
    ) -> Result<Vec<u8>, String> {
        let Some(uniform) = self.uniforms.iter().find(|uniform| uniform.id == id) else {
            return Ok(vec![0; 16]);
        };
        let world = self.world(ctx.width, ctx.height);
        uniform.block.bytes(
            &self.evaluator,
            world,
            &self.own(params),
            &Self::context(id, ctx),
            &super::uniform::Dispatch {
                counting: ctx.counting,
                ..super::uniform::Dispatch::default()
            },
        )
    }
}

/// Returns `tick` as the 32-bit word a graph reads, wrapping as the shader's tick buffer does.
fn tick32(tick: u64) -> u32 {
    tick as u32
}

impl GridHooks for GraphHooks {
    fn dims(&self, params: &[ParamValue]) -> (u32, u32) {
        (extract_u32(params, 0, self.width), extract_u32(params, 1, self.height))
    }

    fn buffer_lens(&self, width: u32, height: u32) -> Vec<usize> {
        let cells = (width as usize) * (height as usize);
        let lanes = &self.evaluator.document().lanes;
        let mut lens: Vec<usize> = self
            .lane_buffers
            .iter()
            .map(|positions| {
                positions
                    .iter()
                    .map(|&lane| cells * crate::host::value::words_per_value(&lanes[lane].value_type).unwrap_or(1))
                    .sum()
            })
            .collect();
        if self.tick_buffer {
            lens.push(1);
        }
        lens
    }

    fn step_dims(&self, width: u32, height: u32) -> (u32, u32) {
        (width, height)
    }

    /// Runs the init passes on the host.
    ///
    /// # Panics
    ///
    /// Panics with the host's message when an init pass faults. The engine builds inside a scope that reports a panic
    /// as a fault of the build.
    fn seed_buffers(&self, width: u32, height: u32, params: &[ParamValue], seed: Option<u64>) -> Vec<Vec<u32>> {
        let own = params.get(GRID_PARAMS..).unwrap_or_default();
        let lanes = self
            .evaluator
            .init(self.world(width, height), own, seed)
            .unwrap_or_else(|error| panic!("{}: {error}", self.evaluator.document().id));
        let mut buffers: Vec<Vec<u32>> = self.lane_words(&lanes);
        if self.tick_buffer {
            buffers.push(vec![0]);
        }
        buffers
    }

    /// Returns the block of pass `pass`, zeros for one whose prelude faults. A rerun of the same press has refused
    /// such an action already.
    fn pass_params_bytes(&self, pass: GridPassId, ctx: GridPassCtx, params: &[ParamValue]) -> Vec<u8> {
        self.block_bytes(pass, ctx, params).unwrap_or_else(|_| {
            let size = self
                .uniforms
                .iter()
                .find(|uniform| uniform.id == pass)
                .map_or(16, |uniform| uniform.block.size);
            vec![0; size]
        })
    }

    fn rerun(
        &self,
        action: usize,
        ctx: GridPassCtx,
        params: &[ParamValue],
        stream: &mut u64,
    ) -> Result<RerunUploads, String> {
        let Some(plan) = self.actions.get(action) else {
            return Ok(Vec::new());
        };
        // Each of the action's passes evaluates its block now, so a fault refuses the press before anything runs.
        for uniform in self
            .uniforms
            .iter()
            .filter(|uniform| matches!(uniform.id, GridPassId::Action { action: known, .. } if known == action))
        {
            self.block_bytes(uniform.id, ctx, params)?;
        }
        if !plan.reruns {
            return Ok(Vec::new());
        }
        let own = params.get(GRID_PARAMS..).unwrap_or_default();
        let mut lanes = HostLanes::new(self.evaluator.document(), self.world(ctx.width, ctx.height))
            .map_err(|error| error.to_string())?;
        let written = self
            .evaluator
            .rerun(&plan.id, ctx.press.count, &mut lanes, own, ctx.seed, stream)
            .map_err(|error| error.to_string())?;
        // A rerun writes every lane of a buffer or none, since lanes pack only with lanes the same actions rerun.
        Ok(self
            .lane_words(&lanes)
            .into_iter()
            .enumerate()
            .filter(|(buffer, _)| self.lane_buffers[*buffer].iter().any(|lane| written.contains(lane)))
            .collect())
    }

    fn stats(&self, counts: &[u32], ctx: GridPassCtx, params: &[ParamValue]) -> Vec<StatValue> {
        self.stats_with_sums(counts, &[], ctx, params)
    }

    fn stats_with_sums(&self, counts: &[u32], sums: &[f32], ctx: GridPassCtx, params: &[ParamValue]) -> Vec<StatValue> {
        let own = self.own(params);
        let world = self.world(ctx.width, ctx.height);
        let context = RunContext::tick(ctx.seed, tick32(ctx.tick));
        let mut frames: Vec<(usize, Option<Vec<HostValue>>)> = Vec::new();
        self.stats
            .iter()
            .map(|source| {
                let Some(position) = source.position else {
                    return StatValue::Scalar(0.0);
                };
                let index = if let Some(index) = frames.iter().position(|(known, _)| *known == position) {
                    index
                } else {
                    {
                        let frame = match self.evaluator.program_at(position) {
                            Some(Ok(program)) => {
                                let readback = run::Readback {
                                    tallies: (0..program.tallies)
                                        .map(|k| u64::from(counts.get(source.first_counter + k).copied().unwrap_or(0)))
                                        .collect(),
                                    sums: self.pass_sums(position, program.sums.len(), sums),
                                    ..run::Readback::default()
                                };
                                run::finish(program, &own, &context, world, Some(&readback)).ok()
                            }
                            _ => None,
                        };
                        frames.push((position, frame));
                        frames.len() - 1
                    }
                };
                frames[index]
                    .1
                    .as_ref()
                    .and_then(|frame| frame.get(source.slot).copied())
                    .map_or(StatValue::Scalar(f64::NAN), stat_value)
            })
            .collect()
    }
}

/// Returns `value` as a stat value: a number as a scalar, a vector as a 2D vector.
pub(crate) fn stat_value(value: HostValue) -> StatValue {
    match value {
        HostValue::Bool(flag) => StatValue::Scalar(f64::from(u8::from(flag))),
        HostValue::U32(number)
        | HostValue::Category(number)
        | HostValue::Layer(number)
        | HostValue::Cell(number)
        | HostValue::Agent(number) => StatValue::Scalar(f64::from(number)),
        HostValue::I32(number) => StatValue::Scalar(f64::from(number)),
        HostValue::F32(number) => StatValue::Scalar(f64::from(number)),
        HostValue::U64(number) => StatValue::Scalar(number as f64),
        HostValue::F64(number) => StatValue::Scalar(number),
        HostValue::Vec2U([x, y]) => StatValue::Vector2D {
            x: f64::from(x),
            y: f64::from(y),
        },
        HostValue::Vec2I([x, y]) => StatValue::Vector2D {
            x: f64::from(x),
            y: f64::from(y),
        },
        HostValue::Vec2F([x, y]) => StatValue::Vector2D {
            x: f64::from(x),
            y: f64::from(y),
        },
        HostValue::Vec2D([x, y]) => StatValue::Vector2D { x, y },
    }
}
