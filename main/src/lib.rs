use std::sync::Arc;
use tracing::info;
use winit::{
    application::ApplicationHandler,
    dpi::PhysicalSize,
    event::WindowEvent,
    window::Window,
};

mod texture;

#[cfg(target_arch = "wasm32")]
use futures::channel::oneshot::Receiver;

pub use web_time::{Duration, Instant};

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
        Self { last_printed_instant: web_time::Instant::now(), frame_count: 0 }
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
            tracing::info!("Frame time {:.2}ms ({:.1} FPS)", frame_time, fps);

            self.last_printed_instant = new_instant;
            self.frame_count = 0;
        }
    }
}



#[derive(Default)]
pub struct App {
    window: Option<Arc<Window>>,
    renderer: Option<Renderer<'static>>,
    last_render_time: Option<Instant>,
    #[cfg(target_arch = "wasm32")]
    renderer_receiver: Option<Receiver<Renderer<'static>>>,
    last_size: (u32, u32),
    frame_couner: FrameCounter,
}

impl ApplicationHandler for App {
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
            self.last_size = (canvas_width, canvas_height);
            attributes = attributes.with_canvas(Some(canvas));
        }

        if let Ok(window) = event_loop.create_window(attributes) {
            let first_window_handle = self.window.is_none();
            let window_handle = Arc::new(window);
            self.window = Some(window_handle.clone());
            if first_window_handle {
                #[cfg(not(target_arch = "wasm32"))]
                {
                    let inner_size = window_handle.inner_size();
                    self.last_size = (inner_size.width, inner_size.height);
                    let logical_size = inner_size.to_logical::<u32>(window_handle.scale_factor());
                    let renderer = pollster::block_on(async move {
                        info!("Window dimensions (physical): {inner_size:?}");
                        info!("Window dimensions (logical): {logical_size:?}");
                        Renderer::new(window_handle.clone(), logical_size.width, logical_size.height).await
                    });
                    self.renderer = Some(renderer);
                }

                #[cfg(target_arch = "wasm32")]
                {
                    let (sender, receiver) = futures::channel::oneshot::channel();
                    self.renderer_receiver = Some(receiver);
                    tracing::info!("Canvas dimensions: ({canvas_width} x {canvas_height})");
                    wasm_bindgen_futures::spawn_local(async move {
                        let renderer =
                            Renderer::new(window_handle.clone(), canvas_width, canvas_height).await;
                        if sender.send(renderer).is_err() {
                            tracing::error!("Failed to create and send renderer!");
                        }
                    });
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
        #[cfg(target_arch = "wasm32")]
        {
            let mut renderer_received = false;
            if let Some(receiver) = self.renderer_receiver.as_mut() {
                if let Ok(Some(renderer)) = receiver.try_recv() {
                    self.renderer = Some(renderer);
                    renderer_received = true;
                }
            }
            if renderer_received {
                self.renderer_receiver = None;
            }
        }

        let (Some(renderer), Some(window), Some(last_render_time)) = (
            self.renderer.as_mut(),
            self.window.as_ref(),
            self.last_render_time.as_mut(),
        ) else {
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
                let (width, height)  = if cfg!(target_arch = "wasm32") {
                    let logical = size.to_logical::<u32>(window.scale_factor());
                    (logical.width, logical.height)
                } else {
                    (size.width, size.height)
                };
                
                tracing::info!("Resizing renderer surface to: ({width}, {height})");
                renderer.resize(width, height);
                self.last_size = (width, height);
            }
            WindowEvent::CloseRequested => {
                tracing::info!("Close requested. Exiting...");
                event_loop.exit();
            }
            WindowEvent::RedrawRequested => {
                let now = Instant::now();
                let delta_time = now - *last_render_time;
                *last_render_time = now;
                self.frame_couner.update();
                renderer.render_frame(delta_time);
            }
            _ =>  {
                // info!("Unhandled window event: {:?}", event);
            },
        }

        window.request_redraw();
    }
}

pub struct Renderer<'window> {
    gpu: Gpu<'window>,
    scene: Scene,
}

impl<'window> Renderer<'window> {
    const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;

    pub async fn new(
        window: impl Into<wgpu::SurfaceTarget<'window>>,
        width: u32,
        height: u32,
    ) -> Self {
        let gpu = Gpu::new_async(window, width, height).await;

        let scene = Scene::new(&gpu.device, &gpu.queue, gpu.surface_format);

        Self {
            gpu,
            scene,
        }
    }

    pub fn resize(&mut self, width: u32, height: u32) {
        self.gpu.resize(width, height);
    }

    pub fn render_frame(
        &mut self,
        delta_time: crate::Duration,
    ) {
        let delta_time = delta_time.as_secs_f32();
        self.scene
            .update(&self.gpu.queue, self.gpu.aspect_ratio(), delta_time);

        let mut encoder = self
            .gpu
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("Render Encoder"),
            });


        let surface_texture = self
            .gpu
            .surface
            .get_current_texture()
            .expect("Failed to get surface texture!");

        let surface_texture_view =
            surface_texture
                .texture
                .create_view(&wgpu::TextureViewDescriptor::default());

        encoder.insert_debug_marker("Render scene");

        {
            let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("Render Pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &surface_texture_view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                occlusion_query_set: None,
                timestamp_writes: None,
            });
            self.scene.render(&mut render_pass);
        }

        self.gpu.queue.submit(std::iter::once(encoder.finish()));
        surface_texture.present();
    }
}

pub struct Gpu<'window> {
    pub surface: wgpu::Surface<'window>,
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
    pub surface_config: wgpu::SurfaceConfiguration,
    pub surface_format: wgpu::TextureFormat,
}

impl<'window> Gpu<'window> {
    pub fn aspect_ratio(&self) -> f32 {
        self.surface_config.width as f32 / self.surface_config.height.max(1) as f32
    }

    pub fn resize(&mut self, width: u32, height: u32) {
        self.surface_config.width = width;
        self.surface_config.height = height;
        self.surface.configure(&self.device, &self.surface_config);
    }
    
    pub async fn new_async(
        window: impl Into<wgpu::SurfaceTarget<'window>>,
        width: u32,
        height: u32,
    ) -> Self {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends: wgpu::util::backend_bits_from_env().unwrap_or_else(wgpu::Backends::all),
            ..Default::default()
        });

        let surface = instance.create_surface(window).unwrap();

        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::default(),
                compatible_surface: Some(&surface),
                force_fallback_adapter: false,
            })
            .await
            .expect("Failed to request adapter!");
        let (device, queue) = {
            tracing::info!("WGPU Adapter Features: {:#?}", adapter.features());
            adapter
                .request_device(
                    &wgpu::DeviceDescriptor {
                        label: Some("WGPU Device"),

                        #[cfg(not(target_arch = "wasm32"))]
                        required_features: wgpu::Features::default(),

                        #[cfg(all(target_arch = "wasm32", feature = "webgpu"))]
                        required_features: wgpu::Features::default(),

                        #[cfg(all(target_arch = "wasm32", feature = "webgl"))]
                        required_features: wgpu::Features::default(),

                        #[cfg(not(target_arch = "wasm32"))]
                        required_limits: wgpu::Limits {
                            max_texture_dimension_2d: 4096, // Allow higher resolutions on native
                            ..wgpu::Limits::downlevel_defaults()
                        },

                        #[cfg(all(target_arch = "wasm32", feature = "webgpu"))]
                        required_limits: wgpu::Limits::default(),

                        #[cfg(all(target_arch = "wasm32", feature = "webgl"))]
                        required_limits: wgpu::Limits::downlevel_webgl2_defaults(),

                        memory_hints: wgpu::MemoryHints::default(),
                    },
                    None,
                )
                .await
                .expect("Failed to request a device!")
        };

        let surface_capabilities = surface.get_capabilities(&adapter);

        // This assumes an sRGB surface texture
        let surface_format = surface_capabilities
            .formats
            .iter()
            .copied()
            .find(|f| !f.is_srgb()) // egui wants a non-srgb surface texture
            .unwrap_or(surface_capabilities.formats[0]);

        let surface_config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format: surface_format,
            width,
            height,
            present_mode: surface_capabilities.present_modes[0],
            alpha_mode: surface_capabilities.alpha_modes[0],
            view_formats: vec![],
            desired_maximum_frame_latency: 2,
        };

        surface.configure(&device, &surface_config);

        Self {
            surface,
            device,
            queue,
            surface_config,
            surface_format,
        }
    }
}

struct Scene {
    pub model: nalgebra_glm::Mat4,
    // pub vertex_buffer: wgpu::Buffer,
    // pub index_buffer: wgpu::Buffer,
    pub pipeline: wgpu::RenderPipeline,
    pub bind_group: wgpu::BindGroup,
}

impl Scene {
    pub fn new(device: &wgpu::Device, queue: &wgpu::Queue, surface_format: wgpu::TextureFormat) -> Self {
        // let vertex_buffer = wgpu::util::DeviceExt::create_buffer_init(
        //     device,
        //     &wgpu::util::BufferInitDescriptor {
        //         label: Some("Vertex Buffer"),
        //         contents: bytemuck::cast_slice(&VERTICES),
        //         usage: wgpu::BufferUsages::VERTEX,
        //     },
        // );
        // let index_buffer = wgpu::util::DeviceExt::create_buffer_init(
        //     device,
        //     &wgpu::util::BufferInitDescriptor {
        //         label: Some("index Buffer"),
        //         contents: bytemuck::cast_slice(&INDICES),
        //         usage: wgpu::BufferUsages::INDEX,
        //     },
        // );
        let pipeline = Self::create_pipeline(device, queue, surface_format);
        let texture_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                entries: &[
                    wgpu::BindGroupLayoutEntry {
                        binding: 0,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Texture {
                            multisampled: false,
                            view_dimension: wgpu::TextureViewDimension::D2,
                            sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 1,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                        count: None,
                    },
                ],
                label: Some("texture_bind_group_layout"),
            });

        let diffuse_bytes = include_bytes!("../static/test.jpg");

        let diffuse_texture =
            texture::Texture::from_bytes(&device, &queue, diffuse_bytes, "test.jpg").unwrap();

        let diffuse_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            layout: &texture_bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&diffuse_texture.view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&diffuse_texture.sampler),
                },
            ],
            label: Some("diffuse_bind_group"),
        });

        Self {
            model: nalgebra_glm::Mat4::identity(),
            bind_group: diffuse_bind_group,
            pipeline,
            // vertex_buffer,
            // index_buffer,
        }
    }

    pub fn render<'rpass>(&'rpass self, renderpass: &mut wgpu::RenderPass<'rpass>) {
        renderpass.set_pipeline(&self.pipeline);
        renderpass.set_bind_group(0, &self.bind_group, &[]);
        renderpass.draw(0..3, 0..1);
    }

    pub fn update(&mut self, queue: &wgpu::Queue, aspect_ratio: f32, delta_time: f32) {
        // let projection =
        //     nalgebra_glm::perspective_lh_zo(aspect_ratio, 80_f32.to_radians(), 0.1, 1000.0);
        // let view = nalgebra_glm::look_at_lh(
        //     &nalgebra_glm::vec3(0.0, 0.0, 4.0),
        //     &nalgebra_glm::vec3(0.0, 0.0, 0.0),
        //     &nalgebra_glm::Vec3::y(),
        // );
        // self.model = nalgebra_glm::rotate(
        //     &self.model,
        //     30_f32.to_radians() * delta_time,
        //     &nalgebra_glm::Vec3::y(),
        // );
        // self.uniform.update_buffer(
        //     queue,
        //     0,
        //     UniformBuffer {
        //         mvp: projection * view * self.model,
        //     },
        // );
    }

    fn create_pipeline(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        surface_format: wgpu::TextureFormat
    ) -> wgpu::RenderPipeline {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: None,
            source: wgpu::ShaderSource::Wgsl(include_str!("shaders/draw_image.wgsl").into()),
        });
        
        let texture_bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        multisampled: false,
                        view_dimension: wgpu::TextureViewDimension::D2,
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
            label: Some("texture_bind_group_layout"),
        });


        let render_pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("Render Pipeline Layout"),
                bind_group_layouts: &[&texture_bind_group_layout],
                push_constant_ranges: &[],
            });

        device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("Render Pipeline"),
            layout: Some(&render_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: "vs_main",
                buffers: &[],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: "fs_main",
                targets: &[Some(wgpu::TextureFormat::Bgra8Unorm.into())],
                compilation_options: Default::default(),
            }),
            
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                ..Default::default()
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        })
    }
}
