//! GPU simulation state and the six-pass spatial-grid compute pipeline.
//!
//! The pipeline runs entirely on the GPU each frame:
//! 1. **Count**    — atomically increment per-cell particle counts.
//! 2. **Prefix A** — 256-thread Blelloch block scan; writes block totals; zeros counts for scatter.
//! 3. **Prefix B** — serial scan of ≤1,173 block totals.
//! 4. **Prefix C** — propagates block offsets to produce final cell offsets.
//! 5. **Scatter**  — claim a sorted slot per particle; writes `SortedEntry{pos, species, index}` directly.
//! 6. **Force**    — 21-cell neighborhood; LDS tile for homogeneous workgroups; `inverseSqrt`-based force.

/// Maximum number of species supported by the attraction matrix and PALETTE.
pub const MAX_SPECIES: usize = 16;
/// Maximum number of permanent field attractors/repulsors.
pub const MAX_ATTRACTORS: usize = 64;
/// Hard cap on the GPU particle buffer. Raising it increases VRAM by ~24 bytes/particle.
pub const MAX_PARTICLES: usize = 2_000_000;
// cell = r_max_norm/2, so grid_w = floor(2/r_max_norm).
// At auto-density with 2M particles the grid reaches ~500×500 = 250K cells.
const MAX_GRID_CELLS: usize = 300_000;
// Block count for the parallel prefix scan (prefix_a dispatches this many workgroups of 256).
const MAX_PREFIX_BLOCKS: usize = MAX_GRID_CELLS.div_ceil(256) + 1;

/// The reference world height against which `r_min`/`r_max` preset values are defined.
///
/// Preset values like `r_max = 0.08` mean "8% of BASE_WORLD_HEIGHT world units", so they
/// encode an absolute physical reach that scales correctly when the world grows.  At the
/// default world (`world_height = 720`) the normalised value passed to the GPU equals the
/// stored value unchanged.
pub const BASE_WORLD_HEIGHT: f32 = 720.0;

/// Default species colours as packed sRGB `0xFF_BB_GG_RR` u32s.
///
/// These are the sRGB-space equivalents of the previous linear values so the
/// on-screen appearance is identical to the original palette.  Stored as sRGB
/// so the vertex shader can do a single sRGB→linear conversion and the GPU's
/// automatic linear→sRGB encoding on the sRGB framebuffer produces the
/// correct final colour.
pub const PALETTE_DEFAULT: [u32; 16] = [
    0xFF8585EF, // salmon-red    sRGB(239, 133, 133)
    0xFF85EF85, // light-green   sRGB(133, 239, 133)
    0xFFEF9885, // periwinkle    sRGB(133, 152, 239)
    0xFF7AE5EF, // pale-yellow   sRGB(239, 229, 122)
    0xFFEF7AD0, // lavender      sRGB(208, 122, 239)
    0xFFEAEA7A, // pale-cyan     sRGB(122, 234, 234)
    0xFF7ABDF4, // peach         sRGB(244, 189, 122)
    0xFFDBA8F4, // rose-pink     sRGB(244, 168, 219)
    0xFF85C4F4, // warm-gold     sRGB(244, 196, 133)
    0xFFABEF85, // spring-green  sRGB(133, 239, 171)
    0xFF85EFCF, // aquamarine    sRGB(207, 239, 133)
    0xFFEFD485, // wheat         sRGB(133, 212, 239)
    0xFFD485EF, // orchid        sRGB(239, 133, 212)
    0xFF85B8EF, // sky-blue      sRGB(239, 184, 133)
    0xFFEF8585, // coral         sRGB(133, 133, 239)
    0xFFB8F485, // yellow-green  sRGB(133, 244, 184)
];

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct SimParams {
    dt: f32,
    r_min: f32,
    r_max: f32,
    friction: f32,
    n_particles: u32,
    n_species: u32,
    force_scale: f32,
    aspect: f32,
    mouse_x: f32,
    mouse_y: f32,
    mouse_strength: f32,
    mouse_range: f32,
    // 0 = Wrap (torus), 1 = Repel (spring wall), 2 = Static (hard wall)
    border_mode: u32,
    border_repel_strength: f32, // multiplier on repel wall force; default 0.3
    speed_limit: f32,           // fraction of r_max a particle may travel per frame
    n_attractors: u32,          // number of active permanent field attractors (was _pad)
}

/// A permanent force emitter placed in world space.  Stored CPU-side; uploaded to the GPU each frame.
///
/// `pos` is in normalised [0,1]² world space.  `strength` is per-species: positive attracts,
/// negative repels, zero ignores.  `velocity` drives drift (world-units per second); [0,0] = static.
#[derive(Clone)]
pub struct AttractorDef {
    pub pos: [f32; 2],
    pub range: f32,
    pub strength: [f32; MAX_SPECIES],
    pub velocity: [f32; 2],
}

/// GPU-layout version of [`AttractorDef`] uploaded to binding 5 of the force pass.
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct GpuAttractor {
    pos: [f32; 2],       // 8 bytes
    range: f32,          // 4 bytes
    _pad: f32,           // 4 bytes (alignment)
    strength: [f32; 16], // 64 bytes  (MAX_SPECIES = 16)
} // Total: 80 bytes

