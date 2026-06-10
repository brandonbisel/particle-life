struct Globals {
    viewport:        vec2<f32>,
    particle_radius: f32,
    _pad0:           f32,
    camera_center:   vec2<f32>,
    camera_zoom:     f32,
    _pad1:           f32,
}

@group(0) @binding(0) var<uniform> globals: Globals;

struct VOut {
    @builtin(position) clip:  vec4<f32>,
    @location(0)       uv:    vec2<f32>,   // [-1,1]² local quad coords for circle test
    @location(1)       color: vec4<f32>,
}

fn unpack_color(packed: u32) -> vec4<f32> {
    let r = f32((packed >>  0u) & 0xFFu) / 255.0;
    let g = f32((packed >>  8u) & 0xFFu) / 255.0;
    let b = f32((packed >> 16u) & 0xFFu) / 255.0;
    return vec4<f32>(r, g, b, 1.0);
}

@vertex
fn vs_main(
    @builtin(vertex_index) vi:     u32,
    @location(0)           pos:    vec2<f32>,  // simulation coords [0,1]²
    @location(1)           vel:    vec2<f32>,  // unused in vertex stage; here for stride
    @location(2)           packed: u32,        // R=bits0-7, G=bits8-15, B=bits16-23
) -> VOut {
    // Two CCW triangles forming a unit quad in [-1,1]²
    var corners = array<vec2<f32>, 6>(
        vec2<f32>(-1.0, -1.0),
        vec2<f32>( 1.0, -1.0),
        vec2<f32>(-1.0,  1.0),
        vec2<f32>( 1.0, -1.0),
        vec2<f32>( 1.0,  1.0),
        vec2<f32>(-1.0,  1.0),
    );

    let corner = corners[vi];
    let r_px   = globals.particle_radius;

    // Convert simulation [0,1]² → NDC [-1,1]² with camera transform.
    // camera_center is the world point at screen center; zoom=1 shows the full [0,1]² world.
    let ndc_pos = (pos - globals.camera_center) * (globals.camera_zoom * 2.0);

    // Scale corner by pixel radius, corrected for aspect ratio
    let ndc_off = corner * vec2<f32>(
        2.0 * r_px / globals.viewport.x,
        2.0 * r_px / globals.viewport.y,
    );

    var out: VOut;
    out.clip  = vec4<f32>(ndc_pos + ndc_off, 0.0, 1.0);
    out.uv    = corner;
    out.color = unpack_color(packed);
    return out;
}

@fragment
fn fs_main(in: VOut) -> @location(0) vec4<f32> {
    let d = length(in.uv);
    if d > 1.0 {
        discard;
    }
    let a = smoothstep(1.0, 0.75, d);
    return vec4<f32>(in.color.rgb * a, a);
}
