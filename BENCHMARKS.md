# Benchmarks

Results recorded with the built-in Suite Benchmark (vsync off, 5s warmup + 15s collection per combo).

## Hardware

| Component | Detail |
|-----------|--------|
| GPU | AMD Radeon RX 9070 XT (RDNA 4) |
| API | Vulkan via wgpu 24 |
| Resolution | 3440 × 1368 |
| Display | 165 Hz |
| OS | Linux (CachyOS) |

## Presets

| Preset | Species | Matrix pattern | Behaviour |
|--------|--------:|----------------|-----------|
| Clusters | 6 | Diagonal: `+0.7`, off-diagonal: `−0.2` | Like attracts like; compact same-colour blobs with mild intermingling |
| Chains | 6 | Circular predator-prey ring | Each species chases the next, flees the previous; trailing spirals and filaments |
| Ecosystem | 6 | Two-group asymmetric | Spiraling predator chain (species 0–2) hunts a tight fleeing cluster (species 3–5); two coexisting emergent structures |
| Symbiosis | 6 | Diagonal: `−0.1`, off-diagonal: `+0.6` | Every species attracts all others, weakly repels its own kind; large mixed-colour aggregates |

Symbiosis is the structural inverse of Clusters — exercises cross-species attraction and uniform spatial mixing rather than species segregation.

## Summary

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

## Per-run detail

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
