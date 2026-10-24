// Textured instances: glyph masks (kind 0), premultiplied colour glyphs and offscreen groups (kind 1). `tint` is the
// premultiplied brush colour for masks and (a,a,a,a) opacity otherwise.
struct View { size: vec2<f32>, pad: vec2<f32> }
@group(0) @binding(0) var<uniform> view: View;
@group(1) @binding(0) var tex: texture_2d<f32>;
@group(1) @binding(1) var samp: sampler;

struct Inst {
    @location(0) rect: vec4<f32>,
    @location(1) uv: vec4<f32>, // u0 v0 u1 v1
    @location(2) tint: vec4<f32>,
    @location(3) kind: vec4<f32>,
}
struct Out {
    @builtin(position) pos: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) @interpolate(flat) tint: vec4<f32>,
    @location(2) @interpolate(flat) kind: f32,
    @location(3) @interpolate(flat) bounds: vec4<f32>,
}

@vertex
fn vs(@builtin(vertex_index) vi: u32, i: Inst) -> Out {
    let corner = vec2<f32>(f32(vi & 1u), f32(vi >> 1u));
    let world = i.rect.xy + corner * i.rect.zw;
    var o: Out;
    o.pos = vec4<f32>(world.x / view.size.x * 2.0 - 1.0, 1.0 - world.y / view.size.y * 2.0, 0.0, 1.0);
    o.uv = mix(i.uv.xy, i.uv.zw, corner);
    o.tint = i.tint;
    o.kind = i.kind.x;
    o.bounds = i.uv;
    return o;
}

@fragment
fn fs(o: Out) -> @location(0) vec4<f32> {
    // Sample inside the instance's own texels (clamp-to-edge per instance): linear filtering must not bleed the
    // neighbouring glyph in an atlas page across a seam.
    let half = 0.5 / vec2<f32>(textureDimensions(tex));
    let uv = clamp(o.uv, min(o.bounds.xy, o.bounds.zw) + half, max(o.bounds.xy, o.bounds.zw) - half);
    let s = textureSample(tex, samp, uv);
    let mask = o.tint * s.r;
    let premul = s * o.tint;
    if (o.kind < 0.5) { return mask; }
    return premul;
}
