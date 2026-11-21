//! Resources a model would allocate, checked against the device before anything is created.
//!
//! Over budget, wgpu panics on the UI thread with a message that refers to a bind group. Checking first lets the
//! message refer to the model's own buffer instead.

use henad_core::helpers::fmt_bytes;

use crate::display_scale::display_dims;
use crate::gpu::MAX_STEPS_PER_SUBMISSION;
use crate::gpu::shader_spec::ShaderBinding;

/// Returns the number of bindings that count against `max_storage_buffers_per_shader_stage`. A uniform and a storage
/// texture each count against a separate limit.
pub(crate) fn storage_bindings(bindings: &[ShaderBinding]) -> u32 {
    bindings
        .iter()
        .filter(|binding| binding.kind.is_storage_buffer())
        .count() as u32
}

/// One buffer a model would allocate, labelled as wgpu would label it so a message points at something greppable.
#[derive(Debug)]
pub struct Alloc {
    /// Label of the buffer.
    pub label: String,
    /// Size in bytes.
    pub bytes: u64,
}

/// Kind of a pass, which decides how often one submission runs it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum PassKind {
    /// Pass of a step, run once per step.
    Step,
    /// Display pass, run once per snapshot.
    Display,
    /// Pass of the stat reduction, run once per snapshot or stats read.
    Reduce,
    /// Pass of the action at this position, run once per press in a submission of its own.
    Action(usize),
}

/// Storage buffers one pass binds, counted against `max_storage_buffers_per_shader_stage`, and its work.
#[derive(Debug)]
pub struct PassBindings {
    /// Label of the pass.
    pub label: String,
    /// Kind of the pass.
    pub kind: PassKind,
    /// Number of storage buffers the pass binds.
    pub storage: u32,
    /// Work of one run of the pass, in work units: its cost per invocation times its invocations, counting texels for a
    /// display pass. Zero when the cost is unknown, as for a hand-written shader.
    pub work: u64,
    /// Size in bytes of the pass's uniform block, counted against `max_uniform_buffer_binding_size`. Zero when unknown,
    /// as for a hand-written shader, and [`Demand::push_pass`] leaves it at zero.
    pub uniform_bytes: u64,
    /// Bytes of the pass's `var<workgroup>` declarations, counted against `max_compute_workgroup_storage_size`. Zero
    /// when unknown, as for a hand-written shader, and [`Demand::push_pass`] leaves it at zero.
    pub workgroup_bytes: u64,
}

/// Work of one run on the host, such as a graph model's init passes at build.
#[derive(Debug)]
pub struct HostWork {
    /// Label of the run.
    pub label: String,
    /// Work of the run in work units: the cost per element of its passes times their elements.
    pub work: u64,
}

/// Work units one command buffer may hold: [`MAX_STEPS_PER_SUBMISSION`] times the work of a step, plus the display
/// and stat passes, or the passes of one press.
///
/// A command buffer that runs too long trips the OS GPU watchdog, with no error, and every later readback reads zero.
/// The budget lowers that risk on a slow GPU and does not remove it. It admits every grid corpus file at 16384 by
/// 16384 cells.
pub const SUBMISSION_WORK_BUDGET: u64 = 600_000_000_000;

/// Work units one run on the host may take, the init passes at build or a rerun on a press.
///
/// A run past it would stall the build thread for minutes. It admits the init of every grid corpus file at 16384 by
/// 16384 cells.
pub const HOST_WORK_BUDGET: u64 = 4_000_000_000;

/// Allocations and passes a model would produce for one set of params.
///
/// Reduction intermediates and the engine's own index and scan passes are left out: both are fixed
/// rather than declared, and both are negligible next to what they sit beside.
#[derive(Debug, Default)]
pub struct Demand {
    /// Buffers the model would allocate.
    pub buffers: Vec<Alloc>,
    /// Size of the display texture, already capped by [`crate::display_scale`].
    pub texture: Option<(u32, u32)>,
    /// Storage bindings and work of each pass.
    pub passes: Vec<PassBindings>,
    /// Runs on the host, each with its work.
    pub host: Vec<HostWork>,
}

