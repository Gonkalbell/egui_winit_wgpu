#![warn(clippy::all)]
#![allow(clippy::single_match)]

mod backend;
mod painter;
pub mod storage;

use winit::{
    dpi,
    event::{ModifiersState, VirtualKeyCode},
    event_loop::ControlFlow,
    window,
};

pub use backend::*;
pub use painter::Painter;

use {clipboard::ClipboardProvider, egui::*};

pub use clipboard::ClipboardContext; // TODO: remove

pub fn input_to_egui(
    event: winit::event::WindowEvent,
    clipboard: Option<&mut ClipboardContext>,
    raw_input: &mut RawInput,
    control_flow: &mut ControlFlow,
    modifier_state: &mut ModifiersState,
) {
    use winit::event::WindowEvent::*;
    match event {
        CloseRequested | Destroyed => *control_flow = ControlFlow::Exit,

        Resized(physical_size) => {
            raw_input.screen_size =
                egui::vec2(physical_size.width as f32, physical_size.height as f32)
                    / raw_input.pixels_per_point.unwrap();
        }

        ScaleFactorChanged { scale_factor, new_inner_size } => {
            raw_input.pixels_per_point = Some(scale_factor as f32);
            raw_input.screen_size =
                egui::vec2(new_inner_size.width as f32, new_inner_size.height as f32)
                    / (scale_factor as f32);
        }

        MouseInput { state, .. } => {
            raw_input.mouse_down = state == winit::event::ElementState::Pressed;
        }
        CursorMoved { position, .. } => {
            raw_input.mouse_pos = Some(pos2(
                position.x as f32 / raw_input.pixels_per_point.unwrap(),
                position.y as f32 / raw_input.pixels_per_point.unwrap(),
            ));
        }
        CursorLeft { .. } => {
            raw_input.mouse_pos = None;
        }
        ReceivedCharacter(ch) => {
            if printable_char(ch) {
                raw_input.events.push(Event::Text(ch.to_string()));
            }
        }
        ModifiersChanged(input) => *modifier_state = input,
        KeyboardInput { input, .. } => {
            if let Some(virtual_keycode) = input.virtual_keycode {
                if modifier_state.logo() && virtual_keycode == VirtualKeyCode::Q {
                    *control_flow = ControlFlow::Exit;
                }

                match virtual_keycode {
                    VirtualKeyCode::Paste => {
                        if let Some(clipboard) = clipboard {
                            match clipboard.get_contents() {
                                Ok(contents) => {
                                    raw_input.events.push(Event::Text(contents));
                                }
                                Err(err) => {
                                    eprintln!("Paste error: {}", err);
                                }
                            }
                        }
                    }
                    VirtualKeyCode::Copy => raw_input.events.push(Event::Copy),
                    VirtualKeyCode::Cut => raw_input.events.push(Event::Cut),
                    _ => {
                        if let Some(key) = translate_virtual_key_code(virtual_keycode) {
                            raw_input.events.push(Event::Key {
                                key,
                                pressed: input.state == winit::event::ElementState::Pressed,
                            });
                        }
                    }
                }
            }
        }
        MouseWheel { delta, .. } => {
            match delta {
                winit::event::MouseScrollDelta::LineDelta(x, y) => {
                    let line_height = 24.0; // TODO
                    raw_input.scroll_delta = vec2(x, y) * line_height;
                }
                winit::event::MouseScrollDelta::PixelDelta(delta) => {
                    // Actually point delta
                    raw_input.scroll_delta = vec2(delta.x as f32, delta.y as f32);
                }
            }
        }
        _ => {
            // dbg!(event);
        }
    }
}

/// Winit sends special keys (backspace, delete, F1, ...) as characters.
/// Ignore those.
/// We also ignore '\r', '\n', '\t'.
/// Newlines are handled by the `Key::Enter` event.
fn printable_char(chr: char) -> bool {
    let is_in_private_use_area = '\u{e000}' <= chr && chr <= '\u{f8ff}'
        || '\u{f0000}' <= chr && chr <= '\u{ffffd}'
        || '\u{100000}' <= chr && chr <= '\u{10fffd}';

    !is_in_private_use_area && !chr.is_ascii_control()
}

pub fn translate_virtual_key_code(key: VirtualKeyCode) -> Option<egui::Key> {
    use VirtualKeyCode::*;

    Some(match key {
        Escape => Key::Escape,
        Insert => Key::Insert,
        Home => Key::Home,
        Delete => Key::Delete,
        End => Key::End,
        PageDown => Key::PageDown,
        PageUp => Key::PageUp,
        Left => Key::Left,
        Up => Key::Up,
        Right => Key::Right,
        Down => Key::Down,
        Back => Key::Backspace,
        Return => Key::Enter,
        // Space => Key::Space,
        Tab => Key::Tab,

        LAlt | RAlt => Key::Alt,
        LShift | RShift => Key::Shift,
        LControl | RControl => Key::Control,
        LWin | RWin => Key::Logo,

        _ => {
            return None;
        }
    })
}

pub fn translate_cursor(cursor_icon: egui::CursorIcon) -> window::CursorIcon {
    match cursor_icon {
        CursorIcon::Default => window::CursorIcon::Default,
        CursorIcon::PointingHand => window::CursorIcon::Hand,
        CursorIcon::ResizeHorizontal => window::CursorIcon::EwResize,
        CursorIcon::ResizeNeSw => window::CursorIcon::NeswResize,
        CursorIcon::ResizeNwSe => window::CursorIcon::NwseResize,
        CursorIcon::ResizeVertical => window::CursorIcon::NsResize,
        CursorIcon::Text => window::CursorIcon::Text,
    }
}

pub fn handle_output(
    output: egui::Output,
    window: &window::Window,
    clipboard: Option<&mut ClipboardContext>,
) {
    if let Some(url) = output.open_url {
        if let Err(err) = webbrowser::open(&url) {
            eprintln!("Failed to open url: {}", err); // TODO show error in imgui
        }
    }

    if !output.copied_text.is_empty() {
        if let Some(clipboard) = clipboard {
            if let Err(err) = clipboard.set_contents(output.copied_text) {
                eprintln!("Copy/Cut error: {}", err);
            }
        }
    }

    window.set_cursor_icon(translate_cursor(output.cursor_icon));
}

pub fn init_clipboard() -> Option<ClipboardContext> {
    match ClipboardContext::new() {
        Ok(clipboard) => Some(clipboard),
        Err(err) => {
            eprintln!("Failed to initialize clipboard: {}", err);
            None
        }
    }
}

// ----------------------------------------------------------------------------

/// Time of day as seconds since midnight. Used for clock in demo app.
pub fn local_time_of_day() -> f64 {
    use chrono::Timelike;
    let time = chrono::Local::now().time();
    time.num_seconds_from_midnight() as f64 + 1e-9 * (time.nanosecond() as f64)
}

pub fn make_raw_input(window: &window::Window) -> egui::RawInput {
    let pixels_per_point = window.scale_factor() as f32;
    egui::RawInput {
        screen_size: {
            let dpi::PhysicalSize { width, height } = window.inner_size();
            vec2(width as f32, height as f32) / pixels_per_point
        },
        pixels_per_point: Some(pixels_per_point),
        ..Default::default()
    }
}
