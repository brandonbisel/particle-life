//! wgpu device/surface management and the particle render pipeline.
//!
//! [`WgpuState`] owns the surface, device, and both the egui and particle
//! render pipelines.  Each frame it calls [`SimulationState::dispatch`] to run
//! the compute passes before issuing the render pass.

use std::mem::size_of;
use std::sync::Arc;
use winit::window::Window;

use crate::simulation::{Particle, SimulationState};

/// Convert an sRGB byte triplet to a wgpu linear-light `Color` suitable for use as a
/// render-pass clear value.
pub fn bg_color_from_srgb(c: [u8; 3]) -> wgpu::Color {
    let lin = |b: u8| {
        let s = b as f64 / 255.0;
        if s <= 0.04045 {
            s / 12.92
        } else {
            ((s + 0.055) / 1.055).powf(2.4)
        }
    };
    wgpu::Color {
        r: lin(c[0]),
        g: lin(c[1]),
        b: lin(c[2]),
        a: 1.0,
    }
}

/// Owns the wgpu device, surface, and both the particle and egui render pipelines.
///
/// Created once at startup (inside [`AppHandler::resumed`](crate::app::AppHandler))
/// and driven each frame by [`render`](WgpuState::render).
pub struct WgpuState {
    device: wgpu::Device,
    queue: wgpu::Queue,
    surface: wgpu::Surface<'static>,
    surface_config: wgpu::SurfaceConfiguration,
    egui_renderer: egui_wgpu::Renderer,
    particle_pipeline: wgpu::RenderPipeline,
    globals_buf: wgpu::Buffer,
    palette_buf: wgpu::Buffer,
    globals_bind_group: wgpu::BindGroup,
    particle_radius: f32,
    immediate_supported: bool,
    vsync: bool,
}

