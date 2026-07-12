//! Concrete GPU-accelerated Game of Life.
//!
//! All grid state lives in GPU storage buffers (`array<u32>`, one cell per element, no bit
//! packing) and never leaves the GPU except for the verification test's readback. Two buffers
//! (`buffer_a`, `buffer_b`) are ping-ponged: each tick the step compute shader reads one and
//! writes the other, then a display compute shader renders whichever buffer now holds the
//! current state into an RGBA texture that the (reused) checkerboard-spike render pipeline
//! samples. This is deliberately non-generic — a specific implementation to be generalized once
//! a second GPU model (SIR) exists to inform the shared shape.
//!
//! Stepping happens on a dedicated thread ([`sim_thread::GpuGolHandle`]), flat-out and decoupled
//! from the UI frame rate, mirroring `henad_compute::sim_thread`'s CPU sim thread. The egui
//! paint callback ([`GpuGolPaint`]) only samples the display texture the sim thread last wrote —
//! see `sim_thread` module docs for the synchronization argument.

pub mod sim_thread;

use eframe::egui_wgpu;
use egui_wgpu::{CallbackResources, CallbackTrait};
use henad_core::helpers::xorshift64;

pub use sim_thread::GpuGolHandle;

/// Default grid width/height in cells.
pub const DEFAULT_WIDTH: u32 = 1024;
pub const DEFAULT_HEIGHT: u32 = 1024;
const WORKGROUP_SIZE: u32 = 16;

/// Grid dimensions, laid out to match `dims: vec2<u32>` in the shaders.
#[repr(C)]
#[derive(Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
struct GridDims {
    width: u32,
    height: u32,
}

fn workgroup_counts(width: u32, height: u32) -> (u32, u32) {
    (
        width.div_ceil(WORKGROUP_SIZE),
        height.div_ceil(WORKGROUP_SIZE),
    )
}

/// A hardcoded blinker (period-2 oscillator) and glider on an otherwise dead grid, so the
/// visual result (oscillation, diagonal travel) is easy to verify by eye.
pub fn seed_patterns(width: u32, height: u32) -> Vec<u32> {
    let mut cells = vec![0u32; (width * height) as usize];
    let idx = |x: u32, y: u32| (y * width + x) as usize;

    // Blinker: horizontal triple at (10, 10).
    for dx in 0..3u32 {
        cells[idx(10 + dx, 10)] = 1;
    }

    // Glider: travels down-right.
    let glider = [(1, 0), (2, 1), (0, 2), (1, 2), (2, 2)];
    for (dx, dy) in glider {
        cells[idx(40 + dx, 40 + dy)] = 1;
    }

    cells
}

/// CPU-seeded random fill at the given density, using the same `xorshift64` PRNG as the CPU
/// `GameOfLifeModel`.
pub fn seed_random(width: u32, height: u32, density: f32, mut rng: u64) -> Vec<u32> {
    let threshold = (density * u32::MAX as f32) as u32;
    let mut cells = vec![0u32; (width * height) as usize];
    for cell in &mut cells {
        rng = xorshift64(rng);
        *cell = u32::from(((rng >> 32) as u32) < threshold);
    }
    cells
}

/// What to reseed the grid with; requested from the UI, applied on the sim thread.
#[derive(Clone, Copy)]
pub(crate) enum ReseedKind {
    Patterns,
    Random { seed: u64, density: f32 },
}

/// The GPU-resident state that steps every tick. Owned exclusively by the GPU sim thread
/// (`sim_thread::GpuGolSimLoop`) — never touched from the UI thread while the thread is alive.
pub struct GpuGolCompute {
    width: u32,
    height: u32,
    buffer_a: wgpu::Buffer,
    #[cfg(test)]
    buffer_b: wgpu::Buffer,
    step_pipeline: wgpu::ComputePipeline,
    bind_a2b: wgpu::BindGroup,
    bind_b2a: wgpu::BindGroup,
    display_pipeline: wgpu::ComputePipeline,
    display_bind_a: wgpu::BindGroup,
    display_bind_b: wgpu::BindGroup,
    /// `true` when `buffer_a` holds the current (latest) state.
    current_is_a: bool,
}

