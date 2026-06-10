mod app;
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
