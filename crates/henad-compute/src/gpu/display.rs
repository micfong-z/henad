//! The display half of a GPU model: a texture the model's compute shader writes, plus the
//! fullscreen-triangle render pipeline that samples it into the viewport.
//!
//! This is model-agnostic — a GPU model only has to write RGBA pixels into
//! [`DisplayTarget::view`], and it gets a paintable [`GpuDisplay`] for free. Which is why it
//! lives here rather than in `henad-models`: the checkerboard/GoL spike's `render.wgsl` was
//! already generic (it just samples a texture), so it becomes shared machinery.

use std::sync::Arc;

/// The render-only half, handed to the UI inside a snapshot. The UI's paint callback holds an
/// `Arc` of this and does nothing but bind and draw — it never touches simulation state.
///
/// Holding this by `Arc` (rather than stashing it in egui's type-keyed `CallbackResources`) is
/// what makes teardown safe: an in-flight paint callback keeps the pipeline and its texture alive
/// even if the sim thread and its state are dropped mid-frame.
pub struct GpuDisplay {
    /// Cell dimensions of the underlying grid — the UI uses this for aspect-ratio fitting.
    pub width: u32,
    pub height: u32,
    pub render_pipeline: wgpu::RenderPipeline,
    pub render_bind_group: wgpu::BindGroup,
}

/// A display texture plus the [`GpuDisplay`] that samples it.
pub struct DisplayTarget {
    /// Bind this as a `texture_storage_2d<rgba8unorm, write>` in the model's display compute pass.
    pub view: wgpu::TextureView,
    pub display: Arc<GpuDisplay>,
}

/// Creates the display texture and the pipeline that samples it into `target_format`.
pub fn build_display_target(
    device: &wgpu::Device,
    target_format: wgpu::TextureFormat,
    width: u32,
    height: u32,
) -> DisplayTarget {
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("henad_gpu_display_texture"),
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
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());

    let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
        label: Some("henad_gpu_display_sampler"),
        mag_filter: wgpu::FilterMode::Nearest,
        min_filter: wgpu::FilterMode::Nearest,
        ..Default::default()
    });

    let shader = device.create_shader_module(wgpu::include_wgsl!("display.wgsl"));

    let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("henad_gpu_display_bind_group_layout"),
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
        label: Some("henad_gpu_display_bind_group"),
        layout: &bind_group_layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(&view),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::Sampler(&sampler),
            },
        ],
    });

    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("henad_gpu_display_pipeline_layout"),
        bind_group_layouts: &[&bind_group_layout],
        push_constant_ranges: &[],
    });

    let render_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("henad_gpu_display_pipeline"),
        layout: Some(&pipeline_layout),
        vertex: wgpu::VertexState {
            module: &shader,
            entry_point: Some("vs_main"),
            buffers: &[],
            compilation_options: wgpu::PipelineCompilationOptions::default(),
        },
        fragment: Some(wgpu::FragmentState {
            module: &shader,
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

    DisplayTarget {
        view,
        display: Arc::new(GpuDisplay {
            width,
            height,
            render_pipeline,
            render_bind_group,
        }),
    }
}
