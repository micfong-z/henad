// Brings `Dims` into this crate's generated bindings. The grid engine takes the uniform layout from there.
// The pass is never dispatched.

#import henad::dims::Dims

@group(0) @binding(0) var<uniform> dims: Dims;

@compute
@workgroup_size(1)
fn main() {
    _ = dims;
}
