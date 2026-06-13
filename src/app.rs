//! winit application handler: event routing, camera, and per-frame orchestration.
//!
//! [`AppHandler`] is the single winit [`ApplicationHandler`] implementation.
//! It owns [`AppState`] which is created on the first [`resumed`](ApplicationHandler::resumed)
//! event and drives the simulation, renderer, and egui each frame.

use std::collections::VecDeque;
use std::sync::Arc;
use std::time::Instant;

use winit::{
    application::ApplicationHandler,
    dpi::PhysicalPosition,
    event::{ElementState, KeyEvent, MouseButton, MouseScrollDelta, WindowEvent},
    event_loop::ActiveEventLoop,
    keyboard::{Key, NamedKey},
    window::{CursorIcon, Fullscreen, Window, WindowId},
};

use crate::{benchmark, config, icon, renderer::WgpuState, simulation::SimulationState, ui};

// ── Camera ────────────────────────────────────────────────────────────────────

#[derive(Clone, Copy)]
struct Camera {
    center: [f32; 2], // world-space point at screen center (normalized [0,1]²)
    zoom_factor: f32, // 1.0 = world fits viewport; 2.0 = 2× zoom in
}

impl Camera {
    fn default_view() -> Self {
        Self {
            center: [0.5, 0.5],
            zoom_factor: 1.0,
        }
    }
}

/// zoom level at which the world exactly fits the viewport (letterboxed/pillarboxed as needed).
fn compute_fit_zoom(world_w: f32, world_h: f32, vp_w: u32, vp_h: u32) -> f32 {
    let vw = vp_w as f32;
    let vh = vp_h as f32;
    let world_aspect = world_w / world_h;
    let vp_aspect = vw / vh;
    // zoom=1 fills screen height; scale down if world is wider than viewport
    (vp_aspect / world_aspect).min(1.0)
}

/// Convert a screen-space cursor position to world-space coordinates (normalized [0,1]²).
fn screen_to_world(
    px: PhysicalPosition<f64>,
    viewport: winit::dpi::PhysicalSize<u32>,
    cam: &Camera,
    world_aspect: f32,
    shader_zoom: f32,
) -> [f32; 2] {
    let vp_aspect = viewport.width as f32 / viewport.height as f32;
    let ndc_x = (px.x as f32 / viewport.width as f32) * 2.0 - 1.0;
    let ndc_y = 1.0 - (px.y as f32 / viewport.height as f32) * 2.0;
    [
        ndc_x * vp_aspect / (world_aspect * shader_zoom * 2.0) + cam.center[0],
        ndc_y / (shader_zoom * 2.0) + cam.center[1],
    ]
}

/// Zoom the camera by `factor` keeping `cursor_world` fixed on screen.
fn apply_zoom(cam: &mut Camera, cursor_world: [f32; 2], factor: f32) {
    let new_zoom = (cam.zoom_factor * factor).clamp(0.1, 40.0);
    let scale = cam.zoom_factor / new_zoom;
    cam.center[0] = cursor_world[0] - (cursor_world[0] - cam.center[0]) * scale;
    cam.center[1] = cursor_world[1] - (cursor_world[1] - cam.center[1]) * scale;
    cam.zoom_factor = new_zoom;
}

// ── AppState ──────────────────────────────────────────────────────────────────

#[derive(Default)]
pub struct AppHandler {
    state: Option<AppState>,
}

struct AppState {
    // Simulation / rendering
    renderer: WgpuState,
    sim: SimulationState,
    egui_ctx: egui::Context,
    egui_state: egui_winit::State,
    last_frame: Instant,
    frame_times: VecDeque<f32>,

    // Camera
    camera: Camera,
    fit_zoom: f32, // computed from world size + viewport; camera.zoom_factor is relative to this

    // Toolbar tool state
    tool: ui::Tool,
    tool_range: f32,
    mouse_strength: f32,

    // Spawn tool state
    spawn_species: Option<usize>, // None = random species per particle
    spawn_rate: u32,              // particles spawned per frame while LMB held

