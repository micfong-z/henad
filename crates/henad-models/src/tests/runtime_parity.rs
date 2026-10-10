//! The example models' shaders composed at run time, and grid rules held to the models they write again.

use henad_compute::cpu::grid_engine::GridModelState;
use henad_compute::entry::{ModelSet, ModelState};
use henad_compute::gpu::compose::{WgslFile, compose_wgsl};
use henad_compute::gpu::runtime_grid::GridRule;
use henad_compute::gpu::sim_thread::GpuSimState;
use henad_compute::gpu::{GpuContext, GpuDisplay};
use henad_core::authoring::primitives::rng::xorshift64;
use henad_core::model::SimState as _;
use henad_core::params::ParamValue;
use henad_core::provenance::ModelCode;
use henad_core::view::{StatEntry, StatValue};
use henad_explore::testing::{CheckSettings, TestDeviceRequest, assert_set_conforms, headless_test_device};

use crate::game_of_life::GameOfLifeModel;

/// The import path of `gpu_ants/state.wgsl`, the one module of the example shaders.
const ANTS_STATE: WgslFile<'static> = WgslFile {
    path: "gpu_ants/state.wgsl",
    text: include_str!("../gpu_ants/state.wgsl"),
};

/// Each compute entry point of henad-models: its path below `src`, its source, and the `SHADER_STRING` henad-build
/// generated from it.
const ENTRIES: &[(&str, &str, &str)] = &[
    (
        "gpu_ants/display.wgsl",
        include_str!("../gpu_ants/display.wgsl"),
        crate::shader_bindings::gpu_ants::display::SHADER_STRING,
    ),
    (
        "gpu_ants/merge.wgsl",
        include_str!("../gpu_ants/merge.wgsl"),
        crate::shader_bindings::gpu_ants::merge::SHADER_STRING,
    ),
    (
        "gpu_ants/reduce.wgsl",
        include_str!("../gpu_ants/reduce.wgsl"),
        crate::shader_bindings::gpu_ants::reduce::SHADER_STRING,
    ),
    (
        "gpu_ants/reset_colony.wgsl",
        include_str!("../gpu_ants/reset_colony.wgsl"),
        crate::shader_bindings::gpu_ants::reset_colony::SHADER_STRING,
    ),
    (
        "gpu_ants/step.wgsl",
        include_str!("../gpu_ants/step.wgsl"),
        crate::shader_bindings::gpu_ants::step::SHADER_STRING,
    ),
    (
        "gpu_boids/randomise_headings.wgsl",
        include_str!("../gpu_boids/randomise_headings.wgsl"),
        crate::shader_bindings::gpu_boids::randomise_headings::SHADER_STRING,
    ),
    (
        "gpu_boids/reduce.wgsl",
        include_str!("../gpu_boids/reduce.wgsl"),
        crate::shader_bindings::gpu_boids::reduce::SHADER_STRING,
    ),
    (
        "gpu_boids/step.wgsl",
        include_str!("../gpu_boids/step.wgsl"),
        crate::shader_bindings::gpu_boids::step::SHADER_STRING,
    ),
    (
        "gpu_game_of_life/clear.wgsl",
        include_str!("../gpu_game_of_life/clear.wgsl"),
        crate::shader_bindings::gpu_game_of_life::clear::SHADER_STRING,
    ),
    (
        "gpu_game_of_life/display.wgsl",
        include_str!("../gpu_game_of_life/display.wgsl"),
        crate::shader_bindings::gpu_game_of_life::display::SHADER_STRING,
    ),
    (
        "gpu_game_of_life/randomise.wgsl",
        include_str!("../gpu_game_of_life/randomise.wgsl"),
        crate::shader_bindings::gpu_game_of_life::randomise::SHADER_STRING,
    ),
    (
        "gpu_game_of_life/reduce.wgsl",
        include_str!("../gpu_game_of_life/reduce.wgsl"),
        crate::shader_bindings::gpu_game_of_life::reduce::SHADER_STRING,
    ),
    (
        "gpu_game_of_life/step.wgsl",
        include_str!("../gpu_game_of_life/step.wgsl"),
        crate::shader_bindings::gpu_game_of_life::step::SHADER_STRING,
    ),
    (
        "gpu_sir/display.wgsl",
        include_str!("../gpu_sir/display.wgsl"),
        crate::shader_bindings::gpu_sir::display::SHADER_STRING,
    ),
    (
        "gpu_sir/reduce.wgsl",
        include_str!("../gpu_sir/reduce.wgsl"),
        crate::shader_bindings::gpu_sir::reduce::SHADER_STRING,
    ),
    (
        "gpu_sir/seed_outbreak.wgsl",
        include_str!("../gpu_sir/seed_outbreak.wgsl"),
        crate::shader_bindings::gpu_sir::seed_outbreak::SHADER_STRING,
    ),
    (
        "gpu_sir/step.wgsl",
        include_str!("../gpu_sir/step.wgsl"),
        crate::shader_bindings::gpu_sir::step::SHADER_STRING,
    ),
];

