# Agent Guidelines — Particle Life

Project-specific context for AI agents working on this codebase.

## Build & Run

```sh
cargo build --release   # release build is necessary for perf testing
cargo run --release
cargo check             # fast type/borrow check without linking
```

Unit tests live in `config.rs` (preset invariants, TOML round-trip) and `simulation.rs` (struct sizes, field offsets). They are headless and require no GPU. `cargo check` is the fastest compile-only verification; `cargo test` runs the full suite.

## Branching Strategy

| Branch | Role |
|--------|------|
| `main` | Production — only release merges land here; triggers a GitHub Release |
| `dev` | Integration — feature branches merge here; triggers CI |
| `release/x.y.z` | Release candidate — cut from `dev`, groomed, then PR'd to `main` |
| `feature/*` | Short-lived feature work; PR target is always `dev` |

Before merging a release to `main`, bump `version` in `Cargo.toml` — the release workflow reads it to tag the GitHub Release.

## CI/CD

Two workflows live in `.github/workflows/`:

**`ci.yml`** — runs on push to `dev` / `release/**` and on PRs targeting `dev` or `main`:

| Job | What it does |
|-----|-------------|
| `check` | `cargo fmt --check`, `cargo clippy -- -D warnings`, `cargo doc --no-deps` |
| `test` | `cargo test` (gates on `check`) |
| `build` | `cargo build --release` for Linux, Windows, macOS in parallel (gates on `check`) |

**`release.yml`** — runs on push to `main` only:
Builds all three platforms, then creates a GitHub Release tagged with the version from `Cargo.toml`, attaching all three binaries with auto-generated release notes.

Clippy is enforced with `-D warnings` — all warnings must be clean before merging to `dev`.

## Platform Support

The renderer uses `wgpu::Backends::PRIMARY` — Vulkan on Linux/Windows, Metal on macOS. On Linux the adapter log line will show `RADV GFX1201` confirming Vulkan is selected. Confirmed working: Linux (AMD RX 9070 XT, RADV). Untested: Windows, macOS.

## Dependency Constraints

**Do not bump these versions without verifying compatibility:**

- `wgpu = "24"` — egui-wgpu 0.31 pins to `^24`; wgpu 25 is a breaking change for the egui integration
- `egui`, `egui-winit`, `egui-wgpu` must all be the same minor version (currently `0.31`)
- `winit = "0.30"` — uses the `ApplicationHandler` trait API introduced in 0.30; prior versions use a different event loop pattern

## Architecture

### Module Summary

| Module | Role |
|--------|------|
| `main.rs` | Entry point; creates `EventLoop` with `ControlFlow::Poll` |
| `app.rs` | `AppHandler` / `AppState`; event routing, camera, per-frame orchestration |
| `renderer.rs` | `WgpuState`; device/surface setup, particle render pipeline, egui renderer |
| `simulation.rs` | `SimulationState`; GPU buffers, 5-pass compute dispatch, preset apply, spawn |
| `config.rs` | `Preset` struct, four built-in presets, TOML I/O, session persistence |
| `benchmark.rs` | `QuickBench` (ad-hoc) and `BenchmarkRunner` (full suite + CSV export) |
| `ui.rs` | All egui draw functions; returns response structs — no app state owned here |

### Data Flow Per Frame

```
about_to_wait()
  └─ window.request_redraw()

RedrawRequested
  ├─ egui frame (draw_ui, draw_toolbar, draw_perf_overlay, draw_world_border, draw_cursor_indicator)
  ├─ apply tool effects → sim.mouse_strength / spawn_particles()
  └─ renderer.render()
       ├─ sim.dispatch()   ← 5 GPU compute passes (physics)
       ├─ particle render pass
       └─ egui render pass
```

Physics runs entirely on GPU. There is no CPU physics update path.

### GPU Compute Pipeline (5 passes, all in `sim.dispatch()`)

| Pass | Shader | Notes |
|------|--------|-------|
| Count | `grid_count.wgsl` | Atomic increment per cell; cell_counts_buf cleared via `encoder.clear_buffer` before this |
| Prefix | `grid_prefix.wgsl` | Serial scan (1 workgroup); produces cell_offsets; zeros cell_counts for reuse as scatter cursors |
| Scatter | `grid_scatter.wgsl` | Assigns each particle a slot in sorted_indices via atomicAdd |
| Reorder | `grid_reorder.wgsl` | Copies `{position, species, index}` → sorted_entries in cell order |
| Force | `compute.wgsl` | 5×5 neighbor cells; reads sorted_entries sequentially (cache-friendly) |

The reorder pass is performance-critical. Without it, the force pass does random reads into the particle buffer, which causes severe cache thrashing at high N. Do not remove or skip it.

### Spatial Grid

- Cell size = `r_max_norm / 2`, so `grid_w = max(5, floor(2 / r_max_norm))`
- `MAX_GRID_CELLS = 300,000` (supports r_max_norm as small as ≈0.00365 → ~547×547)
- Cell index = `y * grid_w + x`, both axes wrapped modulo grid_w
- `r_max_norm` is the value actually sent to the GPU — see **World Size** below for how it is derived from the stored `r_max` and `world_height`

