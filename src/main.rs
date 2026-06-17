//! Particle Life — a GPU-accelerated emergence simulator.
//!
//! Particles of different species attract or repel each other according to a
//! configurable interaction matrix, producing spontaneous flocking, clustering,
//! and predator-prey dynamics.  All force integration runs on the GPU via a
//! six-pass spatial-grid wgpu compute pipeline.

mod app;
mod benchmark;
mod cli;
mod config;
mod icon;
mod pipeline_cache;
mod renderer;
mod simulation;
mod ui;

use clap::Parser as _;

fn main() {
    env_logger::init();
    let args = cli::CliArgs::parse();
    let event_loop = winit::event_loop::EventLoop::new().unwrap();
    event_loop.set_control_flow(winit::event_loop::ControlFlow::Poll);
    let mut handler = app::AppHandler::new(args);
    event_loop.run_app(&mut handler).unwrap();
}
