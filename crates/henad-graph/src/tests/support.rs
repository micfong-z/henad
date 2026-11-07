//! Headless device for this crate's GPU tests, and a runner of one generated shader on it.
//!
//! henad-graph sits below henad-explore's testing kit and keeps its own device, as henad-compute does.

use henad_compute::gpu::wgpu;
use henad_compute::gpu::{BufferSide, ShaderBinding};
use henad_core::authoring::model::binding::{BindingKind, named_buffer_target};

use crate::host::HostLanes;
use crate::wgsl::{Compiled, GeneratedShader};

/// Set in CI to turn "no GPU here" from a skip into a failure.
const REQUIRE_GPU: &str = "HENAD_REQUIRE_GPU";

/// A device and its queue.
pub(super) struct TestDevice {
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
}

/// Returns the words and labels of each lane buffer of `compiled`, built from `lanes`, then the tick buffer holding
/// `tick` when `compiled` has one.
///
/// A packed buffer holds the segments of its lanes one after another, as the engine uploads it.
pub(super) fn lane_buffers(compiled: &Compiled, lanes: &HostLanes, tick: u32) -> (Vec<Vec<u32>>, Vec<String>) {
    let mut words: Vec<Vec<u32>> = compiled
        .layout
        .groups
        .iter()
        .map(|group| {
            group
                .lanes
                .iter()
                .flat_map(|&lane| lanes.columns()[lane].words().iter().copied())
                .collect()
        })
        .collect();
    let mut labels: Vec<String> = compiled.layout.groups.iter().map(|group| group.label.clone()).collect();
    if compiled.tick_buffer {
        words.push(vec![tick]);
        labels.push("henad_tick".to_owned());
    }
    (words, labels)
}

/// Returns the words of the lane at `position` from `buffers`, the buffers that a run of one of the shaders of
/// `compiled` left.
///
/// # Panics
///
/// Panics if no buffer holds the lane.
pub(super) fn lane_words(compiled: &Compiled, buffers: &[Vec<u32>], position: usize) -> Vec<u32> {
    let place = compiled.layout.place(position).expect("a lane on the device");
    place.segment(&buffers[place.buffer()]).to_vec()
}

/// Returns a headless device, or `None` when this machine has none and `HENAD_REQUIRE_GPU` is unset.
///
/// # Panics
///
/// Panics if `HENAD_REQUIRE_GPU` is set and no device could be acquired.
pub(super) fn test_device() -> Option<TestDevice> {
    // A case of a radius fold binds its output lanes, the index and the counters, past the baseline's 8 storage
    // buffers, as a model raises the device for its widest pass.
    device_with(|available| wgpu::Limits {
        max_storage_buffers_per_shader_stage: available.max_storage_buffers_per_shader_stage.min(16),
        ..wgpu::Limits::default()
    })
}

/// Returns a headless device with the WebGPU default limits, or `None` when this machine has none and
/// `HENAD_REQUIRE_GPU` is unset.
///
/// # Panics
///
/// Panics if `HENAD_REQUIRE_GPU` is set and no device could be acquired.
pub(super) fn baseline_test_device() -> Option<TestDevice> {
    device_with(|_| wgpu::Limits::default())
}

/// Returns a headless device with the limits `limits` returns from the adapter's, or `None` when this machine has none
/// and `HENAD_REQUIRE_GPU` is unset.
fn device_with(limits: impl FnOnce(&wgpu::Limits) -> wgpu::Limits) -> Option<TestDevice> {
    let required = std::env::var_os(REQUIRE_GPU).is_some_and(|value| !value.is_empty() && value != "0");
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default()));
    let adapter = match adapter {
        Ok(adapter) => adapter,
        Err(error) => {
            assert!(!required, "{REQUIRE_GPU} is set but no adapter is available: {error}");
            return None;
        }
    };
    let required_limits = limits(&adapter.limits());
    match pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        label: Some("henad_graph_tests"),
        required_limits,
        ..Default::default()
    })) {
        Ok((device, queue)) => Some(TestDevice { device, queue }),
        Err(error) => {
            assert!(
                !required,
                "{REQUIRE_GPU} is set but the adapter gave no device: {error}"
            );
            None
        }
    }
}

/// Buffers and sizes one run of a shader binds.
pub(super) struct ShaderRun<'a> {
    /// Columns of the grid.
    pub width: u32,
    /// Rows of the grid.
    pub height: u32,
    /// Words of each buffer, lanes then the tick buffer, as both sides start.
    pub buffers: &'a [Vec<u32>],
    /// Label of each buffer, in the same order.
    pub labels: &'a [String],
    /// Uniform block bound as `params`.
    pub uniform: &'a [u8],
    /// Shape of a run of a shader of an agents world, `None` for a grid's.
    pub agents: Option<AgentShape<'a>>,
}

