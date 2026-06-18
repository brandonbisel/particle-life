# Benchmarks

Results recorded with the built-in Suite Benchmark (vsync off, 5s warmup + 15s collection per combo).

**Measurement note:** FPS figures are the arithmetic mean of per-frame `1/dt` samples collected inside the benchmark window. Before the fps-counter fix (commit `9f2be06`+), a `dt.min(0.05)` physics cap was incorrectly applied to the timing ring buffer, clamping any frame slower than 20 fps to exactly 20 fps. The Linux results below use the corrected measurement. The Mac Mini results predate the fix — any row with `Min FPS = 20` is likely affected, and Ecosystem/Symbiosis at high particle counts should be treated as lower bounds.

**avg_fps inflation:** At low-to-medium particle counts (≤100K), `avg_fps` (mean of 1/dt) can run 1.1–2.1× higher than actual frame throughput (frames ÷ wall time) when the winit event loop delivers back-to-back near-zero-dt events between GPU submissions. This is visible in the current Linux baseline as max_fps values clustering around 5,000–5,500 at all sub-500K tiers. The **500K GPU-bound tier** is unaffected (avg_fps and frames/wall agree within 1–2%) and is the most reliable cross-run comparison. The `Frames` column divided by `Wall secs` always gives the true rendered-frame throughput.

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

### Summary (current baseline — v0.6.0)

Average FPS by preset and particle count (vsync off):

| Preset    |   10K |   50K |   100K | 500K |
|-----------|------:|------:|-------:|-----:|
| Clusters  | 6,093 | 1,807 |    765 |   60 |
| Chains    | 6,240 | 2,386 |    960 |   60 |
| Ecosystem | 3,355 |   831 |    441 |   31 |
| Symbiosis | 3,636 | 1,012 |    350 |   34 |

**vs previous baseline (post-stride-fix → v0.6.0), reliable 500K throughput (frames ÷ wall_secs):**

| Preset    | 500K tp (v0.6.0) | 500K tp (prev) |      Δ |
|-----------|------------------:|---------------:|-------:|
| Clusters  |             60.1 |           54.3 |   +11% |
| Chains    |             60.1 |           59.5 |    +1% |
| Ecosystem |             16.0 |            9.7 |  +65%¹ |
| Symbiosis |             33.8 |           26.7 |   +27% |

¹ Previous Ecosystem 500K captured a hot-cluster phase (9.7 tp, 12 avg fps); prior corrected run was ~25 fps. New run at 16.0 tp / 31 avg fps remains high-variance — Ecosystem avg_fps inflates ~2× at 500K due to fast frames between cluster scatter events.

avg_fps at ≤100K inflates 1.1–2.1× due to near-zero-dt winit events and is not reliable for cross-run comparison. Use 500K throughput for meaningful benchmarking.

**Performance tiers (v0.6.0):**

- **Chains** and **Clusters** both hit ~60 fps at 500K — the display-rate ceiling at 60 Hz; GPU likely not yet bound at this tier. Both are GPU-bound by ~1–2M particles.
- **Symbiosis** at 500K: 33.8 tp (~34 fps); mixed-blob aggregation creates moderate grid hotspots. GPU-bound from ~50K.
- **Ecosystem** at 500K: 16.0 tp / 31 avg fps; high frame-time variance persists as the cluster forms and scatters. GPU-bound from ~10K. The force pass degrades toward O(n²) locally inside the tight cluster.
- The 500K tier is the most useful cross-preset comparison; ≤100K figures are dominated by CPU/event-loop overhead.

### Per-run detail

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

<details>
<summary>Previous baseline (post-MAX_SPECIES-16 stride fix)</summary>

