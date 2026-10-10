//! Grid rules: GPU grid models declared as data, with a step written in WGSL and composed while the program runs.
//!
//! A [`GridRule`] holds what a compiled GPU grid model declares in Rust: its id and name, its parameters, its states
//! with their colours and initial shares, the grid's size and boundary, and the text of its step. The text defines
//! `fn step_cell(cell: u32, rng: ptr<function, u32>) -> u32`, the function that returns the next state of one cell. It
//! can define helper functions and constants, and import from the shared modules with
//! `#import henad::<module>::{...}`.
//!
//! [`GridRule::compile`] composes the text after an engine *template* that declares what the step reads:
//!
//! - `params`, a uniform of the struct `Params` with the fields `width`, `height` and `seed`, then one field per
//!   parameter of the rule, named by its id. An `f32` parameter's field is an `f32`, and every other kind is a `u32`:
//!   a `u32` value, `1` or `0` for a `bool`, or the index of a choice.
//! - `neighbor(dx, dy)`, the state of the cell `dx` east and `dy` south. Past the edge of a bounded grid it reads 0.
//! - `moore_count(state)` and `von_neumann_count(state)`, the neighbours in `state` among the eight around the cell and
//!   among the four beside it.
//! - `NUM_STATES`, and one constant per state numbered in declaration order, named from its label in upper snake case:
//!   `Dead` gives `DEAD`, and `Infected cells` gives `INFECTED_CELLS`.
//!
//! `rng` starts each step from `pcg_hash(seed ^ pcg_hash(index ^ pcg_hash(tick)))`, a hash of the run's seed, the
//! cell's index and the tick, and the step draws from it with `henad::rng`. The engine owns the rest of the model. It
//! clamps a result past the last state to the last state, colours each cell by its state, and counts the cells in
//! each state, one stat per state. It seeds the cells on the CPU from the run's seed: each state with an initial share
//! takes that share of the cells, and the first state without one takes the rest.
//!
//! A rule's step runs once per cell and writes only that cell. [`GridRule::compile`] refuses text that could do
//! otherwise, at the line that does: a loop, a binding, an entry point, an atomic, a barrier, a module-scope variable
//! outside `private`, any use of an engine buffer other than `params`, a write to memory the cell does not own, a name
//! the template declares, and a module-scope item named as a WGSL built-in. Variables keep to WGSL's limits of 8192
//! bytes for the locals of a function and for the `private` variables of the shader. With no loop, and with a call of
//! `step_cell` limited to 65536 expressions and statements, a step's work is bounded.

mod checks;
mod template;

use std::borrow::Cow;
use std::fmt;
use std::sync::Arc;

use henad_core::authoring::primitives::rng::{below, mix_seed, next_bits};
use henad_core::authoring::primitives::space::Boundary;
use henad_core::diagnostic::CodeDiagnostic;
use henad_core::helpers::{extract_u32, f32_param, u32_param};
use henad_core::metadata::Structure;
use henad_core::params::{ParamDescriptor, ParamValue};
use henad_core::provenance::{ModelCode, ModelSource};
use henad_core::topology::TopologyHint;
use henad_core::view::{StatDescriptor, StatValue};

use crate::cpu::field::{GRID_INIT_SEED, grid_init_rng};
use crate::entry::{EntryDeclarations, ModelEntry};
use crate::gpu::compose::{BEFORE_TEXT_NOTE, Composition, WgslFile, compose, compose_wgsl};
use crate::gpu::grid_spec::{GpuGridSpec, GridHooks, GridShaderSpec};

use checks::Declared;
use template::FieldType;

/// Largest text of a rule's step, in bytes.
pub const MAX_STEP_BYTES: usize = 64 * 1024;

/// Most states a rule declares.
pub const MAX_STATES: usize = 256;

/// Most parameters a rule declares, besides the grid's width and height.
pub const MAX_PARAMS: usize = 64;

/// Largest side of a rule's grid, in cells.
pub const MAX_GRID_SIDE: u32 = 16_384;

/// Step of Conway's Game of Life, the text of [`GridRule::life`].
pub const LIFE_STEP: &str = include_str!("life_step.wgsl");

/// Domain separator of a rule's random stream, so the step and the cell seeding do not start correlated.
const STEP_SEED: u64 = GRID_INIT_SEED ^ 0x0F5E_ED0F_5EED_0F5E;

/// Number of grid parameters the engine puts in front of a rule's own.
const GRID_PARAMS: usize = 2;

