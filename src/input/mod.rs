//! Input handling for keyboard, pointer, and other input devices
//!
//! This module handles:
//! - Keyboard input and keybind processing
//! - Pointer/mouse input
//! - Touch input (future)
//!
//! Supports both winit (nested) and libinput (native) backends.

pub mod grabs;

use smithay::{
    backend::input::{
        AbsolutePositionEvent, ButtonState, Event, InputBackend, InputEvent, KeyState,
        KeyboardKeyEvent, PointerAxisEvent, PointerButtonEvent, PointerMotionEvent,
    },
    input::{
        keyboard::{xkb::Keysym, FilterResult},
        pointer::{AxisFrame, ButtonEvent, MotionEvent, RelativeMotionEvent},
    },
    utils::SERIAL_COUNTER,
};

use crate::config::{KeyAction, Modifiers, ParsedKeybind};
use crate::state::TomoeState;

/// Wrapper for winit input events
pub struct WinitInputEvent<I>(pub I);

/// Handle input events from the winit backend
pub fn handle_input<I>(state: &mut TomoeState, event: WinitInputEvent<I>)
where
    I: Into<InputEvent<smithay::backend::winit::WinitInput>>,
{
    handle_input_event_winit(state, event.0.into());
}

/// Handle input events from libinput (used by udev backend)
pub fn handle_libinput_event(
    state: &mut TomoeState,
    event: InputEvent<smithay::backend::libinput::LibinputInputBackend>,
) {
    handle_input_event_libinput(state, event);
}

/// Handle a winit input event
fn handle_input_event_winit(
    state: &mut TomoeState,
    event: InputEvent<smithay::backend::winit::WinitInput>,
) {
    match event {
        InputEvent::Keyboard { event } => handle_keyboard(state, event),
        InputEvent::PointerMotionAbsolute { event } => {
            handle_pointer_motion_absolute_winit(state, event)
        }
        InputEvent::PointerButton { event } => handle_pointer_button(state, event),
        InputEvent::PointerAxis { event } => handle_pointer_axis(state, event),
        _ => {}
    }
}

/// Handle a libinput input event
fn handle_input_event_libinput(
    state: &mut TomoeState,
    event: InputEvent<smithay::backend::libinput::LibinputInputBackend>,
) {
    match event {
        InputEvent::Keyboard { event } => handle_keyboard(state, event),
        InputEvent::PointerMotion { event } => handle_pointer_motion_relative(state, event),
        InputEvent::PointerMotionAbsolute { event } => {
            handle_pointer_motion_absolute_libinput(state, event)
        }
        InputEvent::PointerButton { event } => handle_pointer_button(state, event),
        InputEvent::PointerAxis { event } => handle_pointer_axis(state, event),
        InputEvent::DeviceAdded { device } => {
            tracing::info!("Input device added: {:?}", device.name());
        }
        InputEvent::DeviceRemoved { device } => {
            tracing::info!("Input device removed: {:?}", device.name());
        }
        _ => {}
    }
}

/// Handle keyboard input (backend-agnostic)
fn handle_keyboard<B: InputBackend, E: KeyboardKeyEvent<B>>(state: &mut TomoeState, event: E) {
    let serial = SERIAL_COUNTER.next_serial();
    let time = Event::time_msec(&event);
    let keyboard = state.seat.get_keyboard().unwrap();

    // Check for keybind match on key press
    if event.state() == KeyState::Pressed {
        let keybind_action = keyboard.input::<Option<KeyAction>, _>(
            state,
            event.key_code(),
            event.state(),
            serial,
            time,
            |state, modifiers, keysym| {
                // Build current modifiers
                let current_mods = Modifiers {
                    ctrl: modifiers.ctrl,
                    alt: modifiers.alt,
                    shift: modifiers.shift,
                    logo: modifiers.logo,
                };

                // Get the key name
                let key_name = keysym_to_key_name(keysym.modified_sym());

                // Check against configured keybinds
                for (bind_str, action) in &state.config.keybinds {
                    if let Some(parsed) = ParsedKeybind::parse(bind_str) {
                        if parsed.modifiers == current_mods
                            && parsed.key.eq_ignore_ascii_case(&key_name)
                        {
                            return FilterResult::Intercept(Some(action.clone()));
                        }
                    }
                }

                FilterResult::Forward
            },
        );

        // Handle the action if we intercepted a keybind
        if let Some(Some(action)) = keybind_action {
            handle_keybind_action(state, action);
            return;
        }
    } else {
        // Key release - just forward
        keyboard.input::<(), _>(
            state,
            event.key_code(),
            event.state(),
            serial,
            time,
            |_, _, _| FilterResult::Forward,
        );
    }
}

