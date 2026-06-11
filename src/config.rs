use std::path::{Path, PathBuf};

pub const SESSION_PATH: &str = "session.toml";
pub const PRESETS_DIR:  &str = "presets";

// ── Preset ────────────────────────────────────────────────────────────────────

#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub struct Preset {
    pub name:                  String,
    #[serde(default)]
    pub description:           String,
    pub particle_count:        usize,
    pub species_count:         usize,
    pub world_width:           f32,
    pub world_height:          f32,
    pub particle_radius:       f32,
    pub r_min:                 f32,
    pub r_max:                 f32,
    pub friction:              f32,
    pub force_scale:           f32,
    pub border_mode:           u32,
    pub border_repel_strength: f32,
    /// Row-major species_count × species_count attraction matrix.
    pub attraction:            Vec<f32>,
}

impl Preset {
    fn new(name: &str, desc: &str, species: usize, attraction: Vec<f32>) -> Self {
        Self {
            name:                  name.into(),
            description:           desc.into(),
            particle_count:        5_000,
            species_count:         species,
            world_width:           1280.0,
            world_height:          720.0,
            particle_radius:       1.5,
            r_min:                 0.025,
            r_max:                 0.08,
            friction:              0.5,
            force_scale:           0.007,
            border_mode:           0,
            border_repel_strength: 5.0,
            attraction,
        }
    }
}

// ── Built-in presets (always from source; never affected by user files) ───────

/// Returns the 4 hardcoded benchmark/built-in presets.
pub fn builtin_presets() -> Vec<Preset> {
    vec![clusters(), chains(), rich_mix(), separation()]
}

fn clusters() -> Preset {
    const N: usize = 6;
    let mut a = vec![0.0f32; N * N];
    for i in 0..N {
        for j in 0..N {
            a[i * N + j] = if i == j { 0.7 } else { -0.2 };
        }
    }
    Preset::new(
        "Clusters",
        "Like attracts like; cross-species repulsion forms compact, coloured clusters.",
        N, a,
    )
}

fn chains() -> Preset {
    // Circular predator-prey: species i chases (i+1), flees from (i-1).
    const N: usize = 6;
    let mut a = vec![0.0f32; N * N];
    for i in 0..N {
        a[i * N + (i + 1) % N]        =  0.9;   // i attracted to next
        a[((i + 1) % N) * N + i]      = -0.4;   // next repelled by i
    }
    Preset::new(
        "Chains",
        "Circular predator-prey chain; produces trailing spirals and filament structures.",
        N, a,
    )
}

fn rich_mix() -> Preset {
    #[rustfmt::skip]
    let a = vec![
         0.3_f32,  0.8,  0.5, -0.4, -0.6,  0.1,
        -0.7,      0.3,  0.7,  0.2, -0.3, -0.5,
        -0.4,     -0.6,  0.3,  0.9,  0.1, -0.2,
         0.2,     -0.3, -0.8,  0.3,  0.7,  0.4,
         0.6,      0.1, -0.1, -0.6,  0.3,  0.8,
        -0.1,      0.5,  0.4, -0.3, -0.7,  0.3,
    ];
    Preset::new(
        "Rich Mix",
        "Hand-crafted asymmetric interactions; produces a variety of emergent structures.",
        6, a,
    )
}

fn separation() -> Preset {
    const N: usize = 4;
    let mut a = vec![0.0f32; N * N];
    for i in 0..N {
        for j in 0..N {
            a[i * N + j] = if i == j { 0.5 } else { -0.9 };
        }
    }
    Preset::new(
        "Separation",
        "Species strongly repel all other species and clump separately.",
        N, a,
    )
}

// ── File I/O ──────────────────────────────────────────────────────────────────

pub fn load_preset_file(path: &Path) -> Result<Preset, String> {
    let s = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
    toml::from_str(&s).map_err(|e| e.to_string())
}

pub fn save_preset_file(preset: &Preset, path: &Path) -> Result<(), String> {
    let s = toml::to_string_pretty(preset).map_err(|e| e.to_string())?;
    std::fs::write(path, s).map_err(|e| e.to_string())
}

/// Scan `PRESETS_DIR` for *.toml files; silently skip parse errors.
pub fn load_presets_dir() -> Vec<Preset> {
    let dir = Path::new(PRESETS_DIR);
    if !dir.is_dir() {
        return vec![];
    }
    let mut paths: Vec<PathBuf> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().map(|e| e == "toml").unwrap_or(false))
        .collect();
    paths.sort();
    paths
        .iter()
        .filter_map(|p| match load_preset_file(p) {
            Ok(preset) => Some(preset),
            Err(e)     => { log::warn!("Skipping {p:?}: {e}"); None }
        })
        .collect()
}

// ── Session ───────────────────────────────────────────────────────────────────

pub fn save_session(preset: &Preset) {
    if let Err(e) = save_preset_file(preset, Path::new(SESSION_PATH)) {
        log::warn!("Failed to save session: {e}");
    }
}

pub fn load_session() -> Option<Preset> {
    load_preset_file(Path::new(SESSION_PATH)).ok()
}
