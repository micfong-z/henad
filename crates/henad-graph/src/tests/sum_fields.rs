//! Fields that add up their deposits: the `resolution` key in the reader and the canonical writer, and the checker's
//! wrap bound.

use super::support::{graph_errors, graph_refusal};
use crate::GraphFile;
use crate::document::FieldCombine;

/// Returns an agents model whose field `f` holds the keys `combine`, whose parameter `amount` reaches `max`, and whose
/// tick pass deposits `value` into `f` at each agent's cell.
fn model(combine: &str, max: f32, value: &str) -> String {
    format!(
        r#"format = "henad-model"
format_version = 1
kind = "graph"
id = "node_sum"

[graph]
topology = "agents"
library = 1

[[target]]
backend = "gpu"

[agents]
count = 8
width = 20.0
height = 10.0

[[lane]]
id = "pos"
domain = "agents"
type = "vec2f"
role = "position"

[[field]]
id = "f"
type = "f32"
layers = ["value"]
static = false
{combine}

[[param]]
id = "amount"
label = "Amount"
kind = "f32"
min = 0.0
max = {max:?}
default = 0.0

[[pass]]
id = "lay"
stage = "tick"
kind = "map"
domain = "agents"

[[pass.node]]
id = "n1"
op = "lane.get"
config.lane = "pos"

[[pass.node]]
id = "n2"
op = "space.cell_of"
in.p = "n1"

[[pass.node]]
id = "n3"
op = "field.deposit"
config.field = "f"
in.layer = {{ layer = "f.value" }}
in.at = "n2.cell"
in.value = {value}
"#
    )
}

/// A sum at 2^-16 written as a decimal reads, writes its resolution in the writer's number form, and reads back to
/// the same text.
#[test]
fn a_sum_field_round_trips() {
    let text = model(
        "combine = \"sum\"\nresolution = 0.0000152587890625",
        1.0,
        "{ f32 = 0.5 }",
    );
    let file = GraphFile::parse(&text).unwrap_or_else(|error| panic!("{error}"));
    let combine = file.document().fields[0].combine;
    assert_eq!(combine, Some(FieldCombine::Sum { resolution_shift: 16 }));
    assert_eq!(combine.and_then(FieldCombine::resolution), Some(1.0 / 65536.0));
    assert_eq!(combine.and_then(FieldCombine::wrap_bound), Some(65536.0));
    let canonical = file.to_toml();
    assert!(
        canonical.contains("static = false\ncombine = \"sum\"\nresolution = 1.52587890625e-5\n"),
        "{canonical}"
    );
    let again = GraphFile::parse(&canonical).unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(again.to_toml(), canonical);
    assert_eq!(again.document().fields[0].combine, combine);
    assert!(graph_errors(&text).is_empty(), "{:?}", graph_errors(&text));

    let whole = model("combine = \"sum\"\nresolution = 1", 1.0, "{ f32 = 0.5 }");
    let file = GraphFile::parse(&whole).unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(
        file.document().fields[0].combine,
        Some(FieldCombine::Sum { resolution_shift: 0 })
    );
    assert!(file.to_toml().contains("resolution = 1.0\n"), "{}", file.to_toml());
}

/// A resolution that is not exactly 2^-k for k from 0 to 24 is refused at its line, with the nearest allowed value.
#[test]
fn a_resolution_that_is_not_a_power_of_two_is_refused() {
    let resolution_line = model("combine = \"sum\"\nresolution = 1.0", 1.0, "{ f32 = 0.5 }")
        .lines()
        .position(|line| line.starts_with("resolution"))
        .and_then(|index| u32::try_from(index + 1).ok());
    for (written, nearest) in [
        ("0.001", "0.0009765625 (2^-10)"),
        ("0.0", "5.960464477539063e-8 (2^-24)"),
        ("-0.25", "5.960464477539063e-8 (2^-24)"),
        ("2.98023223876953125e-8", "5.960464477539063e-8 (2^-24)"),
        ("3.0", "1.0 (2^-0)"),
        ("0.3", "0.25 (2^-2)"),
    ] {
        let refusal = graph_refusal(&model(
            &format!("combine = \"sum\"\nresolution = {written}"),
            1.0,
            "{ f32 = 0.5 }",
        ));
        let (message, line) = (refusal.message, refusal.line);
        assert!(
            message.contains("is not a power of two from 1 down to 2^-24"),
            "{written}: {message}"
        );
        assert!(
            message.contains(&format!("The nearest allowed value is {nearest}")),
            "{written}: {message}"
        );
        assert_eq!(line, resolution_line, "{written}: {message}");
    }
}