/// Handle absolute pointer motion (winit backend)
fn handle_pointer_motion_absolute_winit<
    E: AbsolutePositionEvent<smithay::backend::winit::WinitInput>,
>(
    state: &mut TomoeState,
    event: E,
) {
    let output = match state.space.outputs().next() {
        Some(o) => o.clone(),
        None => return,
    };
    let output_geo = match state.space.output_geometry(&output) {
        Some(geo) => geo,
        None => return,
    };
    let pos = event.position_transformed(output_geo.size) + output_geo.loc.to_f64();

    let serial = SERIAL_COUNTER.next_serial();
    let pointer = state.seat.get_pointer().unwrap();

    let under = state
        .surface_under(pos)
        .map(|(surface, loc)| (surface, loc.to_f64()));

    pointer.motion(
        state,
        under,
        &MotionEvent {
            location: pos,
            serial,
            time: Event::time_msec(&event),
        },
    );
    pointer.frame(state);
}

/// Handle absolute pointer motion (libinput backend)
fn handle_pointer_motion_absolute_libinput<
    E: AbsolutePositionEvent<smithay::backend::libinput::LibinputInputBackend>,
>(
    state: &mut TomoeState,
    event: E,
) {
    let output = match state.space.outputs().next() {
        Some(o) => o.clone(),
        None => return,
    };
    let output_geo = match state.space.output_geometry(&output) {
        Some(geo) => geo,
        None => return,
    };
    let pos = event.position_transformed(output_geo.size) + output_geo.loc.to_f64();

    let serial = SERIAL_COUNTER.next_serial();
    let pointer = state.seat.get_pointer().unwrap();

    let under = state
        .surface_under(pos)
        .map(|(surface, loc)| (surface, loc.to_f64()));

    pointer.motion(
        state,
        under,
        &MotionEvent {
            location: pos,
            serial,
            time: Event::time_msec(&event),
        },
    );
    pointer.frame(state);
}

/// Handle relative pointer motion (libinput - for mice/trackpads)
fn handle_pointer_motion_relative<B: InputBackend, E: PointerMotionEvent<B>>(
    state: &mut TomoeState,
    event: E,
) {
    let pointer = state.seat.get_pointer().unwrap();
    let serial = SERIAL_COUNTER.next_serial();

    // Get current location and add delta
    let mut new_pos = pointer.current_location();
    new_pos.x += event.delta_x();
    new_pos.y += event.delta_y();

    // Clamp to output bounds
    if let Some(output) = state.space.outputs().next() {
        if let Some(output_geo) = state.space.output_geometry(output) {
            new_pos.x = new_pos
                .x
                .max(0.0)
                .min((output_geo.loc.x + output_geo.size.w) as f64);
            new_pos.y = new_pos
                .y
                .max(0.0)
                .min((output_geo.loc.y + output_geo.size.h) as f64);
        }
    }

    let under = state
        .surface_under(new_pos)
        .map(|(surface, loc)| (surface, loc.to_f64()));

    pointer.motion(
        state,
        under.clone(),
        &MotionEvent {
            location: new_pos,
            serial,
            time: Event::time_msec(&event),
        },
    );

    // Also send relative motion event for games/apps that need it
    pointer.relative_motion(
        state,
        under,
        &RelativeMotionEvent {
            delta: (event.delta_x(), event.delta_y()).into(),
            delta_unaccel: (event.delta_x_unaccel(), event.delta_y_unaccel()).into(),
            utime: event.time(),
        },
    );

    pointer.frame(state);
}

