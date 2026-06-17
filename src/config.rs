//! Preset serialisation, built-in preset library, session persistence, and
//! attraction-matrix share codes.
//!
//! A [`Preset`] captures every parameter needed to recreate a simulation state.
//! Built-in presets are compiled into the binary; user presets are read from
//! `presets/*.toml` and the last session is auto-saved to `session.toml`.
//!
//! [`encode_matrix`] / [`decode_matrix`] convert the active N×N attraction
//! matrix to and from a compact base64 string suitable for sharing between
//! running instances.

use std::path::{Path, PathBuf};

/// Path to the auto-save file written on exit and read on startup.
pub const SESSION_PATH: &str = "session.toml";
/// Directory scanned for user-created `*.toml` preset files.
pub const PRESETS_DIR: &str = "presets";
/// Directory where ad-hoc screenshots are saved.
pub const SCREENSHOTS_DIR: &str = "screenshots";
/// Path to the persisted appearance/theme config.
pub const APPEARANCE_PATH: &str = "appearance.toml";

// ── Appearance ────────────────────────────────────────────────────────────────

/// UI colour theme choices shown in the Appearance panel.
#[derive(Clone, Copy, PartialEq, Default, serde::Serialize, serde::Deserialize)]
pub enum UiTheme {
    /// Follow the OS dark/light preference.
    #[default]
    System,
    Dark,
    Light,
    Midnight,
    Nord,
    Catppuccin,
}

/// Persisted appearance preferences (theme + world background colour).
#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub struct AppearanceConfig {
    #[serde(default)]
    pub ui_theme: UiTheme,
    /// World background colour as sRGB bytes `[R, G, B]`.
    #[serde(default = "default_bg")]
    pub bg_color: [u8; 3],
}

fn default_bg() -> [u8; 3] {
    [3, 3, 5]
}

impl Default for AppearanceConfig {
    fn default() -> Self {
        Self {
            ui_theme: UiTheme::default(),
            bg_color: default_bg(),
        }
    }
}

/// Persist appearance config to `appearance.toml` (best-effort; logs on failure).
pub fn save_appearance(a: &AppearanceConfig) {
    match toml::to_string_pretty(a) {
        Ok(s) => {
            if let Err(e) = std::fs::write(APPEARANCE_PATH, s) {
                log::warn!("Failed to save appearance: {e}");
            }
        }
        Err(e) => log::warn!("Failed to serialise appearance: {e}"),
    }
}

/// Load `appearance.toml`, returning defaults if missing or malformed.
pub fn load_appearance() -> AppearanceConfig {
    std::fs::read_to_string(APPEARANCE_PATH)
        .ok()
        .and_then(|s| toml::from_str(&s).ok())
        .unwrap_or_default()
}

// ── Preset ────────────────────────────────────────────────────────────────────

/// A complete snapshot of simulation parameters that can be saved and restored.
///
/// Serialised as TOML; all fields map directly to [`SimulationState`](crate::simulation::SimulationState).
///
/// ## World size and interaction radius
///
/// `r_min` and `r_max` are stored as fractions of [`BASE_WORLD_HEIGHT`](crate::simulation::BASE_WORLD_HEIGHT)
/// (720 world-units).  At the default world (`world_height = 720`) they equal the GPU-normalised
/// value directly; at other world heights the engine scales them so that the *physical* reach in
/// world-units stays constant.  This means increasing `world_height` dilutes particle density and
/// keeps per-particle neighbour count—and therefore GPU load—roughly constant.
#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub struct Preset {
    pub name: String,
    #[serde(default)]
    pub description: String,
    pub particle_count: usize,
    pub species_count: usize,
    /// World width in simulation units.  Together with `world_height` this sets the physical
    /// scale of the world; only the aspect ratio and the ratio to `BASE_WORLD_HEIGHT` (720)
    /// affect physics.
    pub world_width: f32,
    /// World height in simulation units.  See `world_width`.
    pub world_height: f32,
    pub particle_radius: f32,
    /// Hard-core repulsion radius as a fraction of [`BASE_WORLD_HEIGHT`](crate::simulation::BASE_WORLD_HEIGHT).
    pub r_min: f32,
    /// Outer interaction cutoff radius as a fraction of [`BASE_WORLD_HEIGHT`](crate::simulation::BASE_WORLD_HEIGHT).
    pub r_max: f32,
    pub friction: f32,
    pub force_scale: f32,
    /// 0 = Wrap, 1 = Repel, 2 = Static.
    pub border_mode: u32,
    pub border_repel_strength: f32,
    /// When true the engine scales `world_width`/`world_height` to maintain `density_target`
    /// as `particle_count` changes.
    #[serde(default)]
    pub auto_density: bool,
    /// Target particle density in particles per square world-unit; `None` uses the engine default.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub density_target: Option<f32>,
    /// When true (and `auto_density` is on), world size is adjusted dynamically to hit `perf_target_fps`.
    #[serde(default)]
    pub perf_auto: bool,
    /// Target FPS for the auto-performance feedback controller; `None` uses the engine default (60).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub perf_target_fps: Option<f32>,
    /// Row-major `species_count × species_count` attraction matrix; values in `[-1, 1]`.
    pub attraction: Vec<f32>,
    /// Wall attraction row for border mode 3 (Matrix); one value per species in `[-1, 1]`.
    /// Positive → repulsion from walls; negative → attraction. Absent means all zeros.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wall_attraction: Option<Vec<f32>>,
    /// Per-species packed sRGB colours (`0xFF_BB_GG_RR`). Optional; absent means use default.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub palette: Option<Vec<u32>>,
}

