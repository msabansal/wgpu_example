use anyhow::*;
use image::GenericImageView;
use tracing::*;

const TEXTURE_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;

#[tokio::main]
async fn main() -> Result<()> {
    let mut gpu = Gpu::new().await?;
    info!("Loading texture from bytes");
    let image = include_bytes!("../static/test.jpg");
    let image = image::load_from_memory_with_format(image, image::ImageFormat::Jpeg)?;
    info!("Created texture from image");
    let texture = gpu.create_textures(&image, Some("test.jpg"), 2)?;
    gpu.generate_mipmaps(&texture, 2);
    gpu.copy_texture(texture, 1, ((image.width() / 2) as usize, (image.height() / 2) as usize)).await;
    Ok(())
}

struct Gpu {
    device: wgpu::Device,
    queue: wgpu::Queue,
}

impl Gpu {
    pub async fn new() -> Result<Self> {
        let instance = wgpu::Instance::default();
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::default(),
                compatible_surface: None,
                force_fallback_adapter: false,
            })
            .await
            .ok_or(anyhow::anyhow!("Failed to request adapter!"))?;
        let (device, queue) = adapter
            .request_device(
                &wgpu::DeviceDescriptor {
                    label: Some("WGPU Device"),
                    required_features: wgpu::Features::default(),
                    required_limits: wgpu::Limits::default(),
                    memory_hints: wgpu::MemoryHints::default(),
                },
                None,
            )
            .await
            .map_err(|_| anyhow::anyhow!("Failed to request device!"))?;

        Ok(Self { device, queue })
    }

    fn create_textures(
        &self,
        img: &image::DynamicImage,
        label: Option<&str>,
        mip_count: u32,
    ) -> Result<wgpu::Texture> {
        let rgba = img.to_rgba8();
        let dimensions = img.dimensions();

        let size = wgpu::Extent3d {
            width: dimensions.0,
            height: dimensions.1,
            depth_or_array_layers: 1,
        };

        let texture = self.device.create_texture(&wgpu::TextureDescriptor {
            label,
            size,
            mip_level_count: mip_count,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: TEXTURE_FORMAT,
            usage: wgpu::TextureUsages::TEXTURE_BINDING
                | wgpu::TextureUsages::COPY_DST
                | wgpu::TextureUsages::RENDER_ATTACHMENT
                | wgpu::TextureUsages::STORAGE_BINDING
                | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });

        self.queue.write_texture(
            wgpu::ImageCopyTexture {
                aspect: wgpu::TextureAspect::All,
                texture: &texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
            },
            &rgba,
            wgpu::ImageDataLayout {
                offset: 0,
                bytes_per_row: Some(4 * dimensions.0),
                rows_per_image: Some(dimensions.1),
            },
            size,
        );

        Ok(texture)
    }

    async fn copy_texture(&self, storage_texture: wgpu::Texture, mip_level: u32, dimensions: (usize, usize)) -> Vec<u8>{
        let mut texture_data = vec![0u8; dimensions.0 * dimensions.1 * 4];
        let output_staging_buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: size_of_val(&texture_data[..]) as u64,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });

        let mut encoder =
           self.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });

        encoder.copy_texture_to_buffer(
            wgpu::ImageCopyTexture {
                texture: &storage_texture,
                mip_level,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::ImageCopyBuffer {
                buffer: &output_staging_buffer,
                layout: wgpu::ImageDataLayout {
                    offset: 0,
                    // This needs to be padded to 256.
                    bytes_per_row: Some((dimensions.0 * 4) as u32),
                    rows_per_image: Some(dimensions.1 as u32),
                },
            },
            wgpu::Extent3d {
                width: dimensions.0 as u32,
                height: dimensions.1 as u32,
                depth_or_array_layers: 1,
            },
        );
        self.queue.submit(Some(encoder.finish()));

        let buffer_slice = output_staging_buffer.slice(..);
        let (sender, receiver) = flume::bounded(1);
        buffer_slice.map_async(wgpu::MapMode::Read, move |r| sender.send(r).unwrap());
        self.device.poll(wgpu::Maintain::wait()).panic_on_timeout();
        receiver.recv_async().await.unwrap().unwrap();
        info!("Output buffer mapped");
        {
            let view = buffer_slice.get_mapped_range();
            texture_data.copy_from_slice(&view[..]);
        }
        info!("GPU data copied to local.");
        output_staging_buffer.unmap();
        texture_data
    }

    fn generate_mipmaps(&mut self, texture: &wgpu::Texture, mip_count: u32) {
        let shader = self
            .device
            .create_shader_module(wgpu::include_wgsl!("../static/draw_image.wgsl"));

        let pipeline = self
            .device
            .create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("blit"),
                layout: None,
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: "vs_main",
                    compilation_options: Default::default(),
                    buffers: &[],
                },
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: "fs_main",
                    compilation_options: Default::default(),
                    targets: &[Some(TEXTURE_FORMAT.into())],
                }),
                primitive: wgpu::PrimitiveState {
                    topology: wgpu::PrimitiveTopology::TriangleList,
                    ..Default::default()
                },
                depth_stencil: None,
                multisample: wgpu::MultisampleState::default(),
                multiview: None,
                cache: None,
            });

        let bind_group_layout = pipeline.get_bind_group_layout(0);

        let sampler = self.device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("mip"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::FilterMode::Nearest,
            ..Default::default()
        });

        let views = (0..mip_count)
            .map(|mip| {
                texture.create_view(&wgpu::TextureViewDescriptor {
                    label: Some("mip"),
                    format: None,
                    dimension: None,
                    aspect: wgpu::TextureAspect::All,
                    base_mip_level: mip,
                    mip_level_count: Some(1),
                    base_array_layer: 0,
                    array_layer_count: None,
                })
            })
            .collect::<Vec<_>>();
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });

        for target_mip in 1..mip_count as usize {
            let bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                layout: &bind_group_layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(&views[target_mip - 1]),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::Sampler(&sampler),
                    },
                ],
                label: None,
            });

            let mut rpass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: None,
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &views[target_mip],
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::WHITE),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });

            rpass.set_pipeline(&pipeline);
            rpass.set_bind_group(0, &bind_group, &[]);
            rpass.draw(0..3, 0..1);
        }
    }
}
