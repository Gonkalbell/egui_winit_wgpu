#![allow(deprecated)] // legacy implement_vertex macro

use {
    egui::{
        math::clamp,
        paint::{PaintJobs, Triangles, Vertex},
        Rect,
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
    current_texture_id: Option<u64>,
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
                // wgpu::BindGroupLayoutEntry {
                //     binding: 1,
                //     count: None,
                //     ty: wgpu::BindingType::SampledTexture {
                //         component_type: wgpu::TextureComponentType::Float,
                //         dimension: wgpu::TextureViewDimension::D2,
                //         multisampled: false,
                //     },
                //     visibility: wgpu::ShaderStage::FRAGMENT,
                // },
                // wgpu::BindGroupLayoutEntry {
                //     binding: 2,
                //     count: None,
                //     ty: wgpu::BindingType::Sampler { comparison: false },
                //     visibility: wgpu::ShaderStage::FRAGMENT,
                // },
            ],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some(concat!(file!(), "::pipeline_layout")),
            bind_group_layouts: &[&bind_group_layout],
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
                    src_factor: wgpu::BlendFactor::SrcAlpha,
                    dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
                    operation: wgpu::BlendOperation::Add,
                },
                alpha_blend: wgpu::BlendDescriptor {
                    src_factor: wgpu::BlendFactor::OneMinusDstAlpha,
                    dst_factor: wgpu::BlendFactor::One,
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
        // let format = texture::UncompressedFloatFormat::U8;
        // let mipmaps = texture::MipmapsOption::NoMipmap;
        // let texture =
        //     texture::texture2d::Texture2d::with_format(facade, pixels, format, mipmaps).unwrap();

        let uniform_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some(concat!(file!(), "::uniform_buffer")),
            size: mem::size_of::<Uniform>() as _,
            usage: wgpu::BufferUsage::UNIFORM | wgpu::BufferUsage::COPY_DST,
            mapped_at_creation: false,
        });
        // let texture = device.create_texture(&wgpu::TextureDescriptor{
        //     label: Some(concat!(file!(), "::texture")),
        //     size: ,
        //     mip_level_count: (),
        //     sample_count: (),
        //     dimension: (),
        //     format: (),
        //     usage: wgpu::TextureUsage::SAMPLED,
        // });
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some(concat!(file!(), "::bind_group")),
            layout: &bind_group_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::Buffer(uniform_buffer.slice(..)),
            }],
        });
        Painter {
            pipeline,
            vertex_buffers: Vec::new(),
            index_buffers: Vec::new(),
            uniform_buffer,
            bind_group,
            current_texture_id: None,
        }
    }

    fn upload_texture(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        texture: &egui::Texture,
    ) {
        if self.current_texture_id == Some(texture.id) {
            return; // No change
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
            usage: wgpu::TextureUsage::SAMPLED,
        });

        queue.write_texture(
            wgpu::TextureCopyView {
                texture: &gpu_texture,
                mip_level: 1,
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

        //     let pixels: Vec<Vec<u8>> = texture
        //         .pixels
        //         .chunks(texture.width as usize)
        //         .map(|row| row.to_vec())
        //         .collect();

        //     let format = texture::UncompressedFloatFormat::U8;
        //     let mipmaps = texture::MipmapsOption::NoMipmap;
        //     self.texture =
        //         texture::texture2d::Texture2d::with_format(facade, pixels, format, mipmaps).unwrap();
        self.current_texture_id = Some(texture.id);
    }

    pub fn paint_jobs<'r>(
        &'r mut self,
        jobs: PaintJobs,
        window_size: winit::dpi::LogicalSize<f32>,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        rpass: &mut wgpu::RenderPass<'r>,
        texture: &egui::Texture,
    ) {
        // self.upload_texture(texture);

        queue.write_buffer(
            &self.uniform_buffer,
            0,
            bytemuck::bytes_of(&Uniform {
                screen_size: [window_size.width, window_size.height],
                tex_size: [texture.width as f32, texture.height as f32],
            }),
        );

        self.vertex_buffers.clear();
        self.index_buffers.clear();

        rpass.set_pipeline(&self.pipeline);
        for (clip_rect, triangles) in jobs.iter() {
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
            rpass.set_vertex_buffer(0, vertex_buffer.slice(..));
            rpass.set_index_buffer(index_buffer.slice(..));
            rpass.set_bind_group(0, &self.bind_group, &[]);
            rpass.draw_indexed(0..triangles.indices.len() as _, 0, 0..1)
        }
    }

    // #[inline(never)] // Easier profiling
    // fn paint_job(
    //     &mut self,
    //     target: &mut Frame,
    //     display: &glium::Display,
    //     clip_rect: Rect,
    //     triangles: &Triangles,
    //     texture: &egui::Texture,
    // ) {
    //     debug_assert!(triangles.is_valid());

    //     let vertex_buffer = {
    //         #[derive(Copy, Clone)]
    //         struct Vertex {
    //             a_pos: [f32; 2],
    //             a_srgba: [u8; 4],
    //             a_tc: [u16; 2],
    //         }
    //         implement_vertex!(Vertex, a_pos, a_srgba, a_tc);

    //         let vertices: Vec<Vertex> = triangles
    //             .vertices
    //             .iter()
    //             .map(|v| Vertex {
    //                 a_pos: [v.pos.x, v.pos.y],
    //                 a_srgba: [v.color.r, v.color.g, v.color.b, v.color.a],
    //                 a_tc: [v.uv.0, v.uv.1],
    //             })
    //             .collect();

    //         glium::VertexBuffer::new(display, &vertices).unwrap()
    //     };

    //     let indices: Vec<u32> = triangles.indices.iter().map(|idx| *idx as u32).collect();

    //     let index_buffer =
    //         glium::IndexBuffer::new(display, PrimitiveType::TrianglesList, &indices).unwrap();

    //     let pixels_per_point = display.gl_window().window().scale_factor() as f32;
    //     let (width_pixels, height_pixels) = display.get_framebuffer_dimensions();
    //     let width_points = width_pixels as f32 / pixels_per_point;
    //     let height_points = height_pixels as f32 / pixels_per_point;

    //     let uniforms = uniform! {
    //         u_screen_size: [width_points, height_points],
    //         u_tex_size: [texture.width as f32, texture.height as f32],
    //         u_sampler: &self.texture,
    //     };

    //     // Emilib outputs colors with premultiplied alpha:
    //     let blend_func = glium::BlendingFunction::Addition {
    //         source: glium::LinearBlendingFactor::One,
    //         destination: glium::LinearBlendingFactor::OneMinusSourceAlpha,
    //     };
    //     let blend = glium::Blend {
    //         color: blend_func,
    //         alpha: blend_func,
    //         ..Default::default()
    //     };

    //     let clip_min_x = pixels_per_point * clip_rect.min.x;
    //     let clip_min_y = pixels_per_point * clip_rect.min.y;
    //     let clip_max_x = pixels_per_point * clip_rect.max.x;
    //     let clip_max_y = pixels_per_point * clip_rect.max.y;
    //     let clip_min_x = clamp(clip_min_x, 0.0..=width_pixels as f32);
    //     let clip_min_y = clamp(clip_min_y, 0.0..=height_pixels as f32);
    //     let clip_max_x = clamp(clip_max_x, clip_min_x..=width_pixels as f32);
    //     let clip_max_y = clamp(clip_max_y, clip_min_y..=height_pixels as f32);
    //     let clip_min_x = clip_min_x.round() as u32;
    //     let clip_min_y = clip_min_y.round() as u32;
    //     let clip_max_x = clip_max_x.round() as u32;
    //     let clip_max_y = clip_max_y.round() as u32;

    //     let params = glium::DrawParameters {
    //         blend,
    //         scissor: Some(glium::Rect {
    //             left: clip_min_x,
    //             bottom: height_pixels - clip_max_y,
    //             width: clip_max_x - clip_min_x,
    //             height: clip_max_y - clip_min_y,
    //         }),
    //         ..Default::default()
    //     };

    //     target
    //         .draw(
    //             &vertex_buffer,
    //             &index_buffer,
    //             &self.program,
    //             &uniforms,
    //             &params,
    //         )
    //         .unwrap();
    // }
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
