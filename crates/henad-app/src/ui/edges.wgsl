// Instanced edges, drawn as one line per edge, with both ends read from the agent layer's position lanes.
// A directed edge can also get a filled arrowhead at its target end.
//
// The lanes are bound as storage buffers rather than vertex buffers, since each edge looks up its two nodes by index.

#import world::{HIDDEN, is_placed, to_clip}

struct Uniforms {
    world: vec2<f32>,
    // Alpha at the source and target ends. On a directed graph, the source end fades.
    alpha: vec2<f32>,
    // Points per clip unit on each axis. Arrowheads are shaped in points, where both axes have the same scale.
    points_per_clip: vec2<f32>,
    // Length and half width of an arrowhead in points. A zero length draws no arrowhead.
    arrow: vec2<f32>,
    // Distance in points between an arrowhead's tip and the centre of its node.
    arrow_gap: f32,
}

@group(0) @binding(0) var<uniform> u: Uniforms;
@group(0) @binding(1) var<storage, read> pos_x: array<f32>;
@group(0) @binding(2) var<storage, read> pos_y: array<f32>;

struct VertexInput {
    @location(0) src: u32,
    @location(1) dst: u32,
    @location(2) color: vec4<f32>,
}

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) color: vec4<f32>,
}

// The two ends of an edge, in points relative to the centre of the rect.
struct Span {
    tail: vec2<f32>,
    head: vec2<f32>,
    // Whether both ends are placed. Hiding only one end would draw a line off towards it.
    placed: bool,
}

fn span_of(in: VertexInput) -> Span {
    let a = vec2<f32>(pos_x[in.src], pos_y[in.src]);
    let b = vec2<f32>(pos_x[in.dst], pos_y[in.dst]);
    return Span(
        to_clip(a, u.world) * u.points_per_clip,
        to_clip(b, u.world) * u.points_per_clip,
        is_placed(a) && is_placed(b),
    );
}

// Returns the arrowhead length on an edge that is `len` points long.
// The head shrinks on a short edge.
fn head_length(len: f32) -> f32 {
    return min(u.arrow.x, max(len - 2.0 * u.arrow_gap, 0.0));
}

fn to_output(p: vec2<f32>, color: vec4<f32>) -> VertexOutput {
    var out: VertexOutput;
    out.clip_position = vec4<f32>(p / u.points_per_clip, 0.0, 1.0);
    out.color = color;
    return out;
}

@vertex
fn vs_main(@builtin(vertex_index) vertex_index: u32, in: VertexInput) -> VertexOutput {
    let span = span_of(in);
    let at_target = vertex_index == 1u;
    let color = vec4<f32>(in.color.rgb, in.color.a * select(u.alpha.x, u.alpha.y, at_target));
    if !span.placed {
        var hidden: VertexOutput;
        hidden.clip_position = HIDDEN;
        hidden.color = color;
        return hidden;
    }
    if !at_target {
        return to_output(span.tail, color);
    }
    // With an arrowhead, the line stops at the head's base so the two do not overlap.
    let run = span.head - span.tail;
    let len = length(run);
    var back = 0.0;
    if u.arrow.x > 0.0 {
        back = min(u.arrow_gap + head_length(len), len);
    }
    return to_output(span.head - run / max(len, 1e-6) * back, color);
}

// Three vertices per edge, the tip followed by the two corners of the base.
@vertex
fn vs_arrow(@builtin(vertex_index) vertex_index: u32, in: VertexInput) -> VertexOutput {
    let span = span_of(in);
    // Opaque regardless of the line's alpha, since the head is what shows the direction.
    let color = vec4<f32>(in.color.rgb, 1.0);
    if !span.placed {
        var hidden: VertexOutput;
        hidden.clip_position = HIDDEN;
        hidden.color = color;
        return hidden;
    }
    let run = span.head - span.tail;
    let len = length(run);
    let along = run / max(len, 1e-6);
    let across = vec2<f32>(-along.y, along.x);
    let head = head_length(len);
    let width = u.arrow.y * head / max(u.arrow.x, 1e-6);

    let tip = span.head - along * u.arrow_gap;
    let base = tip - along * head;
    var p = tip;
    if vertex_index == 1u {
        p = base + across * width;
    } else if vertex_index == 2u {
        p = base - across * width;
    }
    return to_output(p, color);
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    return in.color;
}
