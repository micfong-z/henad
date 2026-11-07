//! Functions the GPU agent engine calls for a graph model: its sizes, its seed, its uniform blocks, its reruns, its stat
//! values and its faults, each run by the host evaluator.

use std::sync::Arc;

use henad_compute::gpu::{AgentPassId, AgentRun, AgentUploads};
use henad_core::authoring::model::field::Extent;
use henad_core::authoring::model::gpu_agent_model::{Geometry, PassCtx, PassId};
use henad_core::helpers::{extract_f32, extract_u32};
use henad_core::params::ParamValue;
use henad_core::view::StatValue;

use super::agents::AGENT_PARAMS;
use super::emit::{FAULT_WORD, FIRST_COUNTER};
use super::hooks::{ActionPlan, stat_value};
use super::uniform::{Dispatch, UniformBlock};
use crate::document::Domain;
use crate::host::value::HostValue;
use crate::host::{HostEvaluator, HostLanes, HostWorld, RunContext, run};

/// What one buffer of an agents world holds.
#[derive(Debug, Clone)]
pub(crate) enum BufferSource {
    /// The lanes at these columns of the host lanes, each with its words per element, over `domain`, one segment
    /// after another.
    Lanes {
        columns: Vec<(usize, usize)>,
        domain: Domain,
    },
    /// A field's layers, from this column of the host lanes on.
    Field { first: usize, layers: usize },
    /// A buffer that starts at zero, `per_cell` words per cell.
    Cleared { per_cell: usize },
    /// The packed colour of each agent.
    Colors,
}

/// Stats pass of the reduce leaf, with its first tally among the leaf's.
#[derive(Debug, Clone)]
pub(crate) struct ReducePass {
    /// Document position of the pass.
    pub position: usize,
    /// First tally of the pass among the leaf's tallies.
    pub first_tally: usize,
}

/// Slot of a stat value in its stats pass.
#[derive(Debug, Clone)]
pub(crate) struct StatSlot {
    /// Document position of the stats pass.
    pub position: usize,
    /// Slot of the value in the pass's program.
    pub slot: usize,
}

/// Radius fold of a step pass, whose walk [`AgentGraphHooks::step_cost`] charges.
#[derive(Debug, Clone)]
pub(crate) struct WalkCharge {
    /// Document position of the tick pass that holds the fold.
    pub position: usize,
    /// Slot of the fold's `radius` in the pass's program, a prelude value.
    pub radius: usize,
    /// Cost of one visit of the fold's body.
    pub body: u64,
}

/// Work units of one visit of a radius fold besides its body: the reads of the index and of the other agent's position,
/// its delta and the test of the radius.
const VISIT_COST: u64 = 8;

/// Returns the cells along one side of `side` cells that a walk of `radius` covers over cells `cell` wide, as
/// `henad::hash::walk_bounds` bounds them: `2 * reach + 1`, or the whole side for a reach past half of it.
fn walk_side(radius: f32, cell: f32, side: u32) -> u64 {
    let reach = (radius / cell).ceil();
    // A cast saturates, and a negative reach counts as one cell.
    if !reach.is_finite() || reach as u32 > side.saturating_sub(1) / 2 {
        u64::from(side)
    } else {
        2 * u64::from(reach as u32) + 1
    }
}

/// Plan the hooks of a graph of agents read.
#[derive(Debug)]
pub(crate) struct AgentGraphHooks {
    pub evaluator: Arc<HostEvaluator>,
    /// Default population.
    pub count: u32,
    /// Default width of the world.
    pub width: f32,
    /// Default height of the world.
    pub height: f32,
    /// What each buffer holds, in buffer order.
    pub buffers: Vec<BufferSource>,
    /// Uniform block of every pass.
    pub uniforms: Vec<(AgentPassId, UniformBlock)>,
    /// Source of each stat value, `None` for a stat no stats pass gives, which reads 0.
    pub stats: Vec<Option<StatSlot>>,
    /// Stats passes of the reduce leaf, in order.
    pub reduce: Vec<ReducePass>,
    /// Lanes of the reduce leaf, each by the document position of its pass and the number of its sum.
    pub sum_lanes: Vec<(usize, usize)>,
    /// First word of the leaf's tallies in the counters.
    pub tally_base: usize,
    /// Number of the document's counters, after the fault word and the tick.
    pub counters: usize,
    /// Message of each fault code, the code its position plus one.
    pub faults: Vec<String>,
    /// Document position and slot of the radius folds' cell size, when a tick pass has one.
    pub cell_size: Option<(usize, usize)>,
    /// Radius folds of each step pass, in the order of the spec's steps.
    pub walks: Vec<Vec<WalkCharge>>,
    /// Actions, in document order.
    pub actions: Vec<ActionPlan>,
}

