//! Compute passes built from their shaders' bindings, and the double buffered resources they bind, shared by
//! [`super::grid_engine`] and [`super::agent_engine`].
//!
//! Every layout entry and every bind group entry of a pass comes from the name its shader gives the binding, so a slot
//! index cannot disagree with the shader that owns it. A pass has one bind group per side holding the current state,
//! and a step pass of a spec that counts fires has a second set over its counting block.

use henad_core::authoring::model::binding::BindingKind;

use crate::gpu::primitives::pipeline::{compute_pipeline, dispatch_pass, storage_entry, uniform_buffer, uniform_entry};
use crate::gpu::shader_spec::{ShaderBinding, ShaderSpec};
use crate::gpu::sides::{BufferSide, SideTable};

/// A resource that exists once, or once per ping-ponged side.
pub(crate) struct Sides<T> {
    pub(crate) a: T,
    pub(crate) b: Option<T>,
}

impl<T> Sides<T> {
    /// Returns the side that holds the current state, given whether side `a` does. A resource with one side has one
    /// side to return.
    pub(crate) fn pick(&self, a_is_current: bool) -> &T {
        if a_is_current {
            &self.a
        } else {
            self.b.as_ref().unwrap_or(&self.a)
        }
    }

    /// Returns the side that `side` reaches, given whether side `a` is current.
    pub(crate) fn side(&self, side: BufferSide, a_is_current: bool) -> &T {
        self.pick((side == BufferSide::Current) == a_is_current)
    }
}

/// Returns the layout entry that a declaration at `slot` requires.
pub(crate) fn layout_entry(slot: u32, binding: &ShaderBinding) -> wgpu::BindGroupLayoutEntry {
    match binding.kind {
        BindingKind::Storage { read_only } => storage_entry(slot, read_only),
        BindingKind::Uniform => uniform_entry(slot),
        BindingKind::StorageTexture => wgpu::BindGroupLayoutEntry {
            binding: slot,
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

/// Returns the side of the buffer that `binding` names, as a pass bound through `sides` reaches it, or `None` for a
/// reserved name.
///
/// `labels` holds the label of each buffer of the spec, in the order of `buffers`, and `model` prefixes a panic.
///
/// # Panics
///
/// Panics if no buffer carries the label that `binding` names.
pub(crate) fn buffer_binding<'b, 'l>(
    binding: &ShaderBinding,
    model: &str,
    labels: impl IntoIterator<Item = &'l str>,
    buffers: &'b [Sides<wgpu::Buffer>],
    sides: &SideTable,
    a_is_current: bool,
) -> Option<wgpu::BindingResource<'b>> {
    let (label, writes) = binding.buffer_target()?;
    let buffer_index = labels
        .into_iter()
        .position(|buffer_label| buffer_label == label)
        .unwrap_or_else(|| panic!("{model}: no buffer labelled `{label}`, wanted by `{}`", binding.name));
    let row = sides.rows()[buffer_index];
    let side = if writes { row.write } else { row.read };
    Some(buffers[buffer_index].side(side, a_is_current).as_entire_binding())
}

/// Returns the timestamp writes of a pass, `None` for a pass that neither opens nor closes a batch.
///
/// A `ComputePassTimestampWrites` needs at least one index set.
pub(crate) fn stamps(
    query_set: Option<&wgpu::QuerySet>,
    opening: bool,
    closing: bool,
) -> Option<wgpu::ComputePassTimestampWrites<'_>> {
    query_set
        .filter(|_| opening || closing)
        .map(|query_set| wgpu::ComputePassTimestampWrites {
            query_set,
            beginning_of_pass_write_index: opening.then_some(0),
            end_of_pass_write_index: closing.then_some(1),
        })
}

/// Facts a pass is built from, besides the resources its bindings resolve to.
pub(crate) struct PassDescriptor<'a, Id> {
    /// Label of the pipeline, and the prefix of the label of every other wgpu object the pass creates.
    pub(crate) label: String,
    /// Label of the compute pass it records.
    pub(crate) pass_label: String,
    pub(crate) id: Id,
    pub(crate) shader: &'a ShaderSpec,
    pub(crate) groups: (u32, u32),
    /// Uniform block bound as `params`, `None` for a pass with no uniform.
    pub(crate) uniform: Option<Vec<u8>>,
    /// Counting block of a step pass, `None` for every other pass and for a spec that counts no fires.
    pub(crate) counting_uniform: Option<Vec<u8>>,
    /// Whether the pass gets bind groups for side `b` as current. Nothing flips otherwise.
    pub(crate) both_sides: bool,
}

