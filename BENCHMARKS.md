# Benchmarks

Results recorded with the built-in Suite Benchmark (vsync off, 5s warmup + 15s collection per combo).

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
| Clusters  | 4,712 | 1,530 |  660 |   47 |
| Chains    | 4,795 | 1,841 |  694 |   46 |
| Ecosystem | 2,732 |   733 |  386 |   30 |
| Symbiosis | 2,761 |   756 |  305 |   26 |

**Performance tiers:**

- **Clusters and Chains** keep particles distributed across the spatial grid, so cell load stays balanced. At ≤ 100K this is CPU/submission-bound — the GPU has far more headroom. 500K is the meaningful GPU-bound number (~21 ms/frame).
- **Ecosystem and Symbiosis** cause particles to aggregate into dense blobs, creating spatial grid hotspots where individual cells have many more neighbors to evaluate. This produces ~1.6–1.8× lower throughput than the spreading presets even at the same particle count. These presets are GPU-bound as low as 50K.
- The 500K tier is GPU-bound for all presets and is the most useful cross-preset comparison.

### Per-run detail

| Preset    | Particles | Avg FPS | Min FPS | Max FPS | Avg ms | Frames | Wall secs | VSync |
|-----------|----------:|--------:|--------:|--------:|-------:|-------:|----------:|-------|
| Clusters  |    10,000 |   4,712 |     317 |   5,553 |   0.21 | 69,073 |      15.0 | off   |
| Clusters  |    50,000 |   1,530 |     561 |   2,384 |   0.65 | 22,651 |      15.0 | off   |
| Clusters  |   100,000 |     660 |     335 |     999 |   1.52 |  9,737 |      15.0 | off   |
| Clusters  |   500,000 |      47 |      42 |      49 |  21.43 |    700 |      15.0 | off   |
| Chains    |    10,000 |   4,795 |     298 |   5,633 |   0.21 | 70,274 |      15.0 | off   |
| Chains    |    50,000 |   1,841 |     626 |   3,147 |   0.54 | 27,289 |      15.0 | off   |
| Chains    |   100,000 |     694 |     250 |   2,612 |   1.44 | 10,220 |      15.0 | off   |
| Chains    |   500,000 |      46 |      43 |      48 |  21.59 |    695 |      15.0 | off   |
| Ecosystem |    10,000 |   2,732 |     286 |   5,378 |   0.37 | 38,863 |      15.0 | off   |
| Ecosystem |    50,000 |     733 |     261 |   3,221 |   1.36 |  9,871 |      15.0 | off   |
| Ecosystem |   100,000 |     386 |     201 |     656 |   2.59 |  5,379 |      15.0 | off   |
| Ecosystem |   500,000 |      30 |      20 |      42 |  33.95 |    282 |      15.1 | off   |
| Symbiosis |    10,000 |   2,761 |     285 |   5,441 |   0.36 | 40,453 |      15.0 | off   |
| Symbiosis |    50,000 |     756 |     341 |   1,228 |   1.32 | 10,921 |      15.0 | off   |
| Symbiosis |   100,000 |     305 |     178 |     403 |   3.27 |  4,537 |      15.0 | off   |
| Symbiosis |   500,000 |      26 |      21 |      29 |  39.21 |    381 |      15.0 | off   |

---

## macOS — Mac Mini (M4)

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
