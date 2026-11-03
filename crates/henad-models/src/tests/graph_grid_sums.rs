//! Float sums of a grid graph reduced on the device by the GPU grid engine.
//!
//! The lane `x` holds `(index % 7) / 4` at every cell. Every partial sum of such values is a multiple of 0.25 far
//! below 2^22, which an `f32` holds exactly, so the device's sums equal the closed form whatever order it adds in.

use henad_core::params::ParamValue;
use henad_core::view::{StatEntry, StatValue};
use henad_explore::model_file::ModelFile;
use henad_explore::testing::{TestDeviceRequest, headless_test_device};

use super::graph_grid_parity::{build_gpu, snapshot, stat, step};

/// A grid whose init pass sets `x` to `(index % 7) / 4`, whose tick pass keeps it, and whose stats pass reports its
/// sum, its mean and the sum of `(x, -x)`.
const SUMS: &str = r#"format = "henad-model"
format_version = 1
kind = "graph"
id = "node_grid_sums"

[graph]
topology = "grid"
library = 1

[[target]]
backend = "gpu"

[grid]
width = 100
height = 37
boundary = "torus"

[[lane]]
id = "x"
domain = "cells"
type = "f32"

[[stat]]
label = "Total"
from = "n11"
color = [1, 1, 1, 255]

[[stat]]
label = "Mean"
from = "n12"
color = [2, 2, 2, 255]

[[stat]]
label = "Pair"
from = "n15"
color = [3, 3, 3, 255]

[[pass]]
id = "fill"
stage = "init"
kind = "map"
domain = "cells"

[[pass.node]]
id = "n1"
op = "ctx.index"

[[pass.node]]
id = "n2"
op = "math.rem"
in.a = "n1"
in.b = { u32 = 7 }

[[pass.node]]
id = "n3"
op = "conv.to_f32"
in.a = "n2"

[[pass.node]]
id = "n4"
op = "math.mul"
in.a = "n3"
in.b = { f32 = 0.25 }

[[pass.node]]
id = "n5"
op = "lane.set"
config.lane = "x"
in.value = "n4"

[[pass]]
id = "keep"
stage = "tick"
kind = "map"
domain = "cells"

[[pass.node]]
id = "n6"
op = "lane.get"
config.lane = "x"

[[pass.node]]
id = "n7"
op = "lane.set"
config.lane = "x"
in.value = "n6"

[[pass]]
id = "totals"
stage = "stats"
kind = "map"
domain = "cells"

[[pass.node]]
id = "n10"
op = "lane.get"
config.lane = "x"

[[pass.node]]
id = "n11"
op = "stat.sum"
in.value = "n10"

[[pass.node]]
id = "n12"
op = "stat.mean"
in.value = "n10"

[[pass.node]]
id = "n13"
op = "math.neg"
in.a = "n10"

[[pass.node]]
id = "n14"
op = "vec.make"
in.x = "n10"
in.y = "n13"

[[pass.node]]
id = "n15"
op = "stat.sum"
in.value = "n14"
"#;

/// Returns the sum of `(index % 7) / 4` over `cells` cells.
fn closed_form(cells: u32) -> f64 {
    (0..cells).map(|index| f64::from(index % 7) * 0.25).sum()
}

/// Returns the vector stat labelled `label`.
fn vector_stat(stats: &[StatEntry], label: &str) -> (f64, f64) {
    match stats.iter().find(|entry| entry.label() == label).map(StatEntry::value) {
        Some(StatValue::Vector2D { x, y }) => (*x, *y),
        other => panic!("no vector stat `{label}`: {other:?}"),
    }
}

/// A grid graph's `stat.sum` and `stat.mean` over an `f32` lane, and `stat.sum` over a `vec2f`, reduce on the device to
/// their closed forms at sizes that fill no workgroup, fill one exactly and leave a partial row and column of
/// workgroups, before and after a step, and replay.
#[test]
fn float_sums_of_a_grid_graph_reduce_on_the_device() {
    let Some(ctx) = headless_test_device(&TestDeviceRequest::baseline()) else {
        log::warn!("skipping float_sums_of_a_grid_graph_reduce_on_the_device: no adapter");
        return;
    };
    let entry = ModelFile::parse(SUMS)
        .and_then(|file| file.entry())
        .unwrap_or_else(|error| panic!("{error}"));
    for (width, height) in [(1, 1), (16, 16), (100, 37), (300, 211)] {
        let params = [ParamValue::U32(width), ParamValue::U32(height)];
        let cells = width * height;
        let expected = closed_form(cells);
        let mut first = Vec::new();
        for round in 0..2 {
            let mut state = build_gpu(&entry, &params, Some(5), &ctx);
            let mut seen = Vec::new();
            for steps in [0, 3] {
                step(&ctx, &mut *state, steps);
                snapshot(&ctx, &mut *state);
                let stats = state.stats();
                let what = format!("{width} by {height}, round {round}, after {steps} steps");
                assert_eq!(stat(&stats, "Total"), expected, "{what}");
                let mean = stat(&stats, "Mean");
                assert!(
                    (mean - expected / f64::from(cells)).abs() <= 1e-9 * expected.max(1.0),
                    "{what}: the mean is {mean}"
                );
                assert_eq!(vector_stat(&stats, "Pair"), (expected, -expected), "{what}");
                seen.push(format!("{stats:?}"));
            }
            if round == 0 {
                first = seen;
            } else {
                assert_eq!(seen, first, "{width} by {height}: the run replays");
            }
        }
    }
}