/// A built pass, with one bind group for each side holding the current state.
pub(crate) struct BuiltPass<Id> {
    /// Label of the compute pass it records.
    pub(crate) label: String,
    /// Pass of the spec this pass was built from. Its uniform block is written for this pass.
    pub(crate) id: Id,
    pipeline: wgpu::ComputePipeline,
    /// Workgroups it dispatches.
    pub(crate) groups: (u32, u32),
    binds: Sides<wgpu::BindGroup>,
    /// Uniform block bound as `params`, kept so a press can rewrite an action's and a live edit any other.
    pub(crate) uniform: Option<wgpu::Buffer>,
    /// Counting block of a step pass, `None` where [`Self::counting`] is.
    pub(crate) counting_uniform: Option<wgpu::Buffer>,
    /// Bind groups of the counting block of a step pass, `None` for a spec with no fire counters and for every pass
    /// that is no step or binds no `params`.
    counting: Option<Sides<wgpu::BindGroup>>,
}

impl<Id> BuiltPass<Id> {
    /// Builds the pass that `descriptor` describes. `resolve` returns the resource of every binding but `params`, given
    /// whether side `a` is current.
    ///
    /// # Panics
    ///
    /// Panics if the shader binds `params` and `descriptor` holds no uniform block.
    pub(crate) fn new<'r>(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        descriptor: PassDescriptor<'_, Id>,
        resolve: impl Fn(&ShaderBinding, bool) -> wgpu::BindingResource<'r>,
    ) -> Self {
        let PassDescriptor {
            label,
            pass_label,
            id,
            shader,
            groups,
            uniform,
            counting_uniform,
            both_sides,
        } = descriptor;
        let uniform = uniform.map(|bytes| uniform_buffer(device, queue, &format!("{label}_params"), &bytes));
        let counting_uniform =
            counting_uniform.map(|bytes| uniform_buffer(device, queue, &format!("{label}_counting_params"), &bytes));
        let entries: Vec<wgpu::BindGroupLayoutEntry> = shader
            .bindings
            .iter()
            .enumerate()
            .map(|(binding, decl)| layout_entry(binding as u32, decl))
            .collect();
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some(&format!("{label}_layout")),
            entries: &entries,
        });
        let make = |a_is_current: bool, side: &str, uniform: Option<&wgpu::Buffer>| {
            let entries: Vec<wgpu::BindGroupEntry<'_>> = shader
                .bindings
                .iter()
                .enumerate()
                .map(|(binding, decl)| wgpu::BindGroupEntry {
                    binding: binding as u32,
                    resource: if decl.name == "params" {
                        uniform
                            .unwrap_or_else(|| panic!("{label}: `params` has no uniform"))
                            .as_entire_binding()
                    } else {
                        resolve(decl, a_is_current)
                    },
                })
                .collect();
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some(&format!("{label}_bind_{side}")),
                layout: &layout,
                entries: &entries,
            })
        };
        let binds = Sides {
            a: make(true, "a", uniform.as_ref()),
            b: both_sides.then(|| make(false, "b", uniform.as_ref())),
        };
        let counting = counting_uniform.as_ref().map(|counting_uniform| Sides {
            a: make(true, "counting_a", Some(counting_uniform)),
            b: both_sides.then(|| make(false, "counting_b", Some(counting_uniform))),
        });
        let pipeline = compute_pipeline(device, &label, &shader.source, &layout);
        Self {
            label: pass_label,
            id,
            pipeline,
            groups,
            binds,
            uniform,
            counting_uniform,
            counting,
        }
    }

    /// Records the pass into `encoder` as a compute pass of its own, through its counting block when `counting` holds
    /// and the pass has one.
    pub(crate) fn encode(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        a_is_current: bool,
        timestamp_writes: Option<wgpu::ComputePassTimestampWrites<'_>>,
        counting: bool,
    ) {
        let binds = match &self.counting {
            Some(binds) if counting => binds,
            _ => &self.binds,
        };
        dispatch_pass(
            encoder,
            &self.label,
            &self.pipeline,
            binds.pick(a_is_current),
            self.groups,
            timestamp_writes,
        );
    }
}