/// A GPU grid model held as data, with a step written in WGSL.
///
/// Build one with [`Self::new`] or [`Self::life`] and set its fields. [`Self::compile`] checks and composes it.
#[derive(Clone)]
#[non_exhaustive]
pub struct GridRule {
    /// Id of the model, in the model id grammar.
    pub id: String,
    /// Name shown in the UI.
    pub name: String,
    /// One-line description shown in the UI.
    pub description: String,
    /// Parameters of the rule. The engine declares the grid's width and height in front of them.
    ///
    /// A GPU grid model reads every parameter when it builds, and each applies on reload.
    pub params: Vec<ParamDescriptor>,
    /// States a cell can take, numbered from 0 in this order.
    pub states: Vec<RuleState>,
    /// Behaviour of `neighbor` at the grid's edge.
    pub boundary: Boundary,
    /// Default grid width, in cells.
    pub width: u32,
    /// Default grid height, in cells.
    pub height: u32,
    /// WGSL text of the step. It defines `step_cell`.
    pub step: String,
}

/// Prints the id, the counts of parameters and states, and the byte length of the step.
impl fmt::Debug for GridRule {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("GridRule")
            .field("id", &self.id)
            .field("name", &self.name)
            .field("params", &self.params.len())
            .field("states", &self.states.len())
            .field("boundary", &self.boundary)
            .field("width", &self.width)
            .field("height", &self.height)
            .field("step_bytes", &self.step.len())
            .finish_non_exhaustive()
    }
}

/// One state of a [`GridRule`].
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct RuleState {
    /// Name shown in the UI. It also names the state's constant in the step.
    pub label: String,
    /// RGBA colour of a cell in the state, and of its stat.
    pub color: [u8; 4],
    /// Id of the `f32` parameter whose value is the state's initial share of the cells, or `None` for the state that
    /// takes the cells no share claims.
    pub initial: Option<String>,
}

impl RuleState {
    /// Returns a state with `label` and `color` and no initial share.
    pub fn new(label: impl Into<String>, color: [u8; 4]) -> Self {
        Self {
            label: label.into(),
            color,
            initial: None,
        }
    }

    /// Returns the state with its initial share read from the `f32` parameter `param`.
    pub fn with_initial(mut self, param: impl Into<String>) -> Self {
        self.initial = Some(param.into());
        self
    }
}

/// A declaration of a [`GridRule`] that a problem is about.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum RuleDeclaration {
    /// The rule's id.
    Id,
    /// The grid's default width and height.
    Grid,
    /// The state at this index of [`GridRule::states`].
    State(usize),
    /// The parameter at this index of [`GridRule::params`].
    Param(usize),
}

impl GridRule {
    /// Format of the [`ModelCode`] that identifies a rule, with the version of the engine template.
    ///
    /// The version goes up whenever the template changes what a rule's text does, so a results folder never takes a
    /// rule composed with one template for the same code as one composed with another.
    pub const CODE_FORMAT: &'static str = "henad-grid-rule/1";

    /// Returns a rule with id `id`, the name `id`, no description, parameter or state, a 256 by 256 torus, and `step`
    /// as its text.
    pub fn new(id: impl Into<String>, step: impl Into<String>) -> Self {
        let id = id.into();
        Self {
            name: id.clone(),
            id,
            description: String::new(),
            params: Vec::new(),
            states: Vec::new(),
            boundary: Boundary::Torus,
            width: 256,
            height: 256,
            step: step.into(),
        }
    }

    /// Returns Conway's Game of Life as a rule: `Dead`, then `Alive` with the initial share `density` at 0.3, on a 256
    /// by 256 torus, with [`LIFE_STEP`] as its text.
    ///
    /// At the same size, density and seed, it starts from the cells of the example model `game_of_life`, and steps
    /// through the same cells after it.
    pub fn life() -> Self {
        Self {
            name: "Life rule".to_owned(),
            description: "Conway's Game of Life, written as a grid rule".to_owned(),
            params: vec![f32_param("density", "Initial Density", 0.3, 0.0, 1.0, Some(0.01))],
            states: vec![
                RuleState::new("Dead", [0x15, 0x15, 0x15, 0xFF]),
                RuleState::new("Alive", [0x00, 0xE6, 0x76, 0xFF]).with_initial("density"),
            ],
            ..Self::new("life_rule", LIFE_STEP)
        }
    }

    /// Returns every parameter of the model the rule builds: `grid_width` and `grid_height`, then the rule's own,
    /// each applying on reload.
    pub fn param_descriptors(&self) -> Vec<ParamDescriptor> {
        let mut params = vec![
            u32_param("grid_width", "Grid Width", self.width, 1, MAX_GRID_SIDE).on_reload(),
            u32_param("grid_height", "Grid Height", self.height, 1, MAX_GRID_SIDE).on_reload(),
        ];
        params.extend(self.params.iter().cloned().map(ParamDescriptor::on_reload));
        params
    }

