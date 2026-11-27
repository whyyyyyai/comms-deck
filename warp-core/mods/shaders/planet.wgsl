// A seat's planet: its logo drawn on the surface (Y, 04:42), and while it
// works a swarm of particles swirls through it, more of them and faster the
// longer it has been working (Y, 2026-11-27). The particles light up the
// logo above all: where they cross it, it flares and ripples. Save this file
// and the running warp core swaps it in; a broken edit is refused and the
// HUD says why.
//
// colour: the seat's colour (linear)
// params: x = time in seconds
//         y = work (0 idle .. 1 after a long stretch busy)
//         z = busy (0 or 1)
//         w = a per-seat seed, so planets don't swirl in step
// logo:   x = 1 if the seat has a logo (mods/logos/<seat>.png)
//         y = 1 to paint it in the seat's colour (a one-colour logo),
//             0 to keep the logo's own colours
//         z = how bright the planet is now (state, depth, focus)

struct Planet {
    colour: vec4<f32>,
    params: vec4<f32>,
    logo: vec4<f32>,
};

@group(2) @binding(0) var<uniform> planet: Planet;
@group(2) @binding(1) var logo_tex: texture_2d<f32>;
@group(2) @binding(2) var logo_smp: sampler;

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
    let t = planet.params.x;
    let work = clamp(planet.params.y, 0.0, 1.0);
    let busy = planet.params.z;
    let seed = planet.params.w * 91.0;
    let has_logo = planet.logo.x > 0.5;
    let bright = clamp(planet.logo.z, 0.2, 2.0);
    if (busy < 0.5 && !has_logo) {
        discard; // idle, no logo: the planet's own colour shows
    }

    // 10 particles to start, up to 28 after a long stretch of work, each
    // on its own orbit inside the planet, swirling faster as work grows.
    // `swirl` is the particles' pull on the surface: the logo ripples with it.
    var light = 0.0;
    var swirl = vec2<f32>(0.0);
    if (busy > 0.5) {
        let count = i32(10.0 + 18.0 * work);
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
            let d = p - q;
            let g = exp(-dot(d, d) / 0.0035) * (0.6 + 0.4 * sin(t * 6.0 + k));
            light = light + g;
            swirl = swirl + vec2<f32>(-d.y, d.x) * exp(-dot(d, d) / 0.03) * dir;
        }
    }

    // The logo, rippled by the particles passing through it.
    var mask = 0.0;
    var logo_rgb = vec3<f32>(1.0);
    if (has_logo) {
        let warp = swirl * (0.05 + 0.12 * work);
        let luv = (p + warp) / 0.78 * vec2<f32>(0.5, -0.5) + vec2<f32>(0.5, 0.5);
        let inside = step(0.0, luv.x) * step(luv.x, 1.0) * step(0.0, luv.y) * step(luv.y, 1.0);
        let s = textureSample(logo_tex, logo_smp, clamp(luv, vec2<f32>(0.0), vec2<f32>(1.0)));
        if (planet.logo.y > 0.5) {
            // One colour: the shape is wherever it's both opaque and light.
            mask = s.a * max(s.r, max(s.g, s.b)) * inside;
            logo_rgb = mix(planet.colour.rgb, vec3<f32>(1.0), 0.55);
        } else {
            mask = s.a * inside;
            logo_rgb = s.rgb;
        }
    }

    // Particles flare where they cross the logo and stay faint elsewhere.
    let on_logo = select(1.0, mix(0.45, 3.0, mask), has_logo);
    let lit = light * on_logo;
    let core_glow = (1.0 - smoothstep(0.0, 0.9, r)) * 0.25 * (0.5 + work) * busy;
    let edge = 1.0 - smoothstep(0.85, 0.95, r);
    let particles = planet.colour.rgb * (lit * (2.5 + 3.0 * work) + core_glow) + vec3<f32>(lit * 0.6);

    // The surface: darkened a little round the logo so it reads, the logo
    // itself glowing steadily, brighter at work and where particles pass.
    let shade = select(0.0, 0.45, has_logo) * (1.0 - mask);
    let glow = mask * (0.55 + 0.6 * busy + 2.5 * light * busy) * bright;
    let colour = logo_rgb * glow + particles;
    let alpha = clamp(shade + mask + lit + core_glow, 0.0, 1.0) * edge;
    return vec4<f32>(colour, alpha);
}