/// All CPU-side simulation state plus the GPU buffers and compute pipelines.
///
/// `app.rs` owns the single instance and calls [`dispatch`](SimulationState::dispatch)
/// once per frame inside the wgpu command encoder.
pub struct SimulationState {
    /// Target particle count used on the next [`respawn`](SimulationState::respawn).
    pub particle_count: usize,
    /// Number of active species; also the dimension of the N×N attraction matrix.
    pub species_count: usize,
    /// Hard-core repulsion radius as a fraction of [`BASE_WORLD_HEIGHT`].
    pub r_min: f32,
    /// Outer interaction cutoff radius as a fraction of [`BASE_WORLD_HEIGHT`].
    pub r_max: f32,
    /// Velocity damping coefficient applied each frame; 0 = no damping, 1 = full stop.
    pub friction: f32,
    /// Global scale factor applied to all inter-particle forces.
    pub force_scale: f32,
    /// Maximum distance a particle may travel per frame, as a fraction of `r_max`.
    /// Prevents tunnelling; 0.25 means a particle at top speed crosses the interaction
    /// zone in ~4 frames.
    pub speed_limit: f32,
    /// Particle render radius in world units (used when `auto_particle_size` is false).
    pub particle_radius: f32,
    /// When true, the effective radius is computed from particle count and world area so that
    /// visual coverage stays constant.  The stored `particle_radius` is ignored.
    pub auto_particle_size: bool,
    /// Row-major attraction coefficients.
    ///
    /// `[0..256]`: the N×N particle–particle matrix; `attraction[i * MAX_SPECIES + j]` is the
    /// force species `j` exerts on species `i` (row = recipient, col = attractor).
    /// `[256..272]`: the wall-attraction row for border mode 3 (Matrix); `A[256 + j]` is the
    /// wall's pull on species `j`.
    pub attraction: [f32; 272],
    /// Per-species colours as packed sRGB `0xFF_BB_GG_RR` u32s.
    pub palette: [u32; 16],
    /// When true the simulation clock is frozen; no compute dispatches are issued.
    pub paused: bool,
    /// One-shot flag set by the UI/keyboard to advance exactly one frame while paused.
    /// Cleared by `app.rs` immediately after the dispatch completes.
    pub step_requested: bool,
    /// Per-species render visibility; hidden species are uploaded with alpha = 0.
    pub species_visible: [bool; MAX_SPECIES],
    /// Active border behaviour: 0 = Wrap (torus), 1 = Repel (spring wall), 2 = Static (hard wall), 3 = Matrix.
    pub border_mode: u32,
    /// Multiplier applied to the spring-wall force in Repel mode.
    pub border_repel_strength: f32,
    /// World dimensions in simulation units.  At the default 1280×720 these equal pixel
    /// counts; at other sizes only the aspect ratio and ratio to [`BASE_WORLD_HEIGHT`]
    /// affect physics (the normalised interaction radius shrinks as the world grows).
    pub world_width: f32,
    /// See [`world_width`](Self::world_width).
    pub world_height: f32,
    /// When true, [`auto_world_size`](Self::auto_world_size) scales the world to keep
    /// particle density constant at [`density_target`](Self::density_target).
    pub auto_density: bool,
    /// Target particle density in particles per square world-unit.  Default matches the
    /// built-in preset default: 5 000 particles in a 1280×720 world.
    pub density_target: f32,
    /// When true (and `auto_density` is on), the world size is adjusted each frame via a
    /// proportional FPS controller instead of a fixed [`density_target`](Self::density_target).
    pub perf_auto: bool,
    /// Target FPS for the auto-performance feedback controller.
    pub perf_target_fps: f32,
    // Accumulates dt between world-size adjustments in perf_auto mode.
    perf_adj_timer: f32,
    /// Mouse cursor world-space X position for the attractor/repulsor; set by `app.rs` each frame.
    pub mouse_x: f32,
    /// Mouse cursor world-space Y position for the attractor/repulsor; set by `app.rs` each frame.
    pub mouse_y: f32,
    /// Attractor/repulsor force strength; positive = attract, negative = repel, 0 = inactive.
    pub mouse_strength: f32,
    /// World-space radius of the mouse influence zone.
    pub mouse_range: f32,
    /// When true, `respawn` draws species population fractions from a Dirichlet distribution
    /// instead of distributing particles equally across all species.
    pub random_species_dist: bool,

    // True whenever the attraction matrix has changed since the last dispatch.
    // Uses Cell so dispatch (&self) can clear it without &mut self.
    attraction_dirty: std::cell::Cell<bool>,

    /// Permanent force emitters placed in world space.  Survive respawns; cleared only explicitly.
    pub attractors: Vec<AttractorDef>,

    gpu_particle_count: u32, // may exceed particles.len() after spawn_particles calls
    particles: Vec<Particle>, // CPU copy used for respawn seeding only
    rng: Rng,

    particle_buf: wgpu::Buffer,
    params_buf: wgpu::Buffer,
    attraction_buf: wgpu::Buffer,
    attractor_buf: wgpu::Buffer,
    cell_counts_buf: wgpu::Buffer, // atomic u32 per cell; cleared each frame
    #[allow(dead_code)]
    cell_offsets_buf: wgpu::Buffer, // exclusive prefix sum; MAX_GRID_CELLS+1 entries
    #[allow(dead_code)]
    block_sums_buf: wgpu::Buffer, // per-block totals for parallel prefix; MAX_PREFIX_BLOCKS entries
    #[allow(dead_code)]
    sorted_entries_buf: wgpu::Buffer, // position+species+index in cell-sorted order

    count_pipeline: wgpu::ComputePipeline,
    prefix_a_pipeline: wgpu::ComputePipeline,
    prefix_b_pipeline: wgpu::ComputePipeline,
    prefix_c_pipeline: wgpu::ComputePipeline,
    scatter_pipeline: wgpu::ComputePipeline,
    force_pipeline: wgpu::ComputePipeline,

    count_bind_group: wgpu::BindGroup,
    prefix_a_bind_group: wgpu::BindGroup,
    prefix_b_bind_group: wgpu::BindGroup,
    prefix_c_bind_group: wgpu::BindGroup,
    scatter_bind_group: wgpu::BindGroup,
    force_bind_group: wgpu::BindGroup,
}

/// A single particle as stored in the GPU vertex + storage buffer (24 bytes).
///
/// The layout must exactly match the WGSL `Particle` struct in `compute.wgsl`
/// and the vertex buffer attributes declared in `renderer.rs`.
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct Particle {
    /// World-space XY position (bytes 0–7).
    pub position: [f32; 2],
    /// World-space XY velocity (bytes 8–15).
    pub velocity: [f32; 2],
    /// Packed sRGB colour `0xFF_BB_GG_RR` (bytes 16–19).
    pub color: u32,
    /// Species index, 0-based (bytes 20–23).
    pub species: u32,
}

impl SimulationState {
    /// `world_width / world_height` — passed to the shader to correct for non-square worlds.
    pub fn world_aspect(&self) -> f32 {
        self.world_width / self.world_height
    }

    /// Returns the particle radius to use for rendering.
    ///
    /// When `auto_particle_size` is true, computes a radius that keeps ~4% of the world area
    /// covered by particle disks, clamped to `[0.5, 12.0]`.  This keeps contrast good across
    /// the full particle-count range.  When false, returns the manually set `particle_radius`.
    pub fn effective_particle_radius(&self) -> f32 {
        if self.auto_particle_size {
            let area = self.world_width * self.world_height;
            (0.04 * area / (self.particle_count as f32 * std::f32::consts::PI))
                .sqrt()
                .clamp(0.5, 12.0)
        } else {
            self.particle_radius
        }
    }

