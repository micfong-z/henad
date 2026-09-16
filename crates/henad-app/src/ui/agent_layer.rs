//! Instanced renderer for a model's agent population, drawn over the grid layer.
//!
//! Serves both backends. A CPU model uploads into the buffers owned here, a GPU model's already
//! live on the device so [`AgentLayer::paint_gpu`] binds those instead. Pipeline, uniform and
//! sprite size are shared, which keeps the two visually comparable.
//!
//! Network edges are drawn under the nodes.

use std::sync::Arc;

use crate::shader_bindings::agents::Uniforms;
use crate::ui::edge_layer::{EdgeDraw, EdgeLayer, EdgeStyle};
use crate::ui::painted::{Painted, painted};
use eframe::egui_wgpu::{self, CallbackResources, CallbackTrait};
use henad_compute::gpu::GpuAgents;
use henad_compute::snapshot::{EdgeSnapshot, PointSnapshot};

/// Sprite diameter in logical points.
pub const AGENT_SIZE_PT: f32 = 3.0;

const POS_X_ATTRS: [wgpu::VertexAttribute; 1] = wgpu::vertex_attr_array![0 => Float32];
const POS_Y_ATTRS: [wgpu::VertexAttribute; 1] = wgpu::vertex_attr_array![1 => Float32];
const COLOR_ATTRS: [wgpu::VertexAttribute; 1] = wgpu::vertex_attr_array![2 => Unorm8x4];

/// Both position attributes out of one `vec2` lane. Still two `Float32` attributes rather than a
/// `Float32x2`, so the shader is shared verbatim with the layout above.
const POS_XY_ATTRS: [wgpu::VertexAttribute; 2] = wgpu::vertex_attr_array![0 => Float32, 1 => Float32];

/// Pipeline and uniform binding, fixed for the life of the app. `Arc` because a paint callback
/// can still be in flight when the model is switched.
///
/// One pipeline per vertex layout, since a GPU model holds position as one interleaved lane.
struct AgentPipeline {
    pipeline: wgpu::RenderPipeline,
    interleaved_pipeline: wgpu::RenderPipeline,
    bind_group: wgpu::BindGroup,
}

/// Replaced wholesale when the population outgrows them.
struct AgentBuffers {
    pos_x: wgpu::Buffer,
    pos_y: wgpu::Buffer,
    color: wgpu::Buffer,
}

/// Also picks the vertex layout. Held directly rather than through an `Arc<AgentBuffers>` so one
/// callback serves both backends, and the handles are refcounted so the lanes survive a sim thread
/// torn down mid-frame.
enum PositionSource {
    /// As a CPU snapshot uploads them.
    Split { pos_x: wgpu::Buffer, pos_y: wgpu::Buffer },
    /// As a GPU model stores it.
    Interleaved(wgpu::Buffer),
}

struct AgentPaint {
    draw: Painted<AgentDraw>,
}

pub struct AgentDraw {
    pipeline: Arc<AgentPipeline>,
    positions: PositionSource,
    color: wgpu::Buffer,
    count: u32,
    edges: Option<EdgeDraw>,
}

impl AgentDraw {
    /// Records the population into any pass, egui's or an offscreen one.
    pub fn record(&self, render_pass: &mut wgpu::RenderPass<'_>) {
        if let Some(edges) = &self.edges {
            edges.record(render_pass);
        }
        let color_slot = match &self.positions {
            PositionSource::Split { pos_x, pos_y } => {
                render_pass.set_pipeline(&self.pipeline.pipeline);
                render_pass.set_vertex_buffer(0, pos_x.slice(..));
                render_pass.set_vertex_buffer(1, pos_y.slice(..));
                2
            }
            PositionSource::Interleaved(pos) => {
                render_pass.set_pipeline(&self.pipeline.interleaved_pipeline);
                render_pass.set_vertex_buffer(0, pos.slice(..));
                1
            }
        };
        render_pass.set_bind_group(0, &self.pipeline.bind_group, &[]);
        render_pass.set_vertex_buffer(color_slot, self.color.slice(..));
        render_pass.draw(0..4, 0..self.count);
    }
}

impl CallbackTrait for AgentPaint {
    fn paint(
        &self,
        _info: egui::PaintCallbackInfo,
        render_pass: &mut wgpu::RenderPass<'static>,
        _callback_resources: &CallbackResources,
    ) {
        self.draw.record(render_pass);
    }
}