### Key Structs

**`Particle` (24 bytes, `repr(C)`)** — shared between CPU, GPU vertex buffer, and compute storage:
```
position: [f32; 2]   // offset 0  — world coords [0,1]²
velocity: [f32; 2]   // offset 8
color:    u32        // offset 16 — packed RGBA: R=bits0-7, G=bits8-15, B=bits16-23
species:  u32        // offset 20 — index into attraction matrix row
```
The vertex buffer layout in `renderer.rs` and the WGSL struct in every shader must match this exactly. Changing the stride or field order breaks both rendering and compute.

**`SimParams` (64 bytes, `repr(C)`, padded to 4×16B for uniform alignment)**

```
dt, r_min, r_max, friction          (16B)
n_particles, n_species, force_scale, aspect  (16B)
mouse_x, mouse_y, mouse_strength, mouse_range  (16B)
border_mode: u32, border_repel_strength: f32, _pad: [u32; 2]  (16B)
```

`aspect` is `world_width / world_height` (the simulation world's aspect ratio), **not** the viewport pixel ratio. The shader uses it to make inter-particle distances isotropic on screen and to equalize the border repel zone depth on all four walls.

**`SortedEntry` (16 bytes)** — `position: vec2<f32>`, `species: u32`, `index: u32`

### Border Modes

| Value | Mode | Behavior |
|-------|------|----------|
| `0` | Wrap | Torus topology; `torus_delta` used for all distance calculations |
| `1` | Repel | Spring force near walls; position clamped after integration |
| `2` | Static | Hard wall; outward velocity zeroed and position clamped |

In the force shader, use `torus_delta` for neighbor distances only when `border_mode == 0`. In repel/static modes, direct deltas are correct — cross-boundary attraction should vanish naturally.

The border repel zone uses `brange_x = r_max / aspect` for the left/right walls and `brange_y = r_max` for top/bottom. This equalises the visual zone depth on all four walls regardless of world aspect ratio.

## Camera Model

`AppState` holds two zoom values:

- `camera.zoom_factor: f32` — user zoom relative to fit (1.0 = default, 2.0 = 2× in)
- `fit_zoom: f32` — computed from world size + viewport so the full world fits the window at `zoom_factor = 1.0`
- `shader_zoom = camera.zoom_factor * fit_zoom` — passed to the GPU and `world_to_screen()`

`fit_zoom` is recomputed whenever the window resizes or world dimensions change (`compute_fit_zoom(world_w, world_h, vp_w, vp_h)`).

The vertex shader converts world → NDC as:
```wgsl
ndc = (pos - camera_center) * (shader_zoom * 2.0)
```

## Coordinate Systems

- **World space**: `[0, 1]²`, origin bottom-left, y increases upward
- **Screen/NDC**: standard wgpu NDC (-1 to 1), y increases upward
- **Cursor**: `PhysicalPosition<f64>` from winit, y increases downward — `screen_to_world()` in `app.rs` handles the flip
- **Camera**: `center` is the world point at screen center; panning is clamped so the world border never passes the viewport center

## World Size and Density Scaling

`SimulationState` stores `world_width: f32` and `world_height: f32` (simulation units, default 1280×720). The simulation always runs in normalised `[0,1]²` coordinates. World dimensions affect physics in two ways:

1. **Aspect ratio** — `world_aspect() = world_width / world_height` is passed as `SimParams.aspect` and used in the shader to make inter-particle distances isotropic on screen.

2. **Interaction radius scaling** — `r_max` and `r_min` in presets are stored as fractions of `BASE_WORLD_HEIGHT = 720.0`. The normalised values actually sent to the GPU are:
   ```
   r_max_norm = r_max * 720.0 / world_height   (clamped to prevent grid overflow)
   r_min_norm = r_min * 720.0 / world_height
   ```
   At the default world (height=720) these equal the stored values unchanged. At a larger world they shrink, producing a finer grid and fewer neighbours per particle — `O(n)` GPU work instead of `O(n²)`.

**Auto-density mode** (`sim.auto_density = true`): `auto_world_size()` recalculates `world_width/height` to maintain `density_target` (particles per world-unit²) as `particle_count` changes. Enabling this keeps GPU frame time roughly linear with particle count up to the grid-cell limit.

**MAX_PARTICLES = 2,000,000** — GPU buffers are allocated for this at startup (~90 MB total). At auto-density scaling from the 5 K default, 2 M particles requires a world ≈25,600×14,400 and r_max_norm ≈ 0.004 (grid ≈500×500 = 250,000 cells, within MAX_GRID_CELLS = 300,000).

**Preset load** (normal UI path): `app.rs` scales the world to the current window size after calling `apply_preset()`, preserving the preset's density. Benchmark loads bypass this to use the pinned tier dimensions.

`fit_zoom` in `app.rs` is derived from world dimensions so the world fills the viewport at default zoom regardless of aspect ratio.

## Preset System

`config::Preset` is a TOML-serialisable snapshot of all simulation parameters. Key functions:

- `builtin_presets()` — four compiled-in presets used by the benchmark suite and preset picker
- `load_presets_dir()` — scans `presets/*.toml` on startup
- `save_session` / `load_session` — auto-save to `session.toml` on exit, restore on startup
- `SimulationState::apply_preset()` — copies all fields and calls `respawn()`
- `SimulationState::to_preset()` — snapshot current state (used for export and session save)

The `attraction` field in a preset is a compact `species_count × species_count` `Vec<f32>`. `apply_preset` expands it into the full 8×8 GPU layout.

**Preset loading in normal UI flow** (`app.rs`): after calling `apply_preset()`, `app.rs` overrides `world_width/height/particle_count` to preserve the preset's density at the current window size. The preset's stored world dimensions are used only to compute the density ratio — they are not applied directly.

**Benchmark loading**: `BenchmarkRunner::combo_preset()` returns a preset with `world_width`/`world_height`/`auto_density` pinned to the tier's fixed values. The benchmark handler calls `apply_preset()` directly without the density-scaling override, ensuring reproducible results.

## Attraction Matrix

- Stored as `[f32; 64]`, row-major, 8×8 (MAX_SPECIES = 8)
- `A[i, j] = attraction[i * 8 + j]` — force that species `j` exerts on species `i`
- Only the active `species_count × species_count` sub-matrix is meaningful; unused entries are zero
- `randomize_attraction()` fills the active sub-matrix with uniform random `[-1, 1]`
- Uploaded to `attraction_buf` (STORAGE, 256 bytes) each frame in `dispatch()`

## Mouse / Tool State

Mouse attractor state is written to `SimulationState` fields each frame in `app.rs` before `dispatch()` is called:
- `sim.mouse_x`, `sim.mouse_y` — world-space cursor position
- `sim.mouse_strength` — positive = attract, negative = repel, 0.0 = inactive
- `sim.mouse_range` — world-space radius

The shader applies a quadratic falloff: `vel += direction * (strength * t² * dt)` where `t = 1 - dist/range`.

## Simulation Parameters

| Field | Default | Range in UI | Notes |
|-------|---------|-------------|-------|
| `r_min` | 0.025 | 0.001–0.1 | Fraction of `BASE_WORLD_HEIGHT`; GPU value = `r_min * 720 / world_height` |
| `r_max` | 0.08 | 0.01–0.3 | Fraction of `BASE_WORLD_HEIGHT`; GPU value clamped to prevent grid overflow |
| `friction` | 0.5 | 0–5 | |
| `force_scale` | 0.007 | 0.0001–0.05 | |
| `particle_radius` | 1.5 | 0.5–12 | World units; normalised by `world_height` in renderer |
| `border_repel_strength` | 5.0 | 0.1–30 | |
| `particle_count` | 5 000 | 100–2 000 000 | |
| `world_width / world_height` | 1280 / 720 | 100–200 000 | Auto-computed if `auto_density` is on |

CFL velocity cap in the shader: `max_speed = r_max_norm / dt * 0.25`. This prevents tunneling. Do not remove it.

## Critical winit/wgpu Patterns

- **Window creation**: only in `resumed()` — on Wayland the window handle is not valid before the first `resumed()` call
- **`Arc<Window>` → `Surface<'static>`**: wrap in `Arc` before passing to `create_surface` so the surface lifetime is `'static`
- **`request_redraw()`**: call in `about_to_wait()`, not inside `RedrawRequested` — calling it from within the redraw handler can cause missed frames on some platforms
- **egui render pass**: `begin_render_pass(...).forget_lifetime()` is required; `egui_wgpu::Renderer::render()` needs `&mut RenderPass<'static>`
- **egui buffer uploads**: `update_buffers()` returns `Vec<CommandBuffer>` that must be submitted **before** the main encoder — they are staging uploads that the render pass depends on
- **Window drop order**: `window: Arc<Window>` must be the last field in `AppState`; `Surface<'static>` holds a raw pointer into it and must drop first

## What Not to Change Without Care

- **`encoder.clear_buffer(&cell_counts_buf, ...)`** before the count pass — must happen every frame; forgetting it produces garbage grid data
- **Workgroup dispatch**: `(n + 63) / 64` — standard ceiling division for 64-thread workgroups
- **Prefix scan dispatch**: always `(1, 1, 1)` — it's a serial scan of up to 300,000 elements, intentionally single-workgroup (~0.3 ms at 2 M-particle scale)
- **`PresentMode::Fifo`** — vsync; changing to `Mailbox` or `Immediate` is valid for uncapped FPS but changes perceived behavior
- **`brange_x = r_max / aspect`** in the border repel section of `compute.wgsl` — equalises the visual repel zone on all four walls; removing the aspect correction makes left/right walls appear ~1.78× wider than top/bottom on a 16:9 display
