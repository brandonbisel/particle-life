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
| Rich Mix | 6 | Hand-crafted asymmetric | Mixed attractions and repulsions; varied emergent structures simultaneously |
| Symbiosis | 6 | Diagonal: `−0.1`, off-diagonal: `+0.6` | Every species attracts all others, weakly repels its own kind; large mixed-colour aggregates |

Symbiosis is the structural inverse of Clusters — exercises cross-species attraction and uniform spatial mixing rather than species segregation.

## Summary

> **Note:** Results below are pending a re-run after switching from Separation to Symbiosis
> and upgrading to time-based collection. The 500K numbers remain valid; low particle-count
> numbers at vsync-off are CPU-submission-bound and not meaningful GPU throughput figures.

| Particles | Avg FPS (vsync off) | Notes |
|----------:|--------------------:|-------|
| 10,000 | ~4,500 | CPU/submission bound — GPU has far more headroom |
| 50,000 | ~1,700 | CPU/submission bound |
| 100,000 | ~660 | Transitioning to GPU bound |
| 500,000 | ~46 | GPU bound; ~21 ms/frame |

At ≤ 100K particles the frame time is dominated by CPU-GPU submission overhead, not GPU compute.
The 500K tier is the meaningful GPU-bound measurement.

## Per-run detail (pending refresh)

_To be updated after running the suite with the current preset set and benchmark parameters._
_Export CSV from the Suite Benchmark panel and replace this section._
