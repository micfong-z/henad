@group(0) @binding(0) var<storage, read> current: array<u32>;
@group(0) @binding(1) var<storage, read_write> next: array<u32>;
@group(0) @binding(2) var<uniform> dims: vec2<u32>;

@compute
@workgroup_size(16, 16)
fn main(@builtin(global_invocation_id) global_id: vec3<u32>) {
    let width = dims.x;
    let height = dims.y;
    if (global_id.x >= width || global_id.y >= height) {
        return;
    }

    let x = global_id.x;
    let y = global_id.y;

    let left = (x + width - 1u) % width;
    let right = (x + 1u) % width;
    let up = (y + height - 1u) % height;
    let down = (y + 1u) % height;

    var alive_count: u32 = 0u;
    alive_count += current[up * width + left];
    alive_count += current[up * width + x];
    alive_count += current[up * width + right];
    alive_count += current[y * width + left];
    alive_count += current[y * width + right];
    alive_count += current[down * width + left];
    alive_count += current[down * width + x];
    alive_count += current[down * width + right];

    let cell = current[y * width + x];
    let alive = (cell == 1u && (alive_count == 2u || alive_count == 3u))
        || (cell == 0u && alive_count == 3u);

    next[y * width + x] = select(0u, 1u, alive);
}
