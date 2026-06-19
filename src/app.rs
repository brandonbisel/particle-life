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

use crate::{
    benchmark, cli, config, icon, renderer, renderer::WgpuState, simulation::SimulationState, ui,
};

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

/// Convert a screen-space cursor position to world-space coordinates (normalized \[0,1\]²).
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

/// Zoom `camera` by `factor` centered on the current cursor, respecting world aspect ratio.
fn zoom_at_cursor(
    camera: &mut Camera,
    cursor_px: PhysicalPosition<f64>,
    viewport: winit::dpi::PhysicalSize<u32>,
    world_aspect: f32,
    fit_zoom: f32,
    factor: f32,
) {
    let sz = camera.zoom_factor * fit_zoom;
    let cw = screen_to_world(cursor_px, viewport, camera, world_aspect, sz);
    apply_zoom(camera, cw, factor);
}

// ── AppState ──────────────────────────────────────────────────────────────────

/// Top-level winit [`ApplicationHandler`].
///
/// Holds [`AppState`] behind an `Option` because the state cannot be created
/// until the first [`resumed`](ApplicationHandler::resumed) event (required by
/// Wayland, which only provides a valid window handle after that point).
pub struct AppHandler {
    state: Option<AppState>,
    cli: cli::CliArgs,
}

impl AppHandler {
    pub fn new(cli: cli::CliArgs) -> Self {
        Self { state: None, cli }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum AutoBenchKind {
    Full,
    Capacity,
}

struct PanState {
    lmb_panning: bool,
    mmb_panning: bool,
    start_px: PhysicalPosition<f64>,
    start_center: [f32; 2],
}

impl Default for PanState {
    fn default() -> Self {
        Self {
            lmb_panning: false,
            mmb_panning: false,
            start_px: PhysicalPosition::new(0.0, 0.0),
            start_center: [0.5, 0.5],
        }
    }
}

struct ToolState {
    active: ui::Tool,
    range: f32,
    strength: f32,
    spawn_species: Option<usize>,
    spawn_rate: u32,
}

impl Default for ToolState {
    fn default() -> Self {
        Self {
            active: ui::Tool::Pan,
            range: 0.1,
            strength: 2.0,
            spawn_species: None,
            spawn_rate: 50,
        }
    }
}

struct GalleryState {
    presets: Vec<config::Preset>,
    selected: usize,
    thumbnails: Vec<Option<egui::TextureHandle>>,
    open: bool,
}

struct BenchmarkState {
    runner: benchmark::BenchmarkRunner,
    quick: benchmark::QuickBench,
    capacity: benchmark::CapacityBench,
    vsync: bool,
    vsync_override: bool,
    auto_kind: Option<AutoBenchKind>,
    output: Option<std::path::PathBuf>,
}

impl Default for BenchmarkState {
    fn default() -> Self {
        Self {
            runner: benchmark::BenchmarkRunner::new(),
            quick: benchmark::QuickBench::new(),
            capacity: benchmark::CapacityBench::new(),
            vsync: true,
            vsync_override: false,
            auto_kind: None,
            output: None,
        }
    }
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
    fit_zoom: f32,

    // Toolbar + spawn tool
    tool: ToolState,

    // Presets + gallery
    gallery: GalleryState,

    // Per-species particle counts (CPU-tracked; reset on respawn/apply_preset, updated on spawn)
    per_species_count: Vec<usize>,

    // Benchmark
    bench: BenchmarkState,

    // Appearance
    appearance: config::AppearanceConfig,
    themes: Vec<config::ThemeDef>,
    os_dark: bool,

    // UI overlay state
    matrix_popped_out: bool,
    appearance_open: bool,
    about_open: bool,

    // Playback controls (session-only; not persisted in presets)
    /// Multiplier applied to `dt` before each physics dispatch; 1.0 = real-time, 0.05 = 5%.
    time_scale: f32,

    // --capture mode: run the sim for capture_delay seconds, save a screenshot, then exit.
    capture_path: Option<std::path::PathBuf>,
    capture_elapsed: f32,
    capture_delay: f32,

    // Pending one-shot actions triggered by keyboard shortcuts
    pending_screenshot: bool,

    // Mouse tracking
    cursor_px: PhysicalPosition<f64>,
    lmb_down: bool,
    lmb_egui: bool,

    // Pan drag state (LMB+Pan tool or MMB)
    pan: PanState,

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
        if self.cli.fullscreen {
            window.set_fullscreen(Some(Fullscreen::Borderless(None)));
        }

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
            renderer.tile_size(),
            renderer.pipeline_cache(),
            1000,
            6,
            world_width,
            world_height,
        );
        if let Some(ref p) = session {
            sim.apply_preset(renderer.queue(), p);
        }

        // CLI overrides (applied after session restore; later flags override earlier ones)
        if let Some(ref name) = self.cli.preset {
            let idx = name
                .parse::<usize>()
                .ok()
                .filter(|&i| i < preset_library.len())
                .or_else(|| {
                    preset_library
                        .iter()
                        .position(|p| p.name.eq_ignore_ascii_case(name))
                });
            if let Some(i) = idx {
                sim.apply_preset(renderer.queue(), &preset_library[i]);
            } else {
                log::warn!("--preset {name:?}: no matching preset found");
            }
        }
        if let Some((w, h)) = self.cli.world_size {
            sim.world_width = w as f32;
            sim.world_height = h as f32;
            sim.respawn(renderer.queue());
        }
        if let Some(n) = self.cli.particles {
            sim.particle_count = n.clamp(100, crate::simulation::MAX_PARTICLES);
            sim.respawn(renderer.queue());
        }
        if let Some(ref code) = self.cli.matrix.clone() {
            Self::apply_matrix_code(&mut sim, &renderer, code);
        }
        renderer.update_palette(&sim.palette, &sim.species_visible);
        let fit_zoom = compute_fit_zoom(sim.world_width, sim.world_height, size.width, size.height);
        let per_species_count = sim.species_counts();

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
        let preset_thumbnails = ui::load_preset_thumbnails(&preset_library, &egui_ctx);

        let appearance = config::load_appearance();
        let mut themes = config::bundled_themes();
        // User themes in ./themes/ override bundled themes of the same name.
        for user in config::load_themes_dir() {
            if let Some(existing) = themes.iter_mut().find(|t| t.name == user.name) {
                *existing = user;
            } else {
                themes.push(user);
            }
        }
        let os_dark = window
            .theme()
            .map(|t| t == winit::window::Theme::Dark)
            .unwrap_or(true);
        ui::apply_theme(
            &egui_ctx,
            &appearance.ui_theme,
            appearance.overlay_alpha,
            os_dark,
            &themes,
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
            tool: ToolState::default(),
            gallery: GalleryState {
                presets: preset_library,
                selected: 0,
                thumbnails: preset_thumbnails,
                open: false,
            },
            per_species_count,
            bench: BenchmarkState::default(),
            appearance,
            themes,
            os_dark,
            matrix_popped_out: false,
            appearance_open: false,
            about_open: false,
            time_scale: 1.0,
            capture_path: None,
            capture_elapsed: 0.0,
            capture_delay: 5.0,
            pending_screenshot: false,
            cursor_px: PhysicalPosition::new(0.0, 0.0),
            lmb_down: false,
            lmb_egui: false,
            pan: PanState::default(),
            window,
        });

