// The reactor core. Save this file and the running warp core swaps it in;
// a broken edit is refused and the HUD says why.
//
// cool / hot: the shell's colour, blended by how full the crew's meters are
// params: x = time in seconds
//         y = heat (0 = everyone has room, 1 = everyone near a limit)
//         z = warp (0..1, kicked by broadcasts)
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
    let t = core.params.x;
    let heat = clamp(core.params.y, 0.0, 1.0);
    let warp = core.params.z;
    let traffic = core.params.w;

    // Plasma swirling round the centre, faster with traffic.
    let ang = atan2(p.y, p.x);
    let spin = t * (0.4 + traffic * 1.5);
    let swirl = noise(vec2<f32>(ang * 2.5 + spin, r * 5.0 - t * (1.0 + traffic * 2.0)))
              * 0.6 + noise(vec2<f32>(ang * 6.0 - spin * 1.7, r * 11.0)) * 0.4;

    let shell = 1.0 - smoothstep(0.25, 1.0, r);
    let centre = 1.0 - smoothstep(0.0, 0.32, r);
    let base = mix(core.cool.rgb, core.hot.rgb, heat);
    let colour = base * shell * (0.55 + 0.9 * swirl)
               + vec3<f32>(6.0, 6.0, 7.0) * centre * (1.0 + warp * 1.5);
    let alpha = clamp(shell * 1.4 + centre, 0.0, 1.0);
    return vec4<f32>(colour, alpha);
}
