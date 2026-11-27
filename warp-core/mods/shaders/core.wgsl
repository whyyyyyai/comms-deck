// The core as a black hole seen from straight above: a round shadow, a thin
// photon ring at its edge, and the accretion disk face-on around it, its gas
// spiralling inward and heating up as it falls. Save this file and the
// running warp core swaps it in; a broken edit is refused and the HUD says why.
//
// cool / hot: the disk's colour, blended by how full the crew's meters are
// params: x = the disk's angle in radians, carried on by the warp core so a
//             change of pace bends the turning and never jumps the swirl;
//             it turns clockwise, with the ring and the stars
//         y = heat (0 = everyone has room, 1 = everyone near a limit)
//         z = warp (0..1+, kicked by broadcasts and sent notes)
//         w = traffic (0..1, notes in the last 10 minutes)

struct Core {
    cool: vec4<f32>,
    hot: vec4<f32>,
    params: vec4<f32>,
};

@group(2) @binding(0) var<uniform> core: Core;

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(2) uv: vec2<f32>,
};

fn hash(p: vec2<f32>) -> f32 {
    return fract(sin(dot(p, vec2<f32>(127.1, 311.7))) * 43758.5453);
}

fn noise(p: vec2<f32>) -> f32 {
    let i = floor(p);
    let f = fract(p);
    let u = f * f * (3.0 - 2.0 * f);
    let a = hash(i);
    let b = hash(i + vec2<f32>(1.0, 0.0));
    let c = hash(i + vec2<f32>(0.0, 1.0));
    let d = hash(i + vec2<f32>(1.0, 1.0));
    return mix(mix(a, b, u.x), mix(c, d, u.x), u.y);
}

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    let p = vec2<f32>(in.uv.x * 2.0 - 1.0, 1.0 - in.uv.y * 2.0);
    let r = length(p);
    if (r >= 1.0) {
        discard;
    }
    let spin = core.params.x;
    let heat = clamp(core.params.y, 0.0, 1.0);
    let warp = core.params.z;
    let traffic = core.params.w;
    let horizon = 0.26;

    // Gas orbits faster near the hole (Kepler: angular speed ~ r^-1.5) and
    // the arms wind into a spiral: angle shifted by log(r).
    let ang = atan2(p.y, p.x);
    let orbit = spin / pow(max(r, 0.2), 1.5);
    let arm = ang + orbit + 2.2 * log(max(r, 0.05));
    let streaks = noise(vec2<f32>(arm * 3.0, r * 6.0)) * 0.6 + noise(vec2<f32>(arm * 8.0, r * 18.0)) * 0.4;
    let arms = 0.55 + 0.45 * sin(arm * 2.0);

    // The disk: from just outside the photon ring out to the edge, hottest
    // at its inner edge.
    let disk = smoothstep(horizon + 0.02, horizon + 0.08, r) * (1.0 - smoothstep(0.55, 1.0, r));
    let inner_heat = clamp(1.0 - (r - horizon) / 0.5, 0.0, 1.0);
    let base = mix(core.cool.rgb, core.hot.rgb, max(heat, inner_heat * 0.6));
    let gas = base * disk * streaks * arms * (0.7 + 1.6 * inner_heat);

    // The photon ring and the lensed glow just outside the shadow.
    let photon = exp(-pow((r - horizon - 0.012) / 0.01, 2.0)) * (3.0 + warp * 3.0);
    let lensed = exp(-pow((r - horizon - 0.05) / 0.04, 2.0)) * 0.8;

    // Hawking radiation (Y, 05:23): faint sparks born at the photon ring,
    // drifting out on the disk's own turn and fading as they go, the start
    // of the stars that stream away from the core.
    var rad = 0.0;
    for (var i = 0; i < 14; i = i + 1) {
        let k = f32(i);
        let life = fract(spin * 0.11 + hash(vec2<f32>(k, 3.7)));
        let rr = horizon + 0.02 + life * (1.0 - horizon);
        let a = hash(vec2<f32>(k, 9.1)) * 6.2831853 - spin * 0.6 / (1.0 + rr * 2.0) - life * 1.4;
        let q = vec2<f32>(cos(a), sin(a)) * rr;
        let dq = p - q;
        rad = rad + exp(-dot(dq, dq) / 0.00035) * (1.0 - life) * (1.0 - life);
    }
    let radiation = mix(core.cool.rgb, vec3<f32>(1.0, 0.97, 0.92), 0.5) * rad * (0.8 + heat);

    var colour = gas + base * lensed + vec3<f32>(1.0, 0.96, 0.9) * photon + radiation;
    var alpha = clamp(length(gas) * 0.8 + lensed + photon * 0.5 + rad * 0.6, 0.0, 1.0);
    if (r < horizon) {
        // The shadow: nothing comes back out.
        colour = vec3<f32>(0.0);
        alpha = 1.0;
    }
    return vec4<f32>(colour, alpha);
}