        // Auto-benchmark mode: start immediately and exit when done.
        if self.cli.bench || self.cli.capacity_bench {
            let state = self.state.as_mut().unwrap();
            state.renderer.set_vsync(false);
            let sz = state.window.inner_size();
            if self.cli.bench {
                let action = state.bench.runner.start(sz.width, sz.height);
                Self::handle_benchmark_action(&mut state.sim, &state.renderer, action);
                state.bench.auto_kind = Some(AutoBenchKind::Full);
                state.bench.output = Some(
                    self.cli
                        .bench_output
                        .clone()
                        .unwrap_or_else(|| "bench_results.csv".into()),
                );
            } else {
                let action = state.bench.capacity.start(sz.width, sz.height);
                state.frame_times.clear();
                Self::handle_capacity_action(&mut state.sim, &state.renderer, action);
                state.bench.auto_kind = Some(AutoBenchKind::Capacity);
                state.bench.output = Some(
                    self.cli
                        .bench_output
                        .clone()
                        .unwrap_or_else(|| "capacity_results.csv".into()),
                );
            }
        }

        // Capture mode: record the path/delay and let the frame loop handle the countdown.
        if let Some(path) = self.cli.capture.clone() {
            let state = self.state.as_mut().unwrap();
            state.capture_path = Some(path);
            state.capture_delay = self.cli.capture_delay;
        }
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
                    s.renderer.save_pipeline_cache();
                    config::save_session(&s.sim.to_preset("session"));
                    config::save_appearance(&s.appearance);
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
                    s.renderer.save_pipeline_cache();
                    config::save_session(&s.sim.to_preset("session"));
                    config::save_appearance(&s.appearance);
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