/// Owns the agent pipeline and its instance buffers across frames and model switches.
pub struct AgentLayer {
    device: wgpu::Device,
    queue: wgpu::Queue,
    uniform: wgpu::Buffer,
    pipeline: Arc<AgentPipeline>,
    buffers: Arc<AgentBuffers>,
    /// Agents the instance buffers can currently hold.
    capacity: usize,
    count: u32,
    /// Reused so a per-tick upload never allocates. See [`AgentLayer::widen_colors`].
    color_scratch: Vec<u32>,
    /// Colour a uniform population was last filled with, so it only widens when that changes.
    uniform_color: Option<u32>,
    /// Whether a vertex shader can read storage buffers. If so, the position lanes are also bound as storage.
    vertex_storage: bool,
    /// Edge renderer, or `None` if edges cannot be drawn.
    edges: Option<EdgeLayer>,
}

impl AgentLayer {
    pub fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        target_format: wgpu::TextureFormat,
        vertex_storage: bool,
    ) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("henad_agents_shader"),
            source: wgpu::ShaderSource::Wgsl(crate::shader_bindings::agents::SHADER_STRING.into()),
        });

        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("henad_agent_bind_group_layout"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });

        let uniform = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("henad_agent_uniform"),
            size: 16,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("henad_agent_pipeline_layout"),
            bind_group_layouts: &[Some(&bind_group_layout)],
            immediate_size: 0,
        });

        let (pipeline, interleaved_pipeline) = build_pipelines(device, &shader, &pipeline_layout, target_format);

        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("henad_agent_bind_group"),
            layout: &bind_group_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: uniform.as_entire_binding(),
            }],
        });

        let buffers = AgentBuffers::new(device, 0, vertex_storage);
        let edges =
            vertex_storage.then(|| EdgeLayer::new(device, queue, target_format, &buffers.pos_x, &buffers.pos_y));

        Self {
            pipeline: Arc::new(AgentPipeline {
                pipeline,
                interleaved_pipeline,
                bind_group,
            }),
            buffers: Arc::new(buffers),
            device: device.clone(),
            queue: queue.clone(),
            uniform,
            capacity: 0,
            count: 0,
            color_scratch: Vec::new(),
            uniform_color: None,
            vertex_storage,
            edges,
        }
    }

    /// Copies network edges to the GPU, or clears the previous model's edges if there are none.
    pub fn upload_edges(&mut self, edges: Option<&EdgeSnapshot>) {
        let Some(layer) = &mut self.edges else {
            return;
        };
        match edges {
            Some(edges) => layer.upload(edges),
            None => layer.clear(),
        }
    }

    /// Call only when the snapshot actually advanced.
    pub fn upload(&mut self, points: &PointSnapshot) {
        let n = points.pos_x.len().min(points.pos_y.len());
        self.count = u32::try_from(n).unwrap_or(u32::MAX);
        if n == 0 {
            return;
        }

        self.grow_to(n);
        self.queue
            .write_buffer(&self.buffers.pos_x, 0, bytemuck::cast_slice(&points.pos_x[..n]));
        self.queue
            .write_buffer(&self.buffers.pos_y, 0, bytemuck::cast_slice(&points.pos_y[..n]));

        self.widen_colors(points, n);
        self.queue
            .write_buffer(&self.buffers.color, 0, bytemuck::cast_slice(&self.color_scratch[..n]));
    }

    /// Expands `u8` palette indices into packed RGBA.
    ///
    /// The lane cannot be bound as-is. WebGPU has no one-byte vertex format and wants
    /// `array_stride % 4 == 0`, and the storage-buffer alternative needs `VERTEX_STORAGE`, which
    /// WebGL2 lacks. Only this last hop widens, the snapshot copy stays one byte per agent.
    fn widen_colors(&mut self, points: &PointSnapshot, n: usize) {
        let lut = palette_lut(points.palette);

        if points.color.is_empty() {
            // Uniform population, so the buffer only changes when the palette or the count does.
            let solid = lut[0];
            if self.uniform_color != Some(solid) || self.color_scratch.len() < n {
                self.color_scratch.clear();
                self.color_scratch.resize(n, solid);
                self.uniform_color = Some(solid);
            }
            return;
        }
        self.uniform_color = None;

        let color = &points.color[..n.min(points.color.len())];
        self.color_scratch.clear();
        self.color_scratch.resize(n, lut[0]);
        let dst = &mut self.color_scratch[..color.len()];

        use rayon::prelude::*;
        dst.par_iter_mut().zip(color.par_iter()).for_each(|(out, &c)| {
            *out = lut[c as usize];
        });
    }

    /// Powers of two, so a model with a varying population stops reallocating quickly.
    fn grow_to(&mut self, n: usize) {
        if n <= self.capacity {
            return;
        }
        let capacity = n.next_power_of_two();
        self.buffers = Arc::new(AgentBuffers::new(
            &self.device,
            capacity as u64 * 4,
            self.vertex_storage,
        ));
        self.capacity = capacity;
        if let Some(edges) = &mut self.edges {
            edges.bind(&self.buffers.pos_x, &self.buffers.pos_y);
        }
    }

    /// Queues the paint callback for `rect`, sizing sprites relative to that rect.
    ///
    /// The uniform is written here rather than in `CallbackTrait::prepare`, which only sees the
    /// whole window. Writes queued while building UI land before egui submits, so this is safe.
    pub fn paint(&self, ui: &egui::Ui, rect: egui::Rect, world_w: f32, world_h: f32, edges: EdgeStyle) {
        self.paint_lanes(
            ui,
            rect,
            (world_w, world_h),
            PositionSource::Split {
                pos_x: self.buffers.pos_x.clone(),
                pos_y: self.buffers.pos_y.clone(),
            },
            &self.buffers.color,
            self.count,
            self.edge_draw((world_w, world_h), rect.size(), edges),
        );
    }

    /// Draws a GPU model's lanes without uploading anything.
    ///
    /// Nothing is cached here, so it is safe on a frame where the snapshot did not advance.
    pub fn paint_gpu(&self, ui: &egui::Ui, rect: egui::Rect, agents: &GpuAgents) {
        self.paint_lanes(
            ui,
            rect,
            (agents.world_w, agents.world_h),
            PositionSource::Interleaved(agents.pos.clone()),
            &agents.color,
            agents.count,
            None,
        );
    }

    /// The population as an offscreen draw, sized to a `target` of that many pixels.
    ///
    /// An exported image sizes sprites in its own pixels rather than in the panel's points, so it
    /// does not change with the panel.
    pub fn offscreen_draw(
        &self,
        target: egui::Vec2,
        points: Option<&PointSnapshot>,
        agents: Option<&GpuAgents>,
        edges: EdgeStyle,
    ) -> Option<AgentDraw> {
        match (points, agents) {
            (_, Some(agents)) => self.prepare(
                target,
                (agents.world_w, agents.world_h),
                PositionSource::Interleaved(agents.pos.clone()),
                &agents.color,
                agents.count,
                None,
            ),
            (Some(points), None) => self.prepare(
                target,
                (points.world_w, points.world_h),
                PositionSource::Split {
                    pos_x: self.buffers.pos_x.clone(),
                    pos_y: self.buffers.pos_y.clone(),
                },
                &self.buffers.color,
                self.count,
                self.edge_draw((points.world_w, points.world_h), target, edges),
            ),
            (None, None) => None,
        }
    }

    /// Returns the draw for edges over the CPU position lanes, if network edges have been copied to the GPU.
    fn edge_draw(&self, world: (f32, f32), size: egui::Vec2, style: EdgeStyle) -> Option<EdgeDraw> {
        self.edges.as_ref()?.prepare(world, size, style)
    }

    /// Writes the uniform for `size` and builds the draw.
    ///
    /// Returns `None` when there is nothing to draw.
    fn prepare(
        &self,
        size: egui::Vec2,
        world: (f32, f32),
        positions: PositionSource,
        color: &wgpu::Buffer,
        count: u32,
        edges: Option<EdgeDraw>,
    ) -> Option<AgentDraw> {
        let (world_w, world_h) = world;
        if count == 0 || world_w <= 0.0 || world_h <= 0.0 {
            return None;
        }
        // Clip space spans 2.0 across the rect, so half-extent is size over rect. Both sides are
        // logical points, so the scale factor cancels and this is already DPI-correct.
        let half_w = (AGENT_SIZE_PT / size.x.max(1.0)).min(1.0);
        let half_h = (AGENT_SIZE_PT / size.y.max(1.0)).min(1.0);
        self.queue.write_buffer(
            &self.uniform,
            0,
            bytemuck::bytes_of(&Uniforms {
                world: [world_w, world_h],
                half_size: [half_w, half_h],
            }),
        );
        Some(AgentDraw {
            pipeline: Arc::clone(&self.pipeline),
            positions,
            color: color.clone(),
            count,
            edges,
        })
    }

    #[expect(clippy::too_many_arguments, reason = "the draw's parts, passed through to prepare")]
    fn paint_lanes(
        &self,
        ui: &egui::Ui,
        rect: egui::Rect,
        world: (f32, f32),
        positions: PositionSource,
        color: &wgpu::Buffer,
        count: u32,
        edges: Option<EdgeDraw>,
    ) {
        let Some(draw) = self.prepare(rect.size(), world, positions, color, count, edges) else {
            return;
        };

        ui.painter().add(egui_wgpu::Callback::new_paint_callback(
            rect,
            AgentPaint { draw: painted(draw) },
        ));
    }

    /// Drops the population but keeps the pipeline, for a model switch or reset.
    pub fn clear(&mut self) {
        self.count = 0;
        self.uniform_color = None;
        if let Some(edges) = &mut self.edges {
            edges.clear();
        }
    }
}

