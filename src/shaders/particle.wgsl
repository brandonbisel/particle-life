struct Globals {
    viewport:        vec2<f32>,
    particle_radius: f32,    // normalized: particle_radius_px / viewport_height
    _pad0:           f32,
    camera_center:   vec2<f32>,
    camera_zoom:     f32,    // shader zoom = zoom_factor * fit_zoom
    world_aspect:    f32,    // world_width / world_height
}

@group(0) @binding(0) var<uniform>       globals: Globals;
// 8 pre-linearised colours as vec4<f32> (R, G, B, 1.0); sRGB→linear is done on the CPU.
@group(0) @binding(1) var<storage, read> palette: array<vec4<f32>>;

struct VOut {
    @builtin(position) clip:  vec4<f32>,
    @location(0)       uv:    vec2<f32>,   // [-1,1]² local quad coords for circle test
    @location(1)       color: vec4<f32>,
}

@vertex
fn vs_main(
    @builtin(vertex_index) vi:      u32,
    @location(0)           pos:     vec2<f32>,  // simulation coords [0,1]²
    @location(1)           vel:     vec2<f32>,  // unused in vertex stage; here for stride
    @location(2)           species: u32,
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
    let viewport_aspect = globals.viewport.x / globals.viewport.y;

    // Convert simulation [0,1]² → NDC [-1,1]² with camera transform.
    // X is scaled by world_aspect / viewport_aspect so the world appears with correct
    // physical proportions regardless of viewport shape (letterboxed/pillarboxed).
    let ndc_pos = vec2<f32>(
        (pos.x - globals.camera_center.x) * globals.world_aspect * globals.camera_zoom * 2.0 / viewport_aspect,
        (pos.y - globals.camera_center.y) * globals.camera_zoom * 2.0,
    );

    // Particle quad offset: particle_radius is normalized (px / viewport_height).
    // NDC offset is isotropic in screen pixels, producing a circle on screen.
    // pixel_radius = particle_radius * camera_zoom * viewport_height = radius_px * camera_zoom
    // ndc_off_x = pixel_radius / (viewport_w/2) = particle_radius * camera_zoom * 2 / viewport_aspect
    // ndc_off_y = pixel_radius / (viewport_h/2) = particle_radius * camera_zoom * 2
    let r = globals.particle_radius;
    let ndc_off = vec2<f32>(
        corner.x * r * globals.camera_zoom * 2.0 / viewport_aspect,
        corner.y * r * globals.camera_zoom * 2.0,
    );

    var out: VOut;
    out.clip  = vec4<f32>(ndc_pos + ndc_off, 0.0, 1.0);
    out.uv    = corner;
    out.color = palette[species];
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