    /// Checks the rule's declarations: its id, its grid's size, its states and its parameters.
    ///
    /// [`Self::compile`] runs the same checks first. A caller that holds the declarations' places, as a model file
    /// does, places each problem by its [`RuleDeclaration`].
    ///
    /// # Errors
    ///
    /// Returns every problem found, each with the declaration it is about, or `None` for a problem of the rule as a
    /// whole, such as a count of states. No diagnostic has a place.
    pub fn check_declarations(&self) -> Result<(), Vec<(Option<RuleDeclaration>, CodeDiagnostic)>> {
        checks::check_declarations(self).map(|_| ())
    }

    /// Checks the rule and composes its step, display and reduce shaders.
    ///
    /// The step is composed after the engine template, checked by naga against the WebGPU baseline, and held to the
    /// rules of the module docs. Nothing here needs a device.
    ///
    /// # Errors
    ///
    /// Returns every problem found in the declarations, or else every problem found in the text. A problem in the
    /// text has a span, a line and a column in [`Self::step`] when it has a place there. A problem in the template has
    /// none, and a note says so. No diagnostic names a file.
    pub fn compile(&self) -> Result<CompiledGridRule, Vec<CodeDiagnostic>> {
        let declared = checks::check_declarations(self)
            .map_err(|problems| problems.into_iter().map(|(_, problem)| problem).collect::<Vec<_>>())?;
        let problems = checks::check_text(&self.step, &declared.constants);
        if !problems.is_empty() {
            return Err(problems);
        }
        let (composition, start_offset) = self.compose_step(&declared)?;
        let problems = checks::check_module(&composition, start_offset);
        if !problems.is_empty() {
            return Err(problems);
        }
        let step = composition.write()?;
        let palette: Vec<[u8; 4]> = self.states.iter().map(|state| state.color).collect();
        let engine_shader = |text: String| {
            compose_wgsl(
                WgslFile {
                    path: "grid rule",
                    text: &text,
                },
                &[],
            )
            .map(crate::gpu::compose::ComposedWgsl::into_text)
            .map_err(|problems| {
                problems
                    .into_iter()
                    .map(|mut problem| {
                        // The engine's text is no file of the rule's, and a place in it would read as one.
                        problem.file = None;
                        problem.span = None;
                        problem.line = None;
                        problem.column = None;
                        problem.with_note("in a shader the engine writes for every rule")
                    })
                    .collect::<Vec<_>>()
            })
        };
        let display = engine_shader(template::display_shader(&palette))?;
        let reduce = engine_shader(template::reduce_shader(self.states.len()))?;
        Ok(CompiledGridRule {
            parts: Arc::new(CompiledParts {
                rule: self.clone(),
                declared,
                step,
                display,
                reduce,
            }),
        })
    }
}

impl GridRule {
    /// Returns the rule's step composed after its template, and the offset of the template in the module's spans.
    ///
    /// # Errors
    ///
    /// Returns the problems of the composition, each without a file.
    fn compose_step(&self, declared: &Declared) -> Result<(Composition, usize), Vec<CodeDiagnostic>> {
        let fields: Vec<(&str, FieldType)> = declared
            .fields
            .iter()
            .map(|(name, field)| (name.as_str(), *field))
            .collect();
        let template = template::step_template(&fields, &declared.constants, self.boundary);
        let entry = format!("{}{}", template.text, self.step);
        // The rule has no file. A problem in the template is most often a `step_cell` that the template cannot call.
        let without_file = |problems: Vec<CodeDiagnostic>| -> Vec<CodeDiagnostic> {
            problems
                .into_iter()
                .map(|mut problem| {
                    problem.file = None;
                    if problem.line.is_none() && problem.notes.iter().any(|note| note == BEFORE_TEXT_NOTE) {
                        problem = problem.with_note(format!("the engine calls `{}`", checks::STEP_SIGNATURE));
                    }
                    problem
                })
                .collect()
        };
        let composition = compose(
            WgslFile {
                path: &self.id,
                text: &entry,
            },
            &[],
            template.text.len(),
        )
        .map_err(without_file)?;
        let start_offset = composition
            .entry_start_offset(template::ANCHOR, template.anchor_offset)
            .ok_or_else(|| {
                vec![CodeDiagnostic::error(
                    "the composer's positions in the rule could not be read back, and the rule cannot be checked",
                )]
            })?;
        Ok((composition, start_offset))
    }
}