impl AgentBuffers {
    /// Creates the lane buffers. The position lanes also get storage usage if edges need to read them.
    fn new(device: &wgpu::Device, bytes: u64, vertex_storage: bool) -> Self {
        let positions = if vertex_storage {
            wgpu::BufferUsages::STORAGE
        } else {
            wgpu::BufferUsages::empty()
        };
        Self {
            pos_x: instance_buffer(device, "pos_x", bytes, positions),
            pos_y: instance_buffer(device, "pos_y", bytes, positions),
            color: instance_buffer(device, "color", bytes, wgpu::BufferUsages::empty()),
        }
    }
}

/// Returns a palette widened to packed RGBA, for indexing without a bounds check.
///
/// It has 256 entries, so an inner loop can index unconditionally.
/// A model can use an index past the end of its own palette, and such indices take the first colour.
pub fn palette_lut(palette: &[[u8; 4]]) -> [u32; 256] {
    let fallback = palette.first().copied().unwrap_or([0xFF; 4]);
    let mut lut = [0u32; 256];
    for (i, slot) in lut.iter_mut().enumerate() {
        *slot = u32::from_le_bytes(palette.get(i).copied().unwrap_or(fallback));
    }
    lut
}

/// `(split, interleaved)`. They differ only in how the position attributes are fetched.
fn build_pipelines(
    device: &wgpu::Device,
    shader: &wgpu::ShaderModule,
    pipeline_layout: &wgpu::PipelineLayout,
    target_format: wgpu::TextureFormat,
) -> (wgpu::RenderPipeline, wgpu::RenderPipeline) {
    // One vertex buffer per lane, so a CPU model's positions upload straight from the snapshot.
    let split = [
        Some(wgpu::VertexBufferLayout {
            array_stride: 4,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes: &POS_X_ATTRS,
        }),
        Some(wgpu::VertexBufferLayout {
            array_stride: 4,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes: &POS_Y_ATTRS,
        }),
        Some(wgpu::VertexBufferLayout {
            array_stride: 4,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes: &COLOR_ATTRS,
        }),
    ];

    // A GPU model's position lane is `vec2`, so both attributes come out of one buffer.
    let interleaved = [
        Some(wgpu::VertexBufferLayout {
            array_stride: 8,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes: &POS_XY_ATTRS,
        }),
        Some(wgpu::VertexBufferLayout {
            array_stride: 4,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes: &COLOR_ATTRS,
        }),
    ];

    let make = |label: &str, buffers: &[Option<wgpu::VertexBufferLayout<'_>>]| {
        device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some(label),
            layout: Some(pipeline_layout),
            vertex: wgpu::VertexState {
                module: shader,
                entry_point: Some("vs_main"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                buffers,
            },
            fragment: Some(wgpu::FragmentState {
                module: shader,
                entry_point: Some("fs_main"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: target_format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleStrip,
                ..Default::default()
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        })
    };

    (
        make("henad_agent_pipeline", &split),
        make("henad_agent_pipeline_interleaved", &interleaved),
    )
}

fn instance_buffer(device: &wgpu::Device, lane: &str, bytes: u64, extra: wgpu::BufferUsages) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some(&format!("henad_agent_{lane}")),
        size: bytes.max(4),
        usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST | extra,
        mapped_at_creation: false,
    })
}