/// Returns `tick` as the 32-bit word a graph reads, wrapping as the shader's tick buffer does.
fn tick32(tick: u64) -> u32 {
    tick as u32
}

/// Returns the world of `geom`.
fn world_of(geom: &Geometry) -> HostWorld {
    HostWorld::Agents {
        count: geom.num_agents,
        width: geom.extent.w,
        height: geom.extent.h,
    }
}

impl AgentGraphHooks {
    /// Returns the values of the graph's own parameters in `params`, after the agents parameters.
    fn own(&self, params: &[ParamValue]) -> Vec<HostValue> {
        self.evaluator
            .param_values(params.get(AGENT_PARAMS..).unwrap_or_default())
    }

    /// Returns the context the host values of pass `id` are evaluated in.
    fn context(id: AgentPassId, run: AgentRun) -> RunContext {
        match id {
            AgentPassId::Action(action) => RunContext::action(run.seed, action, run.presses, tick32(run.tick)),
            _ => RunContext::tick(run.seed, tick32(run.tick)),
        }
    }

    /// Returns the uniform block of pass `id`, or `Err` with the fault of a prelude the host cannot evaluate.
    fn block_bytes(
        &self,
        id: AgentPassId,
        ctx: PassCtx<'_>,
        run: AgentRun,
        params: &[ParamValue],
    ) -> Result<Vec<u8>, String> {
        let Some((_, block)) = self.uniforms.iter().find(|(known, _)| *known == id) else {
            return Ok(vec![0; 16]);
        };
        let dispatch = Dispatch {
            groups_x: ctx.groups_x,
            hash: ctx.geom.index,
            texture: [ctx.geom.display.0, ctx.geom.display.1],
            counting: run.counting,
        };
        block.bytes(
            &self.evaluator,
            world_of(ctx.geom),
            &self.own(params),
            &Self::context(id, run),
            &dispatch,
        )
    }

    /// Returns the size of `pass`'s uniform block.
    fn block_size(&self, id: AgentPassId) -> usize {
        self.uniforms
            .iter()
            .find(|(known, _)| *known == id)
            .map_or(16, |(_, block)| block.size)
    }

    /// Returns the words of buffer `source` for `lanes`, the init's result over `geom`.
    pub(crate) fn words(source: &BufferSource, lanes: &HostLanes, geom: &Geometry) -> Vec<u32> {
        match source {
            BufferSource::Lanes { columns, .. } => columns
                .iter()
                .filter_map(|(column, _)| lanes.columns().get(*column))
                .flat_map(|column| column.words().iter().copied())
                .collect(),
            BufferSource::Field { first, layers } => {
                let first = first.saturating_sub(lanes.columns().len());
                lanes
                    .layers()
                    .iter()
                    .skip(first)
                    .take(*layers)
                    .flat_map(|column| column.words().iter().copied())
                    .collect()
            }
            BufferSource::Cleared { per_cell } => vec![0; geom.n_cells as usize * per_cell],
            BufferSource::Colors => vec![0; geom.num_agents as usize],
        }
    }
}

impl henad_compute::gpu::AgentHooks for AgentGraphHooks {
    fn dims(&self, params: &[ParamValue]) -> (u32, Extent) {
        (
            extract_u32(params, 0, self.count),
            Extent {
                w: extract_f32(params, 1, self.width),
                h: extract_f32(params, 2, self.height),
            },
        )
    }

    fn buffer_lens(&self, geom: &Geometry) -> Vec<usize> {
        let agents = geom.num_agents as usize;
        let cells = geom.n_cells as usize;
        self.buffers
            .iter()
            .map(|source| match source {
                BufferSource::Lanes { columns, domain } => {
                    columns.iter().map(|(_, words)| words).sum::<usize>()
                        * match domain {
                            Domain::Agents => agents,
                            Domain::Cells => cells,
                        }
                }
                BufferSource::Field { layers, .. } => cells * layers,
                BufferSource::Cleared { per_cell } => cells * per_cell,
                BufferSource::Colors => agents,
            })
            .collect()
    }