/// Every compute shader of the example models composes at run time to the text henad-build embedded at build time,
/// `gpu_ants`' entries with their `state` module.
///
/// `wgsl_bindgen` writes the text into a raw string literal that opens with a line break, so `SHADER_STRING` is the
/// composed text after one `\n`.
#[test]
fn runtime_composition_matches_every_stock_shader() {
    for &(path, source, built) in ENTRIES {
        let composed = compose_wgsl(WgslFile { path, text: source }, &[ANTS_STATE])
            .unwrap_or_else(|problems| panic!("{path} does not compose: {problems:#?}"));
        assert_eq!(format!("\n{}", composed.text()), built, "{path}");
    }
}

/// Lines the corpus inserts: malformed imports, `naga_oil`'s other directives, and text `naga_oil` rewrites.
const JUNK_LINES: &[&str] = &[
    "#import",
    "#import henad::rng::{",
    "#import henad::rng::{pcg_hash",
    "#import henad::rng::}",
    "#import henad::nowhere::{item}",
    "#import henad::rng::{no_such_item}",
    "#import henad::rng::{pcg_hash as}",
    "#import henad::rng::{pcg_hash as pcg_hash}",
    "#import henad::rng::{pcg_hash, pcg_hash}",
    "#import henad::rng::{a::b}",
    "#import henad::rng::*",
    "#import henad::rng",
    "#import \"quoted.wgsl\"::item",
    "#import ::",
    "# import henad::rng::pcg_hash",
    "#import henad::rng::pcg_hash;;",
    "#import gpu_ants::state::{HAS_FOOD_BIT}",
    "#define_import_path somewhere",
    "#ifdef FOO",
    "#ifndef FOO",
    "#if FOO == 3",
    "#else",
    "#else ifdef BAR",
    "#endif",
    "#define FOO 3",
    "#{FOO}",
    "#FOO",
    "#version 450",
    "enable f16;",
    "requires readonly_and_readwrite_storage_textures;",
    "diagnostic(off, derivative_uniformity);",
    "virtual fn hidden() {}",
    "override fn pcg_hash(x: u32) -> u32 { return x; }",
    // naga_oil decodes the module name of an overriding function without checking it, and panics on this line.
    "override fn pcg_hashX_naga_oil_mod_X1X(x: u32) -> u32 { return x; }",
    "override scale: f32 = 1.0;",
    "@group(0) @binding(auto) var<storage> extra: array<u32>;",
    "fn decoratedX_naga_oil_mod_XNBSW4YLEHI5HE3THX() {}",
    "let x = henad::rng::pcg_hash(1u);",
    "/* unterminated",
    "*/",
    "\"",
    "fn main() {}",
];

/// Characters the corpus inserts one at a time.
const JUNK_CHARS: &[&str] = &[
    "{", "}", "(", ")", ";", "#", ":", "\"", "/*", "*/", "@", "<", ">", "é", "\r", "\t",
];

/// Returns `text` with one edit, chosen by `rng`: characters deleted, a line repeated, two lines swapped, a junk line
/// or a junk character inserted.
fn mutate(text: &str, rng: &mut u64) -> String {
    let mut draw = |bound: usize| {
        *rng = xorshift64(*rng);
        (*rng >> 33) as usize % bound.max(1)
    };
    let boundaries: Vec<usize> = text
        .char_indices()
        .map(|(index, _)| index)
        .chain([text.len()])
        .collect();
    let lines: Vec<&str> = text.lines().collect();
    match draw(6) {
        0 => {
            let at = draw(boundaries.len() - 1);
            let len = 1 + draw(8);
            let end = boundaries[(at + len).min(boundaries.len() - 1)];
            format!("{}{}", &text[..boundaries[at]], &text[end..])
        }
        1 => {
            let mut out = lines.clone();
            let at = draw(lines.len());
            out.insert(at, lines[at]);
            out.join("\n")
        }
        2 => {
            let mut out = lines.clone();
            let (a, b) = (draw(lines.len()), draw(lines.len()));
            out.swap(a, b);
            out.join("\n")
        }
        3 | 4 => {
            let mut out = lines.clone();
            let junk = JUNK_LINES[draw(JUNK_LINES.len())];
            out.insert(draw(lines.len() + 1), junk);
            out.join("\n")
        }
        _ => {
            let at = boundaries[draw(boundaries.len())];
            let junk = JUNK_CHARS[draw(JUNK_CHARS.len())];
            format!("{}{junk}{}", &text[..at], &text[at..])
        }
    }
}

