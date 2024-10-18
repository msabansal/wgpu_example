mod gpu;
mod renderer;
mod scene;
mod texture;

use gpu::Gpu;
use renderer::Renderer;
use std::sync::Arc;
use tracing::info;
pub use web_time::{Duration, Instant};
use winit::{
    application::ApplicationHandler, event::WindowEvent, event_loop::EventLoopProxy, window::Window,
};

#[cfg(target_arch = "wasm32")]
use wasm_bindgen::prelude::*;

struct FrameCounter {
    // Instant of the last time we printed the frame time.
    last_printed_instant: web_time::Instant,
    // Number of frames since the last time we printed the frame time.
    frame_count: u32,
}

impl Default for FrameCounter {
    fn default() -> Self {
        Self {
            last_printed_instant: web_time::Instant::now(),
            frame_count: 0,
        }
    }
}

impl FrameCounter {
    fn update(&mut self) {
        self.frame_count += 1;
        let new_instant = web_time::Instant::now();
        let elapsed_secs = (new_instant - self.last_printed_instant).as_secs_f32();
        if elapsed_secs > 1.0 {
            let elapsed_ms = elapsed_secs * 1000.0;
            let frame_time = elapsed_ms / self.frame_count as f32;
            let fps = self.frame_count as f32 / elapsed_secs;
            info!("Frame time {:.2}ms ({:.1} FPS)", frame_time, fps);

            self.last_printed_instant = new_instant;
            self.frame_count = 0;
        }
    }
}

pub struct App {
    window: Option<Arc<Window>>,
    renderer: Option<Renderer<'static>>,
    last_render_time: Option<Instant>,
    frame_couner: FrameCounter,
    event_proxy: EventLoopProxy<UserEvent>,
}

impl App {
    pub fn new(event_proxy: EventLoopProxy<UserEvent>) -> Self {
        Self {
            window: None,
            renderer: None,
            last_render_time: None,
            frame_couner: FrameCounter::default(),
            event_proxy,
        }
    }
}

pub enum UserEvent {
    GPU(Gpu<'static>),
}

impl ApplicationHandler<UserEvent> for App {
    fn resumed(&mut self, event_loop: &winit::event_loop::ActiveEventLoop) {
        let mut attributes = Window::default_attributes();

        #[cfg(not(target_arch = "wasm32"))]
        {
            attributes = attributes.with_title("Standalone Winit/Wgpu Example");
        }

        #[allow(unused_assignments)]
        #[cfg(target_arch = "wasm32")]
        let (mut canvas_width, mut canvas_height) = (0, 0);

        #[cfg(target_arch = "wasm32")]
        {
            use winit::platform::web::WindowAttributesExtWebSys;
            let canvas = web_sys::window()
                .unwrap()
                .document()
                .unwrap()
                .get_element_by_id("canvas")
                .unwrap()
                .dyn_into::<web_sys::HtmlCanvasElement>()
                .unwrap();
            canvas_width = canvas.width();
            canvas_height = canvas.height();
            attributes = attributes.with_canvas(Some(canvas));
        }

        if let Ok(window) = event_loop.create_window(attributes) {
            let first_window_handle = self.window.is_none();
            let window_handle = Arc::new(window);
            self.window = Some(window_handle.clone());
            if first_window_handle {
                let event_proxy = self.event_proxy.clone();
                let (width, height) = {
                    #[cfg(target_arch = "wasm32")]
                    {
                        (canvas_width, canvas_height)
                    }
                    #[cfg(not(target_arch = "wasm32"))]
                    {
                        let inner_size = window_handle.inner_size();
                        (inner_size.width, inner_size.height)
                    }
                };
                let render_closure = async move {
                    let res = Gpu::new_async(window_handle, width, height).await;
                    if let Ok(gpu) = res {
                        if event_proxy.send_event(UserEvent::GPU(gpu)).is_err() {
                            tracing::error!("Failed to create and send renderer!");
                        }
                    } else {
                        tracing::error!("Failed to create renderer! {:?}", res);
                    }
                };

                #[cfg(not(target_arch = "wasm32"))]
                {
                    pollster::block_on(render_closure);
                }

                #[cfg(target_arch = "wasm32")]
                {
                    info!("Canvas dimensions: ({canvas_width} x {canvas_height})");
                    wasm_bindgen_futures::spawn_local(render_closure);
                }
                self.last_render_time = Some(Instant::now());
            }
        }
    }

    fn window_event(
        &mut self,
        event_loop: &winit::event_loop::ActiveEventLoop,
        _window_id: winit::window::WindowId,
        event: winit::event::WindowEvent,
    ) {
        let (Some(renderer), Some(window), Some(last_render_time)) = (
            self.renderer.as_mut(),
            self.window.as_ref(),
            self.last_render_time.as_mut(),
        ) else {
            info!("Ignore event because renderer, window, or last_render_time is None {event:?}");
            return;
        };

        // If the gui didn't consume the event, handle it
        match event {
            WindowEvent::KeyboardInput {
                event:
                    winit::event::KeyEvent {
                        physical_key: winit::keyboard::PhysicalKey::Code(key_code),
                        ..
                    },
                ..
            } => {
                // Exit by pressing the escape key
                if matches!(key_code, winit::keyboard::KeyCode::Escape) {
                    event_loop.exit();
                }
            }
            WindowEvent::Resized(size) => {
                let (width, height) = if cfg!(target_arch = "wasm32") {
                    let logical = size.to_logical::<u32>(window.scale_factor());
                    (logical.width, logical.height)
                } else {
                    (size.width, size.height)
                };
                if width != 0 && height != 0 {
                    info!("Resizing renderer surface to: ({width}, {height})");
                    renderer.resize(width, height);
                }
            }
            WindowEvent::CloseRequested => {
                info!("Close requested. Exiting...");
                event_loop.exit();
            }
            WindowEvent::RedrawRequested => {
                let now = Instant::now();
                *last_render_time = now;
                self.frame_couner.update();
                renderer.render_frame();
            }
            _ => {
                // info!("Unhandled window event: {:?}", event);
            }
        }
    }

    fn user_event(&mut self, _event_loop: &winit::event_loop::ActiveEventLoop, event: UserEvent) {
        match event {
            UserEvent::GPU(gpu) => {
                self.renderer = Some(Renderer::new(gpu));
                self.last_render_time = Some(Instant::now());
                if let Some(window) = self.window.as_ref() {
                    info!("Requested redraw after renderer creation");
                    window.request_redraw();
                }
            }
        }
    }
}
