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

@group(0) @binding(0) var<uniform>             params:       SimParams;
@group(0) @binding(1) var<storage, read_write> cell_counts:  array<atomic<u32>>;
@group(0) @binding(2) var<storage, read_write> cell_offsets: array<u32>;

// Single-thread exclusive prefix sum over all cells.
// Also zeroes cell_counts so they can be reused as per-cell write cursors in the scatter pass.
@compute @workgroup_size(1)
fn cs_main() {
    let grid_w  = max(5u, u32(2.0 / params.r_max));
    let n_cells = grid_w * grid_w;
    var running = 0u;
    for (var i = 0u; i <= n_cells; i++) {
        cell_offsets[i] = running;
        if i < n_cells {
            running += atomicLoad(&cell_counts[i]);
            atomicStore(&cell_counts[i], 0u);
        }
    }
}
