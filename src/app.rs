use std::collections::VecDeque;
use std::sync::Arc;
use std::time::Instant;

use winit::{
    application::ApplicationHandler,
    dpi::PhysicalPosition,
    event::{ElementState, KeyEvent, MouseButton, MouseScrollDelta, WindowEvent},
    event_loop::ActiveEventLoop,
    keyboard::{Key, NamedKey},
    window::{Window, WindowId},
};

use crate::{renderer::WgpuState, simulation::SimulationState, ui};

// ── Camera ────────────────────────────────────────────────────────────────────

#[derive(Clone, Copy)]
struct Camera {
    center: [f32; 2], // world-space point at screen center
    zoom:   f32,       // 1.0 = full [0,1]² world fits screen; 2.0 = 2× zoom in
}

impl Camera {
    fn default_view() -> Self {
        Self { center: [0.5, 0.5], zoom: 1.0 }
    }
}

/// Convert a screen-space cursor position to world-space coordinates.
fn screen_to_world(
    px: PhysicalPosition<f64>,
    viewport: winit::dpi::PhysicalSize<u32>,
    cam: &Camera,
) -> [f32; 2] {
    let ndc_x = (px.x as f32 / viewport.width  as f32) * 2.0 - 1.0;
    let ndc_y = 1.0 - (px.y as f32 / viewport.height as f32) * 2.0;
    [
        ndc_x / (cam.zoom * 2.0) + cam.center[0],
        ndc_y / (cam.zoom * 2.0) + cam.center[1],
    ]
}

/// Zoom the camera by `factor` keeping `cursor_world` fixed on screen.
fn apply_zoom(cam: &mut Camera, cursor_world: [f32; 2], factor: f32) {
    let new_zoom = (cam.zoom * factor).clamp(0.1, 40.0);
    let scale = cam.zoom / new_zoom;
    cam.center[0] = cursor_world[0] - (cursor_world[0] - cam.center[0]) * scale;
    cam.center[1] = cursor_world[1] - (cursor_world[1] - cam.center[1]) * scale;
    cam.zoom = new_zoom;
}

// ── AppState ──────────────────────────────────────────────────────────────────

pub struct AppHandler {
    state: Option<AppState>,
}

