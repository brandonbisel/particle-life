# Agent Guidelines — Particle Life

Project-specific context for AI agents working on this codebase.

## Build & Run

```sh
cargo build --release   # release build is necessary for perf testing
cargo run --release
cargo check             # fast type/borrow check without linking
```

No tests exist yet. `cargo check` is the fastest way to verify changes compile.

## Platform Support

The renderer uses `wgpu::Backends::PRIMARY` — Vulkan on Linux/Windows, Metal on macOS. On Linux the adapter log line will show `RADV GFX1201` confirming Vulkan is selected. Confirmed working: Linux (AMD RX 9070 XT, RADV). Untested: Windows, macOS.

## Dependency Constraints

**Do not bump these versions without verifying compatibility:**

- `wgpu = "24"` — egui-wgpu 0.31 pins to `^24`; wgpu 25 is a breaking change for the egui integration
- `egui`, `egui-winit`, `egui-wgpu` must all be the same minor version (currently `0.31`)
- `winit = "0.30"` — uses the `ApplicationHandler` trait API introduced in 0.30; prior versions use a different event loop pattern

## Architecture

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

- Cell size = `r_max / 2`, so `grid_w = max(5, floor(2 / r_max))`
- MAX_GRID_CELLS = 40,000 (supports r_max as small as 0.01 → 200×200)
- Cell index = `y * grid_w + x`, both axes wrapped modulo grid_w

### Key Structs

**`Particle` (24 bytes, `repr(C)`)** — shared between CPU, GPU vertex buffer, and compute storage:
```
position: [f32; 2]   // offset 0  — world coords [0,1]²
velocity: [f32; 2]   // offset 8
color:    u32        // offset 16 — packed RGBA: R=bits0-7, G=bits8-15, B=bits16-23
species:  u32        // offset 20 — index into attraction matrix row
```
The vertex buffer layout in `renderer.rs` and the WGSL struct in every shader must match this exactly. Changing the stride or field order breaks both rendering and compute.

**`SimParams` (64 bytes, `repr(C)`, pad to 4×16B for uniform alignment)**

**`SortedEntry` (16 bytes)** — `position: vec2<f32>`, `species: u32`, `index: u32`

### Border Modes

| Value | Mode | Behavior |
|-------|------|----------|
| `0` | Wrap | Torus topology; `torus_delta` used for all distance calculations |
| `1` | Repel | Spring force near walls; position clamped after integration |
| `2` | Static | Hard wall; outward velocity zeroed and position clamped |

In the force shader, use `torus_delta` for neighbor distances only when `border_mode == 0`. In repel/static modes, direct deltas are correct — cross-boundary attraction should vanish naturally.

## Critical winit/wgpu Patterns

- **Window creation**: only in `resumed()` — on Wayland the window handle is not valid before the first `resumed()` call
- **`Arc<Window>` → `Surface<'static>`**: wrap in `Arc` before passing to `create_surface` so the surface lifetime is `'static`
- **`request_redraw()`**: call in `about_to_wait()`, not inside `RedrawRequested` — calling it from within the redraw handler can cause missed frames on some platforms
- **egui render pass**: `begin_render_pass(...).forget_lifetime()` is required; `egui_wgpu::Renderer::render()` needs `&mut RenderPass<'static>`
- **egui buffer uploads**: `update_buffers()` returns `Vec<CommandBuffer>` that must be submitted **before** the main encoder — they are staging uploads that the render pass depends on
- **Window drop order**: `window: Arc<Window>` must be the last field in `AppState`; `Surface<'static>` holds a raw pointer into it and must drop first

## Coordinate Systems

- **World space**: `[0, 1]²`, origin bottom-left, y increases upward
- **Screen/NDC**: standard wgpu NDC (-1 to 1), y increases upward
- **Cursor**: `PhysicalPosition<f64>` from winit, y increases downward — `screen_to_world()` in `app.rs` handles the flip
- **Camera**: `center` is the world point at screen center; `zoom = 1.0` shows the full `[0,1]²` world; `zoom = 2.0` is 2× magnification

The vertex shader converts world → NDC as:
```wgsl
ndc = (pos - camera_center) * (camera_zoom * 2.0)
```

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

| Field | Default | Range in UI |
|-------|---------|-------------|
| `r_min` | 0.025 | 0.001–0.1 |
| `r_max` | 0.08 | 0.01–0.3 |
| `friction` | 0.5 | 0–5 |
| `force_scale` | 0.007 | 0.0001–0.05 |
| `particle_radius` | 1.5 px | 0.5–12 px |
| `border_repel_strength` | 5.0 | 0.1–30 |

CFL velocity cap in the shader: `max_speed = r_max / dt * 0.25`. This prevents tunneling. Do not remove it.

## What Not to Change Without Care

- **`encoder.clear_buffer(&cell_counts_buf, ...)`** before the count pass — must happen every frame; forgetting it produces garbage grid data
- **Workgroup dispatch**: `(n + 63) / 64` — standard ceiling division for 64-thread workgroups
- **Prefix scan dispatch**: always `(1, 1, 1)` — it's a serial scan of at most 40,000 elements, intentionally single-workgroup
- **`PresentMode::Fifo`** — vsync; changing to `Mailbox` or `Immediate` is valid for uncapped FPS but changes perceived behavior