    /// Adjust world size toward [`perf_target_fps`](Self::perf_target_fps) using a proportional
    /// controller based on the observed average FPS.
    ///
    /// Throttled to fire at most once every 2 seconds to let FPS stabilise after each adjustment.
    /// No-ops when `!auto_density || !perf_auto`.  Particle positions are unaffected — no
    /// respawn is needed.
    pub fn perf_world_adjust(&mut self, avg_fps: f32, dt: f32) {
        if !self.auto_density || !self.perf_auto || self.paused || avg_fps <= 0.0 {
            return;
        }
        self.perf_adj_timer += dt;
        if self.perf_adj_timer < 2.0 {
            return;
        }
        self.perf_adj_timer = 0.0;

        // GPU work is constant once r_max_norm hits its grid-cell floor.
        // Growing beyond that point has zero effect on performance — cap there.
        let r_norm_floor = 2.0 / (MAX_GRID_CELLS as f32).sqrt();
        let effective_max_h = (self.r_max * BASE_WORLD_HEIGHT / r_norm_floor).min(200_000.0);

        // GPU work ∝ r_max_norm² ∝ 1/world_height²; FPS ∝ world_height².
        // Proportional step toward target: new_h = old_h × sqrt(target/current).
        let ratio = (self.perf_target_fps / avg_fps).sqrt();
        let ratio = ratio.clamp(0.5, 2.0); // max one doubling/halving per step
        let aspect = self.world_aspect();
        let new_h = (self.world_height * ratio).clamp(180.0, effective_max_h);
        self.world_height = new_h;
        self.world_width = new_h * aspect;
    }

    /// Recompute `world_width`/`world_height` to maintain [`density_target`](Self::density_target)
    /// at the current `particle_count`.  No-ops when [`auto_density`](Self::auto_density) is false.
    ///
    /// Call this before [`respawn`](Self::respawn) whenever `particle_count` changes in
    /// auto-density mode.
    pub fn auto_world_size(&mut self) {
        if !self.auto_density {
            return;
        }
        let n = self.particle_count as f32;
        let aspect = self.world_aspect();
        let area = n / self.density_target;
        let h = (area / aspect).sqrt();
        self.world_height = h;
        self.world_width = h * aspect;
    }

    /// The r_max value that will actually be sent to the GPU for the current world size.
    ///
    /// Equal to `r_max * BASE_WORLD_HEIGHT / world_height`, clamped so the grid cell
    /// count never exceeds `MAX_GRID_CELLS`.
    pub fn r_max_normalised(&self) -> f32 {
        (self.r_max * BASE_WORLD_HEIGHT / self.world_height)
            .max(2.0 / (MAX_GRID_CELLS as f32).sqrt())
    }

    /// Current particle density in particles per square world-unit.
    pub fn density(&self) -> f32 {
        self.particle_count as f32 / (self.world_width * self.world_height)
    }

    /// True when the auto-performance controller is at the effective GPU-work floor —
    /// the world cannot grow further to reduce load, so the target FPS may be unachievable.
    pub fn perf_at_limit(&self) -> bool {
        if !self.auto_density || !self.perf_auto {
            return false;
        }
        let r_norm_floor = 2.0 / (MAX_GRID_CELLS as f32).sqrt();
        let effective_max_h = (self.r_max * BASE_WORLD_HEIGHT / r_norm_floor).min(200_000.0);
        self.world_height >= effective_max_h * 0.99
    }

