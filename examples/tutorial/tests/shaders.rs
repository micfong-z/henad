//! Holds the GPU pages' shader copies to the shipped shaders they repeat.
//!
//! A shader carries no model id, so each copy equals its original byte for byte. The one exception is the import
//! path of `gpu_ants/state.wgsl`, which follows the directory name, and the comparison spells it `gpu_foraging` on
//! the shipped side before it compares.

/// Each tutorial copy beside the shipped shader it repeats, named by its path under `src/`.
const GPU_LIFE: [(&str, &str, &str); 3] = [
    (
        "gpu_life/step.wgsl",
        include_str!("../src/gpu_life/step.wgsl"),
        include_str!("../../../crates/henad-models/src/gpu_game_of_life/step.wgsl"),
    ),
    (
        "gpu_life/display.wgsl",
        include_str!("../src/gpu_life/display.wgsl"),
        include_str!("../../../crates/henad-models/src/gpu_game_of_life/display.wgsl"),
    ),
    (
        "gpu_life/reduce.wgsl",
        include_str!("../src/gpu_life/reduce.wgsl"),
        include_str!("../../../crates/henad-models/src/gpu_game_of_life/reduce.wgsl"),
    ),
];

/// As [`GPU_LIFE`], for the GPU ants page.
const GPU_FORAGING: [(&str, &str, &str); 5] = [
    (
        "gpu_foraging/state.wgsl",
        include_str!("../src/gpu_foraging/state.wgsl"),
        include_str!("../../../crates/henad-models/src/gpu_ants/state.wgsl"),
    ),
    (
        "gpu_foraging/step.wgsl",
        include_str!("../src/gpu_foraging/step.wgsl"),
        include_str!("../../../crates/henad-models/src/gpu_ants/step.wgsl"),
    ),
    (
        "gpu_foraging/merge.wgsl",
        include_str!("../src/gpu_foraging/merge.wgsl"),
        include_str!("../../../crates/henad-models/src/gpu_ants/merge.wgsl"),
    ),
    (
        "gpu_foraging/display.wgsl",
        include_str!("../src/gpu_foraging/display.wgsl"),
        include_str!("../../../crates/henad-models/src/gpu_ants/display.wgsl"),
    ),
    (
        "gpu_foraging/reduce.wgsl",
        include_str!("../src/gpu_foraging/reduce.wgsl"),
        include_str!("../../../crates/henad-models/src/gpu_ants/reduce.wgsl"),
    ),
];

#[test]
fn the_gpu_life_shaders_equal_the_shipped_ones() {
    for (path, copy, shipped) in GPU_LIFE {
        assert!(
            copy == shipped,
            "src/{path} differs from its shipped original in crates/henad-models/src/gpu_game_of_life/"
        );
    }
}

#[test]
fn the_gpu_foraging_shaders_equal_the_shipped_ones_apart_from_the_import_path() {
    let mut renamed = 0;
    for (path, copy, shipped) in GPU_FORAGING {
        renamed += shipped.matches("gpu_ants::state").count();
        assert!(
            copy == shipped.replace("gpu_ants::state", "gpu_foraging::state"),
            "src/{path} differs from its shipped original in crates/henad-models/src/gpu_ants/, \
             apart from the import path"
        );
    }
    // The declaration in `state.wgsl` and the imports of `step.wgsl` and `reduce.wgsl`.
    assert_eq!(
        renamed, 3,
        "the shipped shaders name `gpu_ants::state` in other places than before"
    );
}
