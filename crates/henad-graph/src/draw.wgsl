#define_import_path henad_graph::draw

// Counter-based draws of a graph model, the twin of draw.rs in henad-graph.
//
// The core is two rounds of `pcg4d` over `[k, e, t, (i << 4) | (s >> 2)]`, of which sub-draw `s` takes word `s & 3`.
// A change of core is one edit here and one in `draw.rs`.

// The JCGT `pcg4d` hash, with every operation wrapping.
fn pcg4d(input: vec4<u32>) -> vec4<u32> {
    var v = input * 1664525u + 1013904223u;
    v.x = v.x + v.y * v.w;
    v.y = v.y + v.z * v.x;
    v.z = v.z + v.x * v.y;
    v.w = v.w + v.y * v.z;
    v = v ^ (v >> vec4<u32>(16u));
    v.x = v.x + v.y * v.w;
    v.y = v.y + v.z * v.x;
    v.z = v.z + v.x * v.y;
    v.w = v.w + v.y * v.z;
    return v;
}

// Bits of sub-draw `sub_draw` at key word `key_word`, element `element`, tick `tick` and iteration word `iteration`.
fn draw_core(key_word: u32, element: u32, tick: u32, iteration: u32, sub_draw: u32) -> u32 {
    let words = pcg4d(pcg4d(vec4<u32>(key_word, element, tick, (iteration << 4u) | (sub_draw >> 2u))));
    return words[sub_draw & 3u];
}