/// Handle pointer button events (backend-agnostic)
fn handle_pointer_button<B: InputBackend, E: PointerButtonEvent<B>>(
    state: &mut TomoeState,
    event: E,
) {
    let serial = SERIAL_COUNTER.next_serial();
    let pointer = state.seat.get_pointer().unwrap();
    let keyboard = state.seat.get_keyboard().unwrap();

    // On button press, set keyboard focus to the surface under the pointer
    if event.state() == ButtonState::Pressed {
        let pos = pointer.current_location();

        // Check what's under the pointer (layer surfaces first, then windows)
        if let Some((_surface, keyboard_focus)) = state.focus_target_under(pos) {
            // Set keyboard focus if the surface can receive it
            if let Some(focus_surface) = keyboard_focus {
                keyboard.set_focus(state, Some(focus_surface), serial);
            }
        } else {
            keyboard.set_focus(state, None, serial);
        }
    }

    pointer.button(
        state,
        &ButtonEvent {
            button: event.button_code(),
            state: event.state(),
            serial,
            time: Event::time_msec(&event),
        },
    );
    pointer.frame(state);
}

/// Handle pointer axis (scroll) events (backend-agnostic)
fn handle_pointer_axis<B: InputBackend, E: PointerAxisEvent<B>>(state: &mut TomoeState, event: E) {
    let pointer = state.seat.get_pointer().unwrap();

    let horizontal_amount = event
        .amount(smithay::backend::input::Axis::Horizontal)
        .unwrap_or_else(|| {
            event
                .amount_v120(smithay::backend::input::Axis::Horizontal)
                .unwrap_or(0.0)
                * 3.0
        });
    let vertical_amount = event
        .amount(smithay::backend::input::Axis::Vertical)
        .unwrap_or_else(|| {
            event
                .amount_v120(smithay::backend::input::Axis::Vertical)
                .unwrap_or(0.0)
                * 3.0
        });

    let horizontal_amount_discrete = event.amount_v120(smithay::backend::input::Axis::Horizontal);
    let vertical_amount_discrete = event.amount_v120(smithay::backend::input::Axis::Vertical);

    let mut frame = AxisFrame::new(Event::time_msec(&event));

    use smithay::backend::input::Axis;

    if horizontal_amount != 0.0 {
        frame = frame.value(Axis::Horizontal, horizontal_amount);
        if let Some(discrete) = horizontal_amount_discrete {
            frame = frame.v120(Axis::Horizontal, discrete as i32);
        }
    }
    if vertical_amount != 0.0 {
        frame = frame.value(Axis::Vertical, vertical_amount);
        if let Some(discrete) = vertical_amount_discrete {
            frame = frame.v120(Axis::Vertical, discrete as i32);
        }
    }

    if event.source() == smithay::backend::input::AxisSource::Finger {
        if event.amount(Axis::Horizontal) == Some(0.0) {
            frame = frame.stop(Axis::Horizontal);
        }
        if event.amount(Axis::Vertical) == Some(0.0) {
            frame = frame.stop(Axis::Vertical);
        }
    }

    frame = frame.source(event.source());

    pointer.axis(state, frame);
    pointer.frame(state);
}

/// Convert xkb keysym to a key name string
fn keysym_to_key_name(keysym: Keysym) -> String {
    match keysym {
        Keysym::Return => "Return".to_string(),
        Keysym::Escape => "Escape".to_string(),
        Keysym::BackSpace => "BackSpace".to_string(),
        Keysym::Tab => "Tab".to_string(),
        Keysym::space => "space".to_string(),
        Keysym::Left => "Left".to_string(),
        Keysym::Right => "Right".to_string(),
        Keysym::Up => "Up".to_string(),
        Keysym::Down => "Down".to_string(),
        Keysym::Home => "Home".to_string(),
        Keysym::End => "End".to_string(),
        Keysym::Page_Up => "Page_Up".to_string(),
        Keysym::Page_Down => "Page_Down".to_string(),
        Keysym::Delete => "Delete".to_string(),
        Keysym::Insert => "Insert".to_string(),
        Keysym::F1 => "F1".to_string(),
        Keysym::F2 => "F2".to_string(),
        Keysym::F3 => "F3".to_string(),
        Keysym::F4 => "F4".to_string(),
        Keysym::F5 => "F5".to_string(),
        Keysym::F6 => "F6".to_string(),
        Keysym::F7 => "F7".to_string(),
        Keysym::F8 => "F8".to_string(),
        Keysym::F9 => "F9".to_string(),
        Keysym::F10 => "F10".to_string(),
        Keysym::F11 => "F11".to_string(),
        Keysym::F12 => "F12".to_string(),
        _ => {
            // Try to get the character representation
            if let Some(ch) = keysym.key_char() {
                ch.to_string()
            } else {
                // Fallback: use the keysym name
                format!("{:?}", keysym)
            }
        }
    }
}