/// A [`GridRule`] that passed its checks, with its three shaders composed.
///
/// Cheap to clone. Clones share the rule and the shaders.
#[derive(Clone)]
pub struct CompiledGridRule {
    parts: Arc<CompiledParts>,
}

struct CompiledParts {
    rule: GridRule,
    declared: Declared,
    step: String,
    display: String,
    reduce: String,
}

/// Prints the rule's id and the byte lengths of the composed shaders.
impl fmt::Debug for CompiledGridRule {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CompiledGridRule")
            .field("id", &self.parts.rule.id)
            .field("step_bytes", &self.parts.step.len())
            .field("display_bytes", &self.parts.display.len())
            .field("reduce_bytes", &self.parts.reduce.len())
            .finish_non_exhaustive()
    }
}

impl CompiledGridRule {
    /// Rule the shaders were composed from.
    pub fn rule(&self) -> &GridRule {
        &self.parts.rule
    }

    /// Composed step shader, the template and the rule's text, as naga writes it back.
    pub fn step_wgsl(&self) -> &str {
        &self.parts.step
    }

    /// Returns an entry that builds the rule on any device it is handed.
    ///
    /// `code` identifies the rule's code in a results folder: its canonical text, as
    /// `henad_explore::model_file::ModelFile::code` returns it. The entry's source is a runtime source labelled with
    /// the rule's id. The entry holds the composed shaders, and a build reads no file.
    pub fn entry(&self, code: ModelCode) -> ModelEntry {
        let parts = &*self.parts;
        let rule = &parts.rule;
        let stats: Vec<StatDescriptor> = rule
            .states
            .iter()
            .map(|state| StatDescriptor::owned(state.label.clone(), state.color))
            .collect();
        let declarations = EntryDeclarations {
            id: rule.id.clone(),
            name: rule.name.clone(),
            description: rule.description.clone(),
            params: rule.param_descriptors(),
            stats: stats.clone(),
            actions: Vec::new(),
            topology_hint: TopologyHint::GRID,
            structure: Structure::GpuGrid {
                buffers: template::BUFFERS,
                workgroup: template::WORKGROUP_SIZE,
            },
            palette: Some(Cow::Owned(rule.states.iter().map(|state| state.color).collect())),
            replays_exactly: true,
        };
        let spec = Arc::new(GpuGridSpec {
            id: Cow::Owned(rule.id.clone()),
            workgroup_size: template::WORKGROUP_SIZE,
            buffers: template::BUFFERS,
            step: GridShaderSpec {
                source: Cow::Owned(parts.step.clone()),
                bindings: Cow::Borrowed(template::STEP_BINDINGS),
            },
            display: GridShaderSpec {
                source: Cow::Owned(parts.display.clone()),
                bindings: Cow::Borrowed(template::DISPLAY_BINDINGS),
            },
            reduce: GridShaderSpec {
                source: Cow::Owned(parts.reduce.clone()),
                bindings: Cow::Borrowed(template::REDUCE_BINDINGS),
            },
            actions: Vec::new(),
            stats: Cow::Owned(stats),
            hooks: Box::new(self.hooks()),
        });
        ModelEntry::from_grid_spec(declarations, ModelSource::runtime(&rule.id, code), spec)
    }

    /// Returns the functions the engine calls for the rule's sizes, seeds, uniform block and stats.
    pub(crate) fn hooks(&self) -> RuleHooks {
        let rule = &self.parts.rule;
        RuleHooks {
            width: rule.width,
            height: rule.height,
            params: rule.params.clone(),
            shares: self.parts.declared.shares.clone(),
            rest: self.parts.declared.rest,
        }
    }
}

/// Functions of a rule's model, read from its declarations.
pub(crate) struct RuleHooks {
    width: u32,
    height: u32,
    /// The rule's own parameters, after the grid's two.
    params: Vec<ParamDescriptor>,
    shares: Vec<(u32, usize)>,
    rest: u32,
}

impl RuleHooks {
    /// Returns the value of the rule's parameter at `position` among its own, or its default when `params` lacks it.
    fn value(&self, params: &[ParamValue], position: usize) -> ParamValue {
        params
            .get(GRID_PARAMS + position)
            .cloned()
            .unwrap_or_else(|| self.params[position].kind().default_value())
    }