/// The render-only half: samples the display texture the sim thread writes into. Lives in
/// `egui_wgpu`'s `CallbackResources` and is read from the paint callback on the UI thread.
pub struct GpuGolRender {
    render_pipeline: wgpu::RenderPipeline,
    render_bind_group: wgpu::BindGroup,
}

/// Builds both halves of the GPU Game of Life resources, sharing the display texture between
/// them: the compute half's display shader writes it, the render half's fragment shader samples
/// it.
#[expect(
    clippy::too_many_lines,
    reason = "one-shot resource setup: a linear sequence of wgpu object creation calls that would only be split up by moving the same sequence into more functions"
)]
pub fn build(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    target_format: wgpu::TextureFormat,
    width: u32,
    height: u32,
    initial_cells: &[u32],
) -> (GpuGolCompute, GpuGolRender) {
    let cell_count = (width * height) as usize;
    assert_eq!(
        initial_cells.len(),
        cell_count,
        "initial_cells must have width * height elements"
    );

    let buffer_size = (cell_count * std::mem::size_of::<u32>()) as u64;
    let buffer_a = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("gpu_gol_buffer_a"),
        size: buffer_size,
        usage: wgpu::BufferUsages::STORAGE
            | wgpu::BufferUsages::COPY_SRC
            | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let buffer_b = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("gpu_gol_buffer_b"),
        size: buffer_size,
        usage: wgpu::BufferUsages::STORAGE
            | wgpu::BufferUsages::COPY_SRC
            | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    queue.write_buffer(&buffer_a, 0, bytemuck::cast_slice(initial_cells));

    let dims_buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("gpu_gol_dims_buffer"),
        size: std::mem::size_of::<GridDims>() as u64,
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    queue.write_buffer(
        &dims_buffer,
        0,
        bytemuck::bytes_of(&GridDims { width, height }),
    );

    let display_texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("gpu_gol_display_texture"),
        size: wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::STORAGE_BINDING | wgpu::TextureUsages::TEXTURE_BINDING,
        view_formats: &[],
    });
    let display_view = display_texture.create_view(&wgpu::TextureViewDescriptor::default());
    let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
        label: Some("gpu_gol_sampler"),
        mag_filter: wgpu::FilterMode::Nearest,
        min_filter: wgpu::FilterMode::Nearest,
        ..Default::default()
    });

    // --- Step pipeline ---
    let step_shader = device.create_shader_module(wgpu::include_wgsl!("step.wgsl"));
    let step_bind_group_layout =
        device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("gpu_gol_step_bind_group_layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Storage { read_only: true },
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Storage { read_only: false },
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });
    let make_step_bind_group = |label: &str, current: &wgpu::Buffer, next: &wgpu::Buffer| {
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some(label),
            layout: &step_bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: current.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: next.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: dims_buffer.as_entire_binding(),
                },
            ],
        })
    };
    let bind_a2b = make_step_bind_group("gpu_gol_bind_a2b", &buffer_a, &buffer_b);
    let bind_b2a = make_step_bind_group("gpu_gol_bind_b2a", &buffer_b, &buffer_a);
    let step_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("gpu_gol_step_pipeline_layout"),
        bind_group_layouts: &[&step_bind_group_layout],
        push_constant_ranges: &[],
    });
    let step_pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: Some("gpu_gol_step_pipeline"),
        layout: Some(&step_pipeline_layout),
        module: &step_shader,
        entry_point: Some("main"),
        compilation_options: wgpu::PipelineCompilationOptions::default(),
        cache: None,
    });

    // --- Display pipeline ---
    let display_shader = device.create_shader_module(wgpu::include_wgsl!("display.wgsl"));
    let display_bind_group_layout =
        device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("gpu_gol_display_bind_group_layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Storage { read_only: true },
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::StorageTexture {
                        access: wgpu::StorageTextureAccess::WriteOnly,
                        format: wgpu::TextureFormat::Rgba8Unorm,
                        view_dimension: wgpu::TextureViewDimension::D2,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });
    let make_display_bind_group = |label: &str, state: &wgpu::Buffer| {
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some(label),
            layout: &display_bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: state.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&display_view),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: dims_buffer.as_entire_binding(),
                },
            ],
        })
    };
    let display_bind_a = make_display_bind_group("gpu_gol_display_bind_a", &buffer_a);
    let display_bind_b = make_display_bind_group("gpu_gol_display_bind_b", &buffer_b);
    let display_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("gpu_gol_display_pipeline_layout"),
        bind_group_layouts: &[&display_bind_group_layout],
        push_constant_ranges: &[],
    });
    let display_pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: Some("gpu_gol_display_pipeline"),
        layout: Some(&display_pipeline_layout),
        module: &display_shader,
        entry_point: Some("main"),
        compilation_options: wgpu::PipelineCompilationOptions::default(),
        cache: None,
    });

    // --- Render pipeline (reused fullscreen-triangle sampler from the checkerboard spike) ---
    let render_shader = device.create_shader_module(wgpu::include_wgsl!("render.wgsl"));
    let render_bind_group_layout =
        device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("gpu_gol_render_bind_group_layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
    let render_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("gpu_gol_render_bind_group"),
        layout: &render_bind_group_layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(&display_view),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::Sampler(&sampler),
            },
        ],
    });
    let render_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("gpu_gol_render_pipeline_layout"),
        bind_group_layouts: &[&render_bind_group_layout],
        push_constant_ranges: &[],
    });
    let render_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("gpu_gol_render_pipeline"),
        layout: Some(&render_pipeline_layout),
        vertex: wgpu::VertexState {
            module: &render_shader,
            entry_point: Some("vs_main"),
            buffers: &[],
            compilation_options: wgpu::PipelineCompilationOptions::default(),
        },
        fragment: Some(wgpu::FragmentState {
            module: &render_shader,
            entry_point: Some("fs_main"),
            targets: &[Some(wgpu::ColorTargetState {
                format: target_format,
                blend: None,
                write_mask: wgpu::ColorWrites::ALL,
            })],
            compilation_options: wgpu::PipelineCompilationOptions::default(),
        }),
        primitive: wgpu::PrimitiveState::default(),
        depth_stencil: None,
        multisample: wgpu::MultisampleState::default(),
        multiview: None,
        cache: None,
    });

    (
        GpuGolCompute {
            width,
            height,
            buffer_a,
            #[cfg(test)]
            buffer_b,
            step_pipeline,
            bind_a2b,
            bind_b2a,
            display_pipeline,
            display_bind_a,
            display_bind_b,
            current_is_a: true,
        },
        GpuGolRender {
            render_pipeline,
            render_bind_group,
        },
    )
}