impl Demand {
    /// Records buffer `label` of `words` `u32` words.
    pub fn push(&mut self, label: String, words: usize) {
        self.buffers.push(Alloc {
            label,
            bytes: words as u64 * std::mem::size_of::<u32>() as u64,
        });
    }

    /// Records side `_a` of buffer `label`, and side `_b` as well when `doubled` is set.
    pub fn push_sides(&mut self, label: &str, words: usize, doubled: bool) {
        self.push(format!("{label}_a"), words);
        if doubled {
            self.push(format!("{label}_b"), words);
        }
    }

    /// Records the five tables a [`GpuSpatialHash`](crate::gpu::GpuSpatialHash) of `n_cells` cells rebuilds every
    /// tick for `num_agents` agents.
    pub fn push_index(&mut self, label: &str, n_cells: u32, num_agents: u32) {
        let table = n_cells as usize + 1;
        for name in ["counts", "cell_start", "cursor"] {
            self.push(format!("{label}_hash_{name}"), table);
        }
        for name in ["agent_cell", "sorted"] {
            self.push(format!("{label}_hash_{name}"), num_agents as usize);
        }
    }

    /// Records the display texture of a `width` by `height` grid, capped for `limits` as the engine would create it.
    pub fn set_display(&mut self, width: u32, height: u32, limits: &wgpu::Limits) {
        self.texture = Some(display_dims(width, height, limits.max_texture_dimension_2d));
    }

    /// Records pass `label` of kind `kind`, binding `storage` storage buffers and running `work` work units, and
    /// returns its record, whose memory sizes start at zero.
    pub fn push_pass(&mut self, label: String, kind: PassKind, storage: u32, work: u64) -> &mut PassBindings {
        self.passes.push(PassBindings {
            label,
            kind,
            storage,
            work,
            uniform_bytes: 0,
            workgroup_bytes: 0,
        });
        let last = self.passes.len() - 1;
        &mut self.passes[last]
    }

    /// Records run `label` on the host, of `work` work units.
    pub fn push_host(&mut self, label: String, work: u64) {
        self.host.push(HostWork { label, work });
    }

    /// Device bytes the model would occupy.
    pub fn bytes(&self) -> u64 {
        let buffers: u64 = self.buffers.iter().map(|alloc| alloc.bytes).sum();
        let texture = self
            .texture
            .map_or(0, |(w, h)| u64::from(w) * u64::from(h) * RGBA_BYTES);
        buffers + texture
    }

    /// Returns the work units of the heaviest submission: [`MAX_STEPS_PER_SUBMISSION`] steps with the display and
    /// stat passes, or the passes of one press, whichever is more.
    pub fn submission_work(&self) -> u64 {
        self.submissions().map(|(work, _)| work).max().unwrap_or(0)
    }

    /// Returns each kind of submission with its work and its heaviest pass: the batch of steps first, then one per
    /// action.
    fn submissions(&self) -> impl Iterator<Item = (u64, Option<&PassBindings>)> {
        let steps = MAX_STEPS_PER_SUBMISSION as u64;
        let batch = self.passes.iter().fold(0u64, |total, pass| match pass.kind {
            PassKind::Step => total.saturating_add(pass.work.saturating_mul(steps)),
            PassKind::Display | PassKind::Reduce => total.saturating_add(pass.work),
            PassKind::Action(_) => total,
        });
        // Reversed, so that of two equal passes the first is the heaviest.
        let heaviest = self
            .passes
            .iter()
            .rev()
            .filter(|pass| !matches!(pass.kind, PassKind::Action(_)))
            .max_by_key(|pass| match pass.kind {
                PassKind::Step => pass.work.saturating_mul(steps),
                _ => pass.work,
            });
        let actions = self.passes.iter().filter_map(|pass| match pass.kind {
            PassKind::Action(index) => Some(index),
            _ => None,
        });
        let mut indices: Vec<usize> = actions.collect();
        indices.sort_unstable();
        indices.dedup();
        let presses = indices.into_iter().map(move |index| {
            let mine = || {
                self.passes
                    .iter()
                    .filter(move |pass| pass.kind == PassKind::Action(index))
            };
            let work = mine().fold(0u64, |total, pass| total.saturating_add(pass.work));
            (work, mine().rev().max_by_key(|pass| pass.work))
        });
        std::iter::once((batch, heaviest)).chain(presses)
    }

