//! Simple winit application.

use std::error::Error;

use winit::event_loop::EventLoop;

#[path = "util/tracing.rs"]
mod tracing;

fn main() -> Result<(), Box<dyn Error>> {
    #[cfg(web_platform)]
    console_error_panic_hook::set_once();
    tracing::init();

    let event_loop = EventLoop::with_user_event().build()?;

    #[cfg(web_platform)]
    {
        use winit::platform::web::EventLoopExtWebSys;
        let state = main_core::App::default();
        event_loop.spawn_app(state);
        Ok(())
    }

    #[cfg(not(web_platform))]
    {
        let mut state = main_core::App::default();
        event_loop.run_app(&mut state).map_err(Into::into)
    }
}