//! Holds each `shared::space` primitive to its Rust twin on a real device.
//!
//! Drives `shared/parity.wgsl`, where the ops themselves live.
//!
//! Integer results must match exactly. Float results are compared with a tolerance, since WGSL's
//! float `%` is defined through a division while Rust's is an exact fmod, so the two round
//! differently on inputs far outside the world.

use henad_core::authoring::primitives::rng;
use henad_core::authoring::primitives::space::{self, Boundary, MOORE_COLUMN_MAJOR, MOORE_ROW_MAJOR, VON_NEUMANN};

use crate::gpu::headless_context;
use crate::gpu::primitives::pipeline::compute_pipeline;
use crate::shader_bindings::shared::parity::{
    Case, OP_AXIS_DELTA, OP_BELOW, OP_CELL_INDEX, OP_CHOICE3, OP_DIST_SQ, OP_HEADING_OCTANT, OP_NEIGHBOR_COUNT,
    OP_NEIGHBOR_OFFSET, OP_OFFSET_CELL, OP_RANDOM_FLOAT, OP_RESERVOIR_ACCEPT, OP_WRAP_COORD, OP_WRAP_INDEX, Out,
    SHADER_STRING, WgpuBindGroup0,
};
use crate::shader_bindings::shared::space as codes;

/// Absolute slack allowed on a float result that goes through WGSL's float `%`.
const TOLERANCE: f32 = 1e-4;

/// Slack for `op`, zero where the two sides are meant to agree bit for bit.
///
/// Only `random_float` is exact. Everything else float-valued reaches WGSL's `%`, which is defined
/// through a division where Rust's is an fmod, so the two round apart on inputs far outside the
/// world.
fn tolerance_for(op: u32) -> f32 {
    if op == OP_RANDOM_FLOAT { 0.0 } else { TOLERANCE }
}

/// A case plus the result its Rust twin produces.
struct Check {
    case: Case,
    expected: Out,
    /// The primitive and its arguments, so a failure names the case that broke.
    call: String,
}

fn blank(op: u32) -> Case {
    Case {
        op,
        boundary: 0,
        table: 0,
        n: 0,
        i: [0; 4],
        u: [0; 4],
        f: [0.0; 4],
        g: [0.0; 4],
    }
}

fn ints(i: [i32; 4]) -> Out {
    Out { i, f: [0.0; 4] }
}

fn float(v: f32) -> Out {
    Out {
        i: [0; 4],
        f: [v, 0.0, 0.0, 0.0],
    }
}

/// The WGSL side takes a `u32`, so the two spellings of a boundary meet here.
fn boundary_code(boundary: Boundary) -> u32 {
    match boundary {
        Boundary::Torus => codes::TORUS,
        Boundary::Bounded => codes::BOUNDED,
    }
}

fn rust_table(code: u32) -> &'static [(i32, i32)] {
    match code {
        codes::MOORE_COLUMN_MAJOR => &MOORE_COLUMN_MAJOR,
        codes::VON_NEUMANN => &VON_NEUMANN,
        _ => &MOORE_ROW_MAJOR,
    }
}

fn wrap_index_checks(out: &mut Vec<Check>) {
    for m in [1i32, 3, 8, 16, 64] {
        for v in -20i32..=20 {
            let mut case = blank(OP_WRAP_INDEX);
            case.i = [v, m, 0, 0];
            out.push(Check {
                case,
                expected: ints([space::wrap_index(v, m), 0, 0, 0]),
                call: format!("wrap_index({v}, {m})"),
            });
        }
    }
}

fn wrap_coord_checks(out: &mut Vec<Check>) {
    for world in [1.0f32, 7.5, 10.0, 128.0] {
        for k in -40i32..=40 {
            let v = k as f32 * 0.5;
            let mut case = blank(OP_WRAP_COORD);
            case.f = [v, world, 0.0, 0.0];
            out.push(Check {
                case,
                expected: float(space::wrap_coord(v, world)),
                call: format!("wrap_coord({v}, {world})"),
            });
        }
    }
}

fn cell_index_checks(out: &mut Vec<Check>) {
    for w in [1u32, 10, 64, 4096] {
        for y in 0u32..8 {
            for x in 0u32..8 {
                let mut case = blank(OP_CELL_INDEX);
                case.u = [x, y, w, 0];
                out.push(Check {
                    case,
                    expected: ints([space::cell_index(x, y, w) as i32, 0, 0, 0]),
                    call: format!("cell_index({x}, {y}, {w})"),
                });
            }
        }
    }
}