/// Dispatch and resources of a run of a shader of an agents world.
pub(super) struct AgentShape<'a> {
    /// Workgroups the run dispatches.
    pub groups: (u32, u32),
    /// Size of the display texture.
    pub texture: (u32, u32),
    /// Words of each resource the engine owns that the shader may bind, by binding name, such as `counters`,
    /// `partials` or `cell_start`.
    pub named: &'a [(&'a str, Vec<u32>)],
}

/// What one run of a shader leaves.
pub(super) struct ShaderResult {
    /// Words of each buffer after the run: the side the shader wrote, or the current side.
    pub buffers: Vec<Vec<u32>>,
    /// Counters of a reduce shader.
    pub counters: Vec<u32>,
    /// RGBA of each texel of a display shader, in row-major order.
    pub texels: Vec<[u8; 4]>,
    /// Words of each named resource of an agents run after the run, in the order of [`AgentShape::named`].
    pub named: Vec<Vec<u32>>,
}

/// Returns the layout entry of `binding` at `index`.
fn layout_entry(index: u32, binding: &ShaderBinding) -> wgpu::BindGroupLayoutEntry {
    let ty = match binding.kind {
        BindingKind::Storage { read_only } => wgpu::BindingType::Buffer {
            ty: wgpu::BufferBindingType::Storage { read_only },
            has_dynamic_offset: false,
            min_binding_size: None,
        },
        BindingKind::Uniform => wgpu::BindingType::Buffer {
            ty: wgpu::BufferBindingType::Uniform,
            has_dynamic_offset: false,
            min_binding_size: None,
        },
        BindingKind::StorageTexture => wgpu::BindingType::StorageTexture {
            access: wgpu::StorageTextureAccess::WriteOnly,
            format: wgpu::TextureFormat::Rgba8Unorm,
            view_dimension: wgpu::TextureViewDimension::D2,
        },
    };
    wgpu::BindGroupLayoutEntry {
        binding: index,
        visibility: wgpu::ShaderStages::COMPUTE,
        ty,
        count: None,
    }
}

