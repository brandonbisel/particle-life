// Prefix pass B — serial exclusive scan of block_sums.
// Single-thread; at most ceil(MAX_GRID_CELLS/256)+1 = 1174 iterations
// (vs 300 000 in the old single-pass design — ~256x fewer iterations).
// Also stores the grand total in block_sums[n_blocks] for use as the
// cell_offsets sentinel in pass C.

struct SimParams {
    dt: f32, r_min: f32, r_max: f32, friction: f32,
    n_particles: u32, n_species: u32, force_scale: f32, aspect: f32,
}

@group(0) @binding(0) var<uniform>             params:     SimParams;
@group(0) @binding(1) var<storage, read_write> block_sums: array<u32>;

@compute @workgroup_size(1)
fn cs_main() {
    let grid_w   = max(5u, u32(2.0 / params.r_max));
    let n_cells  = grid_w * grid_w;
    let n_blocks = (n_cells + 255u) / 256u;

    var running = 0u;
    for (var b = 0u; b < n_blocks; b++) {
        let old      = block_sums[b];
        block_sums[b] = running;
        running      += old;
    }
    // Store grand total as sentinel for pass C (= n_particles for a well-formed simulation).
    block_sums[n_blocks] = running;
}