fn offset_cell_checks(out: &mut Vec<Check>) {
    for boundary in [Boundary::Torus, Boundary::Bounded] {
        for (w, h) in [(1u32, 1u32), (4, 4), (7, 3), (16, 9)] {
            for y in 0..h.min(4) {
                for x in 0..w.min(4) {
                    // Reaches two cells out, so a bounded edge is crossed by more than one step.
                    for dy in -2i32..=2 {
                        for dx in -2i32..=2 {
                            let mut case = blank(OP_OFFSET_CELL);
                            case.boundary = boundary_code(boundary);
                            case.u = [x, y, w, h];
                            case.i = [dx, dy, 0, 0];
                            let expected = match space::offset_cell(x, y, dx, dy, w, h, boundary) {
                                Some((nx, ny)) => ints([nx as i32, ny as i32, 1, 0]),
                                None => ints([0, 0, 0, 0]),
                            };
                            out.push(Check {
                                case,
                                expected,
                                call: format!("offset_cell({x}, {y}, {dx}, {dy}, {w}, {h}, {boundary:?})"),
                            });
                        }
                    }
                }
            }
        }
    }
}

fn axis_delta_checks(out: &mut Vec<Check>) {
    for boundary in [Boundary::Torus, Boundary::Bounded] {
        for world in [1.0f32, 10.0, 137.5] {
            for j in 0i32..12 {
                for i in 0i32..12 {
                    let a = i as f32 * world / 12.0;
                    let b = j as f32 * world / 12.0;
                    let mut case = blank(OP_AXIS_DELTA);
                    case.boundary = boundary_code(boundary);
                    case.f = [a, b, world, 0.0];
                    out.push(Check {
                        case,
                        expected: float(space::axis_delta(a, b, world, boundary)),
                        call: format!("axis_delta({a}, {b}, {world}, {boundary:?})"),
                    });
                }
            }
        }
    }
}

fn dist_sq_checks(out: &mut Vec<Check>) {
    for boundary in [Boundary::Torus, Boundary::Bounded] {
        let (world_w, world_h) = (10.0f32, 6.0f32);
        for j in 0i32..8 {
            for i in 0i32..8 {
                let (ax, ay) = (0.5, 0.5);
                let (bx, by) = (i as f32 * 1.25, j as f32 * 0.75);
                let mut case = blank(OP_DIST_SQ);
                case.boundary = boundary_code(boundary);
                case.f = [ax, ay, bx, by];
                case.g = [world_w, world_h, 0.0, 0.0];
                out.push(Check {
                    case,
                    expected: float(space::dist_sq(ax, ay, bx, by, world_w, world_h, boundary)),
                    call: format!("dist_sq(({ax}, {ay}), ({bx}, {by}), {boundary:?})"),
                });
            }
        }
    }
}

fn neighbor_checks(out: &mut Vec<Check>) {
    for code in [codes::MOORE_ROW_MAJOR, codes::MOORE_COLUMN_MAJOR, codes::VON_NEUMANN] {
        let table = rust_table(code);

        let mut case = blank(OP_NEIGHBOR_COUNT);
        case.table = code;
        out.push(Check {
            case,
            expected: ints([table.len() as i32, 0, 0, 0]),
            call: format!("neighbor_count({code})"),
        });

        for (n, &(dx, dy)) in table.iter().enumerate() {
            let mut case = blank(OP_NEIGHBOR_OFFSET);
            case.table = code;
            case.n = n as u32;
            out.push(Check {
                case,
                expected: ints([dx, dy, 0, 0]),
                call: format!("neighbor_offset({code}, {n})"),
            });
        }
    }
}

fn heading_octant_checks(out: &mut Vec<Check>) {
    for step in 0..64u32 {
        let angle = step as f32 * std::f32::consts::TAU / 64.0;
        let (vx, vy) = (angle.cos(), angle.sin());
        let mut case = blank(OP_HEADING_OCTANT);
        case.f = [vx, vy, 0.0, 0.0];
        out.push(Check {
            case,
            expected: ints([i32::from(space::heading_octant(vx, vy)), 0, 0, 0]),
            call: format!("heading_octant({vx}, {vy})"),
        });
    }
}

/// Bit patterns chosen to hit the ends and the awkward middles rather than a uniform sweep.
const WORDS: [u32; 12] = [
    0,
    1,
    2,
    3,
    17,
    255,
    1 << 16,
    u32::MAX / 3,
    u32::MAX / 2,
    u32::MAX - 2,
    u32::MAX - 1,
    u32::MAX,
];

