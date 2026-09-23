//! Instanced renderer for network edges, drawn under the agent sprites.
//!
//! Node positions are read directly from the agent layer's position lanes, which are bound as storage buffers.
//! Only the edge list is copied to the GPU (if changed).

use crate::shader_bindings::edges::Uniforms;
use crate::ui::agent_layer::{AGENT_SIZE_PT, palette_lut};
use henad_compute::snapshot::EdgeSnapshot;

/// Alpha at the source of a directed edge.
const TAIL_ALPHA: f32 = 0.5;

const ARROW_LENGTH_PT: f32 = 7.0;
const ARROW_HALF_WIDTH_PT: f32 = 2.5;

/// Distance in points from an arrowhead's tip to the centre of its node.
const ARROW_GAP_PT: f32 = AGENT_SIZE_PT * 0.25;

#[derive(Clone, Copy, Debug)]
pub struct EdgeStyle {
    pub visible: bool,
    pub arrows: bool,
}

// Source, target and colour of an edge, packed into three words per instance.
const INSTANCE_ATTRS: [wgpu::VertexAttribute; 3] = wgpu::vertex_attr_array![0 => Uint32, 1 => Uint32, 2 => Unorm8x4];
const WORDS: usize = 3;

pub struct EdgeDraw {
    pipeline: wgpu::RenderPipeline,
    /// Pipeline for arrowheads, set only if `EdgeStyle.arrows` is true.
    arrow_pipeline: Option<wgpu::RenderPipeline>,
    bind_group: wgpu::BindGroup,
    instances: wgpu::Buffer,
    count: u32,
}

impl EdgeDraw {
    pub fn record(&self, render_pass: &mut wgpu::RenderPass<'_>) {
        render_pass.set_pipeline(&self.pipeline);
        render_pass.set_bind_group(0, &self.bind_group, &[]);
        render_pass.set_vertex_buffer(0, self.instances.slice(..));
        render_pass.draw(0..2, 0..self.count);
        if let Some(arrow_pipeline) = &self.arrow_pipeline {
            render_pass.set_pipeline(arrow_pipeline);
            render_pass.draw(0..3, 0..self.count);
        }
    }
}

/// Renderer for network edges.
pub struct EdgeLayer {
    device: wgpu::Device,
    queue: wgpu::Queue,
    pipeline: wgpu::RenderPipeline,
    arrow_pipeline: wgpu::RenderPipeline,
    bind_group_layout: wgpu::BindGroupLayout,
    uniform: wgpu::Buffer,
    /// Bind group for the uniform and the position lanes.
    ///
    /// `None` if the position lanes are too large to bind, in which case no edge is drawn.
    bind_group: Option<wgpu::BindGroup>,
    instances: wgpu::Buffer,
    /// Number of edges that `instances` can hold.
    capacity: usize,
    count: u32,
    directed: bool,
    /// Version and length of the edge list currently in `instances`.
    uploaded: Option<(u64, usize)>,
    /// Buffer where the instances are packed into before being copied to the GPU.
    scratch: Vec<u32>,
}