    /// Returns the initial state of each cell, drawn in row-major order from `grid_init_rng(seed)`.
    ///
    /// Each cell draws one word with `next_bits`. The states with an initial share take the low ranges of the words
    /// in declaration order, the upper bound of each the running total of the shares, summed in `f32`, times
    /// `u32::MAX`. A word below a bound takes that state, and a word below none takes the rest state.
    fn seed_cells(&self, width: u32, height: u32, params: &[ParamValue], seed: Option<u64>) -> Vec<u32> {
        let mut total = 0.0f32;
        let bounds: Vec<(u32, u32)> = self
            .shares
            .iter()
            .map(|&(state, position)| {
                let share = match self.value(params, position) {
                    ParamValue::F32(share) => share.max(0.0),
                    _ => 0.0,
                };
                total += share;
                ((total * u32::MAX as f32) as u32, state)
            })
            .collect();
        let mut rng = grid_init_rng(seed);
        (0..(width as usize) * (height as usize))
            .map(|_| {
                let bits = next_bits(&mut rng);
                bounds
                    .iter()
                    .find(|(bound, _)| below(bits, *bound))
                    .map_or(self.rest, |&(_, state)| state)
            })
            .collect()
    }
}

impl GridHooks for RuleHooks {
    fn dims(&self, params: &[ParamValue]) -> (u32, u32) {
        (extract_u32(params, 0, self.width), extract_u32(params, 1, self.height))
    }

    fn buffer_lens(&self, width: u32, height: u32) -> Vec<usize> {
        vec![(width as usize) * (height as usize), 1]
    }

    fn step_dims(&self, width: u32, height: u32) -> (u32, u32) {
        (width, height)
    }

    fn seed_buffers(&self, width: u32, height: u32, params: &[ParamValue], seed: Option<u64>) -> Vec<Vec<u32>> {
        vec![self.seed_cells(width, height, params, seed), vec![0]]
    }

    /// Returns `width`, `height`, a word of the seed, and one word per parameter, padded to a multiple of 16 bytes.
    fn step_params_bytes(&self, width: u32, height: u32, params: &[ParamValue], seed: Option<u64>) -> Vec<u8> {
        let mixed = seed.map_or(STEP_SEED, |seed| mix_seed(seed ^ STEP_SEED));
        let mut words = vec![width, height, (mixed ^ (mixed >> 32)) as u32];
        words.extend(
            (0..self.params.len()).map(|position| match self.value(params, position) {
                ParamValue::F32(value) => value.to_bits(),
                ParamValue::U32(value) => value,
                ParamValue::Bool(value) => u32::from(value),
                ParamValue::Choice(index) => u32::try_from(index).unwrap_or(u32::MAX),
            }),
        );
        let mut bytes: Vec<u8> = words.iter().flat_map(|word| word.to_le_bytes()).collect();
        bytes.resize(bytes.len().next_multiple_of(16), 0);
        bytes
    }

    /// Returns nothing. A rule declares no action.
    fn action_params_bytes(
        &self,
        _action: usize,
        _width: u32,
        _height: u32,
        _params: &[ParamValue],
        _seed: u32,
    ) -> Vec<u8> {
        Vec::new()
    }

    fn stats(&self, counts: &[u32]) -> Vec<StatValue> {
        counts
            .iter()
            .map(|&count| StatValue::Scalar(f64::from(count)))
            .collect()
    }
}

#[cfg(test)]
pub(crate) mod test_support {
    //! Items of this module that the tests under `gpu/tests` reach.

    use henad_core::diagnostic::CodeDiagnostic;

    pub(crate) use super::checks::{MAX_FUNCTION_BYTES, MAX_PRIVATE_BYTES, MAX_STEP_COST};
    pub(crate) use super::template::{DISPLAY_BINDINGS, REDUCE_BINDINGS, STEP_BINDINGS};

    use super::{GridRule, checks};
    use crate::gpu::compose::Composition;

    /// Returns the cost of one call of each function of `module`.
    pub(crate) fn costs(module: &naga::Module) -> Vec<u64> {
        checks::function_costs(module)
    }

    /// Returns the composed display shader of `rule`.
    pub(crate) fn display_wgsl(rule: &super::CompiledGridRule) -> &str {
        &rule.parts.display
    }

    /// Returns the composed reduce shader of `rule`.
    pub(crate) fn reduce_wgsl(rule: &super::CompiledGridRule) -> &str {
        &rule.parts.reduce
    }

    /// Returns the composition of `rule`'s step after its template, and the template's offset in its spans.
    ///
    /// # Errors
    ///
    /// Returns the problems of the declarations or of the composition.
    pub(crate) fn composed(rule: &GridRule) -> Result<(Composition, usize), Vec<CodeDiagnostic>> {
        let declared = checks::check_declarations(rule)
            .map_err(|problems| problems.into_iter().map(|(_, problem)| problem).collect::<Vec<_>>())?;
        rule.compose_step(&declared)
    }
}
