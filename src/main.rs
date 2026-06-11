//! Particle Life — a GPU-accelerated emergence simulator.
//!
//! Particles of different species attract or repel each other according to a
//! configurable interaction matrix, producing spontaneous flocking, clustering,
//! and predator-prey dynamics.  All force integration runs on the GPU via a
//! five-pass spatial-grid wgpu compute pipeline.

mod app;
mod benchmark;
mod config;
mod renderer;
mod simulation;
mod ui;

fn main() {
    env_logger::init();
    let event_loop = winit::event_loop::EventLoop::new().unwrap();
    event_loop.set_control_flow(winit::event_loop::ControlFlow::Poll);
    let mut handler = app::AppHandler::default();
    event_loop.run_app(&mut handler).unwrap();
}
