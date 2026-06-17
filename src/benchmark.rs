//! Benchmarking utilities: a lightweight ad-hoc snapshot ([`QuickBench`]), a
//! full automated suite ([`BenchmarkRunner`]) that cycles through every
//! combination of built-in preset × particle-count tier and exports CSV results,
//! and a binary-search capacity finder ([`CapacityBench`]) that locates the
//! maximum particle count sustainable at a target FPS.

use std::path::Path;
use std::time::Instant;

use crate::config::{Preset, builtin_presets};
use crate::simulation::MAX_PARTICLES;

/// A single tier in the full benchmark suite: fixed particle count and fixed world size.
///
/// World size is pinned per-tier so results remain comparable across runs regardless of the
/// user's current auto-density setting.  All tiers use the canonical 1280×720 world so the
/// suite measures raw GPU throughput at increasing density (O(n²) scaling intentional).
#[derive(Clone, Copy)]
pub struct BenchmarkTier {
    /// Particle count for this tier.
    pub particles: usize,
    /// Fixed world width in simulation units (always 1280.0 in the current suite).
    pub world_width: f32,
    /// Fixed world height in simulation units (always 720.0 in the current suite).
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

    /// Abort a running suite and return to idle, discarding partial results.
    pub fn cancel(&mut self) {
        self.state = State::Idle;
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

// ── Capacity benchmark ────────────────────────────────────────────────────────

/// Collect duration for each binary-search iteration.
const CAPACITY_COLLECT_SECS: f64 = 5.0;
/// Minimum warmup before checking for fps stability.
const CAPACITY_MIN_WARMUP_SECS: f64 = 5.0;
/// Hard warmup cap; transition to collect even if fps hasn't stabilised.
const CAPACITY_MAX_WARMUP_SECS: f64 = 20.0;
/// Width of each half-window used by the stability check (seconds).
const CAPACITY_STABLE_WINDOW_SECS: f64 = 2.0;
/// Max fractional fps change between the two half-windows to be considered stable.
const CAPACITY_STABLE_THRESHOLD: f32 = 0.12;
/// Ecosystem (index 2) forms a tight cluster whose compression time varies with
/// particle count, requiring cliff-detection rather than a fixed warmup duration.
const ECOSYSTEM_PRESET_IDX: usize = 2;
/// Maximum binary-search iterations per preset; geometric bisection needs ~6
/// to narrow [10K, 2M] to within 15%, so 10 is a comfortable safety margin.
const CAPACITY_MAX_ITERS: u32 = 10;
/// Minimum particle count ever tested.  At 10K every preset runs at thousands
/// of FPS on any discrete GPU, so this is a safe lower bound for any sane
/// target.  Matches the suite's lowest tier for a clean reference point.
const CAPACITY_MIN_N: usize = 10_000;
/// Stop when hi/lo falls below this ratio (~10% window).
const CAPACITY_CONVERGENCE_RATIO: f64 = 1.10;

/// One result row produced by [`CapacityBench`].
#[derive(Clone)]
pub struct CapacityResult {
    pub preset_name: String,
    /// Largest particle count measured at or above `target_fps`.
    /// Zero if not even `CAPACITY_MIN_N` particles achieved the target.
    pub max_particles: usize,
    /// Average FPS observed at `max_particles`.  Zero when `max_particles` is zero.
    pub achieved_fps: f32,
    pub target_fps: f32,
    /// True when `max_particles == MAX_PARTICLES`, meaning the GPU can sustain
    /// the target even at the hard buffer limit — the real capacity is higher.
    pub capped: bool,
}

enum CapState {
    Idle,
    Warmup {
        preset_idx: usize,
        lo: usize,
        hi: usize,
        mid: usize,
        lo_fps: f32,
        /// FPS measured at `hi`; 0.0 until the first failing test sets it.
        hi_fps: f32,
        iter: u32,
        start: Instant,
        /// Per-frame (elapsed_secs, fps) samples collected during warmup for
        /// stability detection.  Dropped when transitioning to Collect.
        warmup_fps: Vec<(f64, f32)>,
    },
    Collect {
        preset_idx: usize,
        lo: usize,
        hi: usize,
        mid: usize,
        lo_fps: f32,
        hi_fps: f32,
        iter: u32,
        fps: Vec<f32>,
        start: Instant,
    },
    Done,
}

/// Action returned by [`CapacityBench::advance`].
#[must_use]
pub enum CapacityAction {
    Continue,
    /// Apply built-in preset `preset_idx` at `particles` count; world is pinned
    /// to 1280×720 with `auto_density = false`.
    LoadPreset {
        preset_idx: usize,
        particles: usize,
    },
    Done,
}

/// Progress snapshot returned by [`CapacityBench::progress`].
pub struct CapacityProgress {
    /// Index into [`builtin_presets`] currently under test.
    pub preset_idx: usize,
    /// Total number of presets in the search.
    pub total_presets: usize,
    /// Current binary-search iteration (1-based for display).
    pub iter: u32,
    /// Maximum iterations per preset.
    pub max_iters: u32,
    /// Particle count currently being tested.
    pub particles: usize,
    /// Seconds elapsed in the current warmup/collection phase.
    pub elapsed: f32,
    /// Total seconds needed for the current phase.
    pub target_secs: f32,
    /// `true` during warmup, `false` during FPS collection.
    pub is_warmup: bool,
}

/// Binary-search benchmark that finds the maximum particle count sustainable
/// at a configurable target FPS for each built-in preset.
///
/// World is pinned to 1280×720 with `auto_density = false` (identical to the
/// suite benchmark) so results are directly comparable to the suite's 500K tier.
pub struct CapacityBench {
    state: CapState,
    pub results: Vec<CapacityResult>,
    /// Target FPS; editable before a run starts.
    pub target_fps: f32,
    pub vp_width: u32,
    pub vp_height: u32,
}

impl CapacityBench {
    pub fn new() -> Self {
        Self {
            state: CapState::Idle,
            results: vec![],
            target_fps: 30.0,
            vp_width: 0,
            vp_height: 0,
        }
    }

    /// Returns `true` while any preset is in warmup or collection.
    pub fn is_running(&self) -> bool {
        matches!(
            self.state,
            CapState::Warmup { .. } | CapState::Collect { .. }
        )
    }

    /// Abort a running search and return to idle, discarding partial results.
    pub fn cancel(&mut self) {
        self.state = CapState::Idle;
    }

    /// Returns `true` after all presets have been searched.
    pub fn is_done(&self) -> bool {
        matches!(self.state, CapState::Done)
    }

    /// Returns progress info while running; `None` when idle or done.
    pub fn progress(&self) -> Option<CapacityProgress> {
        match &self.state {
            CapState::Warmup {
                preset_idx,
                mid,
                iter,
                start,
                ..
            } => Some(CapacityProgress {
                preset_idx: *preset_idx,
                total_presets: BUILTIN_COUNT,
                iter: *iter,
                max_iters: CAPACITY_MAX_ITERS,
                particles: *mid,
                elapsed: start.elapsed().as_secs_f32(),
                target_secs: if *preset_idx == ECOSYSTEM_PRESET_IDX {
                    CAPACITY_MAX_WARMUP_SECS as f32
                } else {
                    CAPACITY_MIN_WARMUP_SECS as f32
                },
                is_warmup: true,
            }),
            CapState::Collect {
                preset_idx,
                mid,
                iter,
                start,
                ..
            } => Some(CapacityProgress {
                preset_idx: *preset_idx,
                total_presets: BUILTIN_COUNT,
                iter: *iter,
                max_iters: CAPACITY_MAX_ITERS,
                particles: *mid,
                elapsed: start.elapsed().as_secs_f32(),
                target_secs: CAPACITY_COLLECT_SECS as f32,
                is_warmup: false,
            }),
            _ => None,
        }
    }

    /// Build a `Preset` with the given particle count and fixed 1280×720 world.
    pub fn preset_for(preset_idx: usize, particles: usize) -> Preset {
        let mut p = builtin_presets()[preset_idx].clone();
        p.particle_count = particles;
        p.world_width = 1280.0;
        p.world_height = 720.0;
        p.auto_density = false;
        p
    }

    /// Kick off a fresh capacity search.  Returns the first `LoadPreset` action.
    pub fn start(&mut self, vp_w: u32, vp_h: u32) -> CapacityAction {
        self.results.clear();
        self.vp_width = vp_w;
        self.vp_height = vp_h;
        let mid = Self::initial_mid();
        self.state = CapState::Warmup {
            preset_idx: 0,
            lo: CAPACITY_MIN_N,
            hi: MAX_PARTICLES,
            mid,
            lo_fps: 0.0,
            hi_fps: 0.0,
            iter: 0,
            start: Instant::now(),
            warmup_fps: vec![],
        };
        CapacityAction::LoadPreset {
            preset_idx: 0,
            particles: mid,
        }
    }

    /// Call once per frame while running.
    pub fn advance(&mut self, dt: f32) -> CapacityAction {
        let old = std::mem::replace(&mut self.state, CapState::Idle);
        match old {
            CapState::Warmup {
                preset_idx,
                lo,
                hi,
                mid,
                lo_fps,
                hi_fps,
                iter,
                start,
                mut warmup_fps,
            } => {
                let elapsed = start.elapsed().as_secs_f64();
                if dt > 1e-6 {
                    warmup_fps.push((elapsed, 1.0 / dt));
                }
                let ready = elapsed >= CAPACITY_MAX_WARMUP_SECS
                    || (elapsed >= CAPACITY_MIN_WARMUP_SECS
                        && Self::fps_stable_for_collect(
                            &warmup_fps,
                            elapsed,
                            self.target_fps,
                            preset_idx,
                        ));
                if ready {
                    self.state = CapState::Collect {
                        preset_idx,
                        lo,
                        hi,
                        mid,
                        lo_fps,
                        hi_fps,
                        iter,
                        fps: vec![],
                        start: Instant::now(),
                    };
                } else {
                    self.state = CapState::Warmup {
                        preset_idx,
                        lo,
                        hi,
                        mid,
                        lo_fps,
                        hi_fps,
                        iter,
                        start,
                        warmup_fps,
                    };
                }
                CapacityAction::Continue
            }
            CapState::Collect {
                preset_idx,
                lo,
                hi,
                mid,
                lo_fps,
                hi_fps,
                iter,
                mut fps,
                start,
            } => {
                if dt > 1e-6 {
                    fps.push(1.0 / dt);
                }
                if start.elapsed().as_secs_f64() < CAPACITY_COLLECT_SECS {
                    self.state = CapState::Collect {
                        preset_idx,
                        lo,
                        hi,
                        mid,
                        lo_fps,
                        hi_fps,
                        iter,
                        fps,
                        start,
                    };
                    return CapacityAction::Continue;
                }

                let n = fps.len().max(1);
                let avg_fps = fps.iter().sum::<f32>() / n as f32;

                // Update binary search bounds; track fps at both endpoints for interpolation.
                let (new_lo, new_hi, new_lo_fps, new_hi_fps) = if avg_fps >= self.target_fps {
                    (mid, hi, avg_fps, hi_fps) // mid passes; raise lo, keep hi_fps
                } else {
                    (lo, mid, lo_fps, avg_fps) // mid fails; lower hi, record hi_fps
                };

                // Try another iteration or converge.
                let not_converged = iter + 1 < CAPACITY_MAX_ITERS
                    && (new_hi as f64) / (new_lo as f64).max(1.0) >= CAPACITY_CONVERGENCE_RATIO;
                if not_converged
                    && let Some(next_mid) = Self::next_mid_interp(
                        new_lo,
                        new_hi,
                        new_lo_fps,
                        new_hi_fps,
                        self.target_fps,
                    )
                {
                    self.state = CapState::Warmup {
                        preset_idx,
                        lo: new_lo,
                        hi: new_hi,
                        mid: next_mid,
                        lo_fps: new_lo_fps,
                        hi_fps: new_hi_fps,
                        iter: iter + 1,
                        start: Instant::now(),
                        warmup_fps: vec![],
                    };
                    return CapacityAction::LoadPreset {
                        preset_idx,
                        particles: next_mid,
                    };
                }

                // Record result for this preset.
                self.results.push(CapacityResult {
                    preset_name: builtin_presets()[preset_idx].name.clone(),
                    max_particles: new_lo,
                    achieved_fps: new_lo_fps,
                    target_fps: self.target_fps,
                    capped: new_lo >= MAX_PARTICLES,
                });

                let next = preset_idx + 1;
                if next >= BUILTIN_COUNT {
                    self.state = CapState::Done;
                    return CapacityAction::Done;
                }

                let mid = Self::initial_mid();
                self.state = CapState::Warmup {
                    preset_idx: next,
                    lo: CAPACITY_MIN_N,
                    hi: MAX_PARTICLES,
                    mid,
                    lo_fps: 0.0,
                    hi_fps: 0.0,
                    iter: 0,
                    start: Instant::now(),
                    warmup_fps: vec![],
                };
                CapacityAction::LoadPreset {
                    preset_idx: next,
                    particles: mid,
                }
            }
            other => {
                self.state = other;
                CapacityAction::Continue
            }
        }
    }

    /// Write results to CSV.
    pub fn write_csv(&self, path: &Path) -> std::io::Result<()> {
        use std::io::Write;
        let mut f = std::fs::File::create(path)?;
        writeln!(
            f,
            "preset,target_fps,max_particles,achieved_fps,capped,vp_w,vp_h"
        )?;
        for r in &self.results {
            writeln!(
                f,
                "{},{:.1},{},{:.1},{},{},{}",
                r.preset_name,
                r.target_fps,
                r.max_particles,
                r.achieved_fps,
                r.capped,
                self.vp_width,
                self.vp_height,
            )?;
        }
        Ok(())
    }

    /// Returns `true` when fps has stabilised enough to start collecting.
    ///
    /// For non-Ecosystem presets: any stable 2-second window is sufficient.
    /// For Ecosystem: also require that fps has fallen from its warmup peak
    /// (cliff detected) or is clearly above target (no cliff expected at this N).
    fn fps_stable_for_collect(
        samples: &[(f64, f32)],
        elapsed: f64,
        target_fps: f32,
        preset_idx: usize,
    ) -> bool {
        let w = CAPACITY_STABLE_WINDOW_SECS;
        let recent: Vec<f32> = samples
            .iter()
            .filter(|(t, _)| *t >= elapsed - w)
            .map(|(_, f)| *f)
            .collect();
        let prior: Vec<f32> = samples
            .iter()
            .filter(|(t, _)| *t >= elapsed - 2.0 * w && *t < elapsed - w)
            .map(|(_, f)| *f)
            .collect();
        if recent.len() < 3 || prior.len() < 3 {
            return false;
        }
        let r_avg = recent.iter().sum::<f32>() / recent.len() as f32;
        let p_avg = prior.iter().sum::<f32>() / prior.len() as f32;
        let change = (r_avg - p_avg).abs() / p_avg.max(1.0);
        if change >= CAPACITY_STABLE_THRESHOLD {
            return false; // fps still moving
        }
        if preset_idx != ECOSYSTEM_PRESET_IDX {
            return true; // non-Ecosystem: stable fps is sufficient
        }
        // Ecosystem: stable fps must either be well above target (no cliff at
        // this N) or have dropped from the warmup peak (cliff has settled).
        let peak = samples.iter().map(|(_, f)| *f).fold(0.0_f32, f32::max);
        r_avg > target_fps * 3.0 || r_avg < peak * 0.8
    }

    /// Geometric mean of `lo` and `hi`, rounded to the nearest 1000.
    /// Returns `None` if the result cannot be strictly between `lo` and `hi`.
    fn next_mid(lo: usize, hi: usize) -> Option<usize> {
        let mid_f = ((lo as f64).ln() + (hi as f64).ln()) / 2.0;
        let mid = ((mid_f.exp() as usize + 500) / 1000) * 1000;
        let mid = mid.clamp(lo + 1, hi.saturating_sub(1)).min(MAX_PARTICLES);
        if mid <= lo || mid >= hi {
            None
        } else {
            Some(mid)
        }
    }

    /// Log-linear interpolation (regula falsi in log-log space) to predict the
    /// particle count where fps crosses `target_fps`.  Falls back to geometric
    /// bisection when `hi_fps` is not yet known (0.0) or preconditions aren't met.
    fn next_mid_interp(
        lo: usize,
        hi: usize,
        lo_fps: f32,
        hi_fps: f32,
        target_fps: f32,
    ) -> Option<usize> {
        if hi_fps > 0.0 && lo_fps > target_fps && hi_fps < target_fps {
            let ln_lo = (lo as f64).ln();
            let ln_hi = (hi as f64).ln();
            let ln_lo_fps = (lo_fps as f64).ln();
            let ln_hi_fps = (hi_fps as f64).ln();
            let ln_target = (target_fps as f64).ln();
            // t ∈ (0,1): fraction of log-interval where fps == target
            let t = (ln_lo_fps - ln_target) / (ln_lo_fps - ln_hi_fps);
            let ln_n = ln_lo + t * (ln_hi - ln_lo);
            let mid_f = ln_n.exp();
            let mid = ((mid_f as usize + 500) / 1000) * 1000;
            let mid = mid.clamp(lo + 1, hi.saturating_sub(1)).min(MAX_PARTICLES);
            if mid > lo && mid < hi {
                return Some(mid);
            }
        }
        Self::next_mid(lo, hi)
    }

    fn initial_mid() -> usize {
        Self::next_mid(CAPACITY_MIN_N, MAX_PARTICLES)
            .unwrap_or((CAPACITY_MIN_N + MAX_PARTICLES) / 2)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn num_combos_matches_product() {
        assert_eq!(
            BenchmarkRunner::num_combos(),
            BUILTIN_COUNT * BENCHMARK_TIERS.len()
        );
        assert_eq!(BenchmarkRunner::num_combos(), 16);
    }

    #[test]
    fn combo_index_math_is_consistent() {
        let total = BenchmarkRunner::num_combos();
        for combo in 0..total {
            let pi = BenchmarkRunner::combo_preset_idx(combo);
            let ti = BenchmarkRunner::combo_tier_idx(combo);
            assert!(
                pi < BUILTIN_COUNT,
                "preset_idx {pi} out of range for combo {combo}"
            );
            assert!(
                ti < BENCHMARK_TIERS.len(),
                "tier_idx {ti} out of range for combo {combo}"
            );
            // Verify the flat index round-trips.
            assert_eq!(pi * BENCHMARK_TIERS.len() + ti, combo);
        }
    }

    #[test]
    fn combo_preset_has_fixed_world_and_no_auto_density() {
        for combo in 0..BenchmarkRunner::num_combos() {
            let p = BenchmarkRunner::combo_preset(combo);
            let tier = BENCHMARK_TIERS[BenchmarkRunner::combo_tier_idx(combo)];
            assert!(
                !p.auto_density,
                "combo {combo}: auto_density must be false for reproducibility"
            );
            assert_eq!(
                p.world_width, tier.world_width,
                "combo {combo}: world_width mismatch"
            );
            assert_eq!(
                p.world_height, tier.world_height,
                "combo {combo}: world_height mismatch"
            );
            assert_eq!(
                p.particle_count, tier.particles,
                "combo {combo}: particle_count mismatch"
            );
        }
    }

    #[test]
    fn capacity_preset_for_has_fixed_world_and_no_auto_density() {
        for idx in 0..BUILTIN_COUNT {
            let p = CapacityBench::preset_for(idx, 50_000);
            assert!(!p.auto_density);
            assert_eq!(p.world_width, 1280.0);
            assert_eq!(p.world_height, 720.0);
            assert_eq!(p.particle_count, 50_000);
        }
    }
}