    /// Returns the reasons `limits` cannot host the model, empty when it can.
    ///
    /// Buffers of one size share a line. A ping-ponged buffer's two sides would otherwise say the same
    /// sentence twice.
    pub fn shortfalls(&self, limits: &wgpu::Limits) -> Vec<String> {
        let mut lines = self.size_shortfalls(limits);
        let allowed = limits.max_storage_buffers_per_shader_stage;
        lines.extend(self.passes.iter().filter(|pass| pass.storage > allowed).map(|pass| {
            format!(
                "pass '{}' binds {} storage buffers, exceeding the {allowed} this device allows per shader stage",
                pass.label, pass.storage
            )
        }));
        lines.extend(self.memory_shortfalls(limits));
        lines.extend(self.work_shortfalls());
        lines
    }

    /// Returns a line for each pass whose uniform block or workgroup memory is past the limit of `limits`.
    fn memory_shortfalls(&self, limits: &wgpu::Limits) -> Vec<String> {
        let uniform = limits.max_uniform_buffer_binding_size;
        let workgroup = u64::from(limits.max_compute_workgroup_storage_size);
        let mut lines = Vec::new();
        for pass in &self.passes {
            if pass.uniform_bytes > uniform {
                lines.push(format!(
                    "pass '{}' reads a uniform block of {} bytes, exceeding the {uniform} this device allows for one \
                     uniform binding",
                    pass.label, pass.uniform_bytes
                ));
            }
            if pass.workgroup_bytes > workgroup {
                lines.push(format!(
                    "pass '{}' declares {} bytes of workgroup memory, exceeding the {workgroup} this device allows",
                    pass.label, pass.workgroup_bytes
                ));
            }
        }
        lines
    }

    /// Returns a line for each submission and each run on the host past its budget.
    fn work_shortfalls(&self) -> Vec<String> {
        let mut lines: Vec<String> = self
            .submissions()
            .filter(|&(work, _)| work > SUBMISSION_WORK_BUDGET)
            .map(|(work, heaviest)| {
                let mut line = format!(
                    "one submission runs {} units of work, exceeding the budget of {}",
                    fmt_units(work),
                    fmt_units(SUBMISSION_WORK_BUDGET)
                );
                if let Some(pass) = heaviest {
                    let per_run = match pass.kind {
                        PassKind::Step => " per step",
                        _ => "",
                    };
                    line.push_str(&format!(
                        ", and pass '{}' is the heaviest at {}{per_run}",
                        pass.label,
                        fmt_units(pass.work)
                    ));
                }
                line
            })
            .collect();
        lines.extend(self.host.iter().filter(|run| run.work > HOST_WORK_BUDGET).map(|run| {
            format!(
                "'{}' runs {} units of work on the host, exceeding the budget of {}",
                run.label,
                fmt_units(run.work),
                fmt_units(HOST_WORK_BUDGET)
            )
        }));
        lines
    }

    fn size_shortfalls(&self, limits: &wgpu::Limits) -> Vec<String> {
        let mut groups: Vec<Group<'_>> = Vec::new();
        for alloc in &self.buffers {
            // Binding first, since it is the tighter of the two everywhere seen so far.
            let (limit, what) = if alloc.bytes > limits.max_storage_buffer_binding_size {
                (limits.max_storage_buffer_binding_size, "storage binding")
            } else if alloc.bytes > limits.max_buffer_size {
                (limits.max_buffer_size, "buffer")
            } else {
                continue;
            };
            match groups
                .iter_mut()
                .find(|group| group.bytes == alloc.bytes && group.limit == limit)
            {
                Some(group) => group.count += 1,
                None => groups.push(Group {
                    bytes: alloc.bytes,
                    limit,
                    what,
                    first: &alloc.label,
                    count: 1,
                }),
            }
        }
        groups.iter().map(Group::message).collect()
    }
}

