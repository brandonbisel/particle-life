# Benchmarks

Results recorded with the built-in Suite Benchmark (vsync off, 5s warmup + 15s collection per combo).

**Measurement note:** FPS figures are the arithmetic mean of per-frame `1/dt` samples collected inside the benchmark window. Before the fps-counter fix (commit `9f2be06`+), a `dt.min(0.05)` physics cap was incorrectly applied to the timing ring buffer, clamping any frame slower than 20 fps to exactly 20 fps. The Linux results below use the corrected measurement. The Mac Mini results predate the fix — any row with `Min FPS = 20` is likely affected, and Ecosystem/Symbiosis at high particle counts should be treated as lower bounds.

## Presets

| Preset | Species | Matrix pattern | Behaviour |
|--------|--------:|----------------|-----------|
| Clusters | 6 | Diagonal: `+0.7`, off-diagonal: `−0.2` | Like attracts like; compact same-colour blobs with mild intermingling |
| Chains | 6 | Circular predator-prey ring | Each species chases the next, flees the previous; trailing spirals and filaments |
| Ecosystem | 6 | Two-group asymmetric | Spiraling predator chain (species 0–2) hunts a tight fleeing cluster (species 3–5); two coexisting emergent structures |
| Symbiosis | 6 | Diagonal: `−0.1`, off-diagonal: `+0.6` | Every species attracts all others, weakly repels its own kind; large mixed-colour aggregates |

Symbiosis is the structural inverse of Clusters — exercises cross-species attraction and uniform spatial mixing rather than species segregation.

---

## Linux — AMD Radeon RX 9070 XT

### Hardware

| Component | Detail |
|-----------|--------|
| GPU | AMD Radeon RX 9070 XT (RDNA 4) |
| API | Vulkan via wgpu 24 |
| Resolution | 3440 × 1368 |
| Display | 165 Hz |
| OS | Linux (CachyOS) |

### Summary

Average FPS by preset and particle count (vsync off):

| Preset    |   10K |   50K | 100K | 500K |
|-----------|------:|------:|-----:|-----:|
| Clusters  | 4,030 | 1,414 |  599 |   44 |
| Chains    | 4,829 | 1,803 |  671 |   44 |
| Ecosystem | 2,283 |   615 |  357 |    9 |
| Symbiosis | 2,835 |   737 |  352 |   27 |

**Performance tiers:**

- **Clusters and Chains** keep particles distributed across the spatial grid, so cell load stays balanced. At ≤ 100K this is CPU/submission-bound — the GPU has far more headroom. 500K is the meaningful GPU-bound number (~23 ms/frame, ~44 fps).
- **Symbiosis** causes particles to aggregate into large mixed blobs, creating moderate grid hotspots. ~1.5–2× lower throughput than Clusters/Chains; GPU-bound from ~50K.
- **Ecosystem** produces the most extreme spatial non-uniformity: the tight fleeing cluster (species 3–5) concentrates hundreds of thousands of particles into a handful of grid cells. At 500K the 12×12 spatial grid has ~3,500 particles per cell on average, but the cluster region drives individual cells far higher, causing the neighbor-search to degrade toward O(n²) locally. This makes Ecosystem **~5× slower** than Clusters/Chains at 500K (9 fps vs. 44 fps), with high frame-time variance (5–16 fps) as the cluster forms, compresses, and scatters. GPU-bound from ~10K.
- The 500K tier is GPU-bound for all presets and is the most useful cross-preset comparison.

### Per-run detail

| Preset    | Particles | Avg FPS | Min FPS | Max FPS | Avg ms | Frames | Wall secs | VSync |
|-----------|----------:|--------:|--------:|--------:|-------:|-------:|----------:|-------|
| Clusters  |    10,000 |   4,030 |     294 |   5,073 |   0.25 | 59,921 |      15.0 | off   |
| Clusters  |    50,000 |   1,414 |     283 |   2,250 |   0.71 | 21,102 |      15.0 | off   |
| Clusters  |   100,000 |     599 |     126 |   2,852 |   1.67 |  8,918 |      15.0 | off   |
| Clusters  |   500,000 |      44 |      39 |      47 |  22.98 |    653 |      15.0 | off   |
| Chains    |    10,000 |   4,829 |     298 |   5,255 |   0.21 | 71,704 |      15.0 | off   |
| Chains    |    50,000 |   1,803 |      94 |   4,919 |   0.55 | 26,846 |      15.0 | off   |
| Chains    |   100,000 |     671 |     472 |     917 |   1.49 |  9,937 |      15.0 | off   |
| Chains    |   500,000 |      44 |      43 |      46 |  22.69 |    661 |      15.0 | off   |
| Ecosystem |    10,000 |   2,283 |     294 |   4,895 |   0.44 | 32,830 |      15.0 | off   |
| Ecosystem |    50,000 |     615 |     269 |   1,205 |   1.63 |  8,339 |      15.0 | off   |
| Ecosystem |   100,000 |     357 |     167 |     588 |   2.80 |  4,844 |      15.0 | off   |
| Ecosystem |   500,000 |       9 |       5 |      16 | 114.73 |    116 |      15.1 | off   |
| Symbiosis |    10,000 |   2,835 |     505 |   5,049 |   0.35 | 41,945 |      15.0 | off   |
| Symbiosis |    50,000 |     737 |     480 |   1,129 |   1.36 | 10,759 |      15.0 | off   |
| Symbiosis |   100,000 |     352 |     223 |     450 |   2.84 |  5,208 |      15.0 | off   |
| Symbiosis |   500,000 |      27 |      22 |      30 |  37.41 |    401 |      15.0 | off   |