impl WgpuState {
    /// Initialise wgpu, create the surface, and build both render pipelines.
    ///
    /// Blocks the calling thread while the adapter and device are acquired
    /// (`pollster::block_on`).  Must be called from the main thread.
    pub fn new(window: Arc<Window>) -> Self {
        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
            backends: wgpu::Backends::PRIMARY,
            ..Default::default()
        });

        // Arc<Window> is 'static, so create_surface returns Surface<'static>.
        let surface = instance
            .create_surface(Arc::clone(&window))
            .expect("Failed to create wgpu surface");

        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            compatible_surface: Some(&surface),
            force_fallback_adapter: false,
        }))
        .expect("No compatible Vulkan adapter found");

        log::info!("Selected adapter: {:?}", adapter.get_info().name);

        let (device, queue) = pollster::block_on(adapter.request_device(
            &wgpu::DeviceDescriptor {
                label: Some("ParticleLife Device"),
                required_features: wgpu::Features::empty(),
                required_limits: wgpu::Limits::default(),
                memory_hints: wgpu::MemoryHints::default(),
            },
            None,
        ))
        .expect("Failed to create wgpu device");

        let caps = surface.get_capabilities(&adapter);
        let format = caps
            .formats
            .iter()
            .find(|f| f.is_srgb())
            .copied()
            .unwrap_or(caps.formats[0]);
        let immediate_supported = caps.present_modes.contains(&wgpu::PresentMode::Immediate);

        let size = window.inner_size();
        let surface_config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            width: size.width.max(1),
            height: size.height.max(1),
            present_mode: wgpu::PresentMode::Fifo,
            desired_maximum_frame_latency: 2,
            alpha_mode: caps.alpha_modes[0],
            view_formats: vec![],
        };
        surface.configure(&device, &surface_config);

        let egui_renderer = egui_wgpu::Renderer::new(&device, format, None, 1, false);

        // --- Particle render pipeline ---

        let globals_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Globals"),
            size: 32,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        // 8 × vec4<f32> = 128 bytes; holds pre-linearised palette colours for the vertex shader.
        let palette_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Palette"),
            size: 128,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let globals_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("Globals Layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::VERTEX,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Storage { read_only: true },
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });

        let globals_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Globals Bind Group"),
            layout: &globals_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: globals_buf.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: palette_buf.as_entire_binding(),
                },
            ],
        });

        let particle_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Particle Shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shaders/particle.wgsl").into()),
        });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("Particle Pipeline Layout"),
            bind_group_layouts: &[&globals_layout],
            push_constant_ranges: &[],
        });

        let vertex_buf_layout = wgpu::VertexBufferLayout {
            array_stride: size_of::<Particle>() as u64,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes: &[
                wgpu::VertexAttribute {
                    format: wgpu::VertexFormat::Float32x2,
                    offset: 0,
                    shader_location: 0,
                },
                wgpu::VertexAttribute {
                    format: wgpu::VertexFormat::Float32x2,
                    offset: 8,
                    shader_location: 1,
                },
                wgpu::VertexAttribute {
                    format: wgpu::VertexFormat::Uint32,
                    offset: 20, // species field (color at 16 is unused by vertex shader)
                    shader_location: 2,
                },
            ],
        };

        let particle_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("Particle Pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &particle_shader,
                entry_point: Some("vs_main"),
                buffers: &[vertex_buf_layout],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &particle_shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                cull_mode: None,
                ..Default::default()
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });

        let state = Self {
            device,
            queue,
            surface,
            surface_config,
            egui_renderer,
            particle_pipeline,
            globals_buf,
            palette_buf,
            globals_bind_group,
            particle_radius: 3.0,
            immediate_supported,
            vsync: true,
        };
        state.update_globals([0.5, 0.5], 1.0, 1.0, 1.5 / 720.0);
        state
    }

    /// Convert the 8-entry sRGB palette to linear floats and upload to the vertex shader.
    ///
    /// Doing the sRGB→linear conversion here (once, on the CPU) avoids three `pow()` calls
    /// per vertex in the shader, which measurably hurts throughput at CPU-bound particle counts.
    pub fn update_palette(&self, palette: &[u32; 8]) {
        let linear: [[f32; 4]; 8] = std::array::from_fn(|i| {
            let p = palette[i];
            [
                srgb_u8_to_linear((p & 0xFF) as u8),
                srgb_u8_to_linear(((p >> 8) & 0xFF) as u8),
                srgb_u8_to_linear(((p >> 16) & 0xFF) as u8),
                1.0,
            ]
        });
        self.queue
            .write_buffer(&self.palette_buf, 0, bytemuck::cast_slice(&linear));
    }

    /// Switch between vsync-on (`Fifo`) and vsync-off (`Immediate`).
    ///
    /// Falls back silently to `Fifo` if `Immediate` is not supported by the adapter,
    /// in which case `vsync_enabled` will still return `true` after the call.
    pub fn set_vsync(&mut self, enabled: bool) {
        let mode = if enabled || !self.immediate_supported {
            wgpu::PresentMode::Fifo
        } else {
            wgpu::PresentMode::Immediate
        };
        if self.surface_config.present_mode != mode {
            self.surface_config.present_mode = mode;
            self.surface.configure(&self.device, &self.surface_config);
        }
        self.vsync = matches!(self.surface_config.present_mode, wgpu::PresentMode::Fifo);
    }

    /// Whether the adapter supports `PresentMode::Immediate` (vsync-off).
    pub fn vsync_toggle_available(&self) -> bool {
        self.immediate_supported
    }

    /// Reconfigure the surface for a new window size.  No-ops on zero-area sizes.
    pub fn resize(&mut self, new_size: winit::dpi::PhysicalSize<u32>) {
        if new_size.width == 0 || new_size.height == 0 {
            return;
        }
        self.surface_config.width = new_size.width;
        self.surface_config.height = new_size.height;
        self.surface.configure(&self.device, &self.surface_config);
        // globals are updated at the start of each render() call
    }

    fn update_globals(
        &self,
        camera_center: [f32; 2],
        shader_zoom: f32,
        world_aspect: f32,
        particle_radius_norm: f32,
    ) {
        self.queue.write_buffer(
            &self.globals_buf,
            0,
            bytemuck::cast_slice(&[
                self.surface_config.width as f32,
                self.surface_config.height as f32,
                particle_radius_norm,
                0.0f32,
                camera_center[0],
                camera_center[1],
                shader_zoom,
                world_aspect,
            ]),
        );
    }

    /// Render one frame: run the 5-pass simulation compute, draw particles, then draw the egui overlay.
    ///
    /// Returns `Err(SurfaceError::Lost | Outdated)` when the surface needs to be
    /// reconfigured (caller should call `resize`); other errors are propagated as-is.
    #[allow(clippy::too_many_arguments)]
    pub fn render(
        &mut self,
        paint_jobs: &[egui::ClippedPrimitive],
        textures_delta: &egui::TexturesDelta,
        pixels_per_point: f32,
        sim: &SimulationState,
        dt: f32,
        camera_center: [f32; 2],
        shader_zoom: f32,
        bg_color: wgpu::Color,
    ) -> Result<(), wgpu::SurfaceError> {
        let output = self.surface.get_current_texture()?;
        let view = output
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());

        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("Frame"),
            });

        self.particle_radius = sim.particle_radius;
        let world_aspect = sim.world_aspect();
        let particle_radius_norm = sim.particle_radius / self.surface_config.height as f32;
        self.update_globals(
            camera_center,
            shader_zoom,
            world_aspect,
            particle_radius_norm,
        );

        sim.dispatch(&mut encoder, &self.queue, dt);

        // Particle render pass
        {
            let mut rpass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("Particle Pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(bg_color),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            rpass.set_pipeline(&self.particle_pipeline);
            rpass.set_bind_group(0, &self.globals_bind_group, &[]);
            rpass.set_vertex_buffer(0, sim.particle_buffer().slice(..));
            rpass.draw(0..6, 0..sim.particle_count_gpu());
        }

        // egui overlay pass
        let screen_descriptor = egui_wgpu::ScreenDescriptor {
            size_in_pixels: [self.surface_config.width, self.surface_config.height],
            pixels_per_point,
        };

        for (id, delta) in &textures_delta.set {
            self.egui_renderer
                .update_texture(&self.device, &self.queue, *id, delta);
        }

        let extra_cmds = self.egui_renderer.update_buffers(
            &self.device,
            &self.queue,
            &mut encoder,
            paint_jobs,
            &screen_descriptor,
        );

        {
            // forget_lifetime() is required: egui_wgpu::Renderer::render() needs
            // &mut RenderPass<'static>, but begin_render_pass returns RenderPass<'_>.
            let mut egui_pass = encoder
                .begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("egui Pass"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: &view,
                        resolve_target: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Load,
                            store: wgpu::StoreOp::Store,
                        },
                    })],
                    depth_stencil_attachment: None,
                    timestamp_writes: None,
                    occlusion_query_set: None,
                })
                .forget_lifetime();

            self.egui_renderer
                .render(&mut egui_pass, paint_jobs, &screen_descriptor);
        }

        for id in &textures_delta.free {
            self.egui_renderer.free_texture(id);
        }

        // extra_cmds (staging uploads) must precede the main encoder.
        self.queue.submit(
            extra_cmds
                .into_iter()
                .chain(std::iter::once(encoder.finish())),
        );
        output.present();

        Ok(())
    }

    /// Render the current particle state to an offscreen texture and return PNG bytes.
    ///
    /// Does not advance the simulation (no dispatch) — captures whatever is already on the GPU.
    /// The PNG is RGBA 8-bit, sized to the current surface dimensions.
    pub fn capture_png(
        &self,
        sim: &SimulationState,
        camera_center: [f32; 2],
        shader_zoom: f32,
        bg_color: wgpu::Color,
    ) -> Vec<u8> {
        let width = self.surface_config.width;
        let height = self.surface_config.height;

        let texture = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("Screenshot"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: self.surface_config.format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());

        // wgpu requires row strides to be aligned to COPY_BYTES_PER_ROW_ALIGNMENT.
        let bytes_per_pixel = 4u32;
        let align = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
        let unpadded_row = bytes_per_pixel * width;
        let padded_row = unpadded_row.div_ceil(align) * align;

        let staging = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Screenshot Staging"),
            size: (padded_row * height) as u64,
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let world_aspect = sim.world_aspect();
        let particle_radius_norm = sim.particle_radius / height as f32;
        self.update_globals(
            camera_center,
            shader_zoom,
            world_aspect,
            particle_radius_norm,
        );

        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("Screenshot"),
            });

        {
            let mut rpass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("Screenshot Particle Pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(bg_color),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            rpass.set_pipeline(&self.particle_pipeline);
            rpass.set_bind_group(0, &self.globals_bind_group, &[]);
            rpass.set_vertex_buffer(0, sim.particle_buffer().slice(..));
            rpass.draw(0..6, 0..sim.particle_count_gpu());
        }

        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture: &texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &staging,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(padded_row),
                    rows_per_image: None,
                },
            },
            wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
        );

        self.queue.submit(std::iter::once(encoder.finish()));

        let slice = staging.slice(..);
        slice.map_async(wgpu::MapMode::Read, |_| {});
        self.device.poll(wgpu::Maintain::Wait);

        let mapped = slice.get_mapped_range();
        let is_bgra = matches!(
            self.surface_config.format,
            wgpu::TextureFormat::Bgra8Unorm | wgpu::TextureFormat::Bgra8UnormSrgb
        );
        let mut pixels = Vec::with_capacity((width * height * 4) as usize);
        for row in 0..height as usize {
            let start = row * padded_row as usize;
            let row_bytes = &mapped[start..start + unpadded_row as usize];
            if is_bgra {
                for c in row_bytes.chunks_exact(4) {
                    pixels.extend_from_slice(&[c[2], c[1], c[0], c[3]]);
                }
            } else {
                pixels.extend_from_slice(row_bytes);
            }
        }
        drop(mapped);
        staging.unmap();

        let mut png_bytes = Vec::new();
        let mut enc = png::Encoder::new(&mut png_bytes, width, height);
        enc.set_color(png::ColorType::Rgba);
        enc.set_depth(png::BitDepth::Eight);
        enc.write_header()
            .expect("png header")
            .write_image_data(&pixels)
            .expect("png data");
        png_bytes
    }

    /// The wgpu logical device (used by callers to create simulation buffers/pipelines).
    pub fn device(&self) -> &wgpu::Device {
        &self.device
    }

    /// The wgpu command queue (used by callers to upload data to GPU buffers).
    pub fn queue(&self) -> &wgpu::Queue {
        &self.queue
    }

    /// Maximum texture dimension supported by the adapter (passed to egui for atlas sizing).
    pub fn max_texture_side(&self) -> usize {
        self.device.limits().max_texture_dimension_2d as usize
    }
}

fn srgb_u8_to_linear(c: u8) -> f32 {
    let f = c as f32 / 255.0;
    if f <= 0.04045 {
        f / 12.92
    } else {
        ((f + 0.055) / 1.055).powf(2.4)
    }
}
