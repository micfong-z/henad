//! Checks of the contracts between a GPU model's declarations and its shaders, made before anything is allocated.
//!
//! A shader that disagrees with its model about the workgroup size validates cleanly and runs wrong. So does a buffer
//! label that is reserved or ends in `_in` or `_out`, since a binding of that name resolves to something else. Both
//! engines assert these at construction instead.

use henad_core::authoring::model::binding::RESERVED;

/// Returns the `@workgroup_size` the `main` entry point of `source` declares, padded with 1 to three dimensions.
///
/// `None` when the attribute holds anything other than integer literals, such as an override constant. The text
/// henad-build composes is re-emitted by naga. `naga` writes the literals out.
pub(crate) fn declared_workgroup_size(source: &str) -> Option<[u32; 3]> {
    let entry = source.find("fn main(")?;
    let before = &source[..entry];
    let attribute = before.rfind("@workgroup_size(")?;
    // An attribute of another function would have its `fn` between it and `main`.
    if before[attribute..].contains("fn ") {
        return None;
    }
    let arguments = &before[attribute + "@workgroup_size(".len()..];
    let arguments = &arguments[..arguments.find(')')?];
    let mut size = [1u32; 3];
    let mut count = 0;
    for argument in arguments
        .split(',')
        .map(str::trim)
        .filter(|argument| !argument.is_empty())
    {
        let digits = argument.trim_end_matches(['u', 'i']);
        *size.get_mut(count)? = digits.parse().ok()?;
        count += 1;
    }
    (count > 0).then_some(size)
}

/// Asserts that the `main` entry point of `shader` declares the workgroup size the engine dispatches it in.
///
/// # Panics
///
/// When the shader declares another size. Otherwise the dispatch covers part of the domain, or runs invocations
/// past it, with no error.
pub(crate) fn assert_workgroup_size(model_id: &str, pass: &str, shader: &str, dispatched: [u32; 3]) {
    if let Some(declared) = declared_workgroup_size(shader) {
        assert!(
            declared == dispatched,
            "{model_id}: the {pass} shader declares @workgroup_size({}, {}, {}), and the engine dispatches it in \
             workgroups of ({}, {}, {})",
            declared[0],
            declared[1],
            declared[2],
            dispatched[0],
            dispatched[1],
            dispatched[2],
        );
    }
}

/// Asserts that no buffer label is reserved or ends in `_in` or `_out`. A binding of either name resolves to something
/// other than the buffer.
///
/// # Panics
///
/// On a reserved name. A binding of that name binds the engine's own resource. Also on a label ending in `_in` or
/// `_out`, a suffix the engine strips from a binding name before it looks the label up.
pub(crate) fn assert_buffer_labels<'a>(model_id: &str, labels: impl IntoIterator<Item = &'a str>) {
    for label in labels {
        assert!(
            !RESERVED.contains(&label),
            "{model_id}: buffer label `{label}` is reserved, and a binding of that name binds the engine's own \
             resource"
        );
        assert!(
            !label.ends_with("_in") && !label.ends_with("_out"),
            "{model_id}: buffer label `{label}` ends in `_in` or `_out`. The engine strips that suffix from a binding \
             name before it looks the label up"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::{assert_buffer_labels, assert_workgroup_size, declared_workgroup_size};

    #[test]
    fn a_composed_entry_point_reads_its_workgroup_size() {
        let composed = "struct Params {\n    n: u32,\n}\n\n@compute @workgroup_size(16, 16, 1) \nfn main(\
                        @builtin(global_invocation_id) gid: vec3<u32>) {\n}\n";
        assert_eq!(declared_workgroup_size(composed), Some([16, 16, 1]));
        assert_eq!(
            declared_workgroup_size("@compute @workgroup_size(256u) fn main() {}"),
            Some([256, 1, 1])
        );
        assert_eq!(
            declared_workgroup_size("@compute @workgroup_size(8, 4) fn main() {}"),
            Some([8, 4, 1])
        );
    }

    #[test]
    fn a_size_that_is_not_a_literal_is_left_unread() {
        assert_eq!(
            declared_workgroup_size("@compute @workgroup_size(WG, WG) fn main() {}"),
            None
        );
        assert_eq!(declared_workgroup_size("fn main() {}"), None);
        assert_eq!(
            declared_workgroup_size("@compute @workgroup_size(64) fn helper() {}\nfn main() {}"),
            None,
            "the attribute belongs to another function"
        );
    }

    #[test]
    #[should_panic(expected = "dispatches it in workgroups of (16, 16, 1)")]
    fn a_mismatched_workgroup_size_is_refused() {
        assert_workgroup_size(
            "test",
            "step",
            "@compute @workgroup_size(8, 8, 1) fn main() {}",
            [16, 16, 1],
        );
    }

    #[test]
    fn a_matching_workgroup_size_passes() {
        assert_workgroup_size(
            "test",
            "step",
            "@compute @workgroup_size(16, 16, 1) fn main() {}",
            [16, 16, 1],
        );
    }

    #[test]
    #[should_panic(expected = "buffer label `partials` is reserved")]
    fn a_reserved_label_is_refused() {
        assert_buffer_labels("test", ["state", "partials"]);
    }

    #[test]
    #[should_panic(expected = "buffer label `food_in` ends in `_in` or `_out`")]
    fn a_label_with_a_side_suffix_is_refused() {
        assert_buffer_labels("test", ["food_in"]);
    }

    #[test]
    fn plain_labels_pass() {
        assert_buffer_labels("test", ["state", "rng", "inbox", "outcome"]);
    }
}