---

## Linux — Capacity Benchmark (AMD Radeon RX 9070 XT)

Maximum particle count at 30 fps (1280×720, vsync off, `auto_density = false`):

| Preset    | Max Particles | Achieved FPS |
|-----------|--------------:|-------------:|
| Clusters  |       614,000 |         30.8 |
| Chains    |       607,000 |         30.6 |
| Ecosystem |       448,000 |         32.9 |
| Symbiosis |       433,000 |         30.7 |

Results match the performance tier ordering from the suite: Clusters and Chains have near-identical capacity (~610K); Symbiosis is ~30% lower due to grid hotspots from mixed-species blobs; Ecosystem is ~27% lower than Symbiosis due to its extreme tight-cluster hotspot.

**Method:** binary search with log-linear interpolation (regula falsi in log-log space) to bias test points toward the predicted fps crossover. Adaptive warmup: transitions to collect only once two consecutive 2-second fps windows agree within 12%, with a 5s minimum and 20s hard cap. Ecosystem uses the full cliff-detection path — warmup holds until fps drops below 80% of its peak (cluster settled). 5s collect per iteration, 10% convergence window (hi/lo ≤ 1.10).

---

## macOS — Mac Mini (M4)

> **Caveat:** These results were recorded before the fps-counter fix. The `dt.min(0.05)` cap was applied to fps samples, so any frame taking longer than 50 ms was reported as exactly 20 fps rather than its true rate. Rows with `Min FPS = 20` are affected; the true minimum (and in some cases the true average) will be lower. A corrected re-run is pending.

### Hardware

| Component | Detail |
|-----------|--------|
| SoC | Apple M4 (10-core GPU), 16 GB unified memory |
| API | Metal via wgpu 24 |
| Resolution | 1280 × 720 |
| OS | macOS Tahoe |

### Summary

Average FPS by preset and particle count (vsync off):

| Preset    | 10K | 50K | 100K | 500K† |
|-----------|----:|----:|-----:|------:|
| Clusters  | 123 | 190 |   71 |    58 |
| Chains    | 248 | 239 |   63 |   132 |
| Ecosystem | 275 | 224 |   54 |    28 |
| Symbiosis | 249 | 180 |   55 |    23 |

† **500K results are unreliable** — only 35–67 frames were collected during the 15s window (see per-run detail), giving sample sizes too small for meaningful averages.

**macOS-specific notes:**

- **≤ 50K:** The avg FPS figures (arithmetic mean of per-frame 1/dt) run ~1.5–1.7× higher than actual frame throughput (frames ÷ wall time). This is a measurement artefact: on macOS the winit event loop delivers `RedrawRequested` less frequently than on Linux under heavy GPU load, so the GPU dispatches compute at a higher rate than frames are presented. The instantaneous fps samples are biased toward fast frames.
- **100K:** All presets converge near 60 fps — consistent with macOS compositor throttling the presentation rate at this load level.
- **500K:** Frame counts of 35–67 over 15 seconds indicate the event loop stalled significantly. The avg fps values are not representative.

### Per-run detail

| Preset    | Particles | Avg FPS | Min FPS | Max FPS | Avg ms | Frames | Wall secs | VSync |
|-----------|----------:|--------:|--------:|--------:|-------:|-------:|----------:|-------|
| Clusters  |    10,000 |     123 |      40 |     815 |   8.14 |  1,147 |      15.0 | off   |
| Clusters  |    50,000 |     190 |      28 |     895 |   5.26 |  1,685 |      15.0 | off   |
| Clusters  |   100,000 |      71 |      20 |   2,727 |  14.05 |    900 |      15.0 | off   |
| Clusters  |   500,000 |      58 |      20 |     984 |  17.27 |     60 |      15.1 | off   |
| Chains    |    10,000 |     248 |      40 |     963 |   4.03 |  2,525 |      15.0 | off   |
| Chains    |    50,000 |     239 |      21 |   1,745 |   4.19 |  2,277 |      15.0 | off   |
| Chains    |   100,000 |      63 |      33 |     530 |  15.84 |    904 |      15.0 | off   |
| Chains    |   500,000 |     132 |      20 |     941 |   7.57 |     67 |      15.1 | off   |
| Ecosystem |    10,000 |     275 |      35 |     852 |   3.64 |  3,340 |      15.0 | off   |
| Ecosystem |    50,000 |     224 |      29 |     971 |   4.48 |  2,342 |      15.0 | off   |
| Ecosystem |   100,000 |      54 |      20 |     818 |  18.63 |    692 |      15.0 | off   |
| Ecosystem |   500,000 |      28 |      20 |     169 |  35.51 |     35 |      15.2 | off   |
| Symbiosis |    10,000 |     249 |      20 |     986 |   4.01 |  2,792 |      15.0 | off   |
| Symbiosis |    50,000 |     180 |      20 |     742 |   5.55 |  1,723 |      15.0 | off   |
| Symbiosis |   100,000 |      55 |      22 |     219 |  18.23 |    742 |      15.0 | off   |
| Symbiosis |   500,000 |      23 |      20 |     151 |  44.49 |     53 |      15.1 | off   |
