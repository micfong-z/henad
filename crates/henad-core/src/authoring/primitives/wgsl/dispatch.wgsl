#define_import_path henad::dispatch

// Invocations per workgroup of a linear pass.
const WORKGROUP: u32 = 256u;

// Folds a linear invocation domain onto the 2D workgroup grid that `linear_dispatch` picks. 100M
// agents overflow one row of workgroups.
fn linear_index(lid: vec3<u32>, wid: vec3<u32>, groups_x: u32) -> u32 {
    return (wid.y * groups_x + wid.x) * WORKGROUP + lid.x;
}
