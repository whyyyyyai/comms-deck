// The webcam feed in the top-left corner (Y, t156). One textured quad;
// webcam-feed writes raw RGBA frames to
// ~/.local/state/warp-core/webcam/frame.rgba and the warp core swaps the
// texture when the file changes. Save this file and the running warp core
// swaps it in; a broken edit is refused and the HUD says why.
//
// colour: white keeps the camera's own colours; tint it to dim or mark it

struct Webcam {
    colour: vec4<f32>,
};

@group(2) @binding(0) var<uniform> webcam: Webcam;
@group(2) @binding(1) var frame_tex: texture_2d<f32>;
@group(2) @binding(2) var frame_smp: sampler;

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(2) uv: vec2<f32>,
};

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    // The camera's first row is the top of the picture; the quad's v = 0 is
    // its bottom, so flip.
    let c = textureSample(frame_tex, frame_smp, vec2<f32>(in.uv.x, 1.0 - in.uv.y));
    return vec4<f32>(c.rgb * webcam.colour.rgb, c.a * webcam.colour.a);
}