/// A resolution belongs to a sum alone, and a sum needs one.
#[test]
fn a_resolution_needs_a_sum_and_a_sum_needs_a_resolution() {
    let message = graph_refusal(&model("combine = \"max\"\nresolution = 0.25", 1.0, "{ f32 = 0.5 }")).message;
    assert!(
        message.contains("the field 'f' cannot have a `resolution` without `combine = \"sum\"`"),
        "{message}"
    );
    let message = graph_refusal(&model("resolution = 0.25", 1.0, "{ f32 = 0.5 }")).message;
    assert!(message.contains("without `combine = \"sum\"`"), "{message}");
    let message = graph_refusal(&model("combine = \"sum\"", 1.0, "{ f32 = 0.5 }")).message;
    assert!(
        message.contains("the field 'f' adds up its deposits, and needs a `resolution`"),
        "{message}"
    );
}

/// A deposit whose literal or parameter reaches 2^32 times the resolution is refused with the bound, and one below it
/// is not.
#[test]
fn a_deposit_at_the_wrap_bound_is_refused() {
    let sum = "combine = \"sum\"\nresolution = 1.52587890625e-5";
    let found = graph_errors(&model(sum, 1.0, "{ f32 = 65536.0 }"));
    assert!(
        found.iter().any(
            |message| message.contains("n3 deposits up to 65536.0 into the field 'f'")
                && message.contains("wraps once it takes 65536.0 in a tick (2^32 times its resolution)")
        ),
        "{found:?}"
    );
    let found = graph_errors(&model(sum, 70000.0, "{ param = \"amount\" }"));
    assert!(
        found.iter().any(|message| message.contains("deposits up to 70000.0")),
        "{found:?}"
    );
    for (max, value) in [(1.0, "{ f32 = 65535.0 }"), (65535.0, "{ param = \"amount\" }")] {
        let found = graph_errors(&model(sum, max, value));
        assert!(found.is_empty(), "{value}: {found:?}");
    }
    let found = graph_errors(&model("combine = \"max\"", 1.0, "{ f32 = 1e30 }"));
    assert!(found.is_empty(), "a max has no bound: {found:?}");
}

/// The deposit's scale and the merge's resolution come from one exponent for every `resolution_shift`, and a `Sum`
/// built by hand past the largest exponent counts, merges and wraps at 2^-24.
#[test]
fn a_sum_counts_and_merges_at_one_resolution() {
    for resolution_shift in 0..=u8::MAX {
        let combine = FieldCombine::Sum { resolution_shift };
        let (Some(scale), Some(resolution), Some(bound)) =
            (combine.deposit_scale(), combine.resolution(), combine.wrap_bound())
        else {
            panic!("a sum at 2^-{resolution_shift} has a scale, a resolution and a bound");
        };
        assert_eq!(scale * resolution, 1.0, "2^-{resolution_shift}");
        assert_eq!(bound, f64::from(resolution) * 4_294_967_296.0, "2^-{resolution_shift}");
    }
    let past = FieldCombine::Sum { resolution_shift: 30 };
    assert_eq!(past.deposit_scale(), Some(16_777_216.0));
    assert_eq!(past.resolution(), Some(1.0 / 16_777_216.0));
    assert_eq!(past.wrap_bound(), Some(256.0));
    assert_eq!(FieldCombine::Max.deposit_scale(), None);
}
