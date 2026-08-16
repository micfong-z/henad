// Bit-packed Game of Life step, 32 cells per u32 and one invocation per word.
//
// The rule is evaluated SWAR-style. A u32 is 32 independent 1-bit lanes, and the neighbour count
// is kept bit-sliced, so sb0/sb1/sb2 each hold one bit position of all 32 counts rather than one
// 4-bit count per lane. Summing is then a carry-save adder made of plain XOR/AND, so all 32 cells
// resolve at once with no loop.

@group(0) @binding(0) var<storage, read> state_in: array<u32>;
@group(0) @binding(1) var<storage, read_write> state_out: array<u32>;
@group(0) @binding(2) var<uniform> params: vec2<u32>;

// Preloaded row window, with west and east being the cells shifted by 1 bit left and right, respectively.
struct Row {
    cells: u32, // bit j = cell (word*32 + j)
    west: u32,  // bit j = its west neighbour
    east: u32,  // bit j = its east neighbour
}

// One column of the adder tree: `sum` is the weight-w result, `carry` feeds weight 2w.
struct Adder {
    sum: u32,
    carry: u32,
}

fn full_add(a: u32, b: u32, c: u32) -> Adder {
    let t = a ^ b;
    return Adder(t ^ c, (a & b) | (c & t));
}

fn half_add(a: u32, b: u32) -> Adder {
    return Adder(a ^ b, a & b);
}

fn load_row(row: u32, word: u32, stride: u32, width: u32) -> Row {
    let base = row * stride;
    let mid = state_in[base + word];
    let left = state_in[base + (word + stride - 1u) % stride];
    let right = state_in[base + (word + 1u) % stride];

    var r: Row;
    r.cells = mid;
    r.west = (mid << 1u) | (left >> 31u);   // bit 0 comes from the previous word's bit 31
    r.east = (mid >> 1u) | (right << 31u);  // bit 31 comes from the state_out word's bit 0

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
    let width = params.x;
    let height = params.y;
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

    // Compress the 8 neighbours into weight-1 sums and weight-2 carries.
    let a = full_add(r_up.west, r_up.cells, r_up.east);
    let b = full_add(r_down.west, r_down.cells, r_down.east);
    let c = half_add(r_mid.west, r_mid.east);

    // Weight 1: three sums left, one bit out.
    let d = full_add(a.sum, b.sum, c.sum);
    let sb0 = d.sum;

    // Weight 2, four terms. The three stage-1 carries, plus d's.
    let e = full_add(a.carry, b.carry, c.carry);
    let f = half_add(e.sum, d.carry);
    let sb1 = f.sum;

    // Weight 4, two terms. The weight-8 carry is dropped, since only n == 8 sets it, and n == 8
    // has sb1 == 0, so the rule below already excludes it.
    let sb2 = e.carry ^ f.carry;

    // Survive on 2, born on 3. Bit-sliced, 3 is 011 and 2 is 010, so both need sb2 == 0 and
    // sb1 == 1 and differ only in sb0, which folds into (sb0 | cells).
    let alive = ~sb2 & sb1 & (sb0 | r_mid.cells);

    // Trailing bits of a ragged last word hold no cell, and nothing reads them. load_row's patches
    // keep real cells off them, and display/reduce are bounded by width. The layout invariant is
    // still that they stay zero, and there is no `break` to leave them so now.
    let cells_here = min(width - word * 32u, 32u);
    var mask = 0xFFFFFFFFu;
    if cells_here < 32u {
        mask = (1u << cells_here) - 1u;
    }

    state_out[y * stride + word] = alive & mask;
}
