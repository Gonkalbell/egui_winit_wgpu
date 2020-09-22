#![allow(deprecated)] // legacy implement_vertex macro

use {
    egui::{
        paint::{PaintJobs, Vertex},
    },
    std::{borrow::Cow, mem, slice},
    vk_shader_macros as spv,
    wgpu::util::{self, DeviceExt},
};

const VERT_SHADER: &[u32] = spv::include_glsl!("src/shaders/egui.vert");
const FRAG_SHADER: &[u32] = spv::include_glsl!("src/shaders/egui.frag");

pub struct Painter {
    pipeline: wgpu::RenderPipeline,
    vertex_buffers: Vec<wgpu::Buffer>,
    index_buffers: Vec<wgpu::Buffer>,
    uniform_buffer: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
    texture_bind_group_layout: wgpu::BindGroupLayout,
    current_texture: Option<(u64, wgpu::BindGroup)>,
}

impl Painter {
    pub fn new(device: &wgpu::Device, output_format: wgpu::TextureFormat) -> Painter {
        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some(concat!(file!(), "::bind_group_layout")),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    count: None,
                    ty: wgpu::BindingType::UniformBuffer { dynamic: false, min_binding_size: None },
                    visibility: wgpu::ShaderStage::VERTEX,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    count: None,
                    ty: wgpu::BindingType::Sampler { comparison: false },
                    visibility: wgpu::ShaderStage::FRAGMENT,
                },
            ],
        });
        let texture_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some(concat!(file!(), "::bind_group_layout")),
                entries: &[wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    count: None,
                    ty: wgpu::BindingType::SampledTexture {
                        component_type: wgpu::TextureComponentType::Float,
                        dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    visibility: wgpu::ShaderStage::FRAGMENT,
                }],
            });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some(concat!(file!(), "::pipeline_layout")),
            bind_group_layouts: &[&bind_group_layout, &texture_bind_group_layout],
            push_constant_ranges: &[],
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some(concat!(file!(), "::pipeline")),
            layout: Some(&pipeline_layout),
            vertex_stage: wgpu::ProgrammableStageDescriptor {
                module: &device.create_shader_module(wgpu::ShaderModuleSource::SpirV(
                    Cow::Borrowed(VERT_SHADER),
                )),
                entry_point: "main",
            },
            fragment_stage: Some(wgpu::ProgrammableStageDescriptor {
                module: &device.create_shader_module(wgpu::ShaderModuleSource::SpirV(
                    Cow::Borrowed(FRAG_SHADER),
                )),
                entry_point: "main",
            }),
            rasterization_state: Some(wgpu::RasterizationStateDescriptor::default()),
            primitive_topology: wgpu::PrimitiveTopology::TriangleList,
            color_states: &[wgpu::ColorStateDescriptor {
                format: output_format,
                color_blend: wgpu::BlendDescriptor {
                    src_factor: wgpu::BlendFactor::One,
                    dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
                    operation: wgpu::BlendOperation::Add,
                },
                alpha_blend: wgpu::BlendDescriptor {
                    src_factor: wgpu::BlendFactor::One,
                    dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
                    operation: wgpu::BlendOperation::Add,
                },
                write_mask: wgpu::ColorWrite::ALL,
            }],
            depth_stencil_state: None,
            vertex_state: wgpu::VertexStateDescriptor {
                index_format: wgpu::IndexFormat::Uint32,
                vertex_buffers: &[wgpu::VertexBufferDescriptor {
                    stride: mem::size_of::<egui::paint::Vertex>() as _,
                    step_mode: wgpu::InputStepMode::Vertex,
                    attributes: &wgpu::vertex_attr_array![0 => Float2, 1 => Ushort2, 2 => Uchar4],
                }],
            },
            sample_count: 1,
            sample_mask: !0,
            alpha_to_coverage_enabled: false,
        });

        let uniform_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some(concat!(file!(), "::uniform_buffer")),
            size: mem::size_of::<Uniform>() as _,
            usage: wgpu::BufferUsage::UNIFORM | wgpu::BufferUsage::COPY_DST,
            mapped_at_creation: false,
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor::default());
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some(concat!(file!(), "::bind_group")),
            layout: &bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::Buffer(uniform_buffer.slice(..)),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&sampler),
                },
            ],
        });
        Painter {
            pipeline,
            vertex_buffers: Vec::new(),
            index_buffers: Vec::new(),
            uniform_buffer,
            bind_group,
            texture_bind_group_layout,
            current_texture: None,
        }
    }

    fn upload_texture(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        texture: &egui::Texture,
    ) {
        {
            match self.current_texture.as_ref() {
                Some(&(id, _)) if id == texture.id => return,
                _ => (),
            };
        }

        let size =
            wgpu::Extent3d { width: texture.width as _, height: texture.height as _, depth: 1 };

        let gpu_texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some(concat!(file!(), "::texture")),
            size,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::R8Unorm,
            usage: wgpu::TextureUsage::SAMPLED | wgpu::TextureUsage::COPY_DST,
        });

        queue.write_texture(
            wgpu::TextureCopyView {
                texture: &gpu_texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
            },
            texture.pixels.as_slice(),
            wgpu::TextureDataLayout {
                offset: 0,
                bytes_per_row: (texture.pixels.len() / texture.height) as _,
                rows_per_image: texture.height as _,
            },
            size,
        );

        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some(concat!(file!(), "::bind_group")),
            layout: &self.texture_bind_group_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(
                    &gpu_texture.create_view(&wgpu::TextureViewDescriptor::default()),
                ),
            }],
        });
        self.current_texture = Some((texture.id, bind_group));
    }

    pub fn paint_jobs<'r>(
        &'r mut self,
        jobs: PaintJobs,
        physical_size: winit::dpi::PhysicalSize<f32>,
        scale_factor: f64,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        rpass: &mut wgpu::RenderPass<'r>,
        texture: &egui::Texture,
    ) {
        let logical_size = physical_size.to_logical(scale_factor);
        self.upload_texture(device, queue, texture);
        let textures_bind_group = match self.current_texture.as_ref() {
            Some((_, bg)) => bg,
            _ => unreachable!(),
        };

        queue.write_buffer(
            &self.uniform_buffer,
            0,
            bytemuck::bytes_of(&Uniform {
                screen_size: [logical_size.width, logical_size.height],
                tex_size: [texture.width as f32, texture.height as f32],
            }),
        );

        self.vertex_buffers.clear();
        self.index_buffers.clear();

        rpass.set_pipeline(&self.pipeline);
        rpass.set_bind_group(0, &self.bind_group, &[]);
        rpass.set_bind_group(1, &textures_bind_group, &[]);
        for (_, triangles) in jobs.iter() {
            // Safety: VertexPod is a transparent wrapper over Vertex, which _should_ already be a POD type
            let vertex_pods = unsafe {
                slice::from_raw_parts(
                    triangles.vertices.as_ptr() as *const VertexPod,
                    triangles.vertices.len(),
                )
            };
            self.vertex_buffers.push(device.create_buffer_init(&util::BufferInitDescriptor {
                label: None,
                contents: &bytemuck::cast_slice(vertex_pods),
                usage: wgpu::BufferUsage::VERTEX,
            }));
            self.index_buffers.push(device.create_buffer_init(&util::BufferInitDescriptor {
                label: None,
                contents: &bytemuck::cast_slice(triangles.indices.as_slice()),
                usage: wgpu::BufferUsage::INDEX,
            }));
        }

        for (((clip_rect, triangles), vertex_buffer), index_buffer) in
            jobs.iter().zip(self.vertex_buffers.iter()).zip(self.index_buffers.iter())
        {
            let clip_width = clip_rect.max.x - clip_rect.min.x;
            let clip_height = clip_rect.max.y - clip_rect.min.y;
            let scale = scale_factor as f32;
            rpass.set_scissor_rect(
                (scale * clip_rect.min.x) as _,
                (scale * clip_rect.min.y) as _,
                (scale * clip_width) as _,
                (scale * clip_height) as _,
            );
            rpass.set_vertex_buffer(0, vertex_buffer.slice(..));
            rpass.set_index_buffer(index_buffer.slice(..));
            rpass.draw_indexed(0..triangles.indices.len() as _, 0, 0..1)
        }
    }
}

#[repr(transparent)]
#[derive(Debug, Copy, Clone)]
struct VertexPod(Vertex);

unsafe impl bytemuck::Zeroable for VertexPod {}
unsafe impl bytemuck::Pod for VertexPod {}

#[repr(C)]
#[derive(Debug, Copy, Clone)]
struct Uniform {
    screen_size: [f32; 2],
    tex_size: [f32; 2],
}

unsafe impl bytemuck::Zeroable for Uniform {}
unsafe impl bytemuck::Pod for Uniform {}
