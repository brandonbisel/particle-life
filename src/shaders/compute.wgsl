// Grid pass 5 — Force: compute pairwise Particle Life forces for each particle.
//
// Each thread handles one entry from sorted_entries (cell-sorted order).  It walks the
// 5×5 neighbourhood of cells and accumulates force from every nearby particle, then
// integrates velocity and position.  Reading from sorted_entries instead of through
// sorted_indices avoids random pointer-chasing and keeps GPU caches warm at high N.
//
// Shared-memory tile path: when all 64 threads in a workgroup process particles from the
// same grid cell (common in Ecosystem/Symbiosis clustering), their 21-cell neighborhood is
// identical.  The workgroup cooperatively loads each neighbor cell into a 64-entry LDS tile
// in rounds, so each global read is shared across all 64 threads instead of repeated 64x.
// The scalar path handles boundary workgroups and uniformly-distributed particle layouts.

struct Particle {
    position: vec2<f32>,
    velocity: vec2<f32>,
    color:    u32,
    species:  u32,
}

struct SimParams {
    dt:             f32,
    r_min:          f32,
    r_max:          f32,
    friction:       f32,
    n_particles:    u32,
    n_species:      u32,
    force_scale:    f32,
    aspect:         f32,
    mouse_x:        f32,
    mouse_y:        f32,
    mouse_strength: f32,
    mouse_range:    f32,
    border_mode:             u32,
    border_repel_strength:   f32,
    _pad2:                   u32,
    _pad3:                   u32,
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

const TILE: u32 = 64u;

// 64 x 16 B = 1 KB of LDS; plus 12 B for the reference cell and divergence flag.
var<workgroup> tile:        array<SortedEntry, 64>;
var<workgroup> ws_ref_gx:   u32;
var<workgroup> ws_ref_gy:   u32;
var<workgroup> ws_diverged: atomic<u32>;

fn torus_delta(a: vec2<f32>, b: vec2<f32>) -> vec2<f32> {
    var d = b - a;
    d = d - round(d);
    return d;
}

@compute @workgroup_size(64, 1, 1)
fn cs_main(
    @builtin(global_invocation_id) gid:     vec3<u32>,
    @builtin(local_invocation_id)  lid_vec: vec3<u32>,
) {
    let k   = gid.x;
    let lid = lid_vec.x;

    // Out-of-range threads in the last partial workgroup clamp to particle 0 so they
    // still reach every workgroupBarrier() in the tile path (uniform control flow
    // requires all threads to hit barriers regardless of whether they are in-range).
    let in_range = k < params.n_particles;
    let k_read   = select(0u, k, in_range);

    let subj = sorted_entries[k_read];
    let i    = subj.index;

    var force_acc = vec2<f32>(0.0, 0.0);

    let r_min       = params.r_min;
    let r_max       = params.r_max;
    let aspect      = params.aspect;
    let inv_r_min   = 1.0 / r_min;
    let r_sum       = r_max + r_min;
    let inv_r_range = 1.0 / (r_max - r_min);
    let r_max_sq    = r_max * r_max;
    let r_min_sq    = r_min * r_min;
    let wrapping    = params.border_mode == 0u;

    let grid_w = max(5u, u32(2.0 / r_max));
    let igw    = i32(grid_w);
    let gx_i   = i32(min(u32(subj.position.x * f32(grid_w)), grid_w - 1u));
    let gy_i   = i32(min(u32(subj.position.y * f32(grid_w)), grid_w - 1u));

    // Homogeneity check — all 64 threads participate to keep barriers uniform.
    // sorted_entries is cell-sorted, so a workgroup processing particles from a single
    // hot cell (as in Ecosystem clusters) will pass this check.  Boundary workgroups
    // spanning two cells fall back to the scalar path.
    if lid == 0u {
        ws_ref_gx = u32(gx_i);
        ws_ref_gy = u32(gy_i);
        atomicStore(&ws_diverged, 0u);
    }
    workgroupBarrier();
    if u32(gx_i) != ws_ref_gx || u32(gy_i) != ws_ref_gy {
        atomicStore(&ws_diverged, 1u);
    }
    workgroupBarrier();
    let homogeneous = atomicLoad(&ws_diverged) == 0u;

    if homogeneous {
        // TILE PATH — all 64 threads share the same 21-cell neighborhood.
        // Cooperatively load each neighbor cell into LDS in rounds of TILE entries,
        // then all threads compute forces against the tile.
        // start/end/j are uniform across threads, so while/workgroupBarrier are uniform.
        for (var dy = -2; dy <= 2; dy++) {
            for (var dx = -2; dx <= 2; dx++) {
                // Corner cells (|dx|==2 && |dy|==2) are at distance sqrt(2)*r_max —
                // always outside the interaction radius.
                if abs(dx) == 2 && abs(dy) == 2 { continue; }
                let nx   = ((gx_i + dx) % igw + igw) % igw;
                let ny   = ((gy_i + dy) % igw + igw) % igw;
                let cell = u32(ny * igw + nx);

                let start = cell_offsets[cell];
                let end   = cell_offsets[cell + 1u];

                var j = start;
                while j < end {
                    let tile_len = min(TILE, end - j);

                    // Thread lid loads its one entry from global memory into LDS.
                    if lid < tile_len {
                        tile[lid] = sorted_entries[j + lid];
                    }
                    workgroupBarrier();

                    if in_range {
                        for (var t = 0u; t < tile_len; t++) {
                            let entry = tile[t];
                            if entry.index == i { continue; }

                            let delta   = select(entry.position - subj.position,
                                                 torus_delta(subj.position, entry.position),
                                                 wrapping);
                            let dx_asp  = delta.x * aspect;
                            let dist_sq = dx_asp * dx_asp + delta.y * delta.y;

                            if dist_sq > 1e-8 && dist_sq < r_max_sq {
                                let inv_dist = inverseSqrt(dist_sq);
                                let a        = attraction[subj.species * 8u + entry.species];

                                // rep:  delta × (inv_r_min − inv_dist)
                                // int:  delta × a × (inv_dist − |2 − r_sum·inv_dist| × inv_r_range)
                                let rep_f    = inv_r_min - inv_dist;
                                let int_f    = a * (inv_dist - abs(2.0 - r_sum * inv_dist) * inv_r_range);
                                let f_scaled = select(int_f, rep_f, dist_sq < r_min_sq);

                                force_acc += delta * f_scaled;
                            }
                        }
                    }
                    workgroupBarrier();
                    j += TILE;
                }
            }
        }
    } else {
        // SCALAR PATH — threads span different cells; each reads sorted_entries independently.
        if in_range {
            for (var dy = -2; dy <= 2; dy++) {
                for (var dx = -2; dx <= 2; dx++) {
                    if abs(dx) == 2 && abs(dy) == 2 { continue; }
                    let nx   = ((gx_i + dx) % igw + igw) % igw;
                    let ny   = ((gy_i + dy) % igw + igw) % igw;
                    let cell = u32(ny * igw + nx);

                    let start = cell_offsets[cell];
                    let end   = cell_offsets[cell + 1u];

                    for (var j = start; j < end; j++) {
                        let entry = sorted_entries[j];
                        if entry.index == i { continue; }

                        // Use torus shortcut only in wrap mode; in repel/static modes use
                        // direct delta so cross-boundary attraction correctly vanishes.
                        let delta   = select(entry.position - subj.position,
                                             torus_delta(subj.position, entry.position),
                                             wrapping);
                        let dx_asp  = delta.x * aspect;
                        let dist_sq = dx_asp * dx_asp + delta.y * delta.y;

                        if dist_sq > 1e-8 && dist_sq < r_max_sq {
                            let inv_dist = inverseSqrt(dist_sq);
                            let a        = attraction[subj.species * 8u + entry.species];

                            let rep_f    = inv_r_min - inv_dist;
                            let int_f    = a * (inv_dist - abs(2.0 - r_sum * inv_dist) * inv_r_range);
                            let f_scaled = select(int_f, rep_f, dist_sq < r_min_sq);

                            force_acc += delta * f_scaled;
                        }
                    }
                }
            }
        }
    }

    if !in_range { return; }

    var vel = particles[i].velocity + force_acc * (params.force_scale * params.dt);
    vel    *= exp(-params.friction * params.dt);

    // Mouse attractor / repulsor.
    if params.mouse_strength != 0.0 && params.mouse_range > 0.0 {
        let m_pos   = vec2<f32>(params.mouse_x, params.mouse_y);
        let m_delta = select(m_pos - subj.position,
                             torus_delta(subj.position, m_pos),
                             wrapping);
        let m_dx    = m_delta.x * aspect;
        let m_dsq   = m_dx * m_dx + m_delta.y * m_delta.y;
        let m_rsq   = params.mouse_range * params.mouse_range;
        if m_dsq > 1e-8 && m_dsq < m_rsq {
            let m_dist = sqrt(m_dsq);
            let t = 1.0 - m_dist / params.mouse_range;
            vel += (m_delta / m_dist) * (params.mouse_strength * t * t * params.dt);
        }
    }

    // Border repel force (mode 1): spring pushes particles away from each wall within r_max.
    if params.border_mode == 1u {
        // border_repel_strength is in world-units/s at the wall surface.
        // X walls use r_max/aspect so the zone is the same pixel depth as the Y walls
        // (which use r_max directly). Without this correction the wider world-space
        // x-zone creates a visibly larger margin on the left/right sides.
        let s        = params.border_repel_strength * params.dt;
        let brange_y = r_max;
        let brange_x = r_max / aspect;
        if subj.position.x < brange_x {
            let t = 1.0 - subj.position.x / brange_x;
            vel.x += t * t * s;
        }
        if subj.position.x > 1.0 - brange_x {
            let t = 1.0 - (1.0 - subj.position.x) / brange_x;
            vel.x -= t * t * s;
        }
        if subj.position.y < brange_y {
            let t = 1.0 - subj.position.y / brange_y;
            vel.y += t * t * s;
        }
        if subj.position.y > 1.0 - brange_y {
            let t = 1.0 - (1.0 - subj.position.y) / brange_y;
            vel.y -= t * t * s;
        }
    }

    // CFL guard.
    let max_speed = params.r_max / params.dt * 0.25;
    let spd = length(vel);
    vel *= min(1.0, max_speed / max(spd, 1e-6));

    var pos = subj.position + vel * params.dt;

    if params.border_mode == 0u {
        // Wrap: torus topology.
        pos = fract(pos + vec2<f32>(1.0, 1.0));
    } else if params.border_mode == 1u {
        // Repel: spring force already applied; just clamp against tunneling.
        pos = clamp(pos, vec2<f32>(0.0), vec2<f32>(1.0));
    } else {
        // Static: hard wall — clamp and zero the outward velocity component.
        if pos.x < 0.0 { vel.x = max(vel.x, 0.0); pos.x = 0.0; }
        if pos.x > 1.0 { vel.x = min(vel.x, 0.0); pos.x = 1.0; }
        if pos.y < 0.0 { vel.y = max(vel.y, 0.0); pos.y = 0.0; }
        if pos.y > 1.0 { vel.y = min(vel.y, 0.0); pos.y = 1.0; }
    }

    particles[i].velocity = vel;
    particles[i].position = pos;
}