const RGBA_BYTES: u64 = 4;

/// Returns `units` in decimal digits with a comma between each group of three.
fn fmt_units(units: u64) -> String {
    let digits = units.to_string();
    let mut grouped = String::with_capacity(digits.len() + digits.len() / 3);
    for (position, digit) in digits.chars().enumerate() {
        if position > 0 && (digits.len() - position).is_multiple_of(3) {
            grouped.push(',');
        }
        grouped.push(digit);
    }
    grouped
}

/// Buffers of one size missing one limit, reported as a single line.
struct Group<'a> {
    bytes: u64,
    limit: u64,
    what: &'static str,
    first: &'a str,
    count: usize,
}

impl Group<'_> {
    fn message(&self) -> String {
        let subject = match self.count {
            1 => format!("'{}' needs", self.first),
            n => format!("'{}' and {} more each need", self.first, n - 1),
        };
        format!(
            "{subject} {}, exceeding the {} this device allows for one {}",
            fmt_bytes(self.bytes),
            fmt_bytes(self.limit),
            self.what
        )
    }
}

#[cfg(test)]
mod tests {
    use super::{Demand, HOST_WORK_BUDGET, PassKind, SUBMISSION_WORK_BUDGET};
    use crate::gpu::MAX_STEPS_PER_SUBMISSION;

    fn limits(binding: u64, buffer: u64) -> wgpu::Limits {
        wgpu::Limits {
            max_storage_buffer_binding_size: binding,
            max_buffer_size: buffer,
            ..wgpu::Limits::default()
        }
    }

    /// A model that fits reports no shortfall. Otherwise the UI would reject a model that would have built.
    #[test]
    fn a_model_that_fits_has_no_shortfalls() {
        let mut demand = Demand::default();
        demand.push_sides("state", 1024 * 1024, true);
        demand.set_display(1024, 1024, &wgpu::Limits::default());
        assert!(demand.shortfalls(&wgpu::Limits::default()).is_empty());
    }

    /// The `gpu_sir` 6000x6000 case from issue #9, which would otherwise be a wgpu panic.
    #[test]
    fn a_buffer_past_the_binding_limit_is_named() {
        let mut demand = Demand::default();
        demand.push_sides("gpu_sir_buffer0", 36_000_000, true);
        demand.push_sides("gpu_sir_buffer1", 36_000_000, true);
        let found = demand.shortfalls(&limits(134_217_728, 268_435_456));
        assert_eq!(
            found.len(),
            1,
            "four same-sized sides are one line, not four: {found:?}"
        );
        assert!(
            found[0].contains("gpu_sir_buffer0_a")
                && found[0].contains("3 more")
                && found[0].contains("storage binding"),
            "{}",
            found[0]
        );
    }

    /// Two over-budget sizes are two problems, so neither may hide the other.
    #[test]
    fn different_sizes_get_their_own_line() {
        let mut demand = Demand::default();
        demand.push("small".to_owned(), 36_000_000);
        demand.push("large".to_owned(), 50_000_000);
        let found = demand.shortfalls(&limits(134_217_728, 268_435_456));
        assert_eq!(found.len(), 2, "{found:?}");
    }

    /// The message must change, since raising the binding cap alone would not help here.
    #[test]
    fn a_buffer_past_the_buffer_limit_says_so() {
        let mut demand = Demand::default();
        demand.push("huge".to_owned(), 1_000_000_000);
        let found = demand.shortfalls(&limits(u64::MAX, 1 << 30));
        assert_eq!(found.len(), 1);
        assert!(found[0].contains("for one buffer"), "{}", found[0]);
    }

