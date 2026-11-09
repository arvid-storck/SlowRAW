struct Params {
    grid: Grid,
    original: vec2<f32>,
    frame: vec2<f32>,
    crop: vec4<f32>,
    rotation: vec2<f32>,
    orientation: u32,
    flips: u32,
}
@group(0) @binding(7) var<uniform> params: Params;

@compute @workgroup_size(8,8,1)
fn warp(@builtin(global_invocation_id) id: vec3<u32>) {
    if (outside(id)) { return; }
    let position = vec2<f32>(id.xy) / max(vec2<f32>(params.grid.dst) - 1.0, vec2<f32>(1.0));
    let map_size = vec2<u32>(textureDimensions(t1).x / 3u, textureDimensions(t1).y);
    let map_position = position * (vec2<f32>(map_size) - 1.0);
    let tap = tap_from(map_position, map_size);
    let r = tap_sample(t1, tap);
    let g = tap_sample(t1, Tap(tap.p00 + vec2<i32>(i32(map_size.x),0), tap.p11 + vec2<i32>(i32(map_size.x),0), tap.w));
    let b = tap_sample(t1, Tap(tap.p00 + vec2<i32>(i32(map_size.x)*2,0), tap.p11 + vec2<i32>(i32(map_size.x)*2,0), tap.w));
    let scale = (vec2<f32>(textureDimensions(t0)) - 1.0) / max(params.original - 1.0, vec2<f32>(1.0));
    let rc = tap_sample(t0, tap_from(r.xy * scale, textureDimensions(t0))).r * r.z;
    let gc = tap_sample(t0, tap_from(g.xy * scale, textureDimensions(t0))).g * g.z;
    let bc = tap_sample(t0, tap_from(b.xy * scale, textureDimensions(t0))).b * b.z;
    textureStore(out_f16, id.xy, vec4<f32>(rc, gc, bc, 1.0));
}

@compute @workgroup_size(8,8,1)
fn frame(@builtin(global_invocation_id) id: vec3<u32>) {
    if (outside(id)) { return; }
    var p = (vec2<f32>(id.xy) + 0.5) * params.crop.zw / vec2<f32>(params.grid.dst) - 0.5 + params.crop.xy;
    let center = params.frame * 0.5;
    let d = p - center;
    p = vec2<f32>(params.rotation.x*d.x + params.rotation.y*d.y, -params.rotation.y*d.x + params.rotation.x*d.y) + center;
    if (any(p < vec2<f32>(-0.5)) || any(p > params.frame - 0.5)) {
        textureStore(out_f16, id.xy, vec4<f32>(0.0)); return;
    }
    if ((params.flips & 1u) != 0u) { p.x = params.frame.x - 1.0 - p.x; }
    if ((params.flips & 2u) != 0u) { p.y = params.frame.y - 1.0 - p.y; }
    switch params.orientation {
        case 1u: { p = vec2<f32>(p.y, params.original.y - 1.0 - p.x); }
        case 2u: { p = params.original - 1.0 - p; }
        case 3u: { p = vec2<f32>(params.original.x - 1.0 - p.y, p.x); }
        default: {}
    }
    let uv = (p + 0.5) / params.original;
    textureStore(out_f16, id.xy, tap_sample(t0, tap_uv(uv, textureDimensions(t0))));
}