impl EdgeLayer {
    pub fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        target_format: wgpu::TextureFormat,
        pos_x: &wgpu::Buffer,
        pos_y: &wgpu::Buffer,
    ) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("henad_edges_shader"),
            source: wgpu::ShaderSource::Wgsl(crate::shader_bindings::edges::SHADER_STRING.into()),
        });

        let storage = |binding| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::VERTEX,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Storage { read_only: true },
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        };
        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("henad_edge_bind_group_layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                storage(1),
                storage(2),
            ],
        });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("henad_edge_pipeline_layout"),
            bind_group_layouts: &[Some(&bind_group_layout)],
            immediate_size: 0,
        });

        let make = |label: &str, entry_point: &str, topology: wgpu::PrimitiveTopology| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(label),
                layout: Some(&pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some(entry_point),
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                    buffers: &[Some(wgpu::VertexBufferLayout {
                        array_stride: (WORDS * 4) as u64,
                        step_mode: wgpu::VertexStepMode::Instance,
                        attributes: &INSTANCE_ATTRS,
                    })],
                },
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some("fs_main"),
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                    targets: &[Some(wgpu::ColorTargetState {
                        format: target_format,
                        blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                primitive: wgpu::PrimitiveState {
                    topology,
                    ..Default::default()
                },
                depth_stencil: None,
                multisample: wgpu::MultisampleState::default(),
                multiview_mask: None,
                cache: None,
            })
        };
        let pipeline = make("henad_edge_pipeline", "vs_main", wgpu::PrimitiveTopology::LineList);
        let arrow_pipeline = make(
            "henad_edge_arrow_pipeline",
            "vs_arrow",
            wgpu::PrimitiveTopology::TriangleList,
        );

        let uniform = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("henad_edge_uniform"),
            size: size_of::<Uniforms>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let mut layer = Self {
            instances: instance_buffer(device, 0),
            device: device.clone(),
            queue: queue.clone(),
            pipeline,
            arrow_pipeline,
            bind_group_layout,
            uniform,
            bind_group: None,
            capacity: 0,
            count: 0,
            directed: false,
            uploaded: None,
            scratch: Vec::new(),
        };
        layer.bind(pos_x, pos_y);
        layer
    }

    /// Points the edges at the agent layer's position lanes.
    ///
    /// This must be called again whenever those buffers are replaced.
    pub fn bind(&mut self, pos_x: &wgpu::Buffer, pos_y: &wgpu::Buffer) {
        let limit = self.device.limits().max_storage_buffer_binding_size;
        if pos_x.size().max(pos_y.size()) > limit {
            log::warn!("positions exceed the storage binding limit of {limit} bytes, so no edge is drawn");
            self.bind_group = None;
            return;
        }
        self.bind_group = Some(self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("henad_edge_bind_group"),
            layout: &self.bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: self.uniform.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: pos_x.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: pos_y.as_entire_binding(),
                },
            ],
        }));
    }

    /// Copies the edge list into the GPU instance buffer, unless the buffer already holds the same version.
    pub fn upload(&mut self, edges: &EdgeSnapshot) {
        self.directed = edges.directed;
        let n = edges.src.len().min(edges.dst.len());
        if self.uploaded == Some((edges.version, n)) {
            return;
        }
        self.uploaded = Some((edges.version, n));
        self.count = 0;
        if n == 0 || !self.grow_to(n) {
            return;
        }

        let lut = palette_lut(edges.palette);
        let (src, dst, color) = (&edges.src, &edges.dst, &edges.color);
        self.scratch.clear();
        self.scratch.resize(n * WORDS, 0);
        {
            use rayon::prelude::*;
            self.scratch.par_chunks_mut(WORDS).enumerate().for_each(|(e, words)| {
                words[0] = src[e];
                words[1] = dst[e];
                words[2] = lut[usize::from(color.get(e).copied().unwrap_or(0))];
            });
        }
        self.queue
            .write_buffer(&self.instances, 0, bytemuck::cast_slice(&self.scratch));
        self.count = u32::try_from(n).unwrap_or(u32::MAX);
    }

    /// Grows the instance buffer to hold at least `n` edges.
    /// Capacity grows in powers of two.
    ///
    /// Returns false if the edge list is too large for one buffer.
    fn grow_to(&mut self, n: usize) -> bool {
        if n <= self.capacity {
            return true;
        }
        let capacity = n.next_power_of_two();
        let bytes = (capacity * WORDS * 4) as u64;
        let limit = self.device.limits().max_buffer_size;
        if bytes > limit {
            log::warn!("{n} edges exceed the buffer limit of {limit} bytes, so none is drawn");
            return false;
        }
        self.instances = instance_buffer(&self.device, bytes);
        self.capacity = capacity;
        true
    }

    /// Writes the uniform for a world of size `world` drawn into `size` points, and returns the draw.
    ///
    /// Returns `None` if there is nothing to draw, or if edges are switched off.
    pub fn prepare(&self, world: (f32, f32), size: egui::Vec2, style: EdgeStyle) -> Option<EdgeDraw> {
        if self.count == 0 || !style.visible {
            return None;
        }
        let bind_group = self.bind_group.as_ref()?;
        let arrows = style.arrows && self.directed;
        let tail = if self.directed && !arrows { TAIL_ALPHA } else { 1.0 };
        let arrow = if arrows {
            [ARROW_LENGTH_PT, ARROW_HALF_WIDTH_PT]
        } else {
            [0.0; 2]
        };
        let uniforms = Uniforms::new(
            [world.0, world.1],
            [tail, 1.0],
            [size.x.max(1.0) * 0.5, size.y.max(1.0) * 0.5],
            arrow,
            ARROW_GAP_PT,
        );
        self.queue.write_buffer(&self.uniform, 0, bytemuck::bytes_of(&uniforms));
        Some(EdgeDraw {
            pipeline: self.pipeline.clone(),
            arrow_pipeline: arrows.then(|| self.arrow_pipeline.clone()),
            bind_group: bind_group.clone(),
            instances: self.instances.clone(),
            count: self.count,
        })
    }

    /// Drops the edges but keeps the pipelines, for a model switch or reset.
    pub fn clear(&mut self) {
        self.count = 0;
        self.uploaded = None;
    }
}

fn instance_buffer(device: &wgpu::Device, bytes: u64) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("henad_edge_instances"),
        size: bytes.max((WORDS * 4) as u64),
        usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    })
}
