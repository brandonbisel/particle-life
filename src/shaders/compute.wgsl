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
    aspect:      f32,  // viewport width / height; used for isotropic visual distances
}

struct SortedEntry {
    position: vec2<f32>,
    species:  u32,
    index:    u32,
}

@group(0) @binding(0) var<storage, read_write> particles:      array<Particle>;
@group(0) @binding(1) var<uniform>             params:         SimParams;
@group(0) @binding(2) var<storage, read>       attraction:     array<f32, 64>;
@group(0) @binding(3) var<storage, read>       cell_offsets:   array<u32>;
@group(0) @binding(4) var<storage, read>       sorted_entries: array<SortedEntry>;

fn torus_delta(a: vec2<f32>, b: vec2<f32>) -> vec2<f32> {
    var d = b - a;
    d = d - round(d);   // wraps each component to [-0.5, 0.5]
    return d;
}

@compute @workgroup_size(64, 1, 1)
fn cs_main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let k = gid.x;
    if k >= params.n_particles { return; }

    // Dispatch over cell-sorted order so all 64 threads in a wavefront share the same
    // 5×5 neighborhood, turning inner-loop sorted_entries reads from cache misses into
    // cache hits. Position and species come from the sequential sorted_entries[k] read;
    // velocity requires one random read from the original particles buffer.
    let subj = sorted_entries[k];
    let i    = subj.index;   // original index for velocity read and write-back

    var force_acc = vec2<f32>(0.0, 0.0);

    // Hoist uniform-derived constants out of the inner loop.
    // particle_force() uses two divisions (dist/r_min, abs(...)/range); precomputing
    // their reciprocals replaces them with multiplications for every in-range pair.
    let r_min       = params.r_min;
    let r_max       = params.r_max;
    let aspect      = params.aspect;
    let inv_r_min   = 1.0 / r_min;
    let r_sum       = r_max + r_min;
    let inv_r_range = 1.0 / (r_max - r_min);
    let r_max_sq    = r_max * r_max;

    let grid_w = max(5u, u32(2.0 / r_max));
    let igw    = i32(grid_w);
    let gx_i   = i32(min(u32(subj.position.x * f32(grid_w)), grid_w - 1u));
    let gy_i   = i32(min(u32(subj.position.y * f32(grid_w)), grid_w - 1u));

    for (var dy = -2; dy <= 2; dy++) {
        for (var dx = -2; dx <= 2; dx++) {
            // Torus-wrap the neighbor cell coordinates.
            let nx   = ((gx_i + dx) % igw + igw) % igw;
            let ny   = ((gy_i + dy) % igw + igw) % igw;
            let cell = u32(ny * igw + nx);

            let start = cell_offsets[cell];
            let end   = cell_offsets[cell + 1u];

            for (var j = start; j < end; j++) {
                let entry = sorted_entries[j];
                if entry.index == i { continue; }

                let delta   = torus_delta(subj.position, entry.position);
                let dx_asp  = delta.x * aspect;
                let dist_sq = dx_asp * dx_asp + delta.y * delta.y;

                // Skip sqrt for the ~50% of candidates outside r_max.
                if dist_sq > 1e-8 && dist_sq < r_max_sq {
                    let dist = sqrt(dist_sq);
                    let a    = attraction[subj.species * 8u + entry.species];

                    // Inlined particle_force with precomputed reciprocals.
                    let repulsion   = dist * inv_r_min - 1.0;
                    let interaction = a * (1.0 - abs(2.0 * dist - r_sum) * inv_r_range);
                    let mask_rep    = 1.0 - step(r_min, dist);
                    let mask_int    = step(r_min, dist) * (1.0 - step(r_max, dist));
                    let f           = mask_rep * repulsion + mask_int * interaction;

                    force_acc += (delta / dist) * f;
                }
            }
        }
    }

    var vel = particles[i].velocity + force_acc * (params.force_scale * params.dt);
    vel    *= exp(-params.friction * params.dt);

    // CFL guard: cap speed so a particle travels at most 0.25 * r_max per step.
    let max_speed = params.r_max / params.dt * 0.25;
    let spd = length(vel);
    vel *= min(1.0, max_speed / max(spd, 1e-6));

    var pos = subj.position + vel * params.dt;
    pos     = fract(pos + vec2<f32>(1.0, 1.0));

    particles[i].velocity = vel;
    particles[i].position = pos;
}
