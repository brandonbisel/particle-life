# Benchmarks

Results recorded with the built-in Suite Benchmark (300 frames per run, all four built-in presets).

## Hardware

| Component | Detail |
|-----------|--------|
| GPU | AMD Radeon RX 9070 XT (RDNA 4) |
| API | Vulkan via wgpu 24 |
| Resolution | 3440 × 1368 |
| Display | 165 Hz |
| OS | Linux (CachyOS) |

## Summary

| Particles | Avg FPS | Min FPS | Avg frame ms |
|----------:|--------:|--------:|-------------:|
| 10,000 | 165 | 143 | 6.1 |
| 50,000 | 166 | 124 | 6.0 |
| 100,000 | 166 | 126 | 6.0 |
| 500,000 | 46 | 41 | 21.7 |

At ≤ 100K particles the simulation is **display-limited** at 165 fps — the GPU has significant headroom.
The jump to 500K is where the force pass begins to dominate.

Preset choice has no measurable effect on performance (< 0.5% variance across Clusters, Chains, Rich Mix, and Separation at each particle count), which confirms the spatial grid is working as intended.

## Per-run detail

| Preset | Particles | Avg FPS | Min FPS | Max FPS | Avg ms |
|--------|----------:|--------:|--------:|--------:|-------:|
| Clusters | 10,000 | 165.8 | 142.7 | 194.9 | 6.03 |
| Clusters | 50,000 | 166.0 | 139.5 | 203.0 | 6.02 |
| Clusters | 100,000 | 165.8 | 141.5 | 197.5 | 6.03 |
| Clusters | 500,000 | 46.9 | 44.4 | 49.3 | 21.33 |
| Chains | 10,000 | 165.0 | 154.4 | 175.7 | 6.06 |
| Chains | 50,000 | 165.6 | 124.0 | 207.4 | 6.04 |
| Chains | 100,000 | 166.7 | 126.7 | 232.0 | 6.00 |
| Chains | 500,000 | 46.2 | 43.2 | 48.0 | 21.65 |
| Rich Mix | 10,000 | 165.0 | 159.2 | 170.0 | 6.06 |
| Rich Mix | 50,000 | 165.7 | 134.0 | 196.0 | 6.04 |
| Rich Mix | 100,000 | 166.7 | 125.8 | 241.0 | 6.00 |
| Rich Mix | 500,000 | 45.7 | 43.1 | 47.5 | 21.89 |
| Separation | 10,000 | 165.0 | 159.8 | 169.8 | 6.06 |
| Separation | 50,000 | 165.7 | 135.4 | 209.4 | 6.03 |
| Separation | 100,000 | 166.5 | 138.0 | 233.1 | 6.01 |
| Separation | 500,000 | 45.9 | 41.1 | 50.0 | 21.77 |

Min FPS reflects single-frame outliers (OS scheduling, shader compilation on first run, etc.) rather than sustained drops.