/// Start of the one diagnostic `compose_wgsl` returns when `naga_oil` panics, which the directive check exists to
/// prevent.
const COMPOSER_FAILED: &str = "the WGSL composer failed";

/// Thousands of edits of the stock shaders and of `gpu_ants`' module never panic `compose_wgsl`, and never reach a
/// panic inside `naga_oil`.
///
/// Each shader takes 200 edits from a fixed seed, and a third of them stack a second edit on the first.
#[test]
fn compose_never_panics_on_the_mutation_corpus() {
    henad_compute::fault::install_panic_hook();
    let mut rng = 0x5EED_C0DE_D00D_F00Du64;
    let mut cases = 0;
    let mut composed = 0;
    for &(path, source, _) in ENTRIES.iter().chain([&(ANTS_STATE.path, ANTS_STATE.text, "")]) {
        for edit in 0..200 {
            let mut text = mutate(source, &mut rng);
            if edit % 3 == 0 {
                text = mutate(&text, &mut rng);
            }
            // The module is mutated as a module and the entries see the mutated copy.
            let (entry, module) = if path == ANTS_STATE.path {
                (ENTRIES[4].1.to_owned(), text.clone())
            } else {
                (text.clone(), ANTS_STATE.text.to_owned())
            };
            let result = std::panic::catch_unwind(|| {
                compose_wgsl(
                    WgslFile { path, text: &entry },
                    &[WgslFile {
                        path: ANTS_STATE.path,
                        text: &module,
                    }],
                )
            });
            let outcome = result.unwrap_or_else(|_| panic!("compose_wgsl panicked on an edit of {path}:\n{text}"));
            match outcome {
                Ok(_) => composed += 1,
                Err(problems) => {
                    assert!(!problems.is_empty(), "an error without a diagnostic on {path}:\n{text}");
                    if let Some(problem) = problems
                        .iter()
                        .find(|problem| problem.message.starts_with(COMPOSER_FAILED))
                    {
                        panic!("naga_oil panicked on an edit of {path}: {problem}\n{text}");
                    }
                }
            }
            cases += 1;
        }
    }
    assert_eq!(cases, 18 * 200);
    // An edit inside a comment or a repeated blank line still composes. Most edits break the shader.
    assert!(composed > 0 && composed < cases, "{composed} of {cases} edits composed");
}

/// Returns the Life starter's entry, labelled with code that names it.
fn life_entry() -> henad_compute::entry::ModelEntry {
    GridRule::life()
        .compile()
        .unwrap_or_else(|problems| panic!("the Life starter does not compile: {problems:#?}"))
        .entry(ModelCode::new(GridRule::CODE_FORMAT, GridRule::life().step))
}

/// Returns the value of the stat labelled `label`.
fn stat(stats: &[StatEntry], label: &str) -> f64 {
    match stats.iter().find(|entry| entry.label() == label).map(StatEntry::value) {
        Some(StatValue::Scalar(value)) => *value,
        other => panic!("no scalar stat `{label}`: {other:?}"),
    }
}

/// Runs the display and reduce passes of `state` and waits for its stats, as a blocking snapshot does.
fn snapshot(ctx: &GpuContext, state: &mut dyn GpuSimState) {
    let mut encoder = ctx
        .device
        .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
    state.encode_snapshot_passes(&mut encoder);
    ctx.queue.submit(Some(encoder.finish()));
    state.begin_stats_readback();
    state.poll_stats_readback(&ctx.device, true);
}

