//! GPU simulation state and the five-pass spatial-grid compute pipeline.
//!
//! The pipeline runs entirely on the GPU each frame:
//! 1. **Count** — atomically increment per-cell particle counts.
//! 2. **Prefix** — exclusive prefix-sum to convert counts to cell offsets.
//! 3. **Scatter** — write each particle's index into its cell slot.
//! 4. **Reorder** — copy position/species data into cell-sorted order.
//! 5. **Force** — compute pairwise Particle Life forces using the sorted grid.

/// Maximum number of species supported by the attraction matrix and PALETTE.
pub const MAX_SPECIES: usize = 8;
const MAX_PARTICLES: usize = 500_000;
// cell = r_max/2, so grid_w = floor(2/r_max); at r_max=0.01 → 200×200 = 40 000 cells.
const MAX_GRID_CELLS: usize = 40_000;

/// Default species colours as packed sRGB `0xFF_BB_GG_RR` u32s.
///
/// These are the sRGB-space equivalents of the previous linear values so the
/// on-screen appearance is identical to the original palette.  Stored as sRGB
/// so the vertex shader can do a single sRGB→linear conversion and the GPU's
/// automatic linear→sRGB encoding on the sRGB framebuffer produces the
/// correct final colour.
pub const PALETTE_DEFAULT: [u32; 8] = [
    0xFF8585EF, // salmon-red   sRGB(239, 133, 133)
    0xFF85EF85, // light-green  sRGB(133, 239, 133)
    0xFFEF9885, // periwinkle   sRGB(133, 152, 239)
    0xFF7AE5EF, // pale-yellow  sRGB(239, 229, 122)
    0xFFEF7AD0, // lavender     sRGB(208, 122, 239)
    0xFFEAEA7A, // pale-cyan    sRGB(122, 234, 234)
    0xFF7ABDF4, // peach        sRGB(244, 189, 122)
    0xFFDBA8F4, // rose-pink    sRGB(244, 168, 219)
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
    _pad: [u32; 2],             // pad to 64 bytes (4 × 16)
}

/// All CPU-side simulation state plus the GPU buffers and compute pipelines.
///
/// `app.rs` owns the single instance and calls [`dispatch`](SimulationState::dispatch)
/// once per frame inside the wgpu command encoder.
pub struct SimulationState {
    /// Target particle count used on the next [`respawn`](SimulationState::respawn).
    pub particle_count: usize,
    pub species_count: usize,
    pub r_min: f32,
    pub r_max: f32,
    pub friction: f32,
    pub force_scale: f32,
    pub particle_radius: f32,  // world units
    pub attraction: [f32; 64], // row-major 8×8; A[i,j] = attraction[i*8+j]
    /// Per-species colours as packed sRGB `0xFF_BB_GG_RR` u32s.
    pub palette: [u32; 8],
    pub paused: bool,
    pub border_mode: u32, // 0 = Wrap, 1 = Repel, 2 = Static
    pub border_repel_strength: f32,
    pub world_width: f32, // world units (= pixels at default zoom)
    pub world_height: f32,
    // Mouse attractor/repulsor — set by app.rs each frame before dispatch.
    pub mouse_x: f32,
    pub mouse_y: f32,
    pub mouse_strength: f32, // positive = attract, negative = repel, 0 = inactive
    pub mouse_range: f32,    // world-space radius of influence

    gpu_particle_count: u32, // may exceed particles.len() after spawn_particles calls
    particles: Vec<Particle>, // CPU copy used for respawn seeding only
    rng: Rng,

    particle_buf: wgpu::Buffer,
    params_buf: wgpu::Buffer,
    attraction_buf: wgpu::Buffer,
    cell_counts_buf: wgpu::Buffer, // atomic u32 per cell; cleared each frame
    #[allow(dead_code)]
    cell_offsets_buf: wgpu::Buffer, // exclusive prefix sum; MAX_GRID_CELLS+1 entries
    #[allow(dead_code)]
    sorted_indices_buf: wgpu::Buffer, // particle indices sorted by cell
    #[allow(dead_code)]
    sorted_entries_buf: wgpu::Buffer, // position+species+index in cell-sorted order

    count_pipeline: wgpu::ComputePipeline,
    prefix_pipeline: wgpu::ComputePipeline,
    scatter_pipeline: wgpu::ComputePipeline,
    reorder_pipeline: wgpu::ComputePipeline,
    force_pipeline: wgpu::ComputePipeline,