    #[allow(clippy::too_many_arguments)]
    pub fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        tile_size: u32,
        pipeline_cache: Option<&wgpu::PipelineCache>,
        particle_count: usize,
        species_count: usize,
        world_width: f32,
        world_height: f32,
    ) -> Self {
        // ── Buffers ─────────────────────────────────────────────────────────
        let particle_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Particle Buffer"),
            size: (MAX_PARTICLES * std::mem::size_of::<Particle>()) as u64,
            usage: wgpu::BufferUsages::VERTEX
                | wgpu::BufferUsages::STORAGE
                | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let params_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Sim Params"),
            size: std::mem::size_of::<SimParams>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let attraction_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Attraction Matrix"),
            size: ((MAX_SPECIES + 1) * MAX_SPECIES * std::mem::size_of::<f32>()) as u64,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let cell_counts_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Cell Counts"),
            size: (MAX_GRID_CELLS * std::mem::size_of::<u32>()) as u64,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let cell_offsets_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Cell Offsets"),
            size: ((MAX_GRID_CELLS + 1) * std::mem::size_of::<u32>()) as u64,
            usage: wgpu::BufferUsages::STORAGE,
            mapped_at_creation: false,
        });

        // Block sums for the 3-pass parallel prefix scan: one u32 per block of 256 cells + 1 sentinel.
        let block_sums_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Prefix Block Sums"),
            size: (MAX_PREFIX_BLOCKS * std::mem::size_of::<u32>()) as u64,
            usage: wgpu::BufferUsages::STORAGE,
            mapped_at_creation: false,
        });

        // SortedEntry = position(vec2<f32>, 8B) + species(u32, 4B) + index(u32, 4B) = 16B
        let sorted_entries_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Sorted Entries"),
            size: (MAX_PARTICLES * 16) as u64,
            usage: wgpu::BufferUsages::STORAGE,
            mapped_at_creation: false,
        });

        // GpuAttractor = 80 bytes; pre-allocated for MAX_ATTRACTORS slots.
        let attractor_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Attractor Buffer"),
            size: (MAX_ATTRACTORS * std::mem::size_of::<GpuAttractor>()) as u64,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        // ── Bind group layouts ───────────────────────────────────────────────
        let count_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("Count BGL"),
            entries: &[
                storage_bgle(0, true),  // particles (read)
                uniform_bgle(1),        // params
                storage_bgle(2, false), // cell_counts (read_write / atomic)
            ],
        });

        let prefix_a_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("Prefix A BGL"),
            entries: &[
                uniform_bgle(0),        // params
                storage_bgle(1, false), // cell_counts (atomic, zeroed here for scatter)
                storage_bgle(2, false), // cell_offsets (write — local prefix sums)
                storage_bgle(3, false), // block_sums (write — per-block totals)
            ],
        });

        let prefix_b_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("Prefix B BGL"),
            entries: &[
                uniform_bgle(0),        // params
                storage_bgle(1, false), // block_sums (read_write — scan to exclusive prefix)
            ],
        });

        let prefix_c_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("Prefix C BGL"),
            entries: &[
                uniform_bgle(0),        // params
                storage_bgle(1, false), // cell_offsets (read_write — add block base offsets)
                storage_bgle(2, true),  // block_sums (read — block base offsets + sentinel)
            ],
        });

        let scatter_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("Scatter BGL"),
            entries: &[
                storage_bgle(0, true),  // particles (read)
                uniform_bgle(1),        // params
                storage_bgle(2, false), // cell_counts (atomic write cursors)
                storage_bgle(3, true),  // cell_offsets (read)
                storage_bgle(4, false), // sorted_entries (write — merged scatter+reorder)
            ],
        });

        let force_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("Force BGL"),
            entries: &[
                storage_bgle(0, false), // particles (read_write)
                uniform_bgle(1),        // params
                storage_bgle(2, true),  // attraction (read)
                storage_bgle(3, true),  // cell_offsets (read)
                storage_bgle(4, true),  // sorted_entries (read — sequential, cache-friendly)
                storage_bgle(5, true),  // attractors (read — permanent field emitters)
            ],
        });

        // ── Pipelines ────────────────────────────────────────────────────────
        let no_constants = std::collections::HashMap::new();
        // TILE override: AMD uses 64 (one full wavefront); all others use 32.
        let force_constants: std::collections::HashMap<String, f64> = [
            ("TILE".to_string(), tile_size as f64),
            ("MAX_SPECIES".to_string(), MAX_SPECIES as f64),
        ]
        .into_iter()
        .collect();

        let count_pipeline = make_compute_pipeline(
            device,
            "Count Pipeline",
            include_str!("shaders/grid_count.wgsl"),
            &count_bgl,
            &no_constants,
            pipeline_cache,
        );
        let prefix_a_pipeline = make_compute_pipeline(
            device,
            "Prefix A Pipeline",
            include_str!("shaders/grid_prefix_a.wgsl"),
            &prefix_a_bgl,
            &no_constants,
            pipeline_cache,
        );
        let prefix_b_pipeline = make_compute_pipeline(
            device,
            "Prefix B Pipeline",
            include_str!("shaders/grid_prefix_b.wgsl"),
            &prefix_b_bgl,
            &no_constants,
            pipeline_cache,
        );
        let prefix_c_pipeline = make_compute_pipeline(
            device,
            "Prefix C Pipeline",
            include_str!("shaders/grid_prefix_c.wgsl"),
            &prefix_c_bgl,
            &no_constants,
            pipeline_cache,
        );
        let scatter_pipeline = make_compute_pipeline(
            device,
            "Scatter Pipeline",
            include_str!("shaders/grid_scatter.wgsl"),
            &scatter_bgl,
            &no_constants,
            pipeline_cache,
        );
        let force_pipeline = make_compute_pipeline(
            device,
            "Force Pipeline",
            include_str!("shaders/compute.wgsl"),
            &force_bgl,
            &force_constants,
            pipeline_cache,
        );

        // ── Bind groups ──────────────────────────────────────────────────────
        let count_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Count BG"),
            layout: &count_bgl,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: particle_buf.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: params_buf.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: cell_counts_buf.as_entire_binding(),
                },
            ],
        });

        let prefix_a_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Prefix A BG"),
            layout: &prefix_a_bgl,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: params_buf.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: cell_counts_buf.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: cell_offsets_buf.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: block_sums_buf.as_entire_binding(),
                },
            ],
        });

        let prefix_b_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Prefix B BG"),
            layout: &prefix_b_bgl,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: params_buf.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: block_sums_buf.as_entire_binding(),
                },
            ],
        });

        let prefix_c_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Prefix C BG"),
            layout: &prefix_c_bgl,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: params_buf.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: cell_offsets_buf.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: block_sums_buf.as_entire_binding(),
                },
            ],
        });

        let scatter_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Scatter BG"),
            layout: &scatter_bgl,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: particle_buf.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: params_buf.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: cell_counts_buf.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: cell_offsets_buf.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 4,
                    resource: sorted_entries_buf.as_entire_binding(),
                },
            ],
        });

        let force_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Force BG"),
            layout: &force_bgl,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: particle_buf.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: params_buf.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: attraction_buf.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: cell_offsets_buf.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 4,
                    resource: sorted_entries_buf.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 5,
                    resource: attractor_buf.as_entire_binding(),
                },
            ],
        });

        // ── Initial state ────────────────────────────────────────────────────
        let mut rng = Rng::new();
        let mut attraction = [0.0f32; 272];
        seed_attraction_biased(&mut rng, &mut attraction, species_count);

        let mut sim = Self {
            particle_count,
            species_count,
            r_min: 0.025,
            r_max: 0.08,
            friction: 0.5,
            force_scale: 0.007,
            speed_limit: 0.25,
            particle_radius: 1.5,
            auto_particle_size: true,
            attraction,
            palette: PALETTE_DEFAULT,
            paused: false,
            step_requested: false,
            species_visible: [true; MAX_SPECIES],
            border_mode: 0,
            border_repel_strength: 5.0,
            world_width,
            world_height,
            auto_density: false,
            density_target: 5_000.0 / (1280.0 * 720.0),
            perf_auto: false,
            perf_target_fps: 60.0,
            perf_adj_timer: 0.0,
            mouse_x: 0.5,
            mouse_y: 0.5,
            mouse_strength: 0.0,
            mouse_range: 0.1,
            random_species_dist: false,
            attraction_dirty: std::cell::Cell::new(true),
            attractors: Vec::new(),
            gpu_particle_count: 0,
            particles: Vec::new(),
            rng,
            particle_buf,
            params_buf,
            attraction_buf,
            attractor_buf,
            cell_counts_buf,
            cell_offsets_buf,
            block_sums_buf,
            sorted_entries_buf,
            count_pipeline,
            prefix_a_pipeline,
            prefix_b_pipeline,
            prefix_c_pipeline,
            scatter_pipeline,
            force_pipeline,
            count_bind_group,
            prefix_a_bind_group,
            prefix_b_bind_group,
            prefix_c_bind_group,
            scatter_bind_group,
            force_bind_group,
        };
        sim.respawn(queue);
        sim
    }

    /// Scatter `particle_count` particles at random positions and upload to the GPU.
    ///
    /// The attraction matrix and all physics parameters are preserved.
    /// Any particles added via [`spawn_particles`](Self::spawn_particles) are discarded.
    pub fn respawn(&mut self, queue: &wgpu::Queue) {
        let n = self.particle_count.min(MAX_PARTICLES);
        self.particles.clear();
        self.particles.reserve(n);
        if self.random_species_dist && self.species_count > 1 {
            // Dirichlet(1,...,1): normalized exponential variates give a uniform
            // distribution over the simplex, producing truly random population fractions.
            let raw: Vec<f32> = (0..self.species_count)
                .map(|_| -self.rng.next_f32().max(1e-7_f32).ln())
                .collect();
            let total: f32 = raw.iter().sum();
            let mut counts = vec![0usize; self.species_count];
            let mut assigned = 0usize;
            for i in 0..self.species_count - 1 {
                let c = ((raw[i] / total) * n as f32).round() as usize;
                counts[i] = c;
                assigned += c;
            }
            counts[self.species_count - 1] = n.saturating_sub(assigned);
            for (species, &c) in counts.iter().enumerate() {
                for _ in 0..c {
                    self.particles.push(Particle {
                        position: [self.rng.next_f32(), self.rng.next_f32()],
                        velocity: [self.rng.range(-0.05, 0.05), self.rng.range(-0.05, 0.05)],
                        color: self.palette[species],
                        species: species as u32,
                    });
                }
            }
        } else {
            for i in 0..n {
                let species = i % self.species_count;
                self.particles.push(Particle {
                    position: [self.rng.next_f32(), self.rng.next_f32()],
                    velocity: [self.rng.range(-0.05, 0.05), self.rng.range(-0.05, 0.05)],
                    color: self.palette[species],
                    species: species as u32,
                });
            }
        }
        queue.write_buffer(&self.particle_buf, 0, bytemuck::cast_slice(&self.particles));
        self.gpu_particle_count = self.particles.len() as u32;
    }

    /// Scatter new particles near `center` with the given scatter `radius`.
    /// `locked_species` pins the species; `None` randomises each particle.
    /// `aspect` (viewport width / height) corrects the x scatter so the spawn
    /// region matches the circular screen brush exactly.
    /// Particles are written directly to GPU and are transient — lost on next respawn.
    pub fn spawn_particles(
        &mut self,
        queue: &wgpu::Queue,
        center: [f32; 2],
        radius: f32,
        locked_species: Option<usize>,
        aspect: f32,
        batch_size: u32,
    ) {
        let max = MAX_PARTICLES as u32;
        if self.gpu_particle_count >= max {
            return;
        }

        let n = batch_size.min(max - self.gpu_particle_count);
        let mut batch: Vec<Particle> = Vec::with_capacity(n as usize);
        for _ in 0..n {
            let angle = self.rng.next_f32() * (2.0 * std::f32::consts::PI);
            let r = self.rng.next_f32().sqrt() * radius; // sqrt for uniform area distribution
            let sp = locked_species
                .unwrap_or_else(|| (self.rng.next_u32() as usize) % self.species_count);
            // Divide x by aspect so the world-space ellipse maps to a screen circle.
            let x = center[0] + r * angle.cos() / aspect;
            let y = center[1] + r * angle.sin();
            batch.push(Particle {
                position: [x - x.floor(), y - y.floor()],
                velocity: [0.0, 0.0],
                color: self.palette[sp],
                species: sp as u32,
            });
        }

        let offset = self.gpu_particle_count as u64 * std::mem::size_of::<Particle>() as u64;
        queue.write_buffer(&self.particle_buf, offset, bytemuck::cast_slice(&batch));
        self.gpu_particle_count += n;
    }

    /// Submit the six-pass spatial-grid compute pipeline for this frame.
    ///
    /// No-ops when paused or when no particles are present.
    pub fn dispatch(&self, encoder: &mut wgpu::CommandEncoder, queue: &wgpu::Queue, dt: f32) {
        if self.paused {
            return;
        }
        let n = self.gpu_particle_count;
        if n == 0 {
            return;
        }

        // r_min/r_max are stored as fractions of BASE_WORLD_HEIGHT; normalise to [0,1]² space.
        let scale = BASE_WORLD_HEIGHT / self.world_height;
        // Clamp r_max so grid_w² = (2/r_max_norm)² never exceeds MAX_GRID_CELLS.
        let r_max_norm = (self.r_max * scale).max(2.0 / (MAX_GRID_CELLS as f32).sqrt());
        let r_min_norm = self.r_min * scale;

        queue.write_buffer(
            &self.params_buf,
            0,
            bytemuck::bytes_of(&SimParams {
                dt,
                r_min: r_min_norm,
                r_max: r_max_norm,
                friction: self.friction,
                n_particles: n,
                n_species: self.species_count as u32,
                force_scale: self.force_scale,
                aspect: self.world_aspect(),
                mouse_x: self.mouse_x,
                mouse_y: self.mouse_y,
                mouse_strength: self.mouse_strength,
                mouse_range: self.mouse_range,
                border_mode: self.border_mode,
                border_repel_strength: self.border_repel_strength,
                speed_limit: self.speed_limit,
                n_attractors: self.attractors.len() as u32,
            }),
        );
        if self.attraction_dirty.get() {
            queue.write_buffer(
                &self.attraction_buf,
                0,
                bytemuck::cast_slice(&self.attraction),
            );
            self.attraction_dirty.set(false);
        }

        // Upload permanent attractor data (always re-uploaded; positions may have drifted).
        if !self.attractors.is_empty() {
            let gpu: Vec<GpuAttractor> = self
                .attractors
                .iter()
                .map(|a| GpuAttractor {
                    pos: a.pos,
                    range: a.range,
                    _pad: 0.0,
                    strength: a.strength,
                })
                .collect();
            queue.write_buffer(&self.attractor_buf, 0, bytemuck::cast_slice(&gpu));
        }

        // Clear only the cells in use (not the full MAX_GRID_CELLS allocation).
        let grid_w = (2.0_f32 / r_max_norm) as u64;
        let grid_w = grid_w.max(5);
        let n_cells = grid_w * grid_w;
        encoder.clear_buffer(&self.cell_counts_buf, 0, Some(n_cells * 4));

        let workgroups = n.div_ceil(64);

        {
            let mut p = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("Grid Count"),
                timestamp_writes: None,
            });
            p.set_pipeline(&self.count_pipeline);
            p.set_bind_group(0, &self.count_bind_group, &[]);
            p.dispatch_workgroups(workgroups, 1, 1);
        }
        // 3-pass parallel prefix scan (Blelloch block scan → serial block-sum scan → propagate).
        let n_blocks = (n_cells as u32).div_ceil(256);
        {
            let mut p = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("Grid Prefix A — block scan"),
                timestamp_writes: None,
            });
            p.set_pipeline(&self.prefix_a_pipeline);
            p.set_bind_group(0, &self.prefix_a_bind_group, &[]);
            p.dispatch_workgroups(n_blocks, 1, 1);
        }
        {
            let mut p = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("Grid Prefix B — block-sums scan"),
                timestamp_writes: None,
            });
            p.set_pipeline(&self.prefix_b_pipeline);
            p.set_bind_group(0, &self.prefix_b_bind_group, &[]);
            p.dispatch_workgroups(1, 1, 1);
        }
        {
            let mut p = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("Grid Prefix C — propagate"),
                timestamp_writes: None,
            });
            p.set_pipeline(&self.prefix_c_pipeline);
            p.set_bind_group(0, &self.prefix_c_bind_group, &[]);
            // +256 ensures the sentinel at index n_cells is covered by a thread.
            p.dispatch_workgroups((n_cells as u32 + 256) / 256, 1, 1);
        }
        {
            let mut p = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("Grid Scatter"),
                timestamp_writes: None,
            });
            p.set_pipeline(&self.scatter_pipeline);
            p.set_bind_group(0, &self.scatter_bind_group, &[]);
            p.dispatch_workgroups(workgroups, 1, 1);
        }
        {
            let mut p = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("Grid Force"),
                timestamp_writes: None,
            });
            p.set_pipeline(&self.force_pipeline);
            p.set_bind_group(0, &self.force_bind_group, &[]);
            p.dispatch_workgroups(workgroups, 1, 1);
        }
    }

    /// Apply all fields from a `Preset` and respawn particles.
    pub fn apply_preset(&mut self, queue: &wgpu::Queue, preset: &crate::config::Preset) {
        self.particle_count = preset.particle_count.min(MAX_PARTICLES);
        self.species_count = preset.species_count.min(MAX_SPECIES);
        self.world_width = preset.world_width;
        self.world_height = preset.world_height;
        self.particle_radius = preset.particle_radius;
        self.auto_particle_size = preset.auto_particle_size;
        self.r_min = preset.r_min;
        self.r_max = preset.r_max;
        self.friction = preset.friction;
        self.force_scale = preset.force_scale;
        self.border_mode = preset.border_mode;
        self.border_repel_strength = preset.border_repel_strength;
        self.auto_density = preset.auto_density;
        if let Some(dt) = preset.density_target {
            self.density_target = dt;
        }
        self.perf_auto = preset.perf_auto;
        if let Some(fps) = preset.perf_target_fps {
            self.perf_target_fps = fps;
        }

        // Copy compact n×n matrix into the full 16×16 layout; wall row at [256..272].
        self.attraction = [0.0f32; 272];
        let n = self.species_count;
        let pn = preset.species_count.min(MAX_SPECIES);
        for i in 0..n.min(pn) {
            for j in 0..n.min(pn) {
                if let Some(&v) = preset.attraction.get(i * pn + j) {
                    self.attraction[i * MAX_SPECIES + j] = v;
                }
            }
        }
        if let Some(ref wa) = preset.wall_attraction {
            for (s, &v) in wa.iter().enumerate().take(MAX_SPECIES) {
                self.attraction[MAX_SPECIES * MAX_SPECIES + s] = v;
            }
        } else if self.border_mode == 3 {
            for j in 0..self.species_count {
                self.attraction[MAX_SPECIES * MAX_SPECIES + j] = self.rng.range(-1.0, 1.0);
            }
        }
        self.attraction_dirty.set(true);
        self.species_visible = [true; MAX_SPECIES];
        if let Some(ref pal) = preset.palette {
            for (i, &c) in pal.iter().enumerate().take(MAX_SPECIES) {
                self.palette[i] = c;
            }
        } else {
            self.palette = PALETTE_DEFAULT;
        }

        // Load attractors from preset, padding or truncating per-species strengths as needed.
        self.attractors.clear();
        for ac in &preset.attractors {
            let mut strength = [0.0f32; MAX_SPECIES];
            for (i, &v) in ac.strength.iter().enumerate().take(MAX_SPECIES) {
                strength[i] = v;
            }
            self.attractors.push(AttractorDef {
                pos: [ac.x, ac.y],
                range: ac.range,
                strength,
                velocity: [ac.vel_x, ac.vel_y],
            });
            if self.attractors.len() >= MAX_ATTRACTORS {
                break;
            }
        }

        self.respawn(queue);
    }

    /// Snapshot current state as a `Preset` (used for session persistence).
    pub fn to_preset(&self, name: &str) -> crate::config::Preset {
        let n = self.species_count;
        let mut attraction = vec![0.0f32; n * n];
        for i in 0..n {
            for j in 0..n {
                attraction[i * n + j] = self.attraction[i * MAX_SPECIES + j];
            }
        }
        crate::config::Preset {
            name: name.into(),
            description: String::new(),
            particle_count: self.particle_count,
            species_count: self.species_count,
            world_width: self.world_width,
            world_height: self.world_height,
            particle_radius: self.particle_radius,
            auto_particle_size: self.auto_particle_size,
            r_min: self.r_min,
            r_max: self.r_max,
            friction: self.friction,
            force_scale: self.force_scale,
            border_mode: self.border_mode,
            border_repel_strength: self.border_repel_strength,
            auto_density: self.auto_density,
            density_target: if self.auto_density {
                Some(self.density_target)
            } else {
                None
            },
            perf_auto: self.perf_auto,
            perf_target_fps: if self.perf_auto {
                Some(self.perf_target_fps)
            } else {
                None
            },
            attraction,
            wall_attraction: {
                let wa: Vec<f32> = self.attraction
                    [MAX_SPECIES * MAX_SPECIES..MAX_SPECIES * MAX_SPECIES + MAX_SPECIES]
                    .to_vec();
                if wa.iter().all(|&v| v == 0.0) {
                    None
                } else {
                    Some(wa)
                }
            },
            palette: Some(self.palette.to_vec()),
            attractors: self
                .attractors
                .iter()
                .map(|a| crate::config::AttractorConfig {
                    x: a.pos[0],
                    y: a.pos[1],
                    range: a.range,
                    strength: a.strength[..self.species_count].to_vec(),
                    vel_x: a.velocity[0],
                    vel_y: a.velocity[1],
                })
                .collect(),
        }
    }

    /// Restore physics parameters to their defaults without touching the attraction matrix.
    pub fn reset_params(&mut self) {
        self.r_min = 0.025;
        self.r_max = 0.08;
        self.friction = 0.5;
        self.force_scale = 0.007;
        self.speed_limit = 0.25;
        self.particle_radius = 1.5;
    }

    /// Fill the active sub-matrix: positive self-attraction on diagonal, random off-diagonal.
    /// In Matrix border mode the wall row is also randomized.
    pub fn randomize_attraction(&mut self) {
        seed_attraction_biased(&mut self.rng, &mut self.attraction, self.species_count);
        if self.border_mode == 3 {
            self.randomize_wall_row();
        }
        self.attraction_dirty.set(true);
    }

    /// Fill the wall row with random values in [-1, 1].
    pub fn randomize_wall_row(&mut self) {
        for j in 0..self.species_count {
            self.attraction[MAX_SPECIES * MAX_SPECIES + j] = self.rng.range(-1.0, 1.0);
        }
        self.attraction_dirty.set(true);
    }

    /// Mark the attraction matrix as changed so the next dispatch re-uploads it to the GPU.
    pub fn mark_attraction_dirty(&self) {
        self.attraction_dirty.set(true);
    }

    /// Generate random palette colours for the active species using evenly-spaced hues.
    pub fn randomize_palette(&mut self) {
        let n = self.species_count;
        let hue_offset = self.rng.next_f32() * 360.0;
        for i in 0..n {
            let h = (hue_offset + i as f32 * 360.0 / n as f32) % 360.0;
            let s = 0.75 + self.rng.next_f32() * 0.25;
            let v = 0.80 + self.rng.next_f32() * 0.20;
            self.palette[i] = hsv_to_packed_srgb(h, s, v);
        }
    }

    /// The GPU buffer containing all particle data (vertex + storage).
    pub fn particle_buffer(&self) -> &wgpu::Buffer {
        &self.particle_buf
    }

    /// Per-species particle counts from the last `respawn()`.
    ///
    /// Does not include particles added via `spawn_particles()` — the caller
    /// (`app.rs`) tracks those separately and adds them on top.
    pub fn species_counts(&self) -> Vec<usize> {
        let mut counts = vec![0usize; self.species_count];
        for p in &self.particles {
            let s = p.species as usize;
            if s < counts.len() {
                counts[s] += 1;
            }
        }
        counts
    }

    /// Number of particles currently active on the GPU, including any transiently spawned ones.
    pub fn particle_count_gpu(&self) -> u32 {
        self.gpu_particle_count
    }

    /// Advance attractor drift positions by `dt` seconds.
    ///
    /// In wrap mode (border_mode 0) attractors torus-wrap at the [0,1]² boundary.
    /// In all other modes they bounce off the walls.
    /// Call this each frame before [`dispatch`](Self::dispatch).
    pub fn tick_attractors(&mut self, dt: f32) {
        let max_speed = self.r_max_normalised() / dt * self.speed_limit;
        for attr in &mut self.attractors {
            if attr.velocity == [0.0, 0.0] {
                continue;
            }
            let spd = (attr.velocity[0] * attr.velocity[0] + attr.velocity[1] * attr.velocity[1])
                .sqrt();
            if spd > max_speed {
                let scale = max_speed / spd;
                attr.velocity[0] *= scale;
                attr.velocity[1] *= scale;
            }
            attr.pos[0] += attr.velocity[0] * dt;
            attr.pos[1] += attr.velocity[1] * dt;
            if self.border_mode == 0 {
                attr.pos[0] = attr.pos[0].rem_euclid(1.0);
                attr.pos[1] = attr.pos[1].rem_euclid(1.0);
            } else {
                // Bounce off walls in repel/static/matrix modes.
                if attr.pos[0] < 0.0 {
                    attr.pos[0] = -attr.pos[0];
                    attr.velocity[0] = attr.velocity[0].abs();
                }
                if attr.pos[0] > 1.0 {
                    attr.pos[0] = 2.0 - attr.pos[0];
                    attr.velocity[0] = -attr.velocity[0].abs();
                }
                if attr.pos[1] < 0.0 {
                    attr.pos[1] = -attr.pos[1];
                    attr.velocity[1] = attr.velocity[1].abs();
                }
                if attr.pos[1] > 1.0 {
                    attr.pos[1] = 2.0 - attr.pos[1];
                    attr.velocity[1] = -attr.velocity[1].abs();
                }
            }
        }
    }

    /// Add a permanent field attractor.  No-ops when [`MAX_ATTRACTORS`] is already reached.
    pub fn add_attractor(&mut self, def: AttractorDef) {
        if self.attractors.len() < MAX_ATTRACTORS {
            self.attractors.push(def);
        }
    }

    /// Remove the attractor closest to `pos` if it is within `threshold` world-units.
    ///
    /// Returns `true` if an attractor was removed.
    pub fn remove_nearest_attractor(&mut self, pos: [f32; 2], threshold: f32) -> bool {
        let aspect = self.world_aspect();
        let threshold_sq = threshold * threshold;
        let mut best_idx = None;
        let mut best_dsq = f32::MAX;
        for (i, attr) in self.attractors.iter().enumerate() {
            let dx = (attr.pos[0] - pos[0]) * aspect;
            let dy = attr.pos[1] - pos[1];
            let dsq = dx * dx + dy * dy;
            if dsq < best_dsq {
                best_dsq = dsq;
                best_idx = Some(i);
            }
        }
        if let Some(idx) = best_idx
            && best_dsq <= threshold_sq
        {
            self.attractors.remove(idx);
            return true;
        }
        false
    }

    /// Remove all permanent field attractors.
    pub fn clear_attractors(&mut self) {
        self.attractors.clear();
    }
}

