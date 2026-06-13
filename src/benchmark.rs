//! Benchmarking utilities: a lightweight ad-hoc snapshot ([`QuickBench`]) and a
//! full automated suite ([`BenchmarkRunner`]) that cycles through every
//! combination of built-in preset × particle-count tier and exports CSV results.

use std::path::Path;
use std::time::Instant;

use crate::config::{Preset, builtin_presets};

/// A single tier in the full benchmark suite: fixed particle count and fixed world size.
///
/// World size is pinned per-tier so results remain comparable across runs regardless of the
/// user's current auto-density setting.  All tiers use the canonical 1280×720 world so the
/// suite measures raw GPU throughput at increasing density (O(n²) scaling intentional).
#[derive(Clone, Copy)]
pub struct BenchmarkTier {
    pub particles: usize,
    pub world_width: f32,
    pub world_height: f32,
}

/// Tiers used in the full benchmark suite.
pub const BENCHMARK_TIERS: [BenchmarkTier; 4] = [
    BenchmarkTier {
        particles: 10_000,
        world_width: 1280.0,
        world_height: 720.0,
    },
    BenchmarkTier {
        particles: 50_000,
        world_width: 1280.0,
        world_height: 720.0,
    },
    BenchmarkTier {
        particles: 100_000,
        world_width: 1280.0,
        world_height: 720.0,
    },
    BenchmarkTier {
        particles: 500_000,
        world_width: 1280.0,
        world_height: 720.0,
    },
];

const BUILTIN_COUNT: usize = 4;
// Time-based targets: physics are dt-driven so wall-clock seconds = simulation seconds.
// 5s warmup lets friction decay initial conditions (~3.6 half-lives) and structures form.
// 15s collection gives ~700 frames at the GPU-bound 500K tier (47fps) and ~75K at 10K tier.
const WARMUP_SECS: f64 = 5.0;
const COLLECT_SECS: f64 = 15.0;
const GLOBAL_CAP_SECS: f64 = 360.0; // 16 combos × 20s each + margin

// ── Quick (ad-hoc) benchmark ──────────────────────────────────────────────────

const QUICK_WARMUP_SECS: f64 = 5.0;
const QUICK_COLLECT_SECS: f64 = 15.0;

/// Ad-hoc single-point benchmark: warms up for [`QUICK_WARMUP_SECS`] seconds then
/// collects FPS samples for [`QUICK_COLLECT_SECS`] seconds at the current particle count.
pub struct QuickBench {
    state: QuickBenchState,
}

enum QuickBenchState {
    Idle,
    Warmup {
        start: Instant,
    },
    Collecting {
        fps: Vec<f32>,
        particles: u32,
        start: Instant,
    },
    Done {
        avg: f32,
        min: f32,
        max: f32,
        particles: u32,
    },
}

impl QuickBench {
    pub fn new() -> Self {
        Self {
            state: QuickBenchState::Idle,
        }
    }

    /// Begin a new quick-bench run at the current GPU particle count.
    pub fn start(&mut self, particles: u32) {
        let _ = particles; // stored when we enter Collecting
        self.state = QuickBenchState::Warmup {
            start: Instant::now(),
        };
    }

    /// Returns `true` while warmup or sample collection is in progress.
    pub fn is_running(&self) -> bool {
        matches!(
            self.state,
            QuickBenchState::Warmup { .. } | QuickBenchState::Collecting { .. }
        )
    }

    /// Returns true on the frame the run completes.
    pub fn advance(&mut self, dt: f32, particles: u32) -> bool {
        let old = std::mem::replace(&mut self.state, QuickBenchState::Idle);
        match old {
            QuickBenchState::Warmup { start } => {
                if start.elapsed().as_secs_f64() >= QUICK_WARMUP_SECS {
                    self.state = QuickBenchState::Collecting {
                        fps: vec![],
                        particles,
                        start: Instant::now(),
                    };
                } else {
                    self.state = QuickBenchState::Warmup { start };
                }
                false
            }
            QuickBenchState::Collecting {
                mut fps,
                particles,
                start,
            } => {
                if dt > 1e-6 {
                    fps.push(1.0 / dt);
                }
                if start.elapsed().as_secs_f64() >= QUICK_COLLECT_SECS {
                    let n = fps.len().max(1);
                    let avg = fps.iter().sum::<f32>() / n as f32;
                    let min = fps.iter().cloned().fold(f32::MAX, f32::min);
                    let max = fps.iter().cloned().fold(0.0_f32, f32::max);
                    self.state = QuickBenchState::Done {
                        avg,
                        min,
                        max,
                        particles,
                    };
                    return true;
                }
                self.state = QuickBenchState::Collecting {
                    fps,
                    particles,
                    start,
                };
                false
            }
            other => {
                self.state = other;
                false
            }
        }
    }