fn rng_checks(out: &mut Vec<Check>) {
    for bits in WORDS {
        for max in [1.0f32, 0.5, 10.0] {
            let mut case = blank(OP_RANDOM_FLOAT);
            case.u = [bits, 0, 0, 0];
            case.f = [max, 0.0, 0.0, 0.0];
            out.push(Check {
                case,
                expected: float(rng::random_float(bits, max)),
                call: format!("random_float({bits}, {max})"),
            });
        }

        let mut case = blank(OP_CHOICE3);
        case.u = [bits, 0, 0, 0];
        out.push(Check {
            case,
            expected: ints([rng::choice3(bits), 0, 0, 0]),
            call: format!("choice3({bits})"),
        });

        for threshold in [0u32, 1, u32::MAX / 2, u32::MAX] {
            let mut case = blank(OP_BELOW);
            case.u = [bits, threshold, 0, 0];
            out.push(Check {
                case,
                expected: ints([i32::from(rng::below(bits, threshold)), 0, 0, 0]),
                call: format!("below({bits}, {threshold})"),
            });
        }

        // 0 is in range because the ants tie-break starts its counter at 2, not 1, and a future
        // caller could start it anywhere.
        for count in [0u32, 1, 2, 3, 8, 64] {
            let mut case = blank(OP_RESERVOIR_ACCEPT);
            case.u = [bits, count, 0, 0];
            out.push(Check {
                case,
                expected: ints([i32::from(rng::reservoir_accept(bits, count)), 0, 0, 0]),
                call: format!("reservoir_accept({bits}, {count})"),
            });
        }
    }
}

fn all_checks() -> Vec<Check> {
    let mut out = Vec::new();
    wrap_index_checks(&mut out);
    wrap_coord_checks(&mut out);
    cell_index_checks(&mut out);
    offset_cell_checks(&mut out);
    axis_delta_checks(&mut out);
    dist_sq_checks(&mut out);
    neighbor_checks(&mut out);
    heading_octant_checks(&mut out);
    rng_checks(&mut out);
    out
}

/// Runs every case through the parity shader in one dispatch.
fn run_on_gpu(cases: &[Case]) -> Option<Vec<Out>> {
    let ctx = headless_context("henad_space_parity", wgpu::Features::empty())?;
    let (device, queue) = (&ctx.device, &ctx.queue);

    let case_bytes: &[u8] = bytemuck::cast_slice(cases);
    let out_size = (std::mem::size_of::<Out>() * cases.len()) as u64;

    let case_buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("parity_cases"),
        size: case_bytes.len() as u64,
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    queue.write_buffer(&case_buffer, 0, case_bytes);

    let result_buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("parity_results"),
        size: out_size,
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
        mapped_at_creation: false,
    });
    let staging = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("parity_staging"),
        size: out_size,
        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });

    let layout = device.create_bind_group_layout(&WgpuBindGroup0::LAYOUT_DESCRIPTOR);
    let pipeline = compute_pipeline(device, "henad_space_parity", SHADER_STRING, &layout);
    let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("parity_bind"),
        layout: &layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: case_buffer.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: result_buffer.as_entire_binding(),
            },
        ],
    });

    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("parity_encoder"),
    });
    {
        let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some("parity_pass"),
            timestamp_writes: None,
        });
        pass.set_pipeline(&pipeline);
        pass.set_bind_group(0, &bind, &[]);
        pass.dispatch_workgroups((cases.len() as u32).div_ceil(64), 1, 1);
    }
    encoder.copy_buffer_to_buffer(&result_buffer, 0, &staging, 0, out_size);
    queue.submit(Some(encoder.finish()));

    let (tx, rx) = flume::bounded(1);
    staging
        .slice(..)
        .map_async(wgpu::MapMode::Read, move |r| drop(tx.send(r)));
    device
        .poll(wgpu::PollType::wait_indefinitely())
        .expect("the device should drain");
    rx.recv()
        .expect("the map callback should fire")
        .expect("the staging buffer should map");

    let view = staging.slice(..).get_mapped_range().expect("mapped range");
    let results: Vec<Out> = bytemuck::cast_slice(&view).to_vec();
    drop(view);
    staging.unmap();

    Some(results)
}

/// A WGSL primitive and its Rust twin must be the same function.
#[test]
fn every_space_primitive_agrees_with_its_wgsl_twin() {
    let checks = all_checks();
    let cases: Vec<Case> = checks.iter().map(|c| c.case).collect();

    let Some(results) = run_on_gpu(&cases) else {
        return;
    };
    assert_eq!(results.len(), checks.len(), "one result per case");

    let mut failures = Vec::new();
    for (check, got) in checks.iter().zip(&results) {
        if check.expected.i != got.i {
            failures.push(format!(
                "{}: expected ints {:?}, got {:?}",
                check.call, check.expected.i, got.i
            ));
            continue;
        }
        let tolerance = tolerance_for(check.case.op);
        for (k, (expected, got)) in check.expected.f.iter().zip(&got.f).enumerate() {
            if (expected - got).abs() > tolerance {
                failures.push(format!("{}: float {k} expected {expected}, got {got}", check.call));
            }
        }
    }

    assert!(
        failures.is_empty(),
        "{} of {} cases disagree:\n{}",
        failures.len(),
        checks.len(),
        failures.iter().take(20).cloned().collect::<Vec<_>>().join("\n")
    );
}
