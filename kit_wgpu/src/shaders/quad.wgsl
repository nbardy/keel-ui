// Instanced rounded-rect quad: fill, border and drop shadow from one signed-distance evaluation, output
// premultiplied. Written for kit_wgpu in the shape of iced_wgpu's quad shader (rounded-box SDF, per-corner radii,
// smoothstep shadow); iced's source was not available offline, so no upstream text is copied here.
struct View { size: vec2<f32>, pad: vec2<f32> }
@group(0) @binding(0) var<uniform> view: View;

struct Inst {
    @location(0) rect: vec4<f32>,    // x, y, w, h in device px
    @location(1) fill: vec4<f32>,    // premultiplied
    @location(2) border: vec4<f32>,  // premultiplied
    @location(3) shadow: vec4<f32>,  // premultiplied
    @location(4) radii: vec4<f32>,   // tl, tr, br, bl
    @location(5) params: vec4<f32>,  // border width, shadow blur, shadow spread, -
    @location(6) offset: vec4<f32>,  // shadow offset xy, -, -
}

struct Out {
    @builtin(position) pos: vec4<f32>,
    @location(0) local: vec2<f32>,
    @location(1) @interpolate(flat) size: vec2<f32>,
    @location(2) @interpolate(flat) fill: vec4<f32>,
    @location(3) @interpolate(flat) border: vec4<f32>,
    @location(4) @interpolate(flat) shadow: vec4<f32>,
    @location(5) @interpolate(flat) radii: vec4<f32>,
    @location(6) @interpolate(flat) params: vec4<f32>,
    @location(7) @interpolate(flat) offset: vec2<f32>,
}

@vertex
fn vs(@builtin(vertex_index) vi: u32, i: Inst) -> Out {
    let corner = vec2<f32>(f32(vi & 1u), f32(vi >> 1u));
    let hs = i.rect.zw * 0.5;
    let centre = i.rect.xy + hs;
    let reach = i.params.z + i.params.y;
    let lo = min(centre - hs - 1.0, centre + i.offset.xy - hs - reach - 1.0);
    let hi = max(centre + hs + 1.0, centre + i.offset.xy + hs + reach + 1.0);
    let world = mix(lo, hi, corner);
    var o: Out;
    o.pos = vec4<f32>(world.x / view.size.x * 2.0 - 1.0, 1.0 - world.y / view.size.y * 2.0, 0.0, 1.0);
    o.local = world - centre;
    o.size = i.rect.zw;
    o.fill = i.fill;
    o.border = i.border;
    o.shadow = i.shadow;
    o.radii = i.radii;
    o.params = i.params;
    o.offset = i.offset.xy;
    return o;
}

// p relative to the box centre, y down; radii order tl, tr, br, bl.
fn box_sdf(p: vec2<f32>, hs: vec2<f32>, r4: vec4<f32>) -> f32 {
    var r = r4.x;
    if (p.x > 0.0) {
        if (p.y > 0.0) { r = r4.z; } else { r = r4.y; }
    } else {
        if (p.y > 0.0) { r = r4.w; }
    }
    let q = abs(p) - hs + r;
    return length(max(q, vec2<f32>(0.0))) + min(max(q.x, q.y), 0.0) - r;
}

fn coverage(d: f32) -> f32 {
    return clamp(0.5 - d, 0.0, 1.0);
}

@fragment
fn fs(i: Out) -> @location(0) vec4<f32> {
    let hs = i.size * 0.5;
    let bw = i.params.x;
    let outer = coverage(box_sdf(i.local, hs, i.radii));
    let inner = coverage(box_sdf(i.local, hs - bw, max(i.radii - bw, vec4<f32>(0.0))));
    let shape = outer * (i.border * (1.0 - inner) + i.fill * inner);

    let blur = i.params.y;
    let spread = i.params.z;
    let ds = box_sdf(i.local - i.offset, hs + spread, max(i.radii + spread, vec4<f32>(0.0)));
    var sc = coverage(ds);
    if (blur > 0.0) {
        sc = 1.0 - smoothstep(-blur, blur, ds);
    }
    // The shadow is hidden under the box (CSS box-shadow): weight by what the box leaves uncovered.
    return shape + i.shadow * sc * (1.0 - outer);
}