impl GpuGolCompute {
    #[cfg(test)]
    fn current_buffer(&self) -> &wgpu::Buffer {
        if self.current_is_a {
            &self.buffer_a
        } else {
            &self.buffer_b
        }
    }

    /// Overwrites `buffer_a` with `cells` and resets the ping-pong state so `buffer_a` is
    /// current again. Used when reseeding the grid from the UI.
    fn reseed(&mut self, queue: &wgpu::Queue, cells: &[u32]) {
        queue.write_buffer(&self.buffer_a, 0, bytemuck::cast_slice(cells));
        self.current_is_a = true;
    }

    /// Records `count` step dispatches into `encoder`, one compute pass per step.
    ///
    /// Each step is a read-after-write hazard on the ping-ponged state buffers, and wgpu only
    /// inserts synchronization barriers *between* passes, not between dispatches within a single
    /// pass — so this deliberately opens one pass per step rather than looping dispatches inside
    /// one pass (which would read stale data). Batching still happens at the *submission* level:
    /// the caller records all `count` passes into one encoder and submits once, which is what
    /// keeps submission overhead low.
    ///
    /// If `timestamps` is `Some`, the first pass's beginning and the last pass's end are stamped
    /// into query indices 0 and 1 so the caller can measure GPU time for the whole batch.
    fn dispatch_step_batch(
        &mut self,
        encoder: &mut wgpu::CommandEncoder,
        count: u32,
        timestamps: Option<&wgpu::QuerySet>,
    ) {
        let (wg_x, wg_y) = workgroup_counts(self.width, self.height);
        for i in 0..count {
            let bind_group = if self.current_is_a {
                &self.bind_a2b
            } else {
                &self.bind_b2a
            };
            let is_first = i == 0;
            let is_last = i == count - 1;
            // A `ComputePassTimestampWrites` requires at least one of the two indices to be
            // `Some`, so only the first and last passes of the batch get one — everything in
            // between gets `None`.
            let timestamp_writes = timestamps.filter(|_| is_first || is_last).map(|query_set| {
                wgpu::ComputePassTimestampWrites {
                    query_set,
                    beginning_of_pass_write_index: is_first.then_some(0),
                    end_of_pass_write_index: is_last.then_some(1),
                }
            });
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("gpu_gol_step_pass"),
                timestamp_writes,
            });
            pass.set_pipeline(&self.step_pipeline);
            pass.set_bind_group(0, bind_group, &[]);
            pass.dispatch_workgroups(wg_x, wg_y, 1);
            drop(pass);
            self.current_is_a = !self.current_is_a;
        }
    }

    fn dispatch_display(&self, encoder: &mut wgpu::CommandEncoder) {
        let bind_group = if self.current_is_a {
            &self.display_bind_a
        } else {
            &self.display_bind_b
        };
        let (wg_x, wg_y) = workgroup_counts(self.width, self.height);
        let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some("gpu_gol_display_pass"),
            timestamp_writes: None,
        });
        pass.set_pipeline(&self.display_pipeline);
        pass.set_bind_group(0, bind_group, &[]);
        pass.dispatch_workgroups(wg_x, wg_y, 1);
    }
}

