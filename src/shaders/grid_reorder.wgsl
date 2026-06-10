// Reorders particle positions and species into cell-sorted order so the force pass
// can read neighbors sequentially rather than through random sorted_indices indirection.

struct Particle {
    position: vec2<f32>,
    velocity: vec2<f32>,
    color:    u32,
    species:  u32,
}

struct SortedEntry {
    position: vec2<f32>,
    species:  u32,
    index:    u32,   // original particle index, used for self-exclusion in force pass
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
@group(0) @binding(2) var<storage, read>       sorted_indices: array<u32>;
@group(0) @binding(3) var<storage, read_write> sorted_entries: array<SortedEntry>;

@compute @workgroup_size(64)
fn cs_main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let k = gid.x;
    if k >= params.n_particles { return; }

    let j = sorted_indices[k];
    sorted_entries[k] = SortedEntry(
        particles[j].position,
        particles[j].species,
        j,
    );
}
