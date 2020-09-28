use futures::executor;
use std::{iter, time::Instant};

use crate::{
    storage::{FileStorage, WindowSettings},
    *,
};

pub use egui::app::{App, Backend, RunMode, Storage};

const EGUI_MEMORY_KEY: &str = "egui";
const WINDOW_KEY: &str = "window";

pub struct WGpuBackend {
    frame_times: egui::MovementTracker<f32>,
    quit: bool,
    run_mode: RunMode,
}

impl WGpuBackend {
    pub fn new(run_mode: RunMode) -> Self {
        Self { frame_times: egui::MovementTracker::new(1000, 1.0), quit: false, run_mode }
    }
}

impl Backend for WGpuBackend {
    fn run_mode(&self) -> RunMode {
        self.run_mode
    }

    fn set_run_mode(&mut self, run_mode: RunMode) {
        self.run_mode = run_mode;
    }

    fn cpu_time(&self) -> f32 {
        self.frame_times.average().unwrap_or_default()
    }

    fn fps(&self) -> f32 {
        1.0 / self.frame_times.mean_time_interval().unwrap_or_default()
    }

    fn quit(&mut self) {
        self.quit = true;
    }
}

/// Run an egui app
pub fn run(
    title: &str,
    run_mode: RunMode,
    mut storage: FileStorage,
    mut app: impl App + 'static,
) -> ! {
    let event_loop = winit::event_loop::EventLoop::new();
    let mut window = winit::window::WindowBuilder::new()
        .with_decorations(true)
        .with_resizable(true)
        .with_title(title)
        .with_transparent(false);

    let window_settings: Option<WindowSettings> = egui::app::get_value(&storage, WINDOW_KEY);
    if let Some(window_settings) = &window_settings {
        window = window_settings.initialize_size(window);
    }

    let window = window.build(&event_loop).unwrap();
    let instance = wgpu::Instance::new(wgpu::BackendBit::PRIMARY);
    let surface = unsafe { instance.create_surface(&window) };

    let adapter = executor::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::Default,
        compatible_surface: Some(&surface),
    }))
    .unwrap();

    let (device, queue) = executor::block_on(adapter.request_device(
        &wgpu::DeviceDescriptor {
            features: wgpu::Features::empty(),
            limits: wgpu::Limits::default(),
            shader_validation: false,
        },
        None,
    ))
    .unwrap();

    let size = window.inner_size();
    let mut sc_desc = wgpu::SwapChainDescriptor {
        usage: wgpu::TextureUsage::OUTPUT_ATTACHMENT,
        format: wgpu::TextureFormat::Bgra8Unorm,
        width: size.width as u32,
        height: size.height as u32,
        present_mode: wgpu::PresentMode::Mailbox,
    };
    let mut swap_chain = device.create_swap_chain(&surface, &sc_desc);

    if let Some(window_settings) = &window_settings {
        window_settings.restore_positions(&window);
    }

    let mut ctx = egui::Context::new();
    *ctx.memory() = egui::app::get_value(&storage, EGUI_MEMORY_KEY).unwrap_or_default();

    let mut painter = Painter::new(&device, sc_desc.format);
    let mut raw_input = make_raw_input(&window);

    // used to keep track of time for animations
    let start_time = Instant::now();
    let mut runner = WGpuBackend::new(run_mode);
    let mut clipboard = init_clipboard();
    let mut modifier_state = winit::event::ModifiersState::empty();

    event_loop.run(move |event, _, control_flow| {
        *control_flow = winit::event_loop::ControlFlow::Wait;

        match event {
            winit::event::Event::RedrawEventsCleared => {
                let egui_start = Instant::now();
                raw_input.time = start_time.elapsed().as_nanos() as f64 * 1e-9;
                raw_input.seconds_since_midnight = Some(local_time_of_day());

                let mut ui = ctx.begin_frame(raw_input.take());
                app.ui(&mut ui, &mut runner);
                let (output, paint_jobs) = ctx.end_frame();

                let frame_time = (Instant::now() - egui_start).as_secs_f64() as f32;
                runner.frame_times.add(raw_input.time, frame_time);

                let frame = match swap_chain.get_current_frame() {
                    Ok(frame) => frame,
                    Err(e) => {
                        eprintln!("Dropped frame with error: {}", e);
                        return;
                    }
                };
                let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some(concat!(file!(), "::encoder")),
                });
                {
                    let mut rpass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                        color_attachments: &[wgpu::RenderPassColorAttachmentDescriptor {
                            attachment: &frame.output.view,
                            resolve_target: None,
                            ops: wgpu::Operations {
                                load: wgpu::LoadOp::Clear(wgpu::Color {
                                    r: 0.0,
                                    g: 0.0,
                                    b: 0.0,
                                    a: 0.0,
                                }),
                                store: true,
                            },
                        }],
                        depth_stencil_attachment: None,
                    });
                    painter.paint_jobs(
                        paint_jobs,
                        window.inner_size().cast(),
                        window.scale_factor(),
                        &device,
                        &queue,
                        &mut rpass,
                        ctx.texture(),
                    );
                }
                queue.submit(iter::once(encoder.finish()));

                *control_flow = if runner.quit {
                    winit::event_loop::ControlFlow::Exit
                } else if runner.run_mode() == RunMode::Continuous {
                    window.request_redraw();
                    winit::event_loop::ControlFlow::Poll
                } else {
                    if output.needs_repaint {
                        window.request_redraw();
                    }
                    winit::event_loop::ControlFlow::Wait
                };

                handle_output(output, &window, clipboard.as_mut());
            }
            winit::event::Event::WindowEvent { event, .. } => {
                if let winit::event::WindowEvent::Resized(size) = event {
                    sc_desc.width = size.width;
                    sc_desc.height = size.height;
                    swap_chain = device.create_swap_chain(&surface, &sc_desc);
                }
                input_to_egui(
                    event,
                    clipboard.as_mut(),
                    &mut raw_input,
                    control_flow,
                    &mut modifier_state,
                );
                window.request_redraw(); // TODO: maybe only on some events?
            }
            winit::event::Event::LoopDestroyed => {
                egui::app::set_value(
                    &mut storage,
                    WINDOW_KEY,
                    &WindowSettings::from_display(&window),
                );
                egui::app::set_value(&mut storage, EGUI_MEMORY_KEY, &*ctx.memory());
                app.on_exit(&mut storage);
                storage.save();
            }
            _ => (),
        }
    });
}