    // Presets + persistence
    preset_library: Vec<config::Preset>,
    selected_preset: usize,

    // Benchmark
    benchmark: benchmark::BenchmarkRunner,
    quick_bench: benchmark::QuickBench,
    vsync: bool,

    // Pending one-shot actions triggered by keyboard shortcuts
    pending_screenshot: bool,

    // Mouse tracking
    cursor_px: PhysicalPosition<f64>,
    lmb_down: bool,
    lmb_egui: bool, // true while LMB is held and the press was consumed by egui

    // Pan drag state (LMB+Pan tool or MMB)
    lmb_panning: bool,
    mmb_panning: bool,
    pan_start_px: PhysicalPosition<f64>,
    pan_start_center: [f32; 2],

    // window MUST be last: Surface<'static> points into it; drop order matters.
    window: Arc<Window>,
}

// ── ApplicationHandler ────────────────────────────────────────────────────────

impl ApplicationHandler for AppHandler {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.state.is_some() {
            return;
        }

        // On Linux, write icon + .desktop so the Wayland compositor can find
        // the icon via XDG lookup (native Wayland ignores set_window_icon).
        #[cfg(target_os = "linux")]
        icon::install_xdg_resources();

        let mut win_attrs = Window::default_attributes()
            .with_title("Particle Life")
            .with_inner_size(winit::dpi::LogicalSize::new(1280u32, 720u32));

        // Set the Wayland app_id so the compositor links us to the .desktop
        // entry and displays the correct icon in the taskbar.
        #[cfg(target_os = "linux")]
        {
            use winit::platform::wayland::WindowAttributesExtWayland;
            win_attrs = win_attrs.with_name("particle-life", "particle-life");
        }

        let window = Arc::new(
            event_loop
                .create_window(win_attrs)
                .expect("Failed to create window"),
        );

        // X11 / XWayland: sets _NET_WM_ICON on the window directly.
        window.set_window_icon(Some(icon::app_icon()));

        let renderer = WgpuState::new(Arc::clone(&window));
        let size = window.inner_size();

        // Build preset library: 4 builtins + embedded bundled + any user presets from ./presets/
        let mut preset_library = config::builtin_presets();
        preset_library.extend(config::bundled_presets());
        preset_library.extend(config::load_presets_dir());

        // Load session or use defaults from first preset
        let session = config::load_session();
        let (world_width, world_height) = if let Some(ref p) = session {
            (p.world_width, p.world_height)
        } else {
            (size.width as f32, size.height as f32)
        };

        let mut sim = SimulationState::new(
            renderer.device(),
            renderer.queue(),
            1000,
            6,
            world_width,
            world_height,
        );
        if let Some(ref p) = session {
            sim.apply_preset(renderer.queue(), p);
        }
        renderer.update_palette(&sim.palette);
        let fit_zoom = compute_fit_zoom(world_width, world_height, size.width, size.height);

        let egui_ctx = egui::Context::default();
        let mut fonts = egui::FontDefinitions::default();
        egui_phosphor::add_to_fonts(&mut fonts, egui_phosphor::Variant::Regular);
        egui_ctx.set_fonts(fonts);
        let egui_state = egui_winit::State::new(
            egui_ctx.clone(),
            egui::ViewportId::ROOT,
            window.as_ref(),
            Some(window.scale_factor() as f32),
            None,
            Some(renderer.max_texture_side()),
        );