/// Returns the state of each cell as `display` draws it, row after row, one texel per cell. Each state takes its colour
/// in [`GridRule::life`].
fn drawn_cells(display: &GpuDisplay, ctx: &GpuContext) -> Vec<u8> {
    let size = wgpu::Extent3d {
        width: display.width,
        height: display.height,
        depth_or_array_layers: 1,
    };
    let target = ctx.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("henad_test_rule_target"),
        size,
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: ctx.target_format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let view = target.create_view(&wgpu::TextureViewDescriptor::default());
    let row_bytes = display.width * 4;
    let padded_row = row_bytes.next_multiple_of(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT);
    let staging = ctx.device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("henad_test_rule_staging"),
        size: u64::from(padded_row) * u64::from(display.height),
        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let mut encoder = ctx
        .device
        .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
    {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("henad_test_rule_pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_pipeline(&display.render_pipeline);
        pass.set_bind_group(0, &display.render_bind_group, &[]);
        pass.draw(0..3, 0..1);
    }
    encoder.copy_texture_to_buffer(
        wgpu::TexelCopyTextureInfo {
            texture: &target,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        wgpu::TexelCopyBufferInfo {
            buffer: &staging,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(padded_row),
                rows_per_image: Some(display.height),
            },
        },
        size,
    );
    ctx.queue.submit(Some(encoder.finish()));
    let (sender, receiver) = flume::bounded(1);
    staging
        .slice(..)
        .map_async(wgpu::MapMode::Read, move |result| drop(sender.send(result)));
    ctx.device
        .poll(wgpu::PollType::wait_indefinitely())
        .unwrap_or_else(|error| panic!("the device did not drain: {error}"));
    receiver
        .recv()
        .unwrap_or_else(|error| panic!("the map callback did not fire: {error}"))
        .unwrap_or_else(|error| panic!("the staging buffer did not map: {error}"));
    let mapped = staging
        .slice(..)
        .get_mapped_range()
        .unwrap_or_else(|error| panic!("the staging buffer has no range: {error}"));
    let colors: Vec<[u8; 4]> = GridRule::life().states.iter().map(|state| state.color).collect();
    let cells = mapped
        .chunks(padded_row as usize)
        .flat_map(|row| row[..row_bytes as usize].chunks(4))
        .map(|pixel| match colors.iter().position(|color| color[..] == *pixel) {
            Some(state) => state as u8,
            None => panic!("a pixel {pixel:?} has the colour of no state of the Life starter"),
        })
        .collect();
    drop(mapped);
    staging.unmap();
    cells
}

/// The Life starter steps through the cells of the example Game of Life, from the same cells at tick 0, on a 128 by
/// 128 grid for 64 ticks. Its Alive count equals the CPU model's at every tick, and every cell equals the CPU model's
/// every 8 ticks.
#[test]
fn the_life_starter_matches_game_of_life() {
    let Some(ctx) = headless_test_device(&TestDeviceRequest::baseline()) else {
        log::warn!("skipping the_life_starter_matches_game_of_life: no adapter");
        return;
    };
    let (side, ticks, seed) = (128u32, 64u64, Some(11));
    let params = [ParamValue::U32(side), ParamValue::U32(side), ParamValue::F32(0.3)];
    let built = life_entry()
        .build(&params, seed, Some(&ctx))
        .unwrap_or_else(|fault| panic!("the Life starter did not build: {fault}"));
    let ModelState::Gpu(mut gpu) = built else {
        panic!("a grid rule builds a GPU state");
    };
    let mut cpu = GridModelState::<GameOfLifeModel>::from_params_seeded(&params, seed);
    let display = gpu
        .view()
        .display
        .unwrap_or_else(|| panic!("a grid rule draws a display texture"));
    for tick in 0..=ticks {
        snapshot(&ctx, &mut *gpu);
        assert_eq!(gpu.tick(), tick);
        let alive = stat(&cpu.stats(), "Alive");
        assert!(alive > 0.0, "the grid died out at tick {tick}, and compares nothing");
        assert_eq!(stat(&gpu.stats(), "Alive"), alive, "Alive at tick {tick}");
        assert_eq!(
            stat(&gpu.stats(), "Dead"),
            f64::from(side * side) - alive,
            "Dead at tick {tick}"
        );
        if tick % 8 == 0 {
            let cells = cpu.grid_view().map(|view| view.cells.to_vec()).unwrap_or_default();
            assert!(drawn_cells(&display, &ctx) == cells, "the cells differ at tick {tick}");
        }
        let mut encoder = ctx
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
        gpu.encode_steps(&mut encoder, 1, None);
        ctx.queue.submit(Some(encoder.finish()));
        cpu.step();
    }
}

/// Returns the entry of a rule that draws each cell afresh every tick, alive with probability one half.
fn coin_entry() -> henad_compute::entry::ModelEntry {
    let mut rule = GridRule::life();
    rule.id = "coin_rule".to_owned();
    rule.name = "Coin rule".to_owned();
    rule.step = "#import henad::rng::{next_bits, below}\n\
                 fn step_cell(cell: u32, rng: ptr<function, u32>) -> u32 {\n    \
                 return select(DEAD, ALIVE, below(next_bits(rng), 0x80000000u));\n}\n"
        .to_owned();
    rule.compile()
        .unwrap_or_else(|problems| panic!("the coin rule does not compile: {problems:#?}"))
        .entry(ModelCode::new(GridRule::CODE_FORMAT, rule.step.clone()))
}

/// The Life starter's entry, and a rule that draws from its random stream every tick, pass every check of the testing
/// kit, their GPU checks on a baseline device included.
#[test]
fn a_grid_rule_conforms() {
    let mut models = ModelSet::new(henad_core::build_info!());
    for entry in [life_entry(), coin_entry()] {
        models
            .insert(entry)
            .unwrap_or_else(|error| panic!("the set refused the rule: {error}"));
    }
    let mut settings = CheckSettings::default();
    if let Some(device) = headless_test_device(&TestDeviceRequest::baseline()) {
        settings = settings.gpu(device);
    } else {
        log::warn!("checking the Life starter without its GPU checks: no adapter");
    }
    assert_set_conforms(&models, &settings);
}