// ── Helpers ──────────────────────────────────────────────────────────────────

/// Convert HSV (h in \[0,360), s and v in \[0,1\]) to a packed sRGB `0xFF_BB_GG_RR` u32.
fn hsv_to_packed_srgb(h: f32, s: f32, v: f32) -> u32 {
    let c = v * s;
    let x = c * (1.0 - ((h / 60.0) % 2.0 - 1.0).abs());
    let m = v - c;
    let (r, g, b) = if h < 60.0 {
        (c, x, 0.0)
    } else if h < 120.0 {
        (x, c, 0.0)
    } else if h < 180.0 {
        (0.0, c, x)
    } else if h < 240.0 {
        (0.0, x, c)
    } else if h < 300.0 {
        (x, 0.0, c)
    } else {
        (c, 0.0, x)
    };
    let to_u8 = |f: f32| ((f + m).clamp(0.0, 1.0) * 255.0).round() as u32;
    0xFF00_0000 | (to_u8(b) << 16) | (to_u8(g) << 8) | to_u8(r)
}

/// Fill the active N×N sub-matrix with uniform random values in `[-1, 1]`.
fn seed_attraction_biased(rng: &mut Rng, attraction: &mut [f32; 272], species_count: usize) {
    for i in 0..species_count {
        for j in 0..species_count {
            attraction[i * MAX_SPECIES + j] = rng.range(-1.0, 1.0);
        }
    }
}