/// Returns a storage buffer holding `words`, at least one word long.
fn storage(device: &TestDevice, label: &str, words: &[u32]) -> wgpu::Buffer {
    let mut padded = words.to_vec();
    if padded.is_empty() {
        padded.push(0);
    }
    let buffer = device.device.create_buffer(&wgpu::BufferDescriptor {
        label: Some(label),
        size: (padded.len() * 4) as u64,
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    device.queue.write_buffer(
        &buffer,
        0,
        &padded.iter().flat_map(|word| word.to_le_bytes()).collect::<Vec<_>>(),
    );
    buffer
}

/// Returns the bytes of `buffer`, copied out and mapped.
fn read_bytes(device: &TestDevice, buffer: &wgpu::Buffer) -> Vec<u8> {
    let staging = device.device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("henad_graph_test_staging"),
        size: buffer.size(),
        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let mut encoder = device
        .device
        .create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
    encoder.copy_buffer_to_buffer(buffer, 0, &staging, 0, buffer.size());
    device.queue.submit(Some(encoder.finish()));
    let slice = staging.slice(..);
    slice.map_async(wgpu::MapMode::Read, |_| {});
    device
        .device
        .poll(wgpu::PollType::wait_indefinitely())
        .expect("the device polls");
    let bytes = slice.get_mapped_range().expect("the mapped range").to_vec();
    staging.unmap();
    bytes
}

/// Returns the words of `buffer`.
fn read_words(device: &TestDevice, buffer: &wgpu::Buffer) -> Vec<u32> {
    read_bytes(device, buffer)
        .chunks_exact(4)
        .map(|word| u32::from_le_bytes([word[0], word[1], word[2], word[3]]))
        .collect()
}

/// Runs `shader` once over `run`, as the engine binds it, and returns what it leaves.
///
/// # Panics
///
/// Panics on a validation error of the shader or its bindings.
#[expect(
    clippy::too_many_lines,
    reason = "the bindings, the dispatch and the readbacks of one run read in order"
)]
pub(super) fn run_shader(device: &TestDevice, shader: &GeneratedShader, run: &ShaderRun<'_>) -> ShaderResult {
    let scope = device.device.push_error_scope(wgpu::ErrorFilter::Validation);
    let sides: Vec<(wgpu::Buffer, wgpu::Buffer)> = run
        .buffers
        .iter()
        .zip(run.labels)
        .map(|(words, label)| (storage(device, label, words), storage(device, label, words)))
        .collect();
    let uniform = {
        let buffer = device.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("params"),
            size: run.uniform.len().max(16) as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        device.queue.write_buffer(&buffer, 0, run.uniform);
        buffer
    };
    let dims = {
        let buffer = device.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("dims"),
            size: 16,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let words = [run.width, run.height, run.width, run.height];
        device.queue.write_buffer(
            &buffer,
            0,
            &words.iter().flat_map(|word| word.to_le_bytes()).collect::<Vec<_>>(),
        );
        buffer
    };
    let counters = storage(device, "counters", &vec![0; shader.counters()]);
    let named: Vec<(&str, wgpu::Buffer)> = run
        .agents
        .as_ref()
        .map(|shape| {
            shape
                .named
                .iter()
                .map(|(name, words)| (*name, storage(device, name, words)))
                .collect()
        })
        .unwrap_or_default();
    let (texture_width, texture_height) = run
        .agents
        .as_ref()
        .map_or((run.width, run.height), |shape| shape.texture);
    let texture = device.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("output"),
        size: wgpu::Extent3d {
            width: texture_width,
            height: texture_height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::STORAGE_BINDING | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    let rows = shader.sides().rows();
    let side = |binding: &ShaderBinding| -> Option<(usize, BufferSide)> {
        let (label, writes) = named_buffer_target(&binding.name, binding.kind)?;
        let index = run.labels.iter().position(|known| known == label)?;
        let row = rows[index];
        Some((index, if writes { row.write } else { row.read }))
    };
    let entries: Vec<wgpu::BindGroupLayoutEntry> = shader
        .bindings()
        .iter()
        .enumerate()
        .map(|(index, binding)| layout_entry(index as u32, binding))
        .collect();
    let layout = device
        .device
        .create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some(shader.label()),
            entries: &entries,
        });
    let group_entries: Vec<wgpu::BindGroupEntry<'_>> = shader
        .bindings()
        .iter()
        .enumerate()
        .map(|(index, binding)| {
            let resource = if let Some((buffer, side)) = side(binding) {
                match side {
                    BufferSide::Next => sides[buffer].1.as_entire_binding(),
                    _ => sides[buffer].0.as_entire_binding(),
                }
            } else if let Some((_, buffer)) = named.iter().find(|(name, _)| *name == binding.name.as_ref()) {
                buffer.as_entire_binding()
            } else {
                match binding.name.as_ref() {
                    "params" => uniform.as_entire_binding(),
                    "dims" => dims.as_entire_binding(),
                    "counters" => counters.as_entire_binding(),
                    "output" => wgpu::BindingResource::TextureView(&view),
                    other => panic!("{}: no resource for `{other}`", shader.label()),
                }
            };
            wgpu::BindGroupEntry {
                binding: index as u32,
                resource,
            }
        })
        .collect();
    let group = device.device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some(shader.label()),
        layout: &layout,
        entries: &group_entries,
    });
    let module = device.device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some(shader.label()),
        source: wgpu::ShaderSource::Wgsl(shader.composed().into()),
    });
    let pipeline_layout = device.device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some(shader.label()),
        bind_group_layouts: &[Some(&layout)],
        immediate_size: 0,
    });
    let pipeline = device.device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: Some(shader.label()),
        layout: Some(&pipeline_layout),
        module: &module,
        entry_point: Some("main"),
        compilation_options: wgpu::PipelineCompilationOptions::default(),
        cache: None,
    });
    let mut encoder = device
        .device
        .create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
    {
        let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor::default());
        pass.set_pipeline(&pipeline);
        pass.set_bind_group(0, &group, &[]);
        let (x, y) = run
            .agents
            .as_ref()
            .map_or((run.width.div_ceil(16), run.height.div_ceil(16)), |shape| shape.groups);
        pass.dispatch_workgroups(x, y, 1);
    }
    let (width, height) = (texture_width, texture_height);
    let bytes_per_row = (width * 4).next_multiple_of(256);
    let texels = device.device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("texels"),
        size: u64::from(bytes_per_row * height),
        usage: wgpu::BufferUsages::COPY_SRC | wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::STORAGE,
        mapped_at_creation: false,
    });
    encoder.copy_texture_to_buffer(
        texture.as_image_copy(),
        wgpu::TexelCopyBufferInfo {
            buffer: &texels,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(bytes_per_row),
                rows_per_image: None,
            },
        },
        wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
    );
    device.queue.submit(Some(encoder.finish()));
    if let Some(error) = pollster::block_on(scope.pop()) {
        panic!("{}: {error}\n{}", shader.label(), shader.text());
    }
    let buffers = sides
        .iter()
        .enumerate()
        .map(|(index, (current, next))| {
            let written = shader
                .bindings()
                .iter()
                .filter_map(&side)
                .any(|(buffer, side)| buffer == index && side == BufferSide::Next);
            read_words(device, if written { next } else { current })
        })
        .collect();
    let texel_bytes = read_bytes(device, &texels);
    let texels = (0..height as usize)
        .flat_map(|y| (0..width as usize).map(move |x| (y, x)))
        .map(|(y, x)| {
            let start = y * bytes_per_row as usize + x * 4;
            [
                texel_bytes[start],
                texel_bytes[start + 1],
                texel_bytes[start + 2],
                texel_bytes[start + 3],
            ]
        })
        .collect();
    ShaderResult {
        buffers,
        counters: read_words(device, &counters),
        texels,
        named: named.iter().map(|(_, buffer)| read_words(device, buffer)).collect(),
    }
}