    /// Progress `(elapsed_secs, total_secs, is_warmup)`.  `None` when idle/done.
    pub fn progress(&self) -> Option<(f32, f32, bool)> {
        match &self.state {
            QuickBenchState::Warmup { start } => Some((
                start.elapsed().as_secs_f32(),
                QUICK_WARMUP_SECS as f32,
                true,
            )),
            QuickBenchState::Collecting { start, .. } => Some((
                start.elapsed().as_secs_f32(),
                QUICK_COLLECT_SECS as f32,
                false,
            )),
            _ => None,
        }
    }

    /// Returns `(avg_fps, min_fps, max_fps, particles)` once the run is complete.
    pub fn result(&self) -> Option<(f32, f32, f32, u32)> {
        if let QuickBenchState::Done {
            avg,
            min,
            max,
            particles,
        } = &self.state
        {
            Some((*avg, *min, *max, *particles))
        } else {
            None
        }
    }
}

// ── Result ────────────────────────────────────────────────────────────────────

/// Per-combo result produced by [`BenchmarkRunner`] and written to CSV.
#[derive(Clone)]
pub struct BenchmarkResult {
    pub preset_name: String,
    pub particle_count: usize,
    pub species_count: usize,
    /// Fixed world dimensions used for this tier (from [`BENCHMARK_TIERS`]).
    pub world_width: f32,
    pub world_height: f32,
    pub avg_fps: f32,
    pub min_fps: f32,
    pub max_fps: f32,
    pub avg_frame_ms: f32,
    pub frames_collected: u32,
    pub wall_secs: f32,
    /// Whether vsync was enabled during this run.
    pub vsync: bool,
}

// ── State machine ─────────────────────────────────────────────────────────────

enum State {
    Idle,
    Warmup {
        combo: usize,
        start: Instant,
    },
    Collect {
        combo: usize,
        fps: Vec<f32>,
        start: Instant,
    },
    Done,
}

/// Runs the full benchmark suite: every [`builtin_presets`] × [`BENCHMARK_TIERS`] combination.
/// Each combo runs a [`WARMUP_SECS`]-second warm-up followed by [`COLLECT_SECS`] seconds of
/// FPS sample collection.
pub struct BenchmarkRunner {
    state: State,
    pub results: Vec<BenchmarkResult>,
    global_start: Option<Instant>,
    pub vp_width: u32,
    pub vp_height: u32,
    /// When `true` (the default), the suite runs with vsync disabled for accurate timing.
    pub vsync_off: bool,
}

/// Instruction returned by [`BenchmarkRunner::advance`] each frame.
#[must_use]
pub enum BenchmarkAction {
    /// No state change needed; continue rendering normally.
    Continue,
    /// Apply the preset for this combo index and respawn particles.
    LoadCombo(usize),
    /// All combos finished; results are available via [`BenchmarkRunner::results`].
    Done,
}

// ── BenchmarkRunner impl ──────────────────────────────────────────────────────

impl BenchmarkRunner {
    pub fn new() -> Self {
        Self {
            state: State::Idle,
            results: vec![],
            global_start: None,
            vp_width: 0,
            vp_height: 0,
            vsync_off: true,
        }
    }

    /// Total number of (preset × tier) combinations in the suite.
    pub fn num_combos() -> usize {
        BUILTIN_COUNT * BENCHMARK_TIERS.len()
    }

    /// Which built-in preset index a flat combo index maps to.
    pub fn combo_preset_idx(combo: usize) -> usize {
        combo / BENCHMARK_TIERS.len()
    }
    /// Which tier index a flat combo index maps to.
    pub fn combo_tier_idx(combo: usize) -> usize {
        combo % BENCHMARK_TIERS.len()
    }

    /// Returns the Preset for a given combo with particle_count, world size, and auto_density
    /// set to the tier's fixed values so results are comparable across runs.
    pub fn combo_preset(combo: usize) -> Preset {
        let presets = builtin_presets();
        let mut p = presets[Self::combo_preset_idx(combo)].clone();
        let tier = BENCHMARK_TIERS[Self::combo_tier_idx(combo)];
        p.particle_count = tier.particles;
        p.world_width = tier.world_width;
        p.world_height = tier.world_height;
        p.auto_density = false;
        p
    }

    /// Returns `true` while any combo is in warmup or collection.
    pub fn is_running(&self) -> bool {
        matches!(self.state, State::Warmup { .. } | State::Collect { .. })
    }

    /// Returns `true` after all combos have finished.
    pub fn is_done(&self) -> bool {
        matches!(self.state, State::Done)
    }

