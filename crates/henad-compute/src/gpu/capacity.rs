//! Resources a model would allocate, checked against the device before anything is created.
//!
//! Over budget, wgpu panics on the UI thread with a message that refers to a bind group. Checking first lets the
//! message refer to the model's own buffer instead.

use henad_core::helpers::fmt_bytes;

use crate::display_scale::display_dims;
use crate::gpu::primitives::pipeline::{storage_entry, uniform_entry};
use henad_core::authoring::model::binding::{BindingDecl, BindingKind};

/// Returns the number of bindings that count against `max_storage_buffers_per_shader_stage`. A uniform and a storage
/// texture each count against a separate limit.
pub(crate) fn storage_bindings(decls: &[BindingDecl]) -> u32 {
    decls.iter().filter(|d| d.kind.is_storage_buffer()).count() as u32
}

/// Returns the layout entry that a declaration requires.
pub(crate) fn layout_entry(i: u32, decl: &BindingDecl) -> wgpu::BindGroupLayoutEntry {
    match decl.kind {
        BindingKind::Storage { read_only } => storage_entry(i, read_only),
        BindingKind::Uniform => uniform_entry(i),
        BindingKind::StorageTexture => wgpu::BindGroupLayoutEntry {
            binding: i,
            visibility: wgpu::ShaderStages::COMPUTE,
            ty: wgpu::BindingType::StorageTexture {
                access: wgpu::StorageTextureAccess::WriteOnly,
                format: wgpu::TextureFormat::Rgba8Unorm,
                view_dimension: wgpu::TextureViewDimension::D2,
            },
            count: None,
        },
    }
}

/// One buffer a model would allocate, labelled as wgpu would label it so a message points at something greppable.
#[derive(Debug)]
pub struct Alloc {
    /// Label of the buffer.
    pub label: String,
    /// Size in bytes.
    pub bytes: u64,
}

/// Storage buffers one pass binds, counted against `max_storage_buffers_per_shader_stage`.
#[derive(Debug)]
pub struct PassBindings {
    /// Label of the pass.
    pub label: String,
    /// Number of storage buffers the pass binds.
    pub storage: u32,
}

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
    /// Storage bindings of each pass.
    pub passes: Vec<PassBindings>,
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

    /// Records pass `label`, binding `storage` storage buffers.
    pub fn push_pass(&mut self, label: String, storage: u32) {
        self.passes.push(PassBindings { label, storage });
    }

    /// Device bytes the model would occupy.
    pub fn bytes(&self) -> u64 {
        let buffers: u64 = self.buffers.iter().map(|alloc| alloc.bytes).sum();
        let texture = self
            .texture
            .map_or(0, |(w, h)| u64::from(w) * u64::from(h) * RGBA_BYTES);
        buffers + texture
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
                "pass '{}' binds {} storage buffers, past the {allowed} this device allows per shader stage",
                pass.label, pass.storage
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
            match groups.iter_mut().find(|g| g.bytes == alloc.bytes && g.limit == limit) {
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
            "{subject} {}, past the {} this device allows for one {}",
            fmt_bytes(self.bytes),
            fmt_bytes(self.limit),
            self.what
        )
    }
}

#[cfg(test)]
mod tests {
    use super::Demand;

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
        demand.push_pass("gpu_ants_step".to_owned(), 9);
        demand.push_pass("gpu_ants_merge".to_owned(), 2);
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
        demand.push_pass("wide".to_owned(), 99);
        assert_eq!(demand.shortfalls(&wgpu::Limits::default()).len(), 2);
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