fn storage_bgle(binding: u32, read_only: bool) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::COMPUTE,
        ty: wgpu::BindingType::Buffer {
            ty: wgpu::BufferBindingType::Storage { read_only },
            has_dynamic_offset: false,
            min_binding_size: None,
        },
        count: None,
    }
}

fn uniform_bgle(binding: u32) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::COMPUTE,
        ty: wgpu::BindingType::Buffer {
            ty: wgpu::BufferBindingType::Uniform,
            has_dynamic_offset: false,
            min_binding_size: None,
        },
        count: None,
    }
}

fn make_compute_pipeline(
    device: &wgpu::Device,
    label: &str,
    wgsl: &str,
    bgl: &wgpu::BindGroupLayout,
    constants: &std::collections::HashMap<String, f64>,
    cache: Option<&wgpu::PipelineCache>,
) -> wgpu::ComputePipeline {
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some(label),
        source: wgpu::ShaderSource::Wgsl(wgsl.into()),
    });
    let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some(label),
        bind_group_layouts: &[bgl],
        push_constant_ranges: &[],
    });
    device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: Some(label),
        layout: Some(&layout),
        module: &shader,
        entry_point: Some("cs_main"),
        compilation_options: wgpu::PipelineCompilationOptions {
            constants,
            ..Default::default()
        },
        cache,
    })
}

