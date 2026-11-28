// A ring around a seat: the fuel meters and the rule halos. Save this file
// and the running warp core swaps it in; a broken edit is refused and the
// HUD says why.
//
// colour: the lit part (linear, above 1 blooms), its alpha the opacity
// back:   the unlit part
// params: x = how much is lit (0..1, clockwise from the top)
//         y = thickness, as a fraction of the radius
//         z = time in seconds
//         w = pulse strength (0 = steady)

struct Ring {
    colour: vec4<f32>,
    back: vec4<f32>,
    params: vec4<f32>,
};

@group(2) @binding(0) var<uniform> ring: Ring;

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(2) uv: vec2<f32>,
};

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    // Centre at 0, y up.
    let p = vec2<f32>(in.uv.x * 2.0 - 1.0, 1.0 - in.uv.y * 2.0);
    let r = length(p);
    let inner = 1.0 - ring.params.y;
    let band = smoothstep(inner - 0.03, inner, r) * (1.0 - smoothstep(0.97, 1.0, r));
    if (band <= 0.001) {
        discard;
    }
    // 0 at the top, growing clockwise.
    let a = fract(atan2(p.x, p.y) / 6.2831853 + 1.0);
    let lit = step(a, ring.params.x);
    let pulse = 1.0 + ring.params.w * 0.5 * sin(ring.params.z * 4.0);
    let c = mix(ring.back, vec4<f32>(ring.colour.rgb * pulse, ring.colour.a), lit);
    return vec4<f32>(c.rgb, c.a * band);
}