impl Default for AppHandler {
    fn default() -> Self {
        Self { state: None }
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

    // Toolbar tool state
    tool: ui::Tool,
    tool_range: f32,
    mouse_strength: f32,

    // Spawn tool state
    spawn_species: Option<usize>, // None = random species per particle

    // Mouse tracking
    cursor_px: PhysicalPosition<f64>,
    lmb_down: bool,

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

        let window = Arc::new(
            event_loop
                .create_window(
                    Window::default_attributes()
                        .with_title("Particle Life")
                        .with_inner_size(winit::dpi::LogicalSize::new(1280u32, 720u32)),
                )
                .expect("Failed to create window"),
        );

        let renderer = WgpuState::new(Arc::clone(&window));
        let sim = SimulationState::new(renderer.device(), renderer.queue(), 1000, 6);

        let egui_ctx = egui::Context::default();
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
            tool: ui::Tool::Pan,
            tool_range: 0.1,
            mouse_strength: 2.0,
            spawn_species: None,
            cursor_px: PhysicalPosition::new(0.0, 0.0),
            lmb_down: false,
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
                self.state = None;
                event_loop.exit();
                return;
            }
            WindowEvent::KeyboardInput {
                event: KeyEvent {
                    logical_key: Key::Named(NamedKey::Escape),
                    state: ElementState::Pressed,
                    ..
                },
                ..
            } => {
                self.state = None;
                event_loop.exit();
                return;
            }
            _ => {}
        }

        let Some(state) = self.state.as_mut() else { return };

        let window = Arc::clone(&state.window);
        let resp = state.egui_state.on_window_event(&window, &event);

        match event {
            WindowEvent::Resized(size) => {
                state.renderer.resize(size);
            }

            WindowEvent::CursorMoved { position, .. } => {
                state.cursor_px = position;

                if state.lmb_panning || state.mmb_panning {
                    let vp = window.inner_size();
                    state.camera.center = [
                        state.pan_start_center[0]
                            - (position.x - state.pan_start_px.x) as f32
                                / (vp.width as f32 * state.camera.zoom),
                        state.pan_start_center[1]
                            + (position.y - state.pan_start_px.y) as f32
                                / (vp.height as f32 * state.camera.zoom),
                    ];
                    // Clamp so the world border never passes screen center.
                    state.camera.center[0] = state.camera.center[0].clamp(0.0, 1.0);
                    state.camera.center[1] = state.camera.center[1].clamp(0.0, 1.0);
                }
            }

            WindowEvent::MouseInput { button, state: btn_state, .. } => {
                match (button, btn_state) {
                    (MouseButton::Left, ElementState::Pressed) => {
                        state.lmb_down = true;
                        if !resp.consumed {
                            match state.tool {
                                ui::Tool::Pan => {
                                    state.lmb_panning = true;
                                    state.pan_start_px = state.cursor_px;
                                    state.pan_start_center = state.camera.center;
                                }
                                ui::Tool::ZoomIn => {
                                    let cw = screen_to_world(state.cursor_px, window.inner_size(), &state.camera);
                                    apply_zoom(&mut state.camera, cw, 1.5);
                                }
                                ui::Tool::ZoomOut => {
                                    let cw = screen_to_world(state.cursor_px, window.inner_size(), &state.camera);
                                    apply_zoom(&mut state.camera, cw, 1.0 / 1.5);
                                }
                                _ => {} // attract / repel / spawn handled each frame in RedrawRequested
                            }
                        }
                    }
                    (MouseButton::Left, ElementState::Released) => {
                        state.lmb_down = false;
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
                        let cw = screen_to_world(state.cursor_px, window.inner_size(), &state.camera);
                        apply_zoom(&mut state.camera, cw, factor);
                    }
                }
            }

            // Keyboard shortcuts: arrows = pan, +/- = zoom, 0 = reset view.
            WindowEvent::KeyboardInput {
                event: KeyEvent {
                    logical_key,
                    state: ElementState::Pressed,
                    ..
                },
                ..
            } => {
                if !resp.consumed {
                    let step = 0.1 / state.camera.zoom;
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
                        Key::Character(ref c) => {
                            let vp = window.inner_size();
                            let mid = PhysicalPosition::new(
                                vp.width as f64 / 2.0,
                                vp.height as f64 / 2.0,
                            );
                            match c.as_str() {
                                "=" | "+" => {
                                    let cw = screen_to_world(mid, vp, &state.camera);
                                    apply_zoom(&mut state.camera, cw, 1.5);
                                }
                                "-" => {
                                    let cw = screen_to_world(mid, vp, &state.camera);
                                    apply_zoom(&mut state.camera, cw, 1.0 / 1.5);
                                }
                                "0" => state.camera = Camera::default_view(),
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
                let cam_zoom   = state.camera.zoom;

                let (full_output, should_respawn, should_randomize) = {
                    let egui_ctx       = &state.egui_ctx;
                    let sim            = &mut state.sim;
                    let frame_times    = &state.frame_times;
                    let tool           = &mut state.tool;
                    let tool_range     = &mut state.tool_range;
                    let mouse_strength = &mut state.mouse_strength;
                    let spawn_species  = &mut state.spawn_species;
                    let n_species      = sim.species_count;
                    let border_mode    = sim.border_mode;
                    let mut respawn    = false;
                    let mut randomize  = false;
                    let out = egui_ctx.run(raw_input, |ctx| {
                        let (r, m) = ui::draw_ui(ctx, sim);
                        respawn = r;
                        randomize = m;
                        ui::draw_perf_overlay(ctx, frame_times, sim);
                        ui::draw_toolbar(ctx, tool, tool_range, mouse_strength, spawn_species, n_species);
                        ui::draw_world_border(ctx, cam_center, cam_zoom, border_mode);
                        ui::draw_cursor_indicator(ctx, *tool, *tool_range, cam_zoom);
                    });
                    (out, respawn, randomize)
                };

                if should_respawn {
                    state.sim.respawn(state.renderer.queue());
                }
                if should_randomize {
                    state.sim.randomize_attraction();
                }

                // Apply active tool effects to sim mouse state before dispatch.
                let vp = window.inner_size();
                let world = screen_to_world(state.cursor_px, vp, &state.camera);
                state.sim.mouse_x = world[0];
                state.sim.mouse_y = world[1];
                state.sim.mouse_range = state.tool_range;
                state.sim.mouse_strength = match state.tool {
                    ui::Tool::Attract if state.lmb_down =>  state.mouse_strength,
                    ui::Tool::Repel   if state.lmb_down => -state.mouse_strength,
                    _ => 0.0,
                };
                if matches!(state.tool, ui::Tool::Spawn) && state.lmb_down {
                    let queue          = state.renderer.queue();
                    let spawn_species  = state.spawn_species;
                    state.sim.spawn_particles(queue, world, state.tool_range, spawn_species);
                }

                let egui::FullOutput {
                    platform_output,
                    textures_delta,
                    shapes,
                    pixels_per_point,
                    ..
                } = full_output;

                state.egui_state.handle_platform_output(&window, platform_output);
                let paint_jobs = state.egui_ctx.tessellate(shapes, pixels_per_point);

                let cam_center = state.camera.center;
                let cam_zoom   = state.camera.zoom;
                match state.renderer.render(
                    &paint_jobs,
                    &textures_delta,
                    pixels_per_point,
                    &state.sim,
                    dt,
                    cam_center,
                    cam_zoom,
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