        self.state = Some(AppState {
            renderer,
            sim,
            egui_ctx,
            egui_state,
            last_frame: Instant::now(),
            frame_times: VecDeque::with_capacity(120),
            camera: Camera::default_view(),
            fit_zoom,
            preset_library,
            selected_preset: 0,
            benchmark: benchmark::BenchmarkRunner::new(),
            quick_bench: benchmark::QuickBench::new(),
            vsync: true,
            tool: ui::Tool::Pan,
            tool_range: 0.1,
            mouse_strength: 2.0,
            spawn_species: None,
            spawn_rate: 50,
            cursor_px: PhysicalPosition::new(0.0, 0.0),
            pending_screenshot: false,
            lmb_down: false,
            lmb_egui: false,
            lmb_panning: false,
            mmb_panning: false,
            pan_start_px: PhysicalPosition::new(0.0, 0.0),
            pan_start_center: [0.5, 0.5],
            window,
        });
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        _window_id: WindowId,
        event: WindowEvent,
    ) {
        // Handle exit events before borrowing state so we can call self.state = None
        // while the window handle is still live (prevents SIGSEGV in surface teardown).
        match &event {
            WindowEvent::CloseRequested => {
                if let Some(ref s) = self.state {
                    config::save_session(&s.sim.to_preset("session"));
                }
                self.state = None;
                event_loop.exit();
                return;
            }
            WindowEvent::KeyboardInput {
                event:
                    KeyEvent {
                        logical_key: Key::Named(NamedKey::Escape),
                        state: ElementState::Pressed,
                        ..
                    },
                ..
            } => {
                if let Some(ref s) = self.state {
                    config::save_session(&s.sim.to_preset("session"));
                }
                self.state = None;
                event_loop.exit();
                return;
            }
            _ => {}
        }

        let Some(state) = self.state.as_mut() else {
            return;
        };

        let window = Arc::clone(&state.window);
        let resp = state.egui_state.on_window_event(&window, &event);

        match event {
            WindowEvent::Resized(size) => {
                state.renderer.resize(size);
                state.fit_zoom = compute_fit_zoom(
                    state.sim.world_width,
                    state.sim.world_height,
                    size.width,
                    size.height,
                );
            }

            WindowEvent::CursorMoved { position, .. } => {
                state.cursor_px = position;

                if state.lmb_panning || state.mmb_panning {
                    let vp = window.inner_size();
                    let shader_zoom = state.camera.zoom_factor * state.fit_zoom;
                    let world_aspect = state.sim.world_aspect();
                    state.camera.center = [
                        state.pan_start_center[0]
                            - (position.x - state.pan_start_px.x) as f32
                                / (vp.height as f32 * world_aspect * shader_zoom),
                        state.pan_start_center[1]
                            + (position.y - state.pan_start_px.y) as f32
                                / (vp.height as f32 * shader_zoom),
                    ];
                    // Clamp so the world border never passes screen center.
                    state.camera.center[0] = state.camera.center[0].clamp(0.0, 1.0);
                    state.camera.center[1] = state.camera.center[1].clamp(0.0, 1.0);
                }
            }

            WindowEvent::MouseInput {
                button,
                state: btn_state,
                ..
            } => {
                match (button, btn_state) {
                    (MouseButton::Left, ElementState::Pressed) => {
                        state.lmb_down = true;
                        state.lmb_egui = resp.consumed;
                        if !resp.consumed {
                            match state.tool {
                                ui::Tool::Pan => {
                                    state.lmb_panning = true;
                                    state.pan_start_px = state.cursor_px;
                                    state.pan_start_center = state.camera.center;
                                }
                                ui::Tool::ZoomIn => {
                                    let sz = state.camera.zoom_factor * state.fit_zoom;
                                    let cw = screen_to_world(
                                        state.cursor_px,
                                        window.inner_size(),
                                        &state.camera,
                                        state.sim.world_aspect(),
                                        sz,
                                    );
                                    apply_zoom(&mut state.camera, cw, 1.5);
                                }
                                ui::Tool::ZoomOut => {
                                    let sz = state.camera.zoom_factor * state.fit_zoom;
                                    let cw = screen_to_world(
                                        state.cursor_px,
                                        window.inner_size(),
                                        &state.camera,
                                        state.sim.world_aspect(),
                                        sz,
                                    );
                                    apply_zoom(&mut state.camera, cw, 1.0 / 1.5);
                                }
                                _ => {} // attract / repel / spawn handled each frame in RedrawRequested
                            }
                        }
                    }
                    (MouseButton::Left, ElementState::Released) => {
                        state.lmb_down = false;
                        state.lmb_egui = false;
                        state.lmb_panning = false;
                        // If MMB is still panning, re-anchor so the transition is smooth.
                        if state.mmb_panning {
                            state.pan_start_px = state.cursor_px;
                            state.pan_start_center = state.camera.center;
                        }
                    }
                    // Middle mouse always pans, regardless of selected tool or egui focus.
                    (MouseButton::Middle, ElementState::Pressed) => {
                        state.mmb_panning = true;
                        state.pan_start_px = state.cursor_px;
                        state.pan_start_center = state.camera.center;
                    }
                    (MouseButton::Middle, ElementState::Released) => {
                        state.mmb_panning = false;
                        if state.lmb_panning {
                            state.pan_start_px = state.cursor_px;
                            state.pan_start_center = state.camera.center;
                        }
                    }
                    _ => {}
                }
            }

            // Scroll wheel always zooms, centered on the cursor.
            WindowEvent::MouseWheel { delta, .. } => {
                if !resp.consumed {
                    let lines = match delta {
                        MouseScrollDelta::LineDelta(_, y) => y,
                        MouseScrollDelta::PixelDelta(pos) => pos.y as f32 * 0.02,
                    };
                    if lines.abs() > 0.001 {
                        let factor = 1.15_f32.powf(lines);
                        let sz = state.camera.zoom_factor * state.fit_zoom;
                        let cw = screen_to_world(
                            state.cursor_px,
                            window.inner_size(),
                            &state.camera,
                            state.sim.world_aspect(),
                            sz,
                        );
                        apply_zoom(&mut state.camera, cw, factor);
                    }
                }
            }

            // Keyboard shortcuts: arrows = pan, +/- = zoom, 0 = reset view.
            WindowEvent::KeyboardInput {
                event:
                    KeyEvent {
                        logical_key,
                        state: ElementState::Pressed,
                        ..
                    },
                ..
            } => {
                if !resp.consumed {
                    // F11 toggles borderless fullscreen regardless of other key state
                    if logical_key == Key::Named(NamedKey::F11) {
                        window.set_fullscreen(if window.fullscreen().is_some() {
                            None
                        } else {
                            Some(Fullscreen::Borderless(None))
                        });
                    }

                    let step = 0.1 / (state.camera.zoom_factor * state.fit_zoom);
                    match logical_key {
                        Key::Named(NamedKey::ArrowLeft) => {
                            state.camera.center[0] = (state.camera.center[0] - step).max(0.0);
                        }
                        Key::Named(NamedKey::ArrowRight) => {
                            state.camera.center[0] = (state.camera.center[0] + step).min(1.0);
                        }
                        Key::Named(NamedKey::ArrowUp) => {
                            state.camera.center[1] = (state.camera.center[1] + step).min(1.0);
                        }
                        Key::Named(NamedKey::ArrowDown) => {
                            state.camera.center[1] = (state.camera.center[1] - step).max(0.0);
                        }
                        Key::Named(NamedKey::Space) => {
                            state.sim.paused = !state.sim.paused;
                        }
                        Key::Character(ref c) => {
                            let vp = window.inner_size();
                            let mid = PhysicalPosition::new(
                                vp.width as f64 / 2.0,
                                vp.height as f64 / 2.0,
                            );
                            match c.as_str() {
                                "=" | "+" => {
                                    let sz = state.camera.zoom_factor * state.fit_zoom;
                                    let cw = screen_to_world(
                                        mid,
                                        vp,
                                        &state.camera,
                                        state.sim.world_aspect(),
                                        sz,
                                    );
                                    apply_zoom(&mut state.camera, cw, 1.5);
                                }
                                "-" => {
                                    let sz = state.camera.zoom_factor * state.fit_zoom;
                                    let cw = screen_to_world(
                                        mid,
                                        vp,
                                        &state.camera,
                                        state.sim.world_aspect(),
                                        sz,
                                    );
                                    apply_zoom(&mut state.camera, cw, 1.0 / 1.5);
                                }
                                "0" => state.camera = Camera::default_view(),
                                "r" => state.sim.respawn(state.renderer.queue()),
                                "s" => state.pending_screenshot = true,
                                _ => {}
                            }
                        }
                        _ => {}
                    }
                }
            }

            WindowEvent::RedrawRequested => {
                let dt = state.last_frame.elapsed().as_secs_f32().min(0.05);
                state.last_frame = Instant::now();

                state.frame_times.push_back(dt);
                if state.frame_times.len() > 120 {
                    state.frame_times.pop_front();
                }

                // --- Build egui ---
                let raw_input = state.egui_state.take_egui_input(&window);

                // Explicitly split field borrows so the closure can see sim + frame_times
                // while egui_ctx is also borrowed.
                let cam_center = state.camera.center;
                let shader_zoom = state.camera.zoom_factor * state.fit_zoom;
                let world_aspect = state.sim.world_aspect();

                let (full_output, ui_resp, bench_resp, should_reset_view, take_screenshot) = {
                    let egui_ctx = &state.egui_ctx;
                    let sim = &mut state.sim;
                    let frame_times = &state.frame_times;
                    let tool = &mut state.tool;
                    let tool_range = &mut state.tool_range;
                    let mouse_strength = &mut state.mouse_strength;
                    let spawn_species = &mut state.spawn_species;
                    let spawn_rate = &mut state.spawn_rate;
                    let n_species = sim.species_count;
                    let border_mode = sim.border_mode;
                    let preset_library = &state.preset_library;
                    let selected_preset = &mut state.selected_preset;
                    let benchmark = &mut state.benchmark;
                    let quick_bench = &state.quick_bench;
                    let vsync = state.vsync;
                    let vsync_available = state.renderer.vsync_toggle_available();
                    let mut ui_r = ui::UiResponse::default();
                    let mut bench_r = ui::BenchmarkPanelResponse {
                        start: false,
                        export_csv: false,
                        start_quick: false,
                        vsync: None,
                    };
                    let mut reset_view = false;
                    let mut take_screenshot = false;
                    let out = egui_ctx.run(raw_input, |ctx| {
                        ui_r = ui::draw_ui(ctx, sim, preset_library, selected_preset);
                        bench_r = ui::draw_perf_overlay(
                            ctx,
                            frame_times,
                            sim,
                            quick_bench,
                            benchmark,
                            vsync,
                            vsync_available,
                        );
                        let (rv, ss, toolbar_rect) = ui::draw_toolbar(ctx, tool);
                        reset_view = rv;
                        take_screenshot = ss;
                        ui::draw_tool_options(
                            ctx,
                            *tool,
                            toolbar_rect,
                            tool_range,
                            mouse_strength,
                            spawn_species,
                            spawn_rate,
                            n_species,
                            &sim.palette,
                        );
                        ui::draw_world_border(
                            ctx,
                            cam_center,
                            world_aspect,
                            shader_zoom,
                            border_mode,
                        );
                        ui::draw_cursor_indicator(ctx, *tool, *tool_range, shader_zoom);
                    });
                    (out, ui_r, bench_r, reset_view, take_screenshot)
                };

                if ui_resp.respawn {
                    state.sim.respawn(state.renderer.queue());
                }
                if ui_resp.randomize {
                    state.sim.randomize_attraction();
                }
                if ui_resp.randomize_palette {
                    state.sim.randomize_palette();
                }
                if ui_resp.palette_changed || ui_resp.randomize_palette {
                    state.renderer.update_palette(&state.sim.palette);
                }
                if should_reset_view {
                    state.camera = Camera::default_view();
                }
                let take_screenshot = take_screenshot || std::mem::take(&mut state.pending_screenshot);
                if take_screenshot {
                    let dir = std::path::Path::new(config::SCREENSHOTS_DIR);
                    let _ = std::fs::create_dir_all(dir);
                    let secs = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_secs();
                    let path = dir.join(format!("screenshot_{secs}.png"));
                    let png =
                        state
                            .renderer
                            .capture_png(&state.sim, cam_center, shader_zoom);
                    if let Err(e) = std::fs::write(&path, &png) {
                        log::warn!("Screenshot failed: {e}");
                    }
                }
                if ui_resp.match_win {
                    let sz = window.inner_size();
                    state.sim.world_width = sz.width as f32;
                    state.sim.world_height = sz.height as f32;
                    state.fit_zoom = compute_fit_zoom(
                        state.sim.world_width,
                        state.sim.world_height,
                        sz.width,
                        sz.height,
                    );
                }
                if ui_resp.apply_preset
                    && let Some(preset) = state.preset_library.get(state.selected_preset).cloned()
                {
                    state.sim.apply_preset(state.renderer.queue(), &preset);
                    state.fit_zoom = compute_fit_zoom(
                        state.sim.world_width,
                        state.sim.world_height,
                        window.inner_size().width,
                        window.inner_size().height,
                    );
                }
                if ui_resp.import_preset
                    && let Some(path) = rfd::FileDialog::new()
                        .add_filter("TOML preset", &["toml"])
                        .pick_file()
                {
                    match config::load_preset_file(&path) {
                        Ok(mut preset) => {
                            if (preset.name == "exported" || preset.name.is_empty())
                                && let Some(stem) = path.file_stem().and_then(|s| s.to_str())
                            {
                                preset.name = stem.to_string();
                            }
                            state.preset_library.push(preset);
                            state.selected_preset = state.preset_library.len() - 1;
                        }
                        Err(e) => log::warn!("Import failed: {e}"),
                    }
                }
                if ui_resp.export_preset
                    && let Some(path) = rfd::FileDialog::new()
                        .add_filter("TOML preset", &["toml"])
                        .set_file_name("preset.toml")
                        .save_file()
                {
                    let name = path
                        .file_stem()
                        .and_then(|s| s.to_str())
                        .unwrap_or("exported");
                    if let Err(e) =
                        config::save_preset_file(&state.sim.to_preset(name), &path)
                    {
                        log::warn!("Export failed: {e}");
                    } else {
                        let png =
                            state
                                .renderer
                                .capture_png(&state.sim, cam_center, shader_zoom);
                        let thumb_path = path.with_extension("png");
                        if let Err(e) = std::fs::write(&thumb_path, &png) {
                            log::warn!("Thumbnail save failed: {e}");
                        }
                    }
                }

                // Global vsync toggle
                if let Some(new_vsync) = bench_resp.vsync {
                    state.vsync = new_vsync;
                    if !state.benchmark.is_running() {
                        state.renderer.set_vsync(new_vsync);
                    }
                }

                // Benchmark
                if bench_resp.start {
                    let sz = window.inner_size();
                    if state.benchmark.vsync_off {
                        state.renderer.set_vsync(false);
                    }
                    let action = state.benchmark.start(sz.width, sz.height);
                    Self::handle_benchmark_action(
                        &mut state.sim,
                        &state.renderer,
                        action,
                        &state.benchmark,
                    );
                }
                if bench_resp.export_csv
                    && let Some(path) = rfd::FileDialog::new()
                        .add_filter("CSV", &["csv"])
                        .set_file_name("benchmark.csv")
                        .save_file()
                    && let Err(e) = state.benchmark.write_csv(&path)
                {
                    log::warn!("Benchmark CSV export failed: {e}");
                }
                if state.benchmark.is_running() {
                    let action = state.benchmark.advance(dt);
                    if matches!(action, benchmark::BenchmarkAction::Done) {
                        state.renderer.set_vsync(state.vsync);
                    }
                    Self::handle_benchmark_action(
                        &mut state.sim,
                        &state.renderer,
                        action,
                        &state.benchmark,
                    );
                }

                // Quick bench
                if bench_resp.start_quick {
                    state.quick_bench.start(state.sim.particle_count_gpu());
                }
                if state.quick_bench.is_running() {
                    state
                        .quick_bench
                        .advance(dt, state.sim.particle_count_gpu());
                }

                // Apply active tool effects to sim mouse state before dispatch.
                let vp = window.inner_size();
                let world = screen_to_world(
                    state.cursor_px,
                    vp,
                    &state.camera,
                    world_aspect,
                    shader_zoom,
                );
                state.sim.mouse_x = world[0];
                state.sim.mouse_y = world[1];
                state.sim.mouse_range = state.tool_range;
                let sim_lmb = state.lmb_down && !state.lmb_egui;
                state.sim.mouse_strength = match state.tool {
                    ui::Tool::Attract if sim_lmb => state.mouse_strength,
                    ui::Tool::Repel if sim_lmb => -state.mouse_strength,
                    _ => 0.0,
                };
                if matches!(state.tool, ui::Tool::Spawn) && sim_lmb {
                    let queue = state.renderer.queue();
                    let spawn_species = state.spawn_species;
                    let spawn_rate = state.spawn_rate;
                    state.sim.spawn_particles(
                        queue,
                        world,
                        state.tool_range,
                        spawn_species,
                        world_aspect,
                        spawn_rate,
                    );
                }

                let egui::FullOutput {
                    platform_output,
                    textures_delta,
                    shapes,
                    pixels_per_point,
                    ..
                } = full_output;

                // Read egui's cursor intent before the move so we can decide
                // whether to override it with the tool cursor below.
                let egui_cursor = platform_output.cursor_icon;
                state
                    .egui_state
                    .handle_platform_output(&window, platform_output);

                // When egui wants Default (non-interactive area, panel background, canvas):
                // explicitly set either the tool cursor or Default. The explicit reset is
                // required because egui-winit deduplicates cursor calls — if egui keeps
                // emitting Default frame-over-frame, handle_platform_output skips
                // window.set_cursor(), leaving the tool cursor from the previous frame stuck.
                if egui_cursor == egui::CursorIcon::Default {
                    let panning = state.lmb_panning || state.mmb_panning;
                    let cursor = if state.egui_ctx.is_pointer_over_area() {
                        CursorIcon::Default
                    } else {
                        tool_cursor(state.tool, panning)
                    };
                    window.set_cursor(cursor);
                }
                let paint_jobs = state.egui_ctx.tessellate(shapes, pixels_per_point);

                match state.renderer.render(
                    &paint_jobs,
                    &textures_delta,
                    pixels_per_point,
                    &state.sim,
                    dt,
                    cam_center,
                    shader_zoom,
                ) {
                    Ok(()) => {}
                    Err(wgpu::SurfaceError::Lost | wgpu::SurfaceError::Outdated) => {
                        state.renderer.resize(window.inner_size());
                    }
                    Err(wgpu::SurfaceError::OutOfMemory) => event_loop.exit(),
                    Err(wgpu::SurfaceError::Timeout) => log::warn!("Surface timeout"),
                    Err(e) => log::error!("Surface error: {e:?}"),
                }
            }

            _ => {}
        }
    }

    fn about_to_wait(&mut self, _event_loop: &ActiveEventLoop) {
        if let Some(state) = self.state.as_ref() {
            state.window.request_redraw();
        }
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn tool_cursor(tool: ui::Tool, panning: bool) -> CursorIcon {
    match tool {
        ui::Tool::Pan => {
            if panning {
                CursorIcon::Grabbing
            } else {
                CursorIcon::Grab
            }
        }
        ui::Tool::ZoomIn => CursorIcon::ZoomIn,
        ui::Tool::ZoomOut => CursorIcon::ZoomOut,
        ui::Tool::Attract | ui::Tool::Repel | ui::Tool::Spawn => CursorIcon::Crosshair,
    }
}

// ── Private helpers ───────────────────────────────────────────────────────────

impl AppHandler {
    fn handle_benchmark_action(
        sim: &mut SimulationState,
        renderer: &WgpuState,
        action: benchmark::BenchmarkAction,
        runner: &benchmark::BenchmarkRunner,
    ) {
        if let benchmark::BenchmarkAction::LoadCombo(combo) = action {
            let preset = benchmark::BenchmarkRunner::combo_preset(combo);
            sim.apply_preset(renderer.queue(), &preset);
            // Override world size to match benchmark viewport for consistent results
            sim.world_width = runner.vp_width as f32;
            sim.world_height = runner.vp_height as f32;
        }
    }
}