| Preset    | Particles | Avg FPS | Min FPS | Max FPS | Avg ms | Frames | Wall secs | VSync |
|-----------|----------:|--------:|--------:|--------:|-------:|-------:|----------:|-------|
| Clusters  |    10,000 |   4,289 |     150 |   5,356 |   0.23 | 61,019 |      15.0 | off   |
| Clusters  |    50,000 |   1,891 |     614 |   5,317 |   0.53 | 24,148 |      15.0 | off   |
| Clusters  |   100,000 |   1,233 |     287 |   5,152 |   0.81 | 10,247 |      15.0 | off   |
| Clusters  |   500,000 |      54 |      51 |      57 |  18.43 |    814 |      15.0 | off   |
| Chains    |    10,000 |   4,560 |   1,492 |   5,530 |   0.22 | 66,169 |      15.0 | off   |
| Chains    |    50,000 |   2,385 |     855 |   5,431 |   0.42 | 32,328 |      15.0 | off   |
| Chains    |   100,000 |   1,343 |     334 |   5,279 |   0.74 | 13,758 |      15.0 | off   |
| Chains    |   500,000 |      61 |      54 |      85 |  16.32 |    893 |      15.0 | off   |
| Ecosystem |    10,000 |   3,015 |     252 |   5,402 |   0.33 | 39,342 |      15.0 | off   |
| Ecosystem |    50,000 |   1,243 |     165 |   5,341 |   0.80 |  9,798 |      15.0 | off   |
| Ecosystem |   100,000 |     793 |     159 |   5,142 |   1.26 |  5,670 |      15.0 | off   |
| Ecosystem |   500,000 |      12 |       3 |      24 |  82.23 |    147 |      15.2 | off   |
| Symbiosis |    10,000 |   3,256 |     258 |   5,378 |   0.31 | 45,526 |      15.0 | off   |
| Symbiosis |    50,000 |   1,249 |     258 |   5,238 |   0.80 |  9,741 |      15.0 | off   |
| Symbiosis |   100,000 |     824 |     143 |   5,119 |   1.21 |  6,026 |      15.0 | off   |
| Symbiosis |   500,000 |      27 |      21 |      33 |  37.24 |    400 |      15.0 | off   |

</details>

<details>
<summary>Previous baseline (post-rsqrt force reformulation)</summary>

| Preset    | Particles | Avg FPS | Min FPS | Max FPS | Avg ms | Frames | Wall secs | VSync |
|-----------|----------:|--------:|--------:|--------:|-------:|-------:|----------:|-------|
| Clusters  |    10,000 |   4,758 |     323 |   5,019 |   0.21 | 71,059 |      15.0 | off   |
| Clusters  |    50,000 |   1,731 |     787 |   3,317 |   0.58 | 25,732 |      15.0 | off   |
| Clusters  |   100,000 |     726 |     469 |   1,156 |   1.38 | 10,723 |      15.0 | off   |
| Clusters  |   500,000 |      58 |      53 |      62 |  17.23 |    870 |      15.0 | off   |
| Chains    |    10,000 |   4,828 |     319 |   5,128 |   0.21 | 72,125 |      15.0 | off   |
| Chains    |    50,000 |   2,274 |     815 |   4,852 |   0.44 | 33,804 |      15.0 | off   |
| Chains    |   100,000 |     911 |     539 |   1,309 |   1.10 | 13,563 |      15.0 | off   |
| Chains    |   500,000 |      59 |      57 |      61 |  16.99 |    883 |      15.0 | off   |
| Ecosystem |    10,000 |   3,147 |     290 |   4,907 |   0.32 | 44,665 |      15.0 | off   |
| Ecosystem |    50,000 |     784 |     303 |   1,793 |   1.28 | 10,310 |      15.0 | off   |
| Ecosystem |   100,000 |     357 |     128 |     694 |   2.80 |  4,726 |      15.0 | off   |
| Ecosystem |   500,000 |      25 |       6 |      50 |  40.63 |    220 |      15.0 | off   |
| Symbiosis |    10,000 |   3,824 |   1,418 |   4,998 |   0.26 | 56,096 |      15.0 | off   |
| Symbiosis |    50,000 |     877 |     478 |   1,546 |   1.14 | 12,744 |      15.0 | off   |
| Symbiosis |   100,000 |     361 |     181 |     473 |   2.77 |  5,260 |      15.0 | off   |
| Symbiosis |   500,000 |      29 |      25 |      33 |  34.22 |    437 |      15.0 | off   |