    count_bind_group: wgpu::BindGroup,
    prefix_bind_group: wgpu::BindGroup,
    scatter_bind_group: wgpu::BindGroup,
    reorder_bind_group: wgpu::BindGroup,
    force_bind_group: wgpu::BindGroup,
}

/// A single particle as stored in the GPU vertex + storage buffer (24 bytes).
///
/// The layout must exactly match the WGSL `Particle` struct in `compute.wgsl`
/// and the vertex buffer attributes declared in `renderer.rs`.
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct Particle {
    pub position: [f32; 2], // offset  0
    pub velocity: [f32; 2], // offset  8
    pub color: u32,         // offset 16
    pub species: u32,       // offset 20
}

impl SimulationState {
    /// `world_width / world_height` — passed to the shader to correct for non-square worlds.
    pub fn world_aspect(&self) -> f32 {
        self.world_width / self.world_height
    }

    pub fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
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
            size: (MAX_SPECIES * MAX_SPECIES * std::mem::size_of::<f32>()) as u64,
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

        let sorted_indices_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Sorted Indices"),
            size: (MAX_PARTICLES * std::mem::size_of::<u32>()) as u64,
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

        // ── Bind group layouts ───────────────────────────────────────────────
        let count_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("Count BGL"),
            entries: &[
                storage_bgle(0, true),  // particles (read)
                uniform_bgle(1),        // params
                storage_bgle(2, false), // cell_counts (read_write / atomic)
            ],
        });

        let prefix_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("Prefix BGL"),
            entries: &[
                uniform_bgle(0),        // params
                storage_bgle(1, false), // cell_counts (read_write / atomic)
                storage_bgle(2, false), // cell_offsets (write)
            ],
        });

        let scatter_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("Scatter BGL"),
            entries: &[
                storage_bgle(0, true),  // particles (read)
                uniform_bgle(1),        // params
                storage_bgle(2, false), // cell_counts (atomic write cursors)
                storage_bgle(3, true),  // cell_offsets (read)
                storage_bgle(4, false), // sorted_indices (write)
            ],
        });

        let reorder_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("Reorder BGL"),
            entries: &[
                storage_bgle(0, true),  // particles (read)
                uniform_bgle(1),        // params
                storage_bgle(2, true),  // sorted_indices (read)
                storage_bgle(3, false), // sorted_entries (write)
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
            ],
        });

        // ── Pipelines ────────────────────────────────────────────────────────
        let count_pipeline = make_compute_pipeline(
            device,
            "Count Pipeline",
            include_str!("shaders/grid_count.wgsl"),
            &count_bgl,
        );
        let prefix_pipeline = make_compute_pipeline(
            device,
            "Prefix Pipeline",
            include_str!("shaders/grid_prefix.wgsl"),
            &prefix_bgl,
        );
        let scatter_pipeline = make_compute_pipeline(
            device,
            "Scatter Pipeline",
            include_str!("shaders/grid_scatter.wgsl"),
            &scatter_bgl,
        );
        let reorder_pipeline = make_compute_pipeline(
            device,
            "Reorder Pipeline",
            include_str!("shaders/grid_reorder.wgsl"),
            &reorder_bgl,
        );
        let force_pipeline = make_compute_pipeline(
            device,
            "Force Pipeline",
            include_str!("shaders/compute.wgsl"),
            &force_bgl,
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

        let prefix_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Prefix BG"),
            layout: &prefix_bgl,
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
                    resource: sorted_indices_buf.as_entire_binding(),
                },
            ],
        });

        let reorder_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Reorder BG"),
            layout: &reorder_bgl,
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
                    resource: sorted_indices_buf.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
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
            ],
        });

        // ── Initial state ────────────────────────────────────────────────────
        let mut rng = Rng::new();
        let mut attraction = [0.0f32; 64];
        seed_attraction_biased(&mut rng, &mut attraction, species_count);

        let mut sim = Self {
            particle_count,
            species_count,
            r_min: 0.025,
            r_max: 0.08,
            friction: 0.5,
            force_scale: 0.007,
            particle_radius: 1.5,
            attraction,
            palette: PALETTE_DEFAULT,
            paused: false,
            border_mode: 0,
            border_repel_strength: 5.0,
            world_width,
            world_height,
            mouse_x: 0.5,
            mouse_y: 0.5,
            mouse_strength: 0.0,
            mouse_range: 0.1,
            gpu_particle_count: 0,
            particles: Vec::new(),
            rng,
            particle_buf,
            params_buf,
            attraction_buf,
            cell_counts_buf,
            cell_offsets_buf,
            sorted_indices_buf,
            sorted_entries_buf,
            count_pipeline,
            prefix_pipeline,
            scatter_pipeline,
            reorder_pipeline,
            force_pipeline,
            count_bind_group,
            prefix_bind_group,
            scatter_bind_group,
            reorder_bind_group,
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
        for i in 0..n {
            let species = i % self.species_count;
            self.particles.push(Particle {
                position: [self.rng.next_f32(), self.rng.next_f32()],
                velocity: [self.rng.range(-0.05, 0.05), self.rng.range(-0.05, 0.05)],
                color: self.palette[species],
                species: species as u32,
            });
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

    /// Submit the five-pass spatial-grid compute pipeline for this frame.
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

        queue.write_buffer(
            &self.params_buf,
            0,
            bytemuck::bytes_of(&SimParams {
                dt,
                r_min: self.r_min,
                r_max: self.r_max,
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
                _pad: [0; 2],
            }),
        );
        queue.write_buffer(
            &self.attraction_buf,
            0,
            bytemuck::cast_slice(&self.attraction),
        );

        // Clear cell counts to 0 before the count pass.
        encoder.clear_buffer(&self.cell_counts_buf, 0, None);

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
        {
            let mut p = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("Grid Prefix"),
                timestamp_writes: None,
            });
            p.set_pipeline(&self.prefix_pipeline);
            p.set_bind_group(0, &self.prefix_bind_group, &[]);
            p.dispatch_workgroups(1, 1, 1);
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
                label: Some("Grid Reorder"),
                timestamp_writes: None,
            });
            p.set_pipeline(&self.reorder_pipeline);
            p.set_bind_group(0, &self.reorder_bind_group, &[]);
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
        self.r_min = preset.r_min;
        self.r_max = preset.r_max;
        self.friction = preset.friction;
        self.force_scale = preset.force_scale;
        self.border_mode = preset.border_mode;
        self.border_repel_strength = preset.border_repel_strength;

        // Copy compact n×n matrix into the full 8×8 layout.
        self.attraction = [0.0f32; 64];
        let n = self.species_count;
        let pn = preset.species_count.min(MAX_SPECIES);
        for i in 0..n.min(pn) {
            for j in 0..n.min(pn) {
                if let Some(&v) = preset.attraction.get(i * pn + j) {
                    self.attraction[i * MAX_SPECIES + j] = v;
                }
            }
        }
        if let Some(ref pal) = preset.palette {
            for (i, &c) in pal.iter().enumerate().take(MAX_SPECIES) {
                self.palette[i] = c;
            }
        } else {
            self.palette = PALETTE_DEFAULT;
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
            r_min: self.r_min,
            r_max: self.r_max,
            friction: self.friction,
            force_scale: self.force_scale,
            border_mode: self.border_mode,
            border_repel_strength: self.border_repel_strength,
            attraction,
            palette: Some(self.palette.to_vec()),
        }
    }

    /// Restore physics parameters to their defaults without touching the attraction matrix.
    pub fn reset_params(&mut self) {
        self.r_min = 0.025;
        self.r_max = 0.08;
        self.friction = 0.5;
        self.force_scale = 0.007;
        self.particle_radius = 1.5;
    }

    /// Fill the active sub-matrix: positive self-attraction on diagonal, random off-diagonal.
    pub fn randomize_attraction(&mut self) {
        seed_attraction_biased(&mut self.rng, &mut self.attraction, self.species_count);
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

    /// Number of particles currently active on the GPU, including any transiently spawned ones.
    pub fn particle_count_gpu(&self) -> u32 {
        self.gpu_particle_count
    }
}

// ── Helpers ──────────────────────────────────────────────────────────────────

/// Convert HSV (h in [0,360), s and v in [0,1]) to a packed sRGB `0xFF_BB_GG_RR` u32.
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

fn seed_attraction_biased(rng: &mut Rng, attraction: &mut [f32; 64], species_count: usize) {
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
        compilation_options: wgpu::PipelineCompilationOptions::default(),
        cache: None,
    })
}

// ── RNG ──────────────────────────────────────────────────────────────────────

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
}