/// Handle a keybind action
fn handle_keybind_action(state: &mut TomoeState, action: KeyAction) {
    tracing::info!("Keybind action: {:?}", action);

    match action {
        KeyAction::Spawn { command } => {
            if let Err(e) = TomoeState::spawn_command(&command, &state.socket_name) {
                tracing::error!("Failed to spawn '{}': {}", command, e);
            }
        }
        KeyAction::Close => {
            if let Some(window) = state.tiling.focused_window().cloned() {
                if let Some(toplevel) = window.toplevel() {
                    toplevel.send_close();
                }
            }
        }
        KeyAction::FocusNext => {
            state.tiling.focus_next();
            update_focus_from_tiling(state);
            update_window_positions(state);
        }
        KeyAction::FocusPrev => {
            state.tiling.focus_prev();
            update_focus_from_tiling(state);
            update_window_positions(state);
        }
        KeyAction::ScrollLeft => {
            state.tiling.scroll_left();
            update_window_positions(state);
        }
        KeyAction::ScrollRight => {
            state.tiling.scroll_right();
            update_window_positions(state);
        }
        KeyAction::Fullscreen => {
            if let Some(window) = state.tiling.focused_window() {
                if let Some(toplevel) = window.toplevel() {
                    // Toggle fullscreen
                    toplevel.with_pending_state(|pending| {
                        use smithay::reexports::wayland_protocols::xdg::shell::server::xdg_toplevel;
                        if pending.states.contains(xdg_toplevel::State::Fullscreen) {
                            pending.states.unset(xdg_toplevel::State::Fullscreen);
                            pending.size = None;
                        } else {
                            if let Some(output) = state.space.outputs().next() {
                                if let Some(geo) = state.space.output_geometry(output) {
                                    pending.size = Some(geo.size);
                                }
                            }
                            pending.states.set(xdg_toplevel::State::Fullscreen);
                        }
                    });
                    toplevel.send_pending_configure();
                }
            }
        }
        KeyAction::Quit => {
            tracing::info!("Quit requested via keybind");
            state.running = false;
        }

        // Workspace actions
        KeyAction::SwitchToWorkspace { workspace } => {
            // User-facing workspaces are 1-indexed, internal are 0-indexed
            let idx = workspace.saturating_sub(1);
            if state.layout.switch_to_workspace(idx) {
                tracing::info!("Switched to workspace {}", workspace);
                state.layout.reconfigure_all();
            }
        }
        KeyAction::NextWorkspace => {
            if state.layout.switch_to_next_workspace() {
                tracing::info!("Switched to next workspace");
                state.layout.reconfigure_all();
            }
        }
        KeyAction::PrevWorkspace => {
            if state.layout.switch_to_prev_workspace() {
                tracing::info!("Switched to previous workspace");
                state.layout.reconfigure_all();
            }
        }
        KeyAction::MoveToWorkspace { workspace } => {
            // User-facing workspaces are 1-indexed, internal are 0-indexed
            let idx = workspace.saturating_sub(1);
            if state.layout.move_focused_to_workspace(idx) {
                tracing::info!("Moved window to workspace {}", workspace);
                state.layout.reconfigure_all();
            }
        }
        KeyAction::NewWorkspace => {
            if let Some(ws_id) = state.layout.create_workspace(true) {
                tracing::info!("Created new workspace: {:?}", ws_id);
                state.layout.reconfigure_all();
            }
        }
    }
}

/// Update keyboard focus to match tiling layout's focused window
fn update_focus_from_tiling(state: &mut TomoeState) {
    let serial = SERIAL_COUNTER.next_serial();
    let keyboard = state.seat.get_keyboard().unwrap();

    if let Some(window) = state.tiling.focused_window() {
        if let Some(toplevel) = window.toplevel() {
            keyboard.set_focus(state, Some(toplevel.wl_surface().clone()), serial);
        }
    } else {
        keyboard.set_focus(state, None, serial);
    }
}

/// Update window positions based on tiling layout
pub fn update_window_positions(state: &mut TomoeState) {
    let positions = state.tiling.calculate_positions();
    for (window, pos) in positions {
        state.space.map_element(window.clone(), pos, false);
    }
}