</details>

<details>
<summary>Previous baseline (post-P7 LDS tile)</summary>

| Preset    | Particles | Avg FPS | Min FPS | Max FPS | Avg ms | Frames | Wall secs | VSync |
|-----------|----------:|--------:|--------:|--------:|-------:|-------:|----------:|-------|
| Clusters  |    10,000 |   4,827 |     342 |   5,156 |   0.21 | 72,117 |      15.0 | off   |
| Clusters  |    50,000 |   1,694 |     985 |   2,699 |   0.59 | 25,188 |      15.0 | off   |
| Clusters  |   100,000 |     664 |     470 |   1,002 |   1.51 |  9,834 |      15.0 | off   |
| Clusters  |   500,000 |      53 |      48 |      58 |  18.83 |    796 |      15.0 | off   |
| Chains    |    10,000 |   4,900 |     343 |   5,183 |   0.20 | 73,263 |      15.0 | off   |
| Chains    |    50,000 |   2,180 |   1,232 |   3,965 |   0.46 | 32,434 |      15.0 | off   |
| Chains    |   100,000 |     799 |     531 |   1,452 |   1.25 | 11,809 |      15.0 | off   |
| Chains    |   500,000 |      52 |      49 |      54 |  19.23 |    780 |      15.0 | off   |
| Ecosystem |    10,000 |   3,195 |     272 |   4,971 |   0.31 | 44,450 |      15.0 | off   |
| Ecosystem |    50,000 |     983 |     268 |   1,769 |   1.02 | 13,467 |      15.0 | off   |
| Ecosystem |   100,000 |     413 |     181 |     690 |   2.42 |  5,531 |      15.0 | off   |
| Ecosystem |   500,000 |      12 |       5 |      24 |  83.91 |    158 |      15.2 | off   |
| Symbiosis |    10,000 |   3,626 |     317 |   5,032 |   0.28 | 53,358 |      15.0 | off   |
| Symbiosis |    50,000 |     921 |     244 |   1,524 |   1.09 | 13,499 |      15.0 | off   |
| Symbiosis |   100,000 |     361 |     169 |     479 |   2.77 |  5,337 |      15.0 | off   |
| Symbiosis |   500,000 |      28 |      24 |      31 |  35.25 |    426 |      15.0 | off   |

</details>

<details>
<summary>Post-P1-P6 baseline (pre-P7, pre-rsqrt)</summary>

| Preset    | Particles | Avg FPS | Min FPS | Max FPS | Avg ms | Frames | Wall secs | VSync |
|-----------|----------:|--------:|--------:|--------:|-------:|-------:|----------:|-------|
| Clusters  |    10,000 |   4,873 |     313 |   5,194 |   0.21 | 72,639 |      15.0 | off   |
| Clusters  |    50,000 |   1,656 |     950 |   4,552 |   0.60 | 24,625 |      15.0 | off   |
| Clusters  |   100,000 |     680 |     451 |   1,012 |   1.47 | 10,076 |      15.0 | off   |
| Clusters  |   500,000 |      50 |      46 |      55 |  19.89 |    754 |      15.0 | off   |
| Chains    |    10,000 |   4,925 |     340 |   5,264 |   0.20 | 73,292 |      15.0 | off   |
| Chains    |    50,000 |   2,172 |     990 |   4,884 |   0.46 | 32,268 |      15.0 | off   |
| Chains    |   100,000 |     785 |     534 |   1,298 |   1.27 | 11,610 |      15.0 | off   |
| Chains    |   500,000 |      50 |      48 |      51 |  20.19 |    743 |      15.0 | off   |
| Ecosystem |    10,000 |   3,187 |     315 |   5,002 |   0.31 | 45,090 |      15.0 | off   |
| Ecosystem |    50,000 |     922 |     450 |   3,034 |   1.08 | 13,059 |      15.0 | off   |
| Ecosystem |   100,000 |     413 |     145 |     712 |   2.42 |  5,466 |      15.0 | off   |
| Ecosystem |   500,000 |       9 |       5 |      20 | 112.21 |    113 |      15.0 | off   |
| Symbiosis |    10,000 |   3,601 |     330 |   5,047 |   0.28 | 52,619 |      15.0 | off   |
| Symbiosis |    50,000 |     806 |      86 |   4,330 |   1.24 | 11,795 |      15.0 | off   |
| Symbiosis |   100,000 |     355 |     195 |     425 |   2.81 |  5,230 |      15.0 | off   |
| Symbiosis |   500,000 |      25 |      21 |      28 |  40.21 |    373 |      15.0 | off   |