    /// The limit behind issue #30. No amount of shrinking the grid fixes it, so it reads
    /// differently from a size.
    #[test]
    fn a_pass_over_the_binding_count_is_named() {
        let mut demand = Demand::default();
        demand.push_pass("gpu_ants_step".to_owned(), PassKind::Step, 9, 0);
        demand.push_pass("gpu_ants_merge".to_owned(), PassKind::Step, 2, 0);
        let found = demand.shortfalls(&wgpu::Limits::default());
        assert_eq!(found.len(), 1, "only the step pass is over: {found:?}");
        assert!(
            found[0].contains("gpu_ants_step") && found[0].contains("per shader stage"),
            "{}",
            found[0]
        );
    }

    /// Independent failures, so one must not mask the other.
    #[test]
    fn an_oversized_buffer_and_an_overbound_pass_are_both_reported() {
        let mut demand = Demand::default();
        demand.push("huge".to_owned(), 36_000_000);
        demand.push_pass("wide".to_owned(), PassKind::Step, 99, 0);
        assert_eq!(demand.shortfalls(&wgpu::Limits::default()).len(), 2);
    }

    /// A pass of unknown cost counts as no work. Otherwise every hand-written model would be refused.
    #[test]
    fn passes_of_unknown_cost_fit_any_budget() {
        let mut demand = Demand::default();
        demand.push_pass("step".to_owned(), PassKind::Step, 2, 0);
        demand.push_pass("reduce".to_owned(), PassKind::Reduce, 2, 0);
        assert_eq!(demand.submission_work(), 0);
        assert!(demand.shortfalls(&wgpu::Limits::default()).is_empty());
    }

    /// One submission holds 64 steps with the snapshot passes, and the line names the heaviest pass by its share.
    #[test]
    fn a_submission_past_the_budget_names_its_heaviest_pass() {
        let steps = u64::from(MAX_STEPS_PER_SUBMISSION);
        let mut demand = Demand::default();
        // Each step pass alone fits: the 64 steps do not.
        let per_step = SUBMISSION_WORK_BUDGET / steps / 2 + 1;
        demand.push_pass("light".to_owned(), PassKind::Step, 2, 1);
        demand.push_pass("heavy".to_owned(), PassKind::Step, 2, per_step);
        demand.push_pass("same".to_owned(), PassKind::Step, 2, per_step);
        demand.push_pass("display".to_owned(), PassKind::Display, 1, SUBMISSION_WORK_BUDGET / 2);
        assert_eq!(
            demand.submission_work(),
            (1 + 2 * per_step) * steps + SUBMISSION_WORK_BUDGET / 2
        );
        let found = demand.shortfalls(&wgpu::Limits::default());
        assert_eq!(found.len(), 1, "{found:?}");
        assert!(
            found[0].contains("one submission runs") && found[0].contains("'heavy' is the heaviest"),
            "{}",
            found[0]
        );
        assert!(found[0].ends_with("per step"), "{}", found[0]);
    }

    /// A batch of steps just within the budget passes, and one unit more does not.
    #[test]
    fn the_budget_is_inclusive() {
        let steps = u64::from(MAX_STEPS_PER_SUBMISSION);
        for (extra, refused) in [(0, false), (1, true)] {
            let mut demand = Demand::default();
            demand.push_pass("step".to_owned(), PassKind::Step, 2, SUBMISSION_WORK_BUDGET / steps);
            demand.push_pass(
                "reduce".to_owned(),
                PassKind::Reduce,
                2,
                SUBMISSION_WORK_BUDGET % steps + extra,
            );
            assert_eq!(
                !demand.shortfalls(&wgpu::Limits::default()).is_empty(),
                refused,
                "{extra}"
            );
        }
    }

