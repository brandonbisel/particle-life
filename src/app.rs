use std::collections::VecDeque;
use std::sync::Arc;
use std::time::Instant;

use winit::{
    application::ApplicationHandler,
    event::{ElementState, KeyEvent, WindowEvent},
    event_loop::ActiveEventLoop,
    keyboard::{Key, NamedKey},
    window::{Window, WindowId},
};

use crate::{renderer::WgpuState, simulation::SimulationState, ui};

pub struct AppHandler {
    state: Option<AppState>,
}

impl Default for AppHandler {
    fn default() -> Self {
        Self { state: None }
    }
}

struct AppState {
    // window MUST be the last field: it holds the raw handle that Surface<'static>
    // points into. Rust drops fields top-to-bottom, so renderer/sim (and their wgpu
    // resources) are destroyed before the window handle is freed.
    renderer: WgpuState,
    sim: SimulationState,
    egui_ctx: egui::Context,
    egui_state: egui_winit::State,
    last_frame: Instant,
    frame_times: VecDeque<f32>, // rolling window of recent dt values
    window: Arc<Window>,
}

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
        let _resp = state.egui_state.on_window_event(&window, &event);

        match event {
            WindowEvent::Resized(size) => {
                state.renderer.resize(size);
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

                // Explicitly split field borrows: egui_ctx, sim, and frame_times are disjoint.
                let (full_output, should_respawn, should_randomize) = {
                    let egui_ctx = &state.egui_ctx;
                    let sim = &mut state.sim;
                    let frame_times = &state.frame_times;
                    let mut respawn = false;
                    let mut randomize = false;
                    let out = egui_ctx.run(raw_input, |ctx| {
                        let (r, m) = ui::draw_ui(ctx, sim);
                        respawn = r;
                        randomize = m;
                        ui::draw_perf_overlay(ctx, frame_times, sim);
                    });
                    (out, respawn, randomize)
                };

                if should_respawn {
                    state.sim.respawn(state.renderer.queue());
                }
                if should_randomize {
                    state.sim.randomize_attraction();
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

                match state.renderer.render(
                    &paint_jobs,
                    &textures_delta,
                    pixels_per_point,
                    &state.sim,
                    dt,
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