</details>

<details>
<summary>Pre-optimization baseline (v0.4.1)</summary>

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

</details>

---

## Linux — Capacity Benchmark (AMD Radeon RX 9070 XT)

Maximum particle count at 30 fps (1280×720 world, vsync off, `auto_density = false`):

| Preset    | Max Particles | Achieved FPS | vs previous |
|-----------|--------------:|-------------:|------------:|
| Clusters  |       721,000 |         30.3 |       0 (0%) |
| Chains    |       675,000 |         34.2 |    −16K (−2%) |
| Ecosystem |       463,000 |         52.1 |    −26K (−5%) |
| Symbiosis |       498,000 |         31.0 |    −33K (−6%) |

<details>
<summary>Previous capacity baseline (post-rsqrt force reformulation)</summary>

| Preset    | Max Particles | Achieved FPS |
|-----------|--------------:|-------------:|
| Clusters  |       721,000 |         30.4 |
| Chains    |       691,000 |         32.3 |
| Ecosystem |       489,000 |         39.8 |
| Symbiosis |       531,000 |         30.3 |

</details>

<details>
<summary>Previous capacity baseline (post-P7 LDS tile)</summary>

| Preset    | Max Particles | Achieved FPS |
|-----------|--------------:|-------------:|
| Clusters  |       674,000 |         30.5 |
| Chains    |       663,000 |         30.6 |
| Ecosystem |       477,000 |         30.8 |
| Symbiosis |       448,000 |         31.3 |

</details>

<details>
<summary>Post-P1-P6 capacity baseline (pre-P7)</summary>

| Preset    | Max Particles | Achieved FPS |
|-----------|--------------:|-------------:|
| Clusters  |       661,000 |         31.0 |
| Chains    |       650,000 |         30.5 |
| Ecosystem |       455,000 |         46.7 |
| Symbiosis |       477,000 |         33.5 |

</details>

<details>
<summary>Pre-optimization capacity baseline (v0.4.1)</summary>

| Preset    | Max Particles | Achieved FPS |
|-----------|--------------:|-------------:|
| Clusters  |       614,000 |         30.8 |
| Chains    |       607,000 |         30.6 |
| Ecosystem |       448,000 |         32.9 |
| Symbiosis |       433,000 |         30.7 |

</details>

Clusters holds at 721K (identical to the post-rsqrt baseline). Chains, Ecosystem, and Symbiosis are 2–6% below that baseline — within the variance of the binary-search procedure. Symbiosis recovered from the stride-bug low of 280K back to 498K; Ecosystem from 338K back to 463K. Ecosystem's achieved FPS of 52.1 at 463K (above the 30 fps target) indicates the cliff-detection triggered early, before the dense cluster fully formed; the true 30-fps crossover is likely slightly higher. Clusters (~721K) and Chains (~675K) lead; Symbiosis at 498K and Ecosystem at 463K trail due to mixed-blob and cluster hotspots respectively.

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
