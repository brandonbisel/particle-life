# Particle Life

A GPU-accelerated [Particle Life](https://particle-life.com/) simulator written in Rust. Up to 500,000 particles interact via emergent attraction/repulsion rules, running entirely on the GPU at real-time frame rates.

![Particle Life](https://img.shields.io/badge/language-Rust-orange) ![GPU](https://img.shields.io/badge/GPU-wgpu%2024-blue) ![UI](https://img.shields.io/badge/UI-egui%200.31-green)

## Features

- **100K particles** at 120+ fps on a modern discrete GPU (tested: AMD RX 9070 XT)
- **500K particles** at 20–40 fps
- **8 species** with a fully editable N×N attraction matrix
- **3 border modes:** Wrap (torus), Repel (spring wall), Static (hard wall)
- **Interactive tools:** Pan, Zoom, Attract, Repel, Spawn with adjustable range and strength
- **Configurable world size** — independent of window size; zoom/pan always shows the full world at fit
- **Preset system** — save, load, and import/export TOML presets; four built-in presets included
- **Real-time controls:** particle count, species, physics params, matrix randomization
- **Performance overlay:** FPS, frame time min/max/avg, grid stats

## How It Works

### The Force Model

Each pair of particles within range `r_max` interacts via a piecewise force:

- **Repulsion zone** `[0, r_min]`: hard repulsion proportional to overlap — prevents collapse
- **Interaction zone** `[r_min, r_max]`: species-dependent attraction or repulsion, peaking at `(r_min + r_max) / 2`

The attraction coefficient `A[i,j]` ∈ [-1, 1] is stored in an 8×8 matrix. Randomizing the matrix produces qualitatively different emergent behaviors: orbiting clusters, chain structures, single-species stars, and more.

### GPU Pipeline (5 compute passes per frame)

All physics runs on the GPU via [wgpu](https://github.com/gfx-rs/wgpu) compute shaders (WGSL). A spatial grid reduces the force pass from O(N²) to O(N · k) where k is average neighbors per cell.

| Pass | Shader | Work |
|------|--------|------|
| 1. Count | `grid_count.wgsl` | Each particle atomically increments its cell's counter |
| 2. Prefix | `grid_prefix.wgsl` | Serial exclusive scan → cell offsets; zeros counts for reuse |
| 3. Scatter | `grid_scatter.wgsl` | Each particle claims a sorted slot via `atomicAdd` |
| 4. Reorder | `grid_reorder.wgsl` | Copies `{position, species, index}` into `sorted_entries` in cell order |
| 5. Force | `compute.wgsl` | 5×5 neighbor cell check; reads sequentially from `sorted_entries` |

The reorder pass is critical: it converts random pointer-chasing in the force loop into sequential memory reads, recovering near-brute-force GPU cache throughput at large N.

**Grid parameters:** cell size = `r_max / 2`, so `grid_w = max(5, floor(2 / r_max))`. At default `r_max = 0.08`: 25×25 = 625 cells, ~80 particles/cell at 50K.

### Rendering

Particles are rendered as instanced soft circles via a vertex+fragment shader (`particle.wgsl`). Each particle is a 2-triangle quad; the fragment shader discards pixels outside the unit circle and applies a smoothstep alpha for a soft edge. The particle buffer is shared between the compute and render pipelines (no CPU readback).

A camera transform supports pan and zoom; the UI overlay is rendered via [egui](https://github.com/emilk/egui).

## Building

```sh
# Requires Rust (edition 2024) and a Vulkan-capable GPU
cargo build --release
cargo run --release
```

The Vulkan backend is required. Wayland and X11 are both supported via winit.

## Dependencies

| Crate | Version | Purpose |
|-------|---------|---------|
| `wgpu` | 24 | GPU compute + rendering (Vulkan backend) |
| `winit` | 0.30 | Window management (`ApplicationHandler` API) |
| `egui` + `egui-winit` + `egui-wgpu` | 0.31 | Immediate-mode UI overlay |
| `bytemuck` | 1 | Safe Pod casts for GPU buffer uploads |
| `pollster` | 0.3 | Block on async wgpu initialization |
| `serde` + `toml` | 1 / 0.8 | Preset serialisation |
| `rfd` | 0.15 | Native file dialogs for import/export |

## Controls

### Mouse

| Action | Effect |
|--------|--------|
| **Drag** (Pan tool) | Pan the camera |
| **Middle-click drag** | Pan the camera (any tool) |
| **Scroll wheel** | Zoom in/out centered on cursor |
| **Click** (Zoom +/−) | Zoom in/out centered on cursor |
| **Hold click** (Attract/Repel) | Pull/push particles toward cursor |
| **Hold click** (Spawn) | Emit new particles at cursor |

### Keyboard

| Key | Effect |
|-----|--------|
| `Arrow keys` | Pan |
| `+` / `=` | Zoom in |
| `-` | Zoom out |
| `0` | Reset view |
| `Escape` | Quit |

## Project Structure

```
src/
  main.rs              — Entry point; EventLoop + ControlFlow::Poll
  app.rs               — ApplicationHandler; owns window, renderer, sim, egui state, camera
  renderer.rs          — wgpu device/surface/pipeline; render() drives one frame
  simulation.rs        — SimulationState; GPU buffers, 5-pass dispatch, spawn, preset apply
  benchmark.rs         — QuickBench (ad-hoc) and BenchmarkRunner (full suite + CSV export)
  config.rs            — Preset struct, built-in presets, TOML save/load, session persistence
  ui.rs                — egui panels: toolbar, params, attraction matrix, perf overlay
  shaders/
    particle.wgsl      — Instanced soft-circle vertex + fragment shader
    compute.wgsl       — Force integration (pass 5)
    grid_count.wgsl    — Spatial grid pass 1
    grid_prefix.wgsl   — Spatial grid pass 2
    grid_scatter.wgsl  — Spatial grid pass 3
    grid_reorder.wgsl  — Spatial grid pass 4
```

## Default Parameters

| Parameter | Value | Description |
|-----------|-------|-------------|
| `r_min` | 0.025 | Hard-core repulsion radius |
| `r_max` | 0.08 | Interaction cutoff |
| `friction` | 0.5 | Velocity half-life ~1.4s |
| `force_scale` | 0.007 | Global force multiplier |
| `particle_radius` | 1.5 px | Rendered size |
| `world_width/height` | 1280 × 720 | Simulation world dimensions (units) |
| Max particles | 500,000 | Hard GPU buffer limit |
| Max species | 8 | Attraction matrix dimension |
