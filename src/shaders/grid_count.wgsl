struct Particle {
    position: vec2<f32>,
    velocity: vec2<f32>,
    color:    u32,
    species:  u32,
}

struct SimParams {
    dt:          f32,
    r_min:       f32,
    r_max:       f32,
    friction:    f32,
    n_particles: u32,
    n_species:   u32,
    force_scale: f32,
    aspect:      f32,
}

@group(0) @binding(0) var<storage, read>       particles:   array<Particle>;
@group(0) @binding(1) var<uniform>             params:      SimParams;
@group(0) @binding(2) var<storage, read_write> cell_counts: array<atomic<u32>>;

@compute @workgroup_size(64)
fn cs_main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let i = gid.x;
    if i >= params.n_particles { return; }

    let grid_w = max(5u, u32(2.0 / params.r_max));
    let gx = min(u32(particles[i].position.x * f32(grid_w)), grid_w - 1u);
    let gy = min(u32(particles[i].position.y * f32(grid_w)), grid_w - 1u);
    atomicAdd(&cell_counts[gy * grid_w + gx], 1u);
}