    /// Returns `(completed_combos, total_combos, elapsed_secs, target_secs, is_warmup)`
    /// while running; `None` when idle or done.
    pub fn progress(&self) -> Option<(usize, usize, f32, f32, bool)> {
        match &self.state {
            State::Warmup { combo, start } => Some((
                *combo,
                Self::num_combos(),
                start.elapsed().as_secs_f32(),
                WARMUP_SECS as f32,
                true,
            )),
            State::Collect { combo, start, .. } => Some((
                *combo,
                Self::num_combos(),
                start.elapsed().as_secs_f32(),
                COLLECT_SECS as f32,
                false,
            )),
            _ => None,
        }
    }

    /// Kick off a fresh benchmark run. Returns `LoadCombo(0)`.
    pub fn start(&mut self, vp_w: u32, vp_h: u32) -> BenchmarkAction {
        self.results.clear();
        self.global_start = Some(Instant::now());
        self.vp_width = vp_w;
        self.vp_height = vp_h;
        self.state = State::Warmup {
            combo: 0,
            start: Instant::now(),
        };
        BenchmarkAction::LoadCombo(0)
    }

    /// Call once per frame while running; drives the state machine.
    pub fn advance(&mut self, dt: f32) -> BenchmarkAction {
        let old = std::mem::replace(&mut self.state, State::Idle);
        match old {
            State::Warmup { combo, start } => {
                if start.elapsed().as_secs_f64() >= WARMUP_SECS {
                    self.state = State::Collect {
                        combo,
                        fps: vec![],
                        start: Instant::now(),
                    };
                } else {
                    self.state = State::Warmup { combo, start };
                }
                BenchmarkAction::Continue
            }
            State::Collect {
                combo,
                mut fps,
                start,
            } => {
                if dt > 1e-6 {
                    fps.push(1.0 / dt);
                }
                let combo_elapsed = start.elapsed().as_secs_f64();
                let global_elapsed = self
                    .global_start
                    .map(|t| t.elapsed().as_secs_f64())
                    .unwrap_or(0.0);
                let enough = combo_elapsed >= COLLECT_SECS || global_elapsed > GLOBAL_CAP_SECS;
                if enough {
                    self.results
                        .push(Self::summarize(combo, &fps, combo_elapsed, !self.vsync_off));
                    let next = combo + 1;
                    if next >= Self::num_combos() || global_elapsed > GLOBAL_CAP_SECS {
                        self.state = State::Done;
                        return BenchmarkAction::Done;
                    }
                    self.state = State::Warmup {
                        combo: next,
                        start: Instant::now(),
                    };
                    BenchmarkAction::LoadCombo(next)
                } else {
                    self.state = State::Collect { combo, fps, start };
                    BenchmarkAction::Continue
                }
            }
            other => {
                self.state = other;
                BenchmarkAction::Continue
            }
        }
    }

    fn summarize(combo: usize, fps: &[f32], wall_secs: f64, vsync: bool) -> BenchmarkResult {
        let presets = builtin_presets();
        let p = &presets[Self::combo_preset_idx(combo)];
        let tier = BENCHMARK_TIERS[Self::combo_tier_idx(combo)];
        let n = fps.len().max(1);
        let avg = fps.iter().sum::<f32>() / n as f32;
        let min = fps.iter().cloned().fold(f32::MAX, f32::min);
        let max = fps.iter().cloned().fold(0.0_f32, f32::max);
        BenchmarkResult {
            preset_name: p.name.clone(),
            particle_count: tier.particles,
            species_count: p.species_count,
            world_width: tier.world_width,
            world_height: tier.world_height,
            avg_fps: avg,
            min_fps: if fps.is_empty() { 0.0 } else { min },
            max_fps: if fps.is_empty() { 0.0 } else { max },
            avg_frame_ms: if avg > 0.0 { 1000.0 / avg } else { 0.0 },
            frames_collected: fps.len() as u32,
            wall_secs: wall_secs as f32,
            vsync,
        }
    }

    /// Write all collected results to a CSV file at `path`.
    pub fn write_csv(&self, path: &Path) -> std::io::Result<()> {
        use std::io::Write;
        let mut f = std::fs::File::create(path)?;
        writeln!(
            f,
            "preset,particles,species,world_w,world_h,vp_w,vp_h,avg_fps,min_fps,max_fps,avg_frame_ms,frames,wall_secs,vsync"
        )?;
        for r in &self.results {
            writeln!(
                f,
                "{},{},{},{},{},{},{},{:.1},{:.1},{:.1},{:.2},{},{:.1},{}",
                r.preset_name,
                r.particle_count,
                r.species_count,
                r.world_width,
                r.world_height,
                self.vp_width,
                self.vp_height,
                r.avg_fps,
                r.min_fps,
                r.max_fps,
                r.avg_frame_ms,
                r.frames_collected,
                r.wall_secs,
                if r.vsync { "on" } else { "off" },
            )?;
        }
        Ok(())
    }
}
