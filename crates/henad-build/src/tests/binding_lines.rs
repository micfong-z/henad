//! The binding reader, against the declarations `wgsl_bindgen` derives from the composed module.

use std::path::Path;

use henad_core::authoring::model::binding::BindingKind;

use super::support::Scratch;
use crate::binding_lines::{Binding, read_bindings};
use crate::{ShaderBuild, ShaderBuildError};

/// A shader declaring every kind the reader knows, out of `@binding` order, with comments in the way.
const EVERY_KIND: &str = "\
#import henad::dims::Dims

// @binding(9) in a comment is no declaration.
/* So is @binding(8) in a block, /* nested */ or not. */
@group(0) @binding(3) var output: texture_storage_2d<rgba8unorm, write>; // the display
@group(0) @binding(1) var<storage, read_write> state_out: array<u32>;
@group(0) @binding(0) var<storage, read>       state: array<u32>;
@group(0) @binding(2) var<uniform> dims: Dims;
@group(0) @binding(4) var<storage> counters: array<u32>;
@group(1) @binding(0) var<uniform> other_group: vec4<f32>;

@compute
@workgroup_size(8, 8)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let cell = gid.y * dims.grid.x + gid.x;
    state_out[cell] = state[cell] + counters[0] + u32(other_group.x);
    textureStore(output, gid.xy, vec4<f32>(1.0));
}
";

/// Returns the bindings the `WgpuBindGroup0` layout of `shader_bindings` lists, read from its doc lines and types, in
/// `@binding` order. The layout lists them in declaration order.
fn generated_layout(shader_bindings: &str) -> Vec<Binding> {
    let start = shader_bindings
        .find("pub const LAYOUT_DESCRIPTOR")
        .expect("group 0 has a layout");
    let end = start
        + shader_bindings[start..]
            .find("pub fn get_bind_group_layout")
            .expect("the layout ends");
    let mut bindings: Vec<Binding> = Vec::new();
    for line in shader_bindings[start..end].lines().map(str::trim) {
        if let Some(doc) = line.strip_prefix("#[doc = \" @binding(") {
            let (index, rest) = doc.split_once("): \\\"").expect("the doc names the binding");
            let name = rest.split_once("\\\"").expect("the name is quoted").0;
            bindings.push(Binding {
                index: index.parse().expect("the index is a number"),
                name: name.to_owned(),
                kind: BindingKind::Uniform,
            });
        } else if let Some(last) = bindings.last_mut() {
            if line.starts_with("ty: wgpu::BufferBindingType::Storage { read_only: true }") {
                last.kind = BindingKind::Storage { read_only: true };
            } else if line.starts_with("ty: wgpu::BufferBindingType::Storage { read_only: false }") {
                last.kind = BindingKind::Storage { read_only: false };
            } else if line.starts_with("ty: wgpu::BindingType::StorageTexture") {
                last.kind = BindingKind::StorageTexture;
            }
        }
    }
    bindings.sort_by_key(|binding| binding.index);
    bindings
}

#[test]
fn the_binding_parser_matches_the_generated_declarations() {
    let scratch = Scratch::new("parser");
    scratch.write("every_kind.wgsl", EVERY_KIND);
    ShaderBuild::discover(scratch.root())
        .expect("the shader is found")
        .generate_in(&scratch.out_dir())
        .expect("the shader composes");

    let read = read_bindings(Path::new("every_kind.wgsl"), EVERY_KIND).expect("every line is in the one form");
    let names: Vec<&str> = read.iter().map(|binding| binding.name.as_str()).collect();
    assert_eq!(names, ["state", "state_out", "dims", "output", "counters"]);
    assert_eq!(read, generated_layout(&scratch.generated("shader_bindings.rs")));

    let decls = scratch.generated("binding_decls.rs");
    assert!(
        decls.contains(
            "        BindingDecl { name: \"state\", kind: BindingKind::Storage { read_only: true } },\n        \
             BindingDecl { name: \"state_out\", kind: BindingKind::Storage { read_only: false } },\n        \
             BindingDecl { name: \"dims\", kind: BindingKind::Uniform },\n        \
             BindingDecl { name: \"output\", kind: BindingKind::StorageTexture },\n        \
             BindingDecl { name: \"counters\", kind: BindingKind::Storage { read_only: true } },\n"
        ),
        "{decls}"
    );
    assert!(decls.contains("super::shader_bindings::every_kind::WgpuBindGroup0::LAYOUT_DESCRIPTOR.entries.len()"));
}

#[test]
fn a_binding_line_in_another_form_is_refused() {
    let path = Path::new("pass.wgsl");
    for (line, reason) in [
        ("@binding(2) @group(0) var<uniform> params: Params;", "`@group(G)`"),
        ("@binding (0) @group(0) var<uniform> params: Params;", "`@group(G)`"),
        ("@group (0) @binding (0) var<uniform> params: Params;", "`@group(G)`"),
        ("@group(0)\n@binding(0) var<uniform> params: Params;", "`@binding(N)`"),
        ("@group(0) @binding(0)\nvar<uniform> params: Params;", "`var`"),
        ("@group(0) @binding(0) var<uniform> params:\n    Params;", "`;`"),
        ("@group(0) @binding(x) var<uniform> params: Params;", "`@binding(N)`"),
        (
            "@group(0) @binding(0) var input_tex: texture_2d<f32>;",
            "sampled texture",
        ),
        ("@group(0) @binding(0) var input_sampler: sampler;", "sampled texture"),
        (
            "@group(0) @binding(0) var<storage, write> data: array<u32>;",
            "address space",
        ),
        ("@group(0) @binding(0) var<private> data: u32;", "address space"),
    ] {
        match read_bindings(path, line) {
            Err(ShaderBuildError::BindingLine {
                reason: found,
                line: number,
                ..
            }) => {
                assert!(found.contains(reason), "{line:?}: {found}");
                assert_eq!(number, 1, "{line:?}");
            }
            other => panic!("{line:?}: expected a refused line, got {other:?}"),
        }
    }

    let gap = "@group(0) @binding(0) var<uniform> params: Params;\n\
               @group(0) @binding(2) var<storage, read> state: array<u32>;\n";
    assert!(matches!(
        read_bindings(path, gap),
        Err(ShaderBuildError::BindingGap { indices, .. }) if indices == [0, 2]
    ));
    let repeat = "@group(0) @binding(0) var<uniform> params: Params;\n\
                  @group(0) @binding(0) var<storage, read> state: array<u32>;\n";
    assert!(matches!(
        read_bindings(path, repeat),
        Err(ShaderBuildError::BindingGap { .. })
    ));
}

#[test]
fn a_binding_in_a_module_is_refused() {
    let scratch = Scratch::new("module_binding");
    scratch.write("gpu_vote/step.wgsl", super::support::STEP_SHADER);
    scratch.write(
        "gpu_vote/state.wgsl",
        "#define_import_path gpu_vote::state\n\n// @binding(9) in a comment is fine.\n\
         @group(0) @binding(1) var<storage, read> votes: array<u32>;\n",
    );
    match ShaderBuild::discover(scratch.root())
        .expect("the shaders are found")
        .generate_in(&scratch.out_dir())
    {
        Err(error @ ShaderBuildError::ModuleBinding { line: 4, .. }) => {
            assert!(error.to_string().contains("belong in the entry shader"), "{error}");
        }
        other => panic!("expected a refused module binding, got {other:?}"),
    }
}
