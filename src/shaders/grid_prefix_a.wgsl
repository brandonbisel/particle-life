// Prefix pass A — parallel block-level exclusive scan (Blelloch algorithm).
// Each workgroup of 256 threads scans its own 256-cell block in shared memory.
// Outputs:
//   cell_offsets[i]      = exclusive prefix sum within the block (local offset)
//   block_sums[wg_id]    = total particle count for this block
// Also zeroes cell_counts[i] for reuse as per-cell write cursors in scatter.

const BLOCK: u32 = 256u;

struct SimParams {
    dt: f32, r_min: f32, r_max: f32, friction: f32,
    n_particles: u32, n_species: u32, force_scale: f32, aspect: f32,
}

@group(0) @binding(0) var<uniform>             params:       SimParams;
@group(0) @binding(1) var<storage, read_write> cell_counts:  array<atomic<u32>>;
@group(0) @binding(2) var<storage, read_write> cell_offsets: array<u32>;
@group(0) @binding(3) var<storage, read_write> block_sums:   array<u32>;

var<workgroup> sdata:      array<u32, 256>;
var<workgroup> block_total: u32;

@compute @workgroup_size(256)
fn cs_main(
    @builtin(global_invocation_id) gid:  vec3<u32>,
    @builtin(local_invocation_id)  lid_v: vec3<u32>,
    @builtin(workgroup_id)         wgid:  vec3<u32>,
) {
    let lid = lid_v.x;
    let i   = gid.x;

    let grid_w  = max(5u, u32(2.0 / params.r_max));
    let n_cells = grid_w * grid_w;

    // Load cell count into shared memory and zero it for scatter reuse.
    if i < n_cells {
        sdata[lid] = atomicLoad(&cell_counts[i]);
        atomicStore(&cell_counts[i], 0u);
    } else {
        sdata[lid] = 0u;
    }
    workgroupBarrier();

    // Up-sweep (reduce): build partial sums tree.
    var stride = 1u;
    while stride < BLOCK {
        if (lid + 1u) % (2u * stride) == 0u {
            sdata[lid] += sdata[lid - stride];
        }
        workgroupBarrier();
        stride *= 2u;
    }

    // Capture block total before zeroing root for down-sweep.
    if lid == BLOCK - 1u {
        block_total = sdata[lid];
        sdata[lid]  = 0u;
    }
    workgroupBarrier();

    // Down-sweep: convert to exclusive prefix sums.
    stride = BLOCK / 2u;
    while stride > 0u {
        if (lid + 1u) % (2u * stride) == 0u {
            let t        = sdata[lid - stride];
            sdata[lid - stride] = sdata[lid];
            sdata[lid]  += t;
        }
        workgroupBarrier();
        stride /= 2u;
    }

    // Write local exclusive prefix to cell_offsets (propagated in pass C).
    if i < n_cells {
        cell_offsets[i] = sdata[lid];
    }

    // Thread 0 writes the block total; pass B will scan block_sums into global offsets.
    if lid == 0u {
        block_sums[wgid.x] = block_total;
    }
}