// ── RNG ──────────────────────────────────────────────────────────────────────

/// Xorshift32 PRNG. Fast, non-cryptographic; sufficient for particle spawn and attraction init.
struct Rng(u32);

impl Rng {
    fn new() -> Self {
        Self(0xDEAD_BEEF)
    }

    fn next_u32(&mut self) -> u32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 17;
        self.0 ^= self.0 << 5;
        self.0
    }

    fn next_f32(&mut self) -> f32 {
        (self.next_u32() as f32) * (1.0 / u32::MAX as f32)
    }

    fn range(&mut self, lo: f32, hi: f32) -> f32 {
        lo + self.next_f32() * (hi - lo)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::mem;

    #[test]
    fn sim_params_is_64_bytes() {
        assert_eq!(mem::size_of::<SimParams>(), 64);
    }

    #[test]
    fn gpu_attractor_is_80_bytes() {
        assert_eq!(mem::size_of::<GpuAttractor>(), 80);
    }

    #[test]
    fn particle_is_24_bytes() {
        assert_eq!(mem::size_of::<Particle>(), 24);
    }

    // The vertex shader in renderer.rs hardcodes these byte offsets.
    // If they drift the sim renders garbage with no compile error.
    #[test]
    fn particle_field_offsets() {
        assert_eq!(mem::offset_of!(Particle, position), 0);
        assert_eq!(mem::offset_of!(Particle, velocity), 8);
        assert_eq!(mem::offset_of!(Particle, color), 16);
        assert_eq!(mem::offset_of!(Particle, species), 20);
    }

    // The attraction buffer layout is load-bearing: [0..256] is the 16×16 matrix,
    // [256..272] is the wall row.  If MAX_SPECIES changes these must stay consistent.
    #[test]
    fn attraction_array_layout() {
        assert_eq!(
            MAX_SPECIES * MAX_SPECIES,
            256,
            "particle matrix occupies [0..256]"
        );
        assert_eq!(
            MAX_SPECIES * MAX_SPECIES + MAX_SPECIES,
            272,
            "wall row ends at 272"
        );
    }

    #[test]
    fn rng_is_deterministic() {
        let mut a = Rng::new();
        let mut b = Rng::new();
        for _ in 0..100 {
            assert_eq!(a.next_u32(), b.next_u32());
        }
    }

    #[test]
    fn rng_next_f32_is_in_unit_interval() {
        let mut rng = Rng::new();
        for _ in 0..1000 {
            let v = rng.next_f32();
            assert!(v >= 0.0 && v <= 1.0, "next_f32 out of [0,1]: {v}");
        }
    }

    #[test]
    fn rng_range_stays_in_bounds() {
        let mut rng = Rng::new();
        for _ in 0..1000 {
            let v = rng.range(-1.0, 1.0);
            assert!(v >= -1.0 && v <= 1.0, "range out of [-1,1]: {v}");
        }
    }

    #[test]
    fn hsv_to_packed_srgb_red() {
        // Pure red: h=0, s=1, v=1 → RGB(255, 0, 0), packed 0xFF0000FF
        let packed = hsv_to_packed_srgb(0.0, 1.0, 1.0);
        assert_eq!(packed & 0xFF, 255, "R channel"); // R in bits 0-7
        assert_eq!((packed >> 8) & 0xFF, 0, "G channel");
        assert_eq!((packed >> 16) & 0xFF, 0, "B channel");
        assert_eq!((packed >> 24) & 0xFF, 0xFF, "A channel");
    }

    #[test]
    fn hsv_to_packed_srgb_green() {
        // Pure green: h=120, s=1, v=1 → RGB(0, 255, 0), packed 0xFF00FF00
        let packed = hsv_to_packed_srgb(120.0, 1.0, 1.0);
        assert_eq!(packed & 0xFF, 0, "R channel");
        assert_eq!((packed >> 8) & 0xFF, 255, "G channel");
        assert_eq!((packed >> 16) & 0xFF, 0, "B channel");
    }

    #[test]
    fn hsv_to_packed_srgb_blue() {
        // Pure blue: h=240, s=1, v=1 → RGB(0, 0, 255), packed 0xFFFF0000
        let packed = hsv_to_packed_srgb(240.0, 1.0, 1.0);
        assert_eq!(packed & 0xFF, 0, "R channel");
        assert_eq!((packed >> 8) & 0xFF, 0, "G channel");
        assert_eq!((packed >> 16) & 0xFF, 255, "B channel");
    }

    #[test]
    fn hsv_to_packed_srgb_alpha_always_ff() {
        let mut rng = Rng::new();
        for _ in 0..50 {
            let h = rng.next_f32() * 360.0;
            let s = rng.next_f32();
            let v = rng.next_f32();
            let packed = hsv_to_packed_srgb(h, s, v);
            assert_eq!((packed >> 24) & 0xFF, 0xFF, "alpha must always be 0xFF");
        }
    }
}
