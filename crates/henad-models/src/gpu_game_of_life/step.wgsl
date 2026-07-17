// Bit-packed Game of Life step: 32 cells per u32, one invocation per word.

@group(0) @binding(0) var<storage, read> current: array<u32>;
@group(0) @binding(1) var<storage, read_write> next: array<u32>;
@group(0) @binding(2) var<uniform> dims: vec2<u32>;

// Preloaded row window, with west and east being the cells shifted by 1 bit left and right, respectively.
struct Row {
    cells: u32, // bit j = cell (word*32 + j)
    west: u32,  // bit j = its west neighbour
    east: u32,  // bit j = its east neighbour
}

fn load_row(row: u32, word: u32, stride: u32, width: u32) -> Row {
    let base = row * stride;
    let mid = current[base + word];
    let left = current[base + (word + stride - 1u) % stride];
    let right = current[base + (word + 1u) % stride];

    var r: Row;
    r.cells = mid;
    r.west = (mid << 1u) | (left >> 31u);   // bit 0 comes from the previous word's bit 31
    r.east = (mid >> 1u) | (right << 31u);  // bit 31 comes from the next word's bit 0

    // Those two shifts assume the grid's x-wrap lands on a word edge, which holds only when
    // width % 32 == 0. When the last word is ragged, exactly two bits are wrong, and need to be fixed.
    // When it isn't ragged, both patches rewrite the value that's already there.
    let last = width - 1u;
    if word == 0u {
        r.west = (r.west & ~1u) | ((left >> (last % 32u)) & 1u);
    }
    if word == last / 32u {
        let b = last % 32u;
        r.east = (r.east & ~(1u << b)) | ((right & 1u) << b);
    }
    return r;
}

@compute
@workgroup_size(16, 16)
fn main(@builtin(global_invocation_id) global_id: vec3<u32>) {
    let width = dims.x;
    let height = dims.y;
    let stride = (width + 31u) / 32u;

    let word = global_id.x;
    let y = global_id.y;
    if word >= stride || y >= height {
        return;
    }

    let up = (y + height - 1u) % height;
    let down = (y + 1u) % height;
    let r_up = load_row(up, word, stride, width);
    let r_mid = load_row(y, word, stride, width);
    let r_down = load_row(down, word, stride, width);

    var out_word: u32 = 0u;
    for (var i: u32 = 0; i < 32; i++) {
        // Trailing bits of the last word are padding when width % 32 != 0: no cell lives there, so
        // leave them zero. Nothing ever reads them back — display and reduce are bounded by width.
        if word * 32u + i >= width {
            break;
        }

        var n = ((r_up.west >> i) & 1u)   + ((r_up.cells >> i) & 1u)   + ((r_up.east >> i) & 1u)
                        + ((r_mid.west >> i) & 1u)                               + ((r_mid.east >> i) & 1u)
                        + ((r_down.west >> i) & 1u) + ((r_down.cells >> i) & 1u) + ((r_down.east >> i) & 1u);

        let cell = (r_mid.cells >> i) & 1u;
        let alive = (cell == 1u && (n == 2u || n == 3u))
            || (cell == 0u && n == 3u);
        out_word |= u32(alive) << u32(i);
    }

    next[y * stride + word] = out_word;
}
