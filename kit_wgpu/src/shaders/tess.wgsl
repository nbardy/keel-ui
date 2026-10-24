// Lyon-tessellated geometry in device px. `cov` is the feathered-edge coverage ramp (1 inside, 0 at the fringe).
struct View { size: vec2<f32>, pad: vec2<f32> }
@group(0) @binding(0) var<uniform> view: View;

struct VIn {
    @location(0) pos: vec2<f32>,
    @location(1) cov: f32,
    @location(2) color: vec4<f32>, // premultiplied
}
struct Out {
    @builtin(position) pos: vec4<f32>,
    @location(0) color: vec4<f32>,
}

@vertex
fn vs(v: VIn) -> Out {
    var o: Out;
    o.pos = vec4<f32>(v.pos.x / view.size.x * 2.0 - 1.0, 1.0 - v.pos.y / view.size.y * 2.0, 0.0, 1.0);
    o.color = v.color * v.cov;
    return o;
}

@fragment
fn fs(o: Out) -> @location(0) vec4<f32> {
    return o.color;
}
