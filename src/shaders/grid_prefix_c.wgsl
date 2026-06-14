// Prefix pass C — propagate block offsets to produce global exclusive prefix sums.
// Each thread adds its block's base offset (from block_sums) to the local prefix
// already written by pass A.  The thread at index n_cells writes the sentinel
// cell_offsets[n_cells] = total particle count.

struct SimParams {
    dt: f32, r_min: f32, r_max: f32, friction: f32,
    n_particles: u32, n_species: u32, force_scale: f32, aspect: f32,
}

@group(0) @binding(0) var<uniform>             params:       SimParams;
@group(0) @binding(1) var<storage, read_write> cell_offsets: array<u32>;
@group(0) @binding(2) var<storage, read>       block_sums:   array<u32>;

@compute @workgroup_size(256)
fn cs_main(
    @builtin(global_invocation_id) gid:  vec3<u32>,
    @builtin(workgroup_id)         wgid: vec3<u32>,
) {
    let i = gid.x;

    let grid_w  = max(5u, u32(2.0 / params.r_max));
    let n_cells = grid_w * grid_w;

    if i < n_cells {
        // Add this block's base offset to the local prefix from pass A.
        cell_offsets[i] += block_sums[wgid.x];
    } else if i == n_cells {
        // Sentinel: block_sums[n_blocks] = grand total stored by pass B.
        let n_blocks = (n_cells + 255u) / 256u;
        cell_offsets[n_cells] = block_sums[n_blocks];
    }
}
