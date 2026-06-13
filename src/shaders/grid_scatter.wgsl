// Grid pass 3 — Scatter: each particle claims a slot in sorted_indices via atomicAdd.
// cell_counts is reused here as per-cell write cursors (zeroed by the prefix pass).
// After this pass sorted_indices[cell_offsets[cell]..cell_offsets[cell+1]] holds all
// particle indices that belong to that cell, in arbitrary order.

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

@group(0) @binding(0) var<storage, read>       particles:      array<Particle>;
@group(0) @binding(1) var<uniform>             params:         SimParams;
@group(0) @binding(2) var<storage, read_write> cell_counts:    array<atomic<u32>>;
@group(0) @binding(3) var<storage, read>       cell_offsets:   array<u32>;
@group(0) @binding(4) var<storage, read_write> sorted_indices: array<u32>;

@compute @workgroup_size(64)
fn cs_main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let i = gid.x;
    if i >= params.n_particles { return; }

    let grid_w = max(5u, u32(2.0 / params.r_max));
    let gx     = min(u32(particles[i].position.x * f32(grid_w)), grid_w - 1u);
    let gy     = min(u32(particles[i].position.y * f32(grid_w)), grid_w - 1u);
    let cell   = gy * grid_w + gx;

    // Claim a slot in this cell's range; cell_offsets[cell] is the base.
    let slot = atomicAdd(&cell_counts[cell], 1u);
    sorted_indices[cell_offsets[cell] + slot] = i;
}
