// Inside a seat's planet. Idle, it lies calm and dim; busy, a swarm of
// particles swirls inside it, more of them and faster the longer it has been
// working (Y, 2026-11-27). Save this file and the running warp core swaps it
// in; a broken edit is refused and the HUD says why.
//
// colour: the seat's colour (linear)
// params: x = time in seconds
//         y = work (0 idle .. 1 after a long stretch busy)
//         z = busy (0 or 1)
//         w = a per-seat seed, so planets don't swirl in step

struct Planet {
    colour: vec4<f32>,
    params: vec4<f32>,
};

@group(2) @binding(0) var<uniform> planet: Planet;

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(2) uv: vec2<f32>,
};

fn hash1(n: f32) -> f32 {
    return fract(sin(n * 127.1) * 43758.5453);
}

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    let p = vec2<f32>(in.uv.x * 2.0 - 1.0, 1.0 - in.uv.y * 2.0);
    let r = length(p);
    if (r >= 0.95) {
        discard;
    }
    let busy = planet.params.z;
    if (busy < 0.5) {
        discard; // idle: the planet's own colour shows, nothing on top
    }
    let t = planet.params.x;
    let work = clamp(planet.params.y, 0.0, 1.0);
    let seed = planet.params.w * 91.0;

    // 10 particles to start, up to 28 after a long stretch of work, each
    // on its own orbit inside the planet, swirling faster as work grows.
    let count = i32(10.0 + 18.0 * work);
    var light = 0.0;
    for (var i = 0; i < 28; i = i + 1) {
        if (i >= count) {
            break;
        }
        let k = f32(i) + seed;
        let orbit_r = 0.15 + 0.7 * hash1(k);
        let speed = (0.8 + 1.8 * hash1(k + 3.1)) * (0.6 + 1.6 * work);
        let dir = select(-1.0, 1.0, hash1(k + 7.7) > 0.5);
        let a = hash1(k + 1.3) * 6.2831853 + dir * t * speed / max(orbit_r, 0.2);
        let q = vec2<f32>(cos(a), sin(a)) * orbit_r;
        let d = length(p - q);
        light = light + exp(-d * d / 0.0035) * (0.6 + 0.4 * sin(t * 6.0 + k));
    }
    let core_glow = (1.0 - smoothstep(0.0, 0.9, r)) * 0.25 * (0.5 + work);
    let edge = 1.0 - smoothstep(0.85, 0.95, r);
    let c = planet.colour.rgb * (light * (2.5 + 3.0 * work) + core_glow) + vec3<f32>(light * 0.6);
    return vec4<f32>(c, clamp(light + core_glow, 0.0, 1.0) * edge);
}
