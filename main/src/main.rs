//! Simple winit application.

use std::error::Error;

use main_core::UserEvent;
use winit::event_loop::EventLoop;

#[path = "util/tracing.rs"]
mod tracing;

fn main() -> Result<(), Box<dyn Error>> {
    #[cfg(web_platform)]
    console_error_panic_hook::set_once();
    tracing::init();

    let event_loop = EventLoop::<UserEvent>::with_user_event().build()?;
    let proxy = event_loop.create_proxy();
    #[cfg(web_platform)]
    {
        use winit::platform::web::EventLoopExtWebSys;
        let state = main_core::App::new(proxy);
        event_loop.spawn_app(state);
        Ok(())
    }

    #[cfg(not(web_platform))]
    {
        let mut state = main_core::App::new(proxy);
        event_loop.run_app(&mut state).map_err(Into::into)
    }
}