impl Preset {
    fn new(name: &str, desc: &str, species: usize, attraction: Vec<f32>) -> Self {
        Self {
            name: name.into(),
            description: desc.into(),
            particle_count: 5_000,
            species_count: species,
            world_width: 1280.0,
            world_height: 720.0,
            particle_radius: 1.5,
            r_min: 0.025,
            r_max: 0.08,
            friction: 0.5,
            force_scale: 0.007,
            border_mode: 0,
            border_repel_strength: 5.0,
            auto_density: false,
            density_target: None,
            perf_auto: false,
            perf_target_fps: None,
            attraction,
            wall_attraction: None,
            palette: None,
        }
    }
}

// ── Built-in presets (always from source; never affected by user files) ───────

/// Returns the 4 hardcoded benchmark/built-in presets.
pub fn builtin_presets() -> Vec<Preset> {
    vec![clusters(), chains(), ecosystem(), symbiosis()]
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
        N,
        a,
    )
}

fn chains() -> Preset {
    // Circular predator-prey: species i chases (i+1), flees from (i-1).
    const N: usize = 6;
    let mut a = vec![0.0f32; N * N];
    for i in 0..N {
        a[i * N + (i + 1) % N] = 0.9; // i attracted to next
        a[((i + 1) % N) * N + i] = -0.4; // next repelled by i
    }
    Preset::new(
        "Chains",
        "Circular predator-prey chain; produces trailing spirals and filament structures.",
        N,
        a,
    )
}

fn ecosystem() -> Preset {
    // Species 0-2: predator-prey chain (spirals); also attracted to the cluster.
    // Species 3-5: tight mutual-attraction cluster (blob); flees the chain.
    #[rustfmt::skip]
    let a = vec![
        //   0      1      2      3      4      5
         0.0_f32,  0.9,  -0.4,   0.5,   0.5,   0.5,  // 0: chases 1, flees 2, pursues cluster
        -0.4,      0.0,   0.9,   0.5,   0.5,   0.5,  // 1: flees 0, chases 2, pursues cluster
         0.9,     -0.4,   0.0,   0.5,   0.5,   0.5,  // 2: chases 0, flees 1, pursues cluster
        -0.5,     -0.5,  -0.5,   0.5,   0.7,   0.7,  // 3: flees chain, bonds with cluster
        -0.5,     -0.5,  -0.5,   0.7,   0.5,   0.7,  // 4: flees chain, bonds with cluster
        -0.5,     -0.5,  -0.5,   0.7,   0.7,   0.5,  // 5: flees chain, bonds with cluster
    ];
    Preset::new(
        "Ecosystem",
        "Spiraling predator chain (species 0–2) hunts a tight fleeing cluster (species 3–5).",
        6,
        a,
    )
}

fn symbiosis() -> Preset {
    // Structural inverse of Clusters: cross-species attraction, mild self-repulsion.
    // Produces large mixed-colour aggregates instead of separated blobs.
    const N: usize = 6;
    let mut a = vec![0.0f32; N * N];
    for i in 0..N {
        for j in 0..N {
            a[i * N + j] = if i == j { -0.1 } else { 0.6 };
        }
    }
    Preset::new(
        "Symbiosis",
        "Every species attracts all others but weakly repels its own kind; produces large mixed-colour aggregates.",
        N,
        a,
    )
}

// ── Bundled presets (TOML files embedded at compile time) ─────────────────────

/// Returns presets embedded from `assets/presets/*.toml` at compile time.
pub fn bundled_presets() -> Vec<Preset> {
    let sources: &[(&str, &str)] = &[
        (
            "Snakes and Stripes",
            include_str!("../assets/presets/Snakes and Stripes.toml"),
        ),
        (
            "Snakes Rings Ships",
            include_str!("../assets/presets/Snakes Rings Ships.toml"),
        ),
    ];
    sources
        .iter()
        .filter_map(|(name, src)| match toml::from_str::<Preset>(src) {
            Ok(p) => Some(p),
            Err(e) => {
                log::warn!("Bundled preset '{name}' failed to parse: {e}");
                None
            }
        })
        .collect()
}

// ── File I/O ──────────────────────────────────────────────────────────────────

/// Parse a TOML preset from `path`.
pub fn load_preset_file(path: &Path) -> Result<Preset, String> {
    let s = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
    toml::from_str(&s).map_err(|e| e.to_string())
}