    /// Each press is its own submission. Two actions that each fit are not summed, and one past the budget is named.
    #[test]
    fn each_action_is_its_own_submission() {
        let mut demand = Demand::default();
        demand.push_pass("first_a".to_owned(), PassKind::Action(0), 1, SUBMISSION_WORK_BUDGET / 2);
        demand.push_pass("first_b".to_owned(), PassKind::Action(0), 1, SUBMISSION_WORK_BUDGET / 2);
        demand.push_pass("second".to_owned(), PassKind::Action(1), 1, SUBMISSION_WORK_BUDGET);
        assert!(demand.shortfalls(&wgpu::Limits::default()).is_empty());
        demand.push_pass("first_c".to_owned(), PassKind::Action(0), 1, 2);
        let found = demand.shortfalls(&wgpu::Limits::default());
        assert_eq!(found.len(), 1, "{found:?}");
        assert!(found[0].contains("'first_a' is the heaviest"), "{}", found[0]);
        assert!(!found[0].ends_with("per step"), "{}", found[0]);
    }

    /// A uniform block or workgroup memory past the device's limit is named with its pass, and one at the limit is
    /// not. The baseline allows 64 KiB of uniform block and 16 KiB of workgroup memory.
    #[test]
    fn pass_memory_past_the_limits_is_named() {
        let limits = wgpu::Limits::default();
        let uniform = limits.max_uniform_buffer_binding_size;
        let workgroup = u64::from(limits.max_compute_workgroup_storage_size);
        assert_eq!((uniform, workgroup), (65_536, 16_384));
        let mut demand = Demand::default();
        let fits = demand.push_pass("fits".to_owned(), PassKind::Step, 2, 0);
        fits.uniform_bytes = uniform;
        fits.workgroup_bytes = workgroup;
        assert!(demand.shortfalls(&limits).is_empty());
        demand.push_pass("wide".to_owned(), PassKind::Step, 2, 0).uniform_bytes = uniform + 16;
        demand
            .push_pass("counts".to_owned(), PassKind::Reduce, 2, 0)
            .workgroup_bytes = workgroup + 4;
        let found = demand.shortfalls(&limits);
        assert_eq!(found.len(), 2, "{found:?}");
        assert!(
            found[0].contains("'wide'") && found[0].contains("uniform block of 65552 bytes"),
            "{}",
            found[0]
        );
        assert!(
            found[1].contains("'counts'") && found[1].contains("16388 bytes of workgroup memory"),
            "{}",
            found[1]
        );
    }

    /// Work saturates instead of wrapping, so a huge cost is still refused.
    #[test]
    fn saturated_work_is_refused() {
        let mut demand = Demand::default();
        demand.push_pass("step".to_owned(), PassKind::Step, 2, u64::MAX);
        assert_eq!(demand.submission_work(), u64::MAX);
        assert_eq!(demand.shortfalls(&wgpu::Limits::default()).len(), 1);
    }

    /// A run on the host past its budget is named, and each run is held to it alone.
    #[test]
    fn a_host_run_past_its_budget_is_named() {
        let mut demand = Demand::default();
        demand.push_host("model_init".to_owned(), HOST_WORK_BUDGET);
        demand.push_host("model_action_reset_rerun".to_owned(), HOST_WORK_BUDGET);
        assert!(demand.shortfalls(&wgpu::Limits::default()).is_empty());
        demand.push_host("model_action_more_rerun".to_owned(), HOST_WORK_BUDGET + 1);
        let found = demand.shortfalls(&wgpu::Limits::default());
        assert_eq!(found.len(), 1, "{found:?}");
        assert!(
            found[0].contains("'model_action_more_rerun'") && found[0].contains("on the host"),
            "{}",
            found[0]
        );
    }

    #[test]
    fn units_are_grouped_in_threes() {
        assert_eq!(super::fmt_units(0), "0");
        assert_eq!(super::fmt_units(999), "999");
        assert_eq!(super::fmt_units(1_000), "1,000");
        assert_eq!(super::fmt_units(600_000_000_000), "600,000,000,000");
    }

    /// The cap is the point, so display bytes must not grow with the grid.
    #[test]
    fn display_bytes_stop_growing_with_the_grid() {
        let mut small = Demand::default();
        small.set_display(4096, 4096, &wgpu::Limits::default());
        let mut huge = Demand::default();
        huge.set_display(100_000, 100_000, &wgpu::Limits::default());
        assert_eq!(small.bytes(), huge.bytes());
    }
}