    /// Runs the init passes on the host.
    ///
    /// # Panics
    ///
    /// Panics with the host's message when an init pass faults. The engine builds inside a scope that reports a panic
    /// as a fault of the build.
    fn seed_buffers(&self, geom: &Geometry, params: &[ParamValue], seed: Option<u64>) -> Vec<Vec<u8>> {
        let own = params.get(AGENT_PARAMS..).unwrap_or_default();
        let lanes = self
            .evaluator
            .init(world_of(geom), own, seed)
            .unwrap_or_else(|error| panic!("{}: {error}", self.evaluator.document().id));
        self.buffers
            .iter()
            .map(|source| match source {
                // A buffer that starts at zero is cleared by the engine.
                BufferSource::Cleared { .. } | BufferSource::Colors => Vec::new(),
                _ => Self::words(source, &lanes, geom)
                    .iter()
                    .flat_map(|word| word.to_le_bytes())
                    .collect(),
            })
            .collect()
    }

    fn index_cell_size(&self, params: &[ParamValue]) -> f32 {
        let Some((position, slot)) = self.cell_size else {
            return 1.0;
        };
        let (count, extent) = self.dims(params);
        let world = HostWorld::Agents {
            count: count.max(1),
            width: extent.w.max(1.0),
            height: extent.h.max(1.0),
        };
        let Some(Ok(program)) = self.evaluator.program_at(position) else {
            return 1.0;
        };
        run::prelude(program, &self.own(params), &RunContext::tick(None, 0), world)
            .ok()
            .and_then(|frame| frame.get(slot)?.as_f32())
            .unwrap_or(1.0)
    }

    /// Returns the block of pass `pass` at tick 0 of the default seed, zeros for one whose prelude faults.
    ///
    /// The engine calls [`Self::run_params_bytes`] instead, which reports the fault.
    fn pass_params_bytes(&self, pass: PassId, ctx: PassCtx<'_>, params: &[ParamValue]) -> Vec<u8> {
        let id = match pass {
            PassId::Step(index) => AgentPassId::Step(index),
            PassId::Display => AgentPassId::Display,
            PassId::Reduce => AgentPassId::Reduce,
            PassId::Action(index) => AgentPassId::Action(index),
        };
        self.run_params_bytes(id, ctx, AgentRun::new(None, 0, 0), params)
            .unwrap_or_else(|_| vec![0; self.block_size(id)])
    }

    fn stats(&self, sums: &[f32], counters: &[u32], geom: &Geometry) -> Vec<StatValue> {
        self.run_stats(sums, counters, geom, AgentRun::new(None, 0, 0), &[])
    }

    /// Returns the block of pass `pass`, or the fault of a prelude the host cannot evaluate, which names its pass and
    /// node.
    fn run_params_bytes(
        &self,
        pass: AgentPassId,
        ctx: PassCtx<'_>,
        run: AgentRun,
        params: &[ParamValue],
    ) -> Result<Vec<u8>, String> {
        self.block_bytes(pass, ctx, run, params)
    }

    fn run_stats(
        &self,
        sums: &[f32],
        counters: &[u32],
        geom: &Geometry,
        run: AgentRun,
        params: &[ParamValue],
    ) -> Vec<StatValue> {
        let own = self.own(params);
        let world = world_of(geom);
        let context = RunContext::tick(run.seed, tick32(run.tick));
        let own_counters: Vec<u32> = (0..self.counters)
            .map(|k| counters.get(FIRST_COUNTER + k).copied().unwrap_or(0))
            .collect();
        let mut frames: Vec<(usize, Option<Vec<HostValue>>)> = Vec::new();
        self.stats
            .iter()
            .map(|source| {
                let Some(source) = source else {
                    return StatValue::Scalar(0.0);
                };
                let index = if let Some(index) = frames.iter().position(|(known, _)| *known == source.position) {
                    index
                } else {
                    let frame = self.finish(source.position, &own, sums, counters, &own_counters, world, &context);
                    frames.push((source.position, frame));
                    frames.len() - 1
                };
                frames[index]
                    .1
                    .as_ref()
                    .and_then(|frame| frame.get(source.slot).copied())
                    .map_or(StatValue::Scalar(f64::NAN), stat_value)
            })
            .collect()
    }

