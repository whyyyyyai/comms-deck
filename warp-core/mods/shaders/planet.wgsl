// Inside a seat's planet: what it's doing. Idle, the bands lie calm and dim;
// working, they churn and brighten, more the longer it has been at it.
// Save this file and the running warp core swaps it in; a broken edit is
// refused and the HUD says why.
//
// colour: the seat's colour (linear)
// params: x = time in seconds
//         y = work (0 idle .. 1 after a long stretch busy)
//         z = busy (0 or 1)
//         w = a per-seat seed, so planets don't churn in step

struct Planet {
    colour: vec4<f32>,
    params: vec4<f32>,
};

@group(2) @binding(0) var<uniform> planet: Planet;

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
    return mix(mix(hash(i), hash(i + vec2<f32>(1.0, 0.0)), u.x),
               mix(hash(i + vec2<f32>(0.0, 1.0)), hash(i + vec2<f32>(1.0, 1.0)), u.x), u.y);
}

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    let p = vec2<f32>(in.uv.x * 2.0 - 1.0, 1.0 - in.uv.y * 2.0);
    let r = length(p);
    if (r >= 0.97) {
        discard;
    }
    let t = planet.params.x + planet.params.w * 17.0;
    let work = clamp(planet.params.y, 0.0, 1.0);
    let busy = planet.params.z;
    // A sphere: bands follow latitude, squeezed toward the limb.
    let lat = p.y / sqrt(max(1.0 - p.x * p.x, 0.05));
    let speed = 0.15 + work * 1.6;
    let churn = 0.4 + work * 2.5;
    let flow = noise(vec2<f32>(p.x * 3.0 - t * speed, lat * 5.0 + noise(p * 4.0 + t * 0.3 * churn) * churn));
    let bands = 0.5 + 0.5 * sin(lat * 9.0 + flow * 3.0 * (0.5 + work));
    // Hot spots that flare while it works.
    let storms = pow(noise(p * 7.0 + vec2<f32>(t * 0.9, -t * 0.6)), 6.0) * busy * (1.0 + work * 6.0);
    let limb = 1.0 - smoothstep(0.55, 0.97, r);
    let light = (0.12 + 0.45 * bands * (0.35 + 0.65 * work) + storms) * (0.5 + 0.5 * limb);
    let alpha = clamp(0.25 + 0.55 * busy * (0.3 + 0.7 * work), 0.0, 0.85) * (1.0 - smoothstep(0.9, 0.97, r));
    return vec4<f32>(planet.colour.rgb * light * (1.0 + 2.0 * busy * work), alpha);
}