                // If egui grabbed the pointer this frame (e.g. resize drag started),
                // cancel any world-panning that slipped through the press-time check.
                if state.pan.lmb_panning && state.egui_ctx.is_using_pointer() {
                    state.pan.lmb_panning = false;
                    state.lmb_egui = true;
                }

                if state.pan.lmb_panning || state.pan.mmb_panning {
                    let vp = window.inner_size();
                    let shader_zoom = state.camera.zoom_factor * state.fit_zoom;
                    let world_aspect = state.sim.world_aspect();
                    state.camera.center = [
                        state.pan.start_center[0]
                            - (position.x - state.pan.start_px.x) as f32
                                / (vp.height as f32 * world_aspect * shader_zoom),
                        state.pan.start_center[1]
                            + (position.y - state.pan.start_px.y) as f32
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
                        let egui_wants_mouse = resp.consumed
                            || state.egui_ctx.is_pointer_over_area()
                            || state.egui_ctx.is_using_pointer();
                        state.lmb_egui = egui_wants_mouse;
                        if !egui_wants_mouse {
                            match state.tool.active {
                                ui::Tool::Pan => {
                                    state.pan.lmb_panning = true;
                                    state.pan.start_px = state.cursor_px;
                                    state.pan.start_center = state.camera.center;
                                }
                                ui::Tool::ZoomIn => {
                                    zoom_at_cursor(
                                        &mut state.camera,
                                        state.cursor_px,
                                        window.inner_size(),
                                        state.sim.world_aspect(),
                                        state.fit_zoom,
                                        1.5,
                                    );
                                }
                                ui::Tool::ZoomOut => {
                                    zoom_at_cursor(
                                        &mut state.camera,
                                        state.cursor_px,
                                        window.inner_size(),
                                        state.sim.world_aspect(),
                                        state.fit_zoom,
                                        1.0 / 1.5,
                                    );
                                }
                                _ => {} // attract / repel / spawn handled each frame in RedrawRequested
                            }
                        }
                    }
                    (MouseButton::Left, ElementState::Released) => {
                        state.lmb_down = false;
                        state.lmb_egui = false;
                        state.pan.lmb_panning = false;
                        // If MMB is still panning, re-anchor so the transition is smooth.
                        if state.pan.mmb_panning {
                            state.pan.start_px = state.cursor_px;
                            state.pan.start_center = state.camera.center;
                        }
                    }
                    // Middle mouse always pans, regardless of selected tool or egui focus.
                    (MouseButton::Middle, ElementState::Pressed) => {
                        state.pan.mmb_panning = true;
                        state.pan.start_px = state.cursor_px;
                        state.pan.start_center = state.camera.center;
                    }
                    (MouseButton::Middle, ElementState::Released) => {
                        state.pan.mmb_panning = false;
                        if state.pan.lmb_panning {
                            state.pan.start_px = state.cursor_px;
                            state.pan.start_center = state.camera.center;
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
                        zoom_at_cursor(
                            &mut state.camera,
                            state.cursor_px,
                            window.inner_size(),
                            state.sim.world_aspect(),
                            state.fit_zoom,
                            1.15_f32.powf(lines),
                        );
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
                            if state.sim.paused {
                                state.sim.step_requested = true;
                            } else {
                                state.camera.center[0] = (state.camera.center[0] + step).min(1.0);
                            }
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
                                    zoom_at_cursor(
                                        &mut state.camera,
                                        mid,
                                        vp,
                                        state.sim.world_aspect(),
                                        state.fit_zoom,
                                        1.5,
                                    );
                                }
                                "-" => {
                                    zoom_at_cursor(
                                        &mut state.camera,
                                        mid,
                                        vp,
                                        state.sim.world_aspect(),
                                        state.fit_zoom,
                                        1.0 / 1.5,
                                    );
                                }
                                "0" => state.camera = Camera::default_view(),
                                "r" => {
                                    state.sim.respawn(state.renderer.queue());
                                    state.per_species_count = state.sim.species_counts();
                                }
                                "s" => state.pending_screenshot = true,
                                _ => {}
                            }
                        }
                        _ => {}
                    }
                }
            }

            WindowEvent::ThemeChanged(t) => {
                state.os_dark = t == winit::window::Theme::Dark;
                ui::apply_theme(
                    &state.egui_ctx,
                    &state.appearance.ui_theme,
                    state.appearance.overlay_alpha,
                    state.os_dark,
                    &state.themes,
                );
            }

            WindowEvent::RedrawRequested => {
                let (raw_dt, dt) = state.tick_frame();
                let perf_active = state.sim.auto_density
                    && state.sim.perf_auto
                    && !state.bench.runner.is_running();
                state.tick_perf_auto(perf_active, raw_dt, window.inner_size());

                // --- Build egui ---
                let raw_input = state.egui_state.take_egui_input(&window);

                // Explicitly split field borrows so the closure can see sim + frame_times
                // while egui_ctx is also borrowed.
                let cam_center = state.camera.center;
                let shader_zoom = state.camera.zoom_factor * state.fit_zoom;
                let world_aspect = state.sim.world_aspect();

                let bench_running =
                    state.bench.runner.is_running() || state.bench.capacity.is_running();

                let (full_output, ui_resp, bench_resp, should_reset_view, take_screenshot) = {
                    let egui_ctx = &state.egui_ctx;
                    let sim = &mut state.sim;
                    let frame_times = &state.frame_times;
                    let tool = &mut state.tool.active;
                    let tool_range = &mut state.tool.range;
                    let mouse_strength = &mut state.tool.strength;
                    let spawn_species = &mut state.tool.spawn_species;
                    let spawn_rate = &mut state.tool.spawn_rate;
                    let n_species = sim.species_count;
                    let border_mode = sim.border_mode;
                    let preset_library = &state.gallery.presets;
                    let selected_preset = &mut state.gallery.selected;
                    let benchmark = &mut state.bench.runner;
                    let quick_bench = &state.bench.quick;
                    let capacity_bench = &mut state.bench.capacity;
                    let vsync = state.bench.vsync;
                    let vsync_managed = state.bench.vsync_override;
                    let vsync_available = state.renderer.vsync_toggle_available();
                    let preset_thumbnails = &state.gallery.thumbnails;
                    let gallery_open = &mut state.gallery.open;
                    let appearance_open = &mut state.appearance_open;
                    let about_open = &mut state.about_open;
                    let matrix_popped_out = &mut state.matrix_popped_out;
                    let per_species_count = &state.per_species_count;
                    let appearance = &mut state.appearance;
                    let themes = &state.themes;
                    let time_scale = &mut state.time_scale;
                    let os_dark = state.os_dark;
                    let mut ui_r = ui::UiResponse::default();
                    let mut bench_r = ui::BenchmarkPanelResponse::default();
                    let mut reset_view = false;
                    let mut take_screenshot = false;
                    let out = egui_ctx.run(raw_input, |ctx| {
                        ui_r = ui::draw_ui(ctx, sim, bench_running, matrix_popped_out, time_scale);
                        if *matrix_popped_out && ui::draw_matrix_window(ctx, sim, matrix_popped_out)
                        {
                            ui_r.randomize = true;
                        }
                        let appearance_resp = ui::draw_appearance_overlay(
                            ctx,
                            sim,
                            appearance,
                            os_dark,
                            appearance_open,
                            themes,
                        );
                        ui_r.palette_changed |= appearance_resp.palette_changed;
                        ui_r.randomize_palette |= appearance_resp.randomize_palette;
                        ui_r.appearance_changed |= appearance_resp.appearance_changed;
                        bench_r = ui::draw_perf_overlay(
                            ctx,
                            frame_times,
                            sim,
                            quick_bench,
                            benchmark,
                            capacity_bench,
                            vsync,
                            vsync_managed,
                            vsync_available,
                            per_species_count,
                        );
                        let (rv, ss, tg, ta, tab, toolbar_rect) = ui::draw_toolbar(
                            ctx,
                            tool,
                            *gallery_open,
                            *appearance_open,
                            *about_open,
                            bench_running,
                        );
                        reset_view = rv;
                        take_screenshot = ss;
                        if tg {
                            *gallery_open = !*gallery_open;
                        }
                        if ta {
                            *appearance_open = !*appearance_open;
                        }
                        if tab {
                            *about_open = !*about_open;
                        }
                        ui::draw_about_window(ctx, about_open);
                        if *gallery_open
                            && !bench_running
                            && ui::draw_gallery(
                                ctx,
                                preset_library,
                                preset_thumbnails,
                                selected_preset,
                                gallery_open,
                            )
                        {
                            ui_r.apply_preset = true;
                        }
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

                state.handle_ui_responses(
                    ui_resp,
                    &window,
                    cam_center,
                    shader_zoom,
                    should_reset_view,
                    take_screenshot,
                );
                if state.handle_benchmark_responses(bench_resp, &window, raw_dt) {
                    event_loop.exit();
                    return;
                }
                state.handle_tool_effects(
                    window.inner_size(),
                    world_aspect,
                    shader_zoom,
                    bench_running,
                );

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
                    let panning = state.pan.lmb_panning || state.pan.mmb_panning;
                    let cursor = if state.egui_ctx.is_pointer_over_area() {
                        CursorIcon::Default
                    } else {
                        tool_cursor(state.tool.active, panning)
                    };
                    window.set_cursor(cursor);
                }
                let paint_jobs = state.egui_ctx.tessellate(shapes, pixels_per_point);

                // Frame stepping: temporarily unpause for exactly one dispatch.
                let stepping = state.sim.paused && state.sim.step_requested;
                if stepping {
                    state.sim.paused = false;
                    state.sim.step_requested = false;
                }

                let physics_dt = dt * state.time_scale;
                match state.renderer.render(
                    &paint_jobs,
                    &textures_delta,
                    pixels_per_point,
                    &state.sim,
                    physics_dt,
                    cam_center,
                    shader_zoom,
                    renderer::bg_color_from_srgb(state.appearance.bg_color),
                ) {
                    Ok(()) => {}
                    Err(wgpu::SurfaceError::Lost | wgpu::SurfaceError::Outdated) => {
                        state.renderer.resize(window.inner_size());
                    }
                    Err(wgpu::SurfaceError::OutOfMemory) => event_loop.exit(),
                    Err(wgpu::SurfaceError::Timeout) => log::warn!("Surface timeout"),
                    Err(e) => log::error!("Surface error: {e:?}"),
                }

                if stepping {
                    state.sim.paused = true;
                }

                // Capture mode: count down and take screenshot when delay expires.
                if state.capture_path.is_some() {
                    state.capture_elapsed += raw_dt;
                    if state.capture_elapsed >= state.capture_delay {
                        let path = state.capture_path.take().unwrap();
                        let png = state.renderer.capture_png(
                            &state.sim,
                            cam_center,
                            shader_zoom,
                            renderer::bg_color_from_srgb(state.appearance.bg_color),
                        );
                        match std::fs::write(&path, &png) {
                            Ok(()) => println!("Captured screenshot to {}", path.display()),
                            Err(e) => log::error!("Capture failed: {e}"),
                        }
                        event_loop.exit();
                    }
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

// ── AppState helpers ──────────────────────────────────────────────────────────

impl AppState {
    /// Sample the current frame time, update the rolling history, and return `(raw_dt, dt)`.
    ///
    /// `dt` is physics-capped at 50 ms to prevent particles from tunnelling after focus loss.
    fn tick_frame(&mut self) -> (f32, f32) {
        let raw_dt = self.last_frame.elapsed().as_secs_f32();
        self.last_frame = std::time::Instant::now();
        self.frame_times.push_back(raw_dt);
        if self.frame_times.len() > 120 {
            self.frame_times.pop_front();
        }
        (raw_dt, raw_dt.min(0.05))
    }

    /// Tick the auto-performance world-size controller and vsync override.
    ///
    /// Must be called after `tick_frame` so `frame_times` is up-to-date.
    fn tick_perf_auto(
        &mut self,
        perf_active: bool,
        raw_dt: f32,
        viewport: winit::dpi::PhysicalSize<u32>,
    ) {
        if perf_active {
            let n = self.frame_times.len();
            let avg_fps = if n > 0 {
                n as f32 / self.frame_times.iter().sum::<f32>()
            } else {
                0.0
            };
            self.sim.perf_world_adjust(avg_fps, raw_dt);
            let vp_aspect = viewport.width as f32 / viewport.height as f32;
            self.sim.world_width = self.sim.world_height * vp_aspect;
            self.fit_zoom = compute_fit_zoom(
                self.sim.world_width,
                self.sim.world_height,
                viewport.width,
                viewport.height,
            );
        }
        // Auto-performance forces vsync off so the FPS controller sees real GPU load.
        if perf_active != self.bench.vsync_override {
            self.bench.vsync_override = perf_active;
            if !self.bench.runner.is_running() {
                self.renderer.set_vsync(self.bench.vsync && !perf_active);
            }
        }
    }

    /// Apply all `UiResponse` side-effects (respawn, presets, screenshots, appearance, …).
    ///
    /// `cam_center` and `shader_zoom` are used for screenshot/export-preset capture;
    /// they must be computed before the egui frame so they reflect the camera at render time.
    fn handle_ui_responses(
        &mut self,
        resp: ui::UiResponse,
        window: &Window,
        cam_center: [f32; 2],
        shader_zoom: f32,
        should_reset_view: bool,
        take_screenshot_from_toolbar: bool,
    ) {
        if resp.toggle_gallery {
            self.gallery.open = !self.gallery.open;
        }
        if resp.matrix_pop_out_toggled {
            self.matrix_popped_out = !self.matrix_popped_out;
        }
        if resp.respawn {
            if self.sim.auto_density {
                self.sim.auto_world_size();
                self.fit_zoom = compute_fit_zoom(
                    self.sim.world_width,
                    self.sim.world_height,
                    window.inner_size().width,
                    window.inner_size().height,
                );
            }
            self.sim.respawn(self.renderer.queue());
            self.frame_times.clear();
            self.per_species_count = self.sim.species_counts();
        }
        if resp.randomize {
            self.sim.randomize_attraction();
        }
        if resp.randomize_palette {
            self.sim.randomize_palette();
        }
        if resp.palette_changed || resp.randomize_palette {
            self.renderer
                .update_palette(&self.sim.palette, &self.sim.species_visible);
        }
        if should_reset_view {
            self.camera = Camera::default_view();
        }
        let take_screenshot =
            take_screenshot_from_toolbar || std::mem::take(&mut self.pending_screenshot);
        if take_screenshot {
            let dir = std::path::Path::new(config::SCREENSHOTS_DIR);
            let _ = std::fs::create_dir_all(dir);
            let secs = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs();
            let path = dir.join(format!("screenshot_{secs}.png"));
            let png = self.renderer.capture_png(
                &self.sim,
                cam_center,
                shader_zoom,
                renderer::bg_color_from_srgb(self.appearance.bg_color),
            );
            if let Err(e) = std::fs::write(&path, &png) {
                log::warn!("Screenshot failed: {e}");
            }
        }
        if resp.match_win {
            let sz = window.inner_size();
            self.sim.world_width = sz.width as f32;
            self.sim.world_height = sz.height as f32;
            self.sim.auto_density = false;
            self.fit_zoom = compute_fit_zoom(
                self.sim.world_width,
                self.sim.world_height,
                sz.width,
                sz.height,
            );
        }
        if resp.apply_preset
            && let Some(preset) = self.gallery.presets.get(self.gallery.selected).cloned()
        {
            let preset_density =
                preset.particle_count as f32 / (preset.world_width * preset.world_height);
            self.sim.apply_preset(self.renderer.queue(), &preset);
            let sz = window.inner_size();
            let aspect = sz.width as f32 / sz.height as f32;
            let window_area = sz.width as f32 * sz.height as f32;
            let max_area = crate::simulation::MAX_PARTICLES as f32 / preset_density.max(1e-10);
            let target_area = window_area.min(max_area);
            let world_h = (target_area / aspect).sqrt();
            self.sim.world_height = world_h;
            self.sim.world_width = world_h * aspect;
            self.sim.particle_count =
                ((preset_density * self.sim.world_width * self.sim.world_height) as usize)
                    .clamp(100, crate::simulation::MAX_PARTICLES);
            self.sim.respawn(self.renderer.queue());
            self.frame_times.clear();
            self.fit_zoom = compute_fit_zoom(
                self.sim.world_width,
                self.sim.world_height,
                sz.width,
                sz.height,
            );
            self.camera = Camera::default_view();
            self.per_species_count = self.sim.species_counts();
            self.renderer
                .update_palette(&self.sim.palette, &self.sim.species_visible);
        }
        if resp.import_preset
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
                    let thumb = ui::load_preset_thumbnail(&preset.name, &self.egui_ctx);
                    self.gallery.presets.push(preset);
                    self.gallery.thumbnails.push(thumb);
                    self.gallery.selected = self.gallery.presets.len() - 1;
                }
                Err(e) => log::warn!("Import failed: {e}"),
            }
        }
        if resp.export_preset
            && let Some(path) = rfd::FileDialog::new()
                .add_filter("TOML preset", &["toml"])
                .set_file_name("preset.toml")
                .save_file()
        {
            let name = path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("exported");
            if let Err(e) = config::save_preset_file(&self.sim.to_preset(name), &path) {
                log::warn!("Export failed: {e}");
            } else {
                let png = self.renderer.capture_png(
                    &self.sim,
                    cam_center,
                    shader_zoom,
                    renderer::bg_color_from_srgb(self.appearance.bg_color),
                );
                let thumb_path = path.with_extension("png");
                if let Err(e) = std::fs::write(&thumb_path, &png) {
                    log::warn!("Thumbnail save failed: {e}");
                }
            }
        }
        if resp.paste_share_code
            && let Some(text) = self.egui_state.clipboard_text()
        {
            self.egui_ctx.data_mut(|d| {
                d.insert_temp(egui::Id::new("share_code_paste_buf"), text);
            });
        }
        if let Some(code) = resp.apply_share_code
            && AppHandler::apply_matrix_code(&mut self.sim, &self.renderer, code.as_str())
        {
            self.frame_times.clear();
            self.per_species_count = self.sim.species_counts();
            self.renderer
                .update_palette(&self.sim.palette, &self.sim.species_visible);
        }
        if resp.appearance_changed {
            ui::apply_theme(
                &self.egui_ctx,
                &self.appearance.ui_theme,
                self.appearance.overlay_alpha,
                self.os_dark,
                &self.themes,
            );
            config::save_appearance(&self.appearance);
        }
    }

    /// Tick benchmark state machines and handle bench panel responses.
    ///
    /// Returns `true` if the auto-bench run has completed and the app should exit.
    fn handle_benchmark_responses(
        &mut self,
        resp: ui::BenchmarkPanelResponse,
        window: &Window,
        raw_dt: f32,
    ) -> bool {
        if let Some(new_vsync) = resp.vsync {
            self.bench.vsync = new_vsync;
            if !self.bench.runner.is_running() {
                self.renderer.set_vsync(new_vsync);
            }
        }

        if resp.start {
            let sz = window.inner_size();
            if self.bench.runner.vsync_off {
                self.renderer.set_vsync(false);
            }
            let action = self.bench.runner.start(sz.width, sz.height);
            AppHandler::handle_benchmark_action(&mut self.sim, &self.renderer, action);
        }
        if resp.export_csv
            && let Some(path) = rfd::FileDialog::new()
                .add_filter("CSV", &["csv"])
                .set_file_name("benchmark.csv")
                .save_file()
            && let Err(e) = self.bench.runner.write_csv(&path)
        {
            log::warn!("Benchmark CSV export failed: {e}");
        }
        if self.bench.runner.is_running() {
            let action = self.bench.runner.advance(raw_dt);
            if matches!(action, benchmark::BenchmarkAction::Done) {
                self.renderer.set_vsync(self.bench.vsync);
                if self.bench.auto_kind == Some(AutoBenchKind::Full) {
                    let path = self
                        .bench
                        .output
                        .take()
                        .unwrap_or_else(|| "bench_results.csv".into());
                    match self.bench.runner.write_csv(&path) {
                        Ok(()) => println!("Benchmark results written to {}", path.display()),
                        Err(e) => log::error!("Benchmark CSV write failed: {e}"),
                    }
                    return true;
                }
            }
            AppHandler::handle_benchmark_action(&mut self.sim, &self.renderer, action);
        }

        if resp.start_quick {
            self.bench.quick.start(self.sim.particle_count_gpu());
        }
        if self.bench.quick.is_running() {
            self.bench
                .quick
                .advance(raw_dt, self.sim.particle_count_gpu());
        }

        if resp.start_capacity {
            let sz = window.inner_size();
            self.renderer.set_vsync(false);
            let action = self.bench.capacity.start(sz.width, sz.height);
            self.frame_times.clear();
            AppHandler::handle_capacity_action(&mut self.sim, &self.renderer, action);
        }
        if resp.export_capacity_csv
            && let Some(path) = rfd::FileDialog::new()
                .add_filter("CSV", &["csv"])
                .set_file_name("capacity.csv")
                .save_file()
            && let Err(e) = self.bench.capacity.write_csv(&path)
        {
            log::warn!("Capacity CSV export failed: {e}");
        }
        if self.bench.capacity.is_running() {
            let action = self.bench.capacity.advance(raw_dt);
            if matches!(action, benchmark::CapacityAction::Done) {
                self.renderer.set_vsync(self.bench.vsync);
                if self.bench.auto_kind == Some(AutoBenchKind::Capacity) {
                    let path = self
                        .bench
                        .output
                        .take()
                        .unwrap_or_else(|| "capacity_results.csv".into());
                    match self.bench.capacity.write_csv(&path) {
                        Ok(()) => println!("Capacity results written to {}", path.display()),
                        Err(e) => log::error!("Capacity benchmark CSV write failed: {e}"),
                    }
                    return true;
                }
            }
            if matches!(action, benchmark::CapacityAction::LoadPreset { .. }) {
                self.frame_times.clear();
            }
            AppHandler::handle_capacity_action(&mut self.sim, &self.renderer, action);
        }

        if resp.cancel {
            self.bench.runner.cancel();
            self.renderer.set_vsync(self.bench.vsync);
        }
        if resp.cancel_capacity {
            self.bench.capacity.cancel();
            self.renderer.set_vsync(self.bench.vsync);
        }
        false
    }

    /// Push the current cursor position and tool intent into the simulation's mouse state,
    /// and run the Spawn tool if LMB is held.
    fn handle_tool_effects(
        &mut self,
        viewport: winit::dpi::PhysicalSize<u32>,
        world_aspect: f32,
        shader_zoom: f32,
        bench_running: bool,
    ) {
        let world = screen_to_world(
            self.cursor_px,
            viewport,
            &self.camera,
            world_aspect,
            shader_zoom,
        );
        self.sim.mouse_x = world[0];
        self.sim.mouse_y = world[1];
        self.sim.mouse_range = self.tool.range;
        let sim_lmb = self.lmb_down && !self.lmb_egui && !bench_running;
        self.sim.mouse_strength = match self.tool.active {
            ui::Tool::Attract if sim_lmb => self.tool.strength,
            ui::Tool::Repel if sim_lmb => -self.tool.strength,
            _ => 0.0,
        };
        if matches!(self.tool.active, ui::Tool::Spawn) && sim_lmb {
            let queue = self.renderer.queue();
            let spawn_species = self.tool.spawn_species;
            let spawn_rate = self.tool.spawn_rate;
            let before = self.sim.particle_count_gpu();
            self.sim.spawn_particles(
                queue,
                world,
                self.tool.range,
                spawn_species,
                world_aspect,
                spawn_rate,
            );
            let spawned = (self.sim.particle_count_gpu() - before) as usize;
            if spawned > 0 {
                let n = self.sim.species_count;
                match spawn_species {
                    Some(s) if s < self.per_species_count.len() => {
                        self.per_species_count[s] += spawned;
                    }
                    _ => {
                        let per = spawned / n;
                        let rem = spawned % n;
                        for c in self.per_species_count.iter_mut() {
                            *c += per;
                        }
                        for c in self.per_species_count.iter_mut().take(rem) {
                            *c += 1;
                        }
                    }
                }
            }
        }
    }
}

// ── Private helpers ───────────────────────────────────────────────────────────

impl AppHandler {
    fn handle_benchmark_action(
        sim: &mut SimulationState,
        renderer: &WgpuState,
        action: benchmark::BenchmarkAction,
    ) {
        if let benchmark::BenchmarkAction::LoadCombo(combo) = action {
            // combo_preset() already pins world_width/height and disables auto_density for
            // the tier, so results are comparable across runs regardless of user settings.
            let preset = benchmark::BenchmarkRunner::combo_preset(combo);
            // Benchmarks always use equal species distribution for reproducibility.
            let saved_dist = sim.random_species_dist;
            sim.random_species_dist = false;
            sim.apply_preset(renderer.queue(), &preset);
            sim.random_species_dist = saved_dist;
        }
    }

    fn handle_capacity_action(
        sim: &mut SimulationState,
        renderer: &WgpuState,
        action: benchmark::CapacityAction,
    ) {
        if let benchmark::CapacityAction::LoadPreset {
            preset_idx,
            particles,
        } = action
        {
            let preset = benchmark::CapacityBench::preset_for(preset_idx, particles);
            // Benchmarks always use equal species distribution for reproducibility.
            let saved_dist = sim.random_species_dist;
            sim.random_species_dist = false;
            sim.apply_preset(renderer.queue(), &preset);
            sim.random_species_dist = saved_dist;
        }
    }

    /// Decode a share code and apply it to the simulation, then respawn.
    ///
    /// Returns `true` on success. Logs a warning and returns `false` on decode failure.
    fn apply_matrix_code(sim: &mut SimulationState, renderer: &WgpuState, code: &str) -> bool {
        match config::decode_matrix(code) {
            Ok((n, matrix)) => {
                sim.species_count = n;
                sim.attraction = [0.0f32; 272];
                for i in 0..n {
                    for j in 0..n {
                        sim.attraction[i * crate::simulation::MAX_SPECIES + j] = matrix[i * n + j];
                    }
                }
                sim.mark_attraction_dirty();
                sim.respawn(renderer.queue());
                true
            }
            Err(e) => {
                log::warn!("Share code apply failed: {e}");
                false
            }
        }
    }
}