    fn rerun(
        &self,
        action: usize,
        geom: &Geometry,
        run: AgentRun,
        params: &[ParamValue],
        stream: &mut u64,
    ) -> Result<AgentUploads, String> {
        let Some(plan) = self.actions.get(action) else {
            return Ok(Vec::new());
        };
        // The action's pass evaluates its block now, so a fault refuses the press before anything runs.
        if let Some((id, block)) = self.uniforms.iter().find(|(id, _)| *id == AgentPassId::Action(action)) {
            block.bytes(
                &self.evaluator,
                world_of(geom),
                &self.own(params),
                &Self::context(*id, run),
                &Dispatch::default(),
            )?;
        }
        if !plan.reruns {
            return Ok(Vec::new());
        }
        let own = params.get(AGENT_PARAMS..).unwrap_or_default();
        let mut lanes = HostLanes::new(self.evaluator.document(), world_of(geom)).map_err(|error| error.to_string())?;
        let written = self
            .evaluator
            .rerun(&plan.id, run.presses, &mut lanes, own, run.seed, stream)
            .map_err(|error| error.to_string())?;
        Ok(self
            .buffers
            .iter()
            .enumerate()
            .filter(|(_, source)| match source {
                BufferSource::Lanes { columns, .. } => columns.iter().any(|(column, _)| written.contains(column)),
                BufferSource::Field { first, layers } => (*first..first + layers).any(|layer| written.contains(&layer)),
                _ => false,
            })
            .map(|(buffer, source)| (buffer, Self::words(source, &lanes, geom)))
            .collect())
    }

    fn reports_faults(&self) -> bool {
        !self.faults.is_empty()
    }

    /// Returns the walks of the step's radius folds: per fold, the cells it covers, then the agents a uniform spread
    /// puts in them times the cost of a visit, its body's cost plus [`VISIT_COST`].
    ///
    /// A walk past [`crate::limits::MAX_WALK_CELLS`] cells faults before it starts and costs one unit. The visits
    /// assume a uniform spread, at most every agent and at most the visit cap. Agents that gather visit more than
    /// the charge, and the device's caps bound each walk then.
    fn step_cost(&self, step: usize, geom: &Geometry, params: &[ParamValue]) -> u64 {
        let (Some(walks), Some(grid)) = (self.walks.get(step), geom.index) else {
            return 0;
        };
        let own = self.own(params);
        let world = world_of(geom);
        let index_cells = u64::from(grid.grid_w) * u64::from(grid.grid_h);
        let agents = u64::from(geom.num_agents);
        walks
            .iter()
            .map(|walk| {
                let radius = match self.evaluator.program_at(walk.position) {
                    Some(Ok(program)) => run::prelude(program, &own, &RunContext::tick(None, 0), world)
                        .ok()
                        .and_then(|frame| frame.get(walk.radius)?.as_f32()),
                    _ => None,
                };
                // A radius the host cannot read walks every cell.
                let cells = radius.map_or(index_cells, |radius| {
                    walk_side(radius, grid.cell_w, grid.grid_w) * walk_side(radius, grid.cell_h, grid.grid_h)
                });
                if cells > u64::from(crate::limits::MAX_WALK_CELLS) {
                    return 1;
                }
                let visits = agents
                    .saturating_mul(cells)
                    .div_ceil(index_cells.max(1))
                    .min(agents)
                    .min(u64::from(crate::limits::MAX_FOLD_CANDIDATES));
                cells.saturating_add(visits.saturating_mul(walk.body.saturating_add(VISIT_COST)))
            })
            .fold(0, u64::saturating_add)
    }

    fn fault(&self, counters: &[u32]) -> Option<String> {
        let code = *counters.get(FAULT_WORD)?;
        if code == 0 {
            return None;
        }
        Some(
            self.faults
                .get(code as usize - 1)
                .cloned()
                .unwrap_or_else(|| format!("a shader reported the fault {code}, which the compiler never wrote")),
        )
    }
}

impl AgentGraphHooks {
    /// Returns the frame of the stats pass at `position` after its post block, with the graph's parameter values `own`,
    /// from the reduce leaf's `sums` and the engine's `counters`.
    #[expect(clippy::too_many_arguments, reason = "each argument is one input of the post block")]
    fn finish(
        &self,
        position: usize,
        own: &[HostValue],
        sums: &[f32],
        counters: &[u32],
        own_counters: &[u32],
        world: HostWorld,
        context: &RunContext,
    ) -> Option<Vec<HostValue>> {
        let Some(Ok(program)) = self.evaluator.program_at(position) else {
            return None;
        };
        let first_tally = self.reduce.iter().find(|pass| pass.position == position)?.first_tally;
        let tallies = (0..program.tallies)
            .map(|k| u64::from(counters.get(self.tally_base + first_tally + k).copied().unwrap_or(0)))
            .collect();
        let pass_sums = (0..program.sums.len())
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
                        .map_or(0.0, |&x| f64::from(x))
                };
                ([value(0), value(1)], lanes.len() > 1)
            })
            .collect();
        let readback = run::Readback {
            tallies,
            sums: pass_sums,
            counters: own_counters.to_vec(),
        };
        run::finish(program, own, context, world, Some(&readback)).ok()
    }
}
