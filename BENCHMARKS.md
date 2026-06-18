# Benchmarks

Results recorded with the built-in Suite Benchmark (vsync off, 5s warmup + 15s collection per combo).

**avg_fps vs throughput:** `avg_fps` (mean of per-frame `1/dt`) inflates 1.1–2.1× at ≤100K particles due to near-zero-dt winit events between GPU submissions. `Frames ÷ Wall secs` is the reliable metric. At the 500K GPU-bound tier, avg_fps and throughput agree within 1–2%.

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
| Resolution | 3440 × 1440 |
| Display | 165 Hz |
| OS | Linux (CachyOS) |

### Suite Benchmark (v0.6.0)

| Preset    | Particles | Avg FPS | Min FPS | Max FPS | Avg ms | Frames | Wall secs | VSync |
|-----------|----------:|--------:|--------:|--------:|-------:|-------:|----------:|-------|
| Clusters  |    10,000 |   6,093 |     325 |   6,546 |   0.16 | 90,914 |      15.0 | off   |
| Clusters  |    50,000 |   1,807 |   1,173 |   5,969 |   0.55 | 26,933 |      15.0 | off   |
| Clusters  |   100,000 |     765 |     532 |   1,281 |   1.31 | 11,289 |      15.0 | off   |
| Clusters  |   500,000 |      60 |      54 |      65 |  16.65 |    901 |      15.0 | off   |
| Chains    |    10,000 |   6,240 |   1,555 |   6,695 |   0.16 | 93,258 |      15.0 | off   |
| Chains    |    50,000 |   2,386 |   1,308 |   4,731 |   0.42 | 35,649 |      15.0 | off   |
| Chains    |   100,000 |     960 |     316 |   4,945 |   1.04 | 14,346 |      15.0 | off   |
| Chains    |   500,000 |      60 |      58 |      62 |  16.65 |    901 |      15.0 | off   |
| Ecosystem |    10,000 |   3,355 |     305 |   6,534 |   0.30 | 47,718 |      15.0 | off   |
| Ecosystem |    50,000 |     831 |     340 |   1,822 |   1.20 | 11,125 |      15.0 | off   |
| Ecosystem |   100,000 |     441 |     209 |     751 |   2.27 |  5,954 |      15.0 | off   |
| Ecosystem |   500,000 |      31 |       4 |      53 |  32.62 |    243 |      15.2 | off   |
| Symbiosis |    10,000 |   3,636 |     293 |   6,450 |   0.28 | 53,606 |      15.0 | off   |
| Symbiosis |    50,000 |   1,012 |     263 |   1,485 |   0.99 | 14,957 |      15.0 | off   |
| Symbiosis |   100,000 |     350 |     226 |     456 |   2.86 |  5,158 |      15.0 | off   |
| Symbiosis |   500,000 |      34 |      28 |      38 |  29.54 |    507 |      15.0 | off   |

**500K performance (GPU-bound, most reliable):** Chains and Clusters hit ~60 tp (display-rate ceiling at this run); Symbiosis at 33.8 tp; Ecosystem at 16.0 tp with high frame-time variance (dense cluster hotspot — avg_fps inflates ~2× here due to fast frames between scatter events).

### Capacity Benchmark

Maximum particle count at 30 fps (1280×720 world, vsync off, `auto_density = false`):

| Preset    | Max Particles | Achieved FPS |
|-----------|--------------:|-------------:|
| Clusters  |       721,000 |         30.3 |
| Chains    |       675,000 |         34.2 |
| Ecosystem |       463,000 |         52.1 |
| Symbiosis |       498,000 |         31.0 |

Clusters and Chains lead due to spatially uniform distributions. Symbiosis and Ecosystem trail from mixed-blob and cluster hotspots. Ecosystem's 52.1 fps at 463K indicates cliff-detection triggered early before the dense cluster formed; the true 30-fps crossover is likely somewhat higher.

**Method:** binary search with log-linear interpolation. Adaptive warmup: two consecutive 2-second windows within 12%, 5s minimum / 20s hard cap. Ecosystem uses cliff-detection (warmup holds until fps drops below 80% of peak). 5s collect per iteration, 10% convergence window (hi/lo ≤ 1.10).

---

## macOS — Mac Mini (M4)

> **Caveat:** These results were recorded before the fps-counter fix. The `dt.min(0.05)` cap was incorrectly applied to fps samples, clamping any frame slower than 20 fps to exactly 20 fps. Rows with `Min FPS = 20` are affected; the true minimum (and average) will be lower. A corrected re-run is pending.

### Hardware

| Component | Detail |
|-----------|--------|
| SoC | Apple M4 (10-core GPU), 16 GB unified memory |
| API | Metal via wgpu 24 |
| Resolution | 1280 × 720 |
| OS | macOS Tahoe |

### Suite Benchmark

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

**Notes:** At ≤50K, avg FPS runs ~1.5–1.7× above throughput — macOS winit delivers `RedrawRequested` less frequently under heavy GPU load, biasing fps samples toward fast frames. At 100K all presets converge near 60 fps (compositor throttle). At 500K only 35–67 frames were collected; values are not meaningful.