/// Serialise `preset` to pretty TOML at `path`.
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
        .filter(|p| {
            !p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with('_'))
        })
        .collect();
    paths.sort();
    paths
        .iter()
        .filter_map(|p| match load_preset_file(p) {
            Ok(mut preset) => {
                if (preset.name == "exported" || preset.name.is_empty())
                    && let Some(stem) = p.file_stem().and_then(|s| s.to_str())
                {
                    preset.name = stem.to_string();
                }
                Some(preset)
            }
            Err(e) => {
                log::warn!("Skipping {p:?}: {e}");
                None
            }
        })
        .collect()
}

// ── Share codes ───────────────────────────────────────────────────────────────

use base64::{Engine as _, engine::general_purpose::STANDARD};

/// Encode the active n×n attraction matrix as a compact base64 share code.
///
/// Format: 1 byte `species_count`, then `n*n` bytes where each byte is the
/// attraction value quantised from `[-1.0, 1.0]` to `i8` `[-127, 127]`.
pub fn encode_matrix(species: usize, attraction: &[f32; 272]) -> String {
    let mut bytes = Vec::with_capacity(1 + species * species);
    bytes.push(species as u8);
    for i in 0..species {
        for j in 0..species {
            let v = attraction[i * crate::simulation::MAX_SPECIES + j];
            bytes.push((v.clamp(-1.0, 1.0) * 127.0).round() as i8 as u8);
        }
    }
    STANDARD.encode(&bytes)
}

/// Decode a share code produced by [`encode_matrix`].
///
/// Returns `(species_count, row_major_n×n_values)` on success.
pub fn decode_matrix(code: &str) -> Result<(usize, Vec<f32>), String> {
    let bytes = STANDARD.decode(code.trim()).map_err(|e| e.to_string())?;
    if bytes.is_empty() {
        return Err("empty code".into());
    }
    let n = bytes[0] as usize;
    if n == 0 || n > crate::simulation::MAX_SPECIES {
        return Err(format!("invalid species count {n}"));
    }
    let expected = 1 + n * n;
    if bytes.len() != expected {
        return Err(format!("expected {expected} bytes, got {}", bytes.len()));
    }
    let values = bytes[1..].iter().map(|&b| b as i8 as f32 / 127.0).collect();
    Ok((n, values))
}

// ── Session ───────────────────────────────────────────────────────────────────

/// Persist `preset` as the current session state (best-effort; logs on failure).
pub fn save_session(preset: &Preset) {
    if let Err(e) = save_preset_file(preset, Path::new(SESSION_PATH)) {
        log::warn!("Failed to save session: {e}");
    }
}

/// Load the last saved session, if any.
pub fn load_session() -> Option<Preset> {
    load_preset_file(Path::new(SESSION_PATH)).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_preset_invariants(p: &Preset) {
        assert!(p.r_min > 0.0, "{}: r_min must be positive", p.name);
        assert!(p.r_max > p.r_min, "{}: r_max must exceed r_min", p.name);
        assert!(
            p.friction >= 0.0,
            "{}: friction must be non-negative",
            p.name
        );
        assert!(
            p.force_scale > 0.0,
            "{}: force_scale must be positive",
            p.name
        );
        assert!(
            p.species_count >= 1 && p.species_count <= crate::simulation::MAX_SPECIES,
            "{}: species_count {} out of range [1, {}]",
            p.name,
            p.species_count,
            crate::simulation::MAX_SPECIES
        );
        assert_eq!(
            p.attraction.len(),
            p.species_count * p.species_count,
            "{}: attraction matrix length {} != species_count² {}",
            p.name,
            p.attraction.len(),
            p.species_count * p.species_count
        );
        assert!(
            p.world_width > 0.0,
            "{}: world_width must be positive",
            p.name
        );
        assert!(
            p.world_height > 0.0,
            "{}: world_height must be positive",
            p.name
        );
    }

    #[test]
    fn builtin_presets_are_valid() {
        for preset in builtin_presets() {
            assert_preset_invariants(&preset);
        }
    }

    #[test]
    fn preset_round_trips_via_toml() {
        for preset in builtin_presets() {
            let serialized = toml::to_string_pretty(&preset).expect("serialize failed");
            let restored: Preset = toml::from_str(&serialized).expect("deserialize failed");

            assert_eq!(restored.name, preset.name);
            assert_eq!(restored.particle_count, preset.particle_count);
            assert_eq!(restored.species_count, preset.species_count);
            assert_eq!(restored.border_mode, preset.border_mode);
            assert_eq!(
                restored.attraction.len(),
                preset.attraction.len(),
                "{}: attraction length changed after round-trip",
                preset.name
            );
            for (i, (a, b)) in restored
                .attraction
                .iter()
                .zip(preset.attraction.iter())
                .enumerate()
            {
                assert!(
                    (a - b).abs() < 1e-6,
                    "{}: attraction[{i}] {a} != {b} after round-trip",
                    preset.name
                );
            }
        }
    }
}