/// Paints the display texture the GPU sim thread last wrote. Stepping no longer happens here —
/// see the `sim_thread` module for where and how it happens now.
struct GpuGolPaint;

impl CallbackTrait for GpuGolPaint {
    fn paint(
        &self,
        _info: egui::PaintCallbackInfo,
        render_pass: &mut wgpu::RenderPass<'static>,
        callback_resources: &CallbackResources,
    ) {
        let resources: &GpuGolRender = callback_resources
            .get()
            .expect("GpuGolRender must be inserted before the GoL panel runs");

        render_pass.set_pipeline(&resources.render_pipeline);
        render_pass.set_bind_group(0, &resources.render_bind_group, &[]);
        render_pass.draw(0..3, 0..1);
    }
}

/// Opens a window showing the live GPU Game of Life, with pause, batch-size (fixed or adaptive),
/// and reseed controls, plus wall-clock TPS and GPU per-step time readouts from the sim thread.
pub fn gpu_gol_panel(ctx: &egui::Context, handle: &GpuGolHandle) {
    let paused_id = egui::Id::new("gpu_gol_paused");
    let mut paused = ctx.data(|d| d.get_temp::<bool>(paused_id)).unwrap_or(false);
    let batch_id = egui::Id::new("gpu_gol_batch_size");
    let mut batch_size = ctx
        .data(|d| d.get_temp::<u32>(batch_id))
        .unwrap_or(sim_thread::DEFAULT_BATCH_SIZE);
    let adaptive_id = egui::Id::new("gpu_gol_adaptive");
    let mut adaptive = ctx
        .data(|d| d.get_temp::<bool>(adaptive_id))
        .unwrap_or(false);
    let target_ms_id = egui::Id::new("gpu_gol_target_ms");
    let mut target_ms = ctx
        .data(|d| d.get_temp::<f64>(target_ms_id))
        .unwrap_or(sim_thread::DEFAULT_TARGET_MS);

    egui::Window::new("GPU Game of Life").show(ctx, |ui| {
        let stats = handle.stats();
        ui.label(format!("Wall-clock TPS: {:.0}", stats.wall_tps));
        ui.label(match stats.gpu_us_per_step {
            Some(us) => format!("GPU time/step: {us:.2} \u{b5}s"),
            None => "GPU time/step: N/A".to_owned(),
        });

        if ui.checkbox(&mut paused, "Paused").changed() {
            if paused {
                handle.pause();
            } else {
                handle.resume();
            }
        }

        if ui
            .checkbox(&mut adaptive, "Adaptive batching")
            .on_hover_text(
                "Automatically pick steps-per-batch to keep each GPU submission under the \
                 target time below, instead of a fixed batch size, so large batches don't \
                 block egui's own rendering on the shared queue.",
            )
            .changed()
        {
            handle.set_adaptive(adaptive);
        }

        if adaptive {
            // Live batch size is the controller's output, not the (disabled) local slider value
            // — read it from stats every frame so it visibly tracks GPU cost.
            let mut live_batch_size = stats.batch_size;
            ui.add_enabled(
                false,
                egui::Slider::new(&mut live_batch_size, 1..=sim_thread::MAX_BATCH_SIZE)
                    .text("Steps per batch (live)"),
            );

            if ui
                .add(
                    egui::Slider::new(&mut target_ms, 1.0..=16.0)
                        .text("Target ms/batch")
                        .fixed_decimals(1),
                )
                .changed()
            {
                handle.set_target_ms(target_ms);
            }
        } else if ui
            .add(egui::Slider::new(&mut batch_size, 1..=2000).text("Steps per batch"))
            .changed()
        {
            handle.set_batch_size(batch_size);
        }

        ui.horizontal(|ui| {
            if ui.button("Reseed: patterns").clicked() {
                handle.reseed(ReseedKind::Patterns);
            }
            if ui.button("Reseed: random").clicked() {
                let seed = ctx.input(|i| i.time.to_bits()).max(1);
                handle.reseed(ReseedKind::Random { seed, density: 0.3 });
            }
        });

        let size = egui::Vec2::splat(512.0);
        let (rect, _response) = ui.allocate_exact_size(size, egui::Sense::hover());
        ui.painter()
            .add(egui_wgpu::Callback::new_paint_callback(rect, GpuGolPaint));
    });

    if !paused {
        ctx.request_repaint();
    }

    ctx.data_mut(|d| {
        d.insert_temp(paused_id, paused);
        d.insert_temp(batch_id, batch_size);
        d.insert_temp(adaptive_id, adaptive);
        d.insert_temp(target_ms_id, target_ms);
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use henad_core::grid_model::GridModel as _;
    use henad_models::game_of_life::GameOfLifeModel;

    fn headless_device() -> Option<(wgpu::Device, wgpu::Queue)> {
        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor::default());
        let adapter =
            pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default()))
                .ok()?;
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("gpu_gol_test_device"),
            ..Default::default()
        }))
        .ok()?;
        Some((device, queue))
    }

    fn read_buffer(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        buffer: &wgpu::Buffer,
        len: usize,
    ) -> Vec<u32> {
        let size = (len * std::mem::size_of::<u32>()) as u64;
        let staging = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("gpu_gol_test_readback"),
            size,
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let mut encoder =
            device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
        encoder.copy_buffer_to_buffer(buffer, 0, &staging, 0, size);
        queue.submit(Some(encoder.finish()));

        let slice = staging.slice(..);
        let (tx, rx) = flume::bounded(1);
        slice.map_async(wgpu::MapMode::Read, move |result| {
            drop(tx.send(result));
        });
        device
            .poll(wgpu::PollType::wait_indefinitely())
            .expect("device poll failed");
        rx.recv()
            .expect("map_async channel closed")
            .expect("buffer mapping failed");

        let data = slice.get_mapped_range();
        let cells: Vec<u32> = bytemuck::cast_slice(&data).to_vec();
        drop(data);
        staging.unmap();
        cells
    }

    fn cpu_reference_step(cells: &[u32], width: u32, height: u32) -> Vec<u32> {
        let (w, h) = (width as usize, height as usize);
        let mut next = vec![0u32; w * h];
        let mut rng = 0u64;
        for y in 0..h {
            for x in 0..w {
                let xm1 = (x + w - 1) % w;
                let xp1 = (x + 1) % w;
                let ym1 = (y + h - 1) % h;
                let yp1 = (y + 1) % h;
                let neighbor_coords = [
                    (xm1, ym1),
                    (x, ym1),
                    (xp1, ym1),
                    (xm1, y),
                    (xp1, y),
                    (xm1, yp1),
                    (x, yp1),
                    (xp1, yp1),
                ];
                let neighbors: Vec<u8> = neighbor_coords
                    .iter()
                    .map(|&(nx, ny)| cells[ny * w + nx] as u8)
                    .collect();
                let cell = cells[y * w + x] as u8;
                next[y * w + x] =
                    u32::from(GameOfLifeModel::step_cell(cell, &neighbors, &(), &mut rng) == 1);
            }
        }
        next
    }

    /// Exercises the exact code path the sim thread uses: several steps recorded into one
    /// encoder and submitted once (`dispatch_step_batch`), not one submit per step.
    #[test]
    fn gpu_matches_cpu_reference() {
        let Some((device, queue)) = headless_device() else {
            log::warn!("skipping gpu_matches_cpu_reference: no wgpu adapter available");
            return;
        };

        let width = 64;
        let height = 64;
        let initial = seed_random(width, height, 0.3, 42);

        let (mut compute, _render) = build(
            &device,
            &queue,
            wgpu::TextureFormat::Rgba8Unorm,
            width,
            height,
            &initial,
        );

        let ticks = 5;
        let mut encoder =
            device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
        compute.dispatch_step_batch(&mut encoder, ticks, None);
        queue.submit(Some(encoder.finish()));

        let mut cpu_state = initial.clone();
        for _ in 0..ticks {
            cpu_state = cpu_reference_step(&cpu_state, width, height);
        }

        let gpu_state = read_buffer(
            &device,
            &queue,
            compute.current_buffer(),
            (width * height) as usize,
        );

        assert_eq!(
            gpu_state, cpu_state,
            "GPU state after a batch of {ticks} steps in one submission must match {ticks} CPU steps"
        );
    }

    #[test]
    fn blinker_returns_after_two_ticks() {
        let Some((device, queue)) = headless_device() else {
            log::warn!("skipping blinker_returns_after_two_ticks: no wgpu adapter available");
            return;
        };

        let width = 10;
        let height = 10;
        let mut initial = vec![0u32; (width * height) as usize];
        // Horizontal blinker in the middle, away from the toroidal edges.
        initial[5 * width as usize + 3] = 1;
        initial[5 * width as usize + 4] = 1;
        initial[5 * width as usize + 5] = 1;

        let (mut compute, _render) = build(
            &device,
            &queue,
            wgpu::TextureFormat::Rgba8Unorm,
            width,
            height,
            &initial,
        );

        let mut encoder =
            device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
        compute.dispatch_step_batch(&mut encoder, 2, None);
        queue.submit(Some(encoder.finish()));

        let gpu_state = read_buffer(
            &device,
            &queue,
            compute.current_buffer(),
            (width * height) as usize,
        );

        assert_eq!(
            gpu_state, initial,
            "blinker must return to its original state after 2 ticks"
        );
    }
}
