use winit::event_loop::{ControlFlow, EventLoop};
use WGPU_WINIT_0_30::App;

fn main() {
    let event_loop = EventLoop::new().expect("failed to get event loop");
    event_loop.set_control_flow(ControlFlow::Poll);
    let mut app = App::default();
    let _ = event_loop.run_app(&mut app);
}
