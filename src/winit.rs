use smithay::{
    backend::{
        input::AbsolutePositionEvent,
        renderer::{
            damage::OutputDamageTracker,
            element::{surface::WaylandSurfaceRenderElement, AsRenderElements},
            gles::GlesRenderer,
            ImportDma,
        },
        winit::{self, WinitEvent},
    },
    desktop::{layer_map_for_output, space::render_output},
    output::{Mode, Output, PhysicalProperties, Subpixel},
    reexports::calloop::EventLoop,
    utils::{Scale, Size, Transform},
    wayland::shell::wlr_layer::Layer as WlrLayer,
};
use tracing::info;

use crate::config::{KeyAction, Modifiers, ParsedKeybind};
use crate::state::TomoeState;

pub fn init_winit(
    event_loop: &mut EventLoop<TomoeState>,
    state: &mut TomoeState,
) -> Result<(), Box<dyn std::error::Error>> {
    let (mut backend, winit_event_loop) = winit::init::<GlesRenderer>()?;

    // Initialize dmabuf support with renderer formats
    {
        let (renderer, _) = backend.bind()?;
        let dmabuf_formats = renderer.dmabuf_formats();
        let dmabuf_global = state
            .dmabuf_state
            .create_global::<TomoeState>(&state.display_handle, dmabuf_formats);
        state.dmabuf_global = Some(dmabuf_global);
        info!("DMA-BUF initialized");
    }

    let initial_size = backend.window_size();
    let mode = Mode {
        size: initial_size,
        refresh: 60_000,
    };

    let output = Output::new(
        "winit".to_string(),
        PhysicalProperties {
            size: (0, 0).into(),
            subpixel: Subpixel::Unknown,
            make: "Smithay".into(),
            model: "Winit".into(),
        },
    );
    output.change_current_state(
        Some(mode),
        Some(Transform::Flipped180),
        None,
        Some((0, 0).into()),
    );
    output.set_preferred(mode);

    output.create_global::<TomoeState>(&state.display_handle);
    state.space.map_output(&output, (0, 0));

    // Set initial tiling layout size
    state
        .tiling
        .set_output_size(Size::from((initial_size.w as i32, initial_size.h as i32)));

    let mut damage_tracker = OutputDamageTracker::from_output(&output);
    let start_time = state.start_time;

    event_loop
        .handle()
        .insert_source(winit_event_loop, move |event, _, state| {
            match event {
                WinitEvent::Resized { size, .. } => {
                    let mode = Mode {
                        size,
                        refresh: 60_000,
                    };
                    output.change_current_state(Some(mode), None, None, None);

                    // Update tiling layout size
                    state
                        .tiling
                        .set_output_size(Size::from((size.w as i32, size.h as i32)));
                    state.tiling.reconfigure_all();

                    // Reposition windows
                    update_window_positions(state);
                }
                WinitEvent::Focus(focused) => {
                    // When the compositor window loses focus, clear client keyboard focus
                    if !focused {
                        tracing::debug!("Compositor window lost focus, clearing client focus");
                        let keyboard = state.seat.get_keyboard().unwrap();
                        let serial = smithay::utils::SERIAL_COUNTER.next_serial();
                        keyboard.set_focus(state, None, serial);

                        // Also clear pointer focus
                        let pointer = state.seat.get_pointer().unwrap();
                        pointer.motion(
                            state,
                            None,
                            &smithay::input::pointer::MotionEvent {
                                location: pointer.current_location(),
                                serial,
                                time: 0,
                            },
                        );
                        pointer.frame(state);
                    } else {
                        // When the compositor window gains focus, restore focus to the tiled focused window
                        tracing::debug!("Compositor window gained focus, restoring client focus");
                        if let Some(window) = state.tiling.focused_window() {
                            if let Some(toplevel) = window.toplevel() {
                                let keyboard = state.seat.get_keyboard().unwrap();
                                let serial = smithay::utils::SERIAL_COUNTER.next_serial();
                                keyboard.set_focus(
                                    state,
                                    Some(toplevel.wl_surface().clone()),
                                    serial,
                                );
                            }
                        }
                    }
                }
                WinitEvent::Input(event) => {
                    handle_input(state, event);
                }
                WinitEvent::Redraw => {
                    let (renderer, mut framebuffer) = backend.bind().unwrap();

                    // Collect layer surface render elements
                    let layer_map = layer_map_for_output(&output);
                    let mut layer_elements: Vec<WaylandSurfaceRenderElement<GlesRenderer>> =
                        Vec::new();

                    // Get output scale for rendering
                    let output_scale = Scale::from(output.current_scale().fractional_scale());

                    // Render layer surfaces in order: Background, Bottom, Top, Overlay
                    // Background and Bottom are rendered below windows (but we use custom_elements which are on top)
                    // So we need to render them as part of the space or handle differently
                    // For now, render all layer surfaces as custom elements (on top of windows)
                    // TODO: Properly layer Background/Bottom under windows

                    let layer_count = layer_map.layers().count();
                    for layer in [
                        WlrLayer::Background,
                        WlrLayer::Bottom,
                        WlrLayer::Top,
                        WlrLayer::Overlay,
                    ] {
                        for layer_surface in layer_map.layers_on(layer) {
                            let geo = layer_map.layer_geometry(layer_surface);
                            tracing::debug!("Layer surface: geo={:?}, layer={:?}", geo, layer);
                            if let Some(geo) = geo {
                                let loc = geo.loc.to_physical_precise_round(output_scale);
                                let elements: Vec<WaylandSurfaceRenderElement<GlesRenderer>> =
                                    layer_surface.render_elements(renderer, loc, output_scale, 1.0);
                                tracing::debug!(
                                    "Layer surface rendered {} elements",
                                    elements.len()
                                );
                                layer_elements.extend(elements);
                            }
                        }
                    }
                    drop(layer_map);

                    if layer_count > 0 {
                        tracing::debug!(
                            "Rendering {} layer surfaces with {} elements",
                            layer_count,
                            layer_elements.len()
                        );
                    }

                    let render_res =
                        render_output::<_, WaylandSurfaceRenderElement<GlesRenderer>, _, _>(
                            &output,
                            renderer,
                            &mut framebuffer,
                            1.0,
                            0,
                            [&state.space],
                            &layer_elements,
                            &mut damage_tracker,
                            [0.1, 0.1, 0.1, 1.0],
                        );
                    drop(framebuffer);

                    if let Err(ref e) = render_res {
                        tracing::error!("Render error: {:?}", e);
                    }

                    if render_res.is_ok() {
                        backend.submit(None).unwrap();
                    }

                    // Send frame callbacks to windows
                    state.space.elements().for_each(|window| {
                        window.send_frame(
                            &output,
                            start_time.elapsed(),
                            Some(std::time::Duration::ZERO),
                            |_, _| Some(output.clone()),
                        );
                    });

                    // Send frame callbacks to layer surfaces
                    let layer_map = layer_map_for_output(&output);
                    for layer_surface in layer_map.layers() {
                        layer_surface.send_frame(
                            &output,
                            start_time.elapsed(),
                            Some(std::time::Duration::ZERO),
                            |_, _| Some(output.clone()),
                        );
                    }

                    state.space.refresh();
                    state.popups.cleanup();

                    // Flush client events - essential for clients to receive protocol messages
                    let _ = state.display_handle.flush_clients();

                    backend.window().request_redraw();
                }
                WinitEvent::CloseRequested => {
                    state.running = false;
                }
            }
        })?;

    Ok(())
}

/// Update window positions based on tiling layout
fn update_window_positions(state: &mut TomoeState) {
    let positions = state.tiling.calculate_positions();
    for (window, pos) in positions {
        state.space.map_element(window.clone(), pos, false);
    }
}

fn handle_input(
    state: &mut TomoeState,
    event: smithay::backend::input::InputEvent<winit::WinitInput>,
) {
    use smithay::backend::input::{
        ButtonState, Event, InputEvent, KeyState, KeyboardKeyEvent, PointerButtonEvent,
    };
    use smithay::input::keyboard::FilterResult;
    use smithay::input::pointer::{ButtonEvent, MotionEvent};

    match event {
        InputEvent::Keyboard { event } => {
            let serial = smithay::utils::SERIAL_COUNTER.next_serial();
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
        InputEvent::PointerMotionAbsolute { event } => {
            use smithay::desktop::WindowSurfaceType;

            let output = state.space.outputs().next().unwrap().clone();
            let output_geo = state.space.output_geometry(&output).unwrap();
            let pos = event.position_transformed(output_geo.size) + output_geo.loc.to_f64();

            let serial = smithay::utils::SERIAL_COUNTER.next_serial();
            let pointer = state.seat.get_pointer().unwrap();

            let under = state.space.element_under(pos).and_then(|(w, window_loc)| {
                let pos_in_window = pos - window_loc.to_f64();
                w.surface_under(pos_in_window, WindowSurfaceType::ALL).map(
                    |(surface, surface_offset)| {
                        let surface_loc = window_loc + surface_offset;
                        (surface, surface_loc.to_f64())
                    },
                )
            });

            pointer.motion(
                state,
                under,
                &MotionEvent {
                    location: pos,
                    serial,
                    time: event.time_msec(),
                },
            );
            pointer.frame(state);
        }
        InputEvent::PointerButton { event } => {
            use smithay::desktop::WindowSurfaceType;

            let serial = smithay::utils::SERIAL_COUNTER.next_serial();
            let pointer = state.seat.get_pointer().unwrap();
            let keyboard = state.seat.get_keyboard().unwrap();

            // On button press, set keyboard focus to the window under the pointer
            if event.state() == ButtonState::Pressed {
                let pos = pointer.current_location();

                let window_info = state.space.element_under(pos).map(|(w, window_loc)| {
                    let pos_in_window = pos - window_loc.to_f64();
                    let focus_surface = w
                        .surface_under(pos_in_window, WindowSurfaceType::TOPLEVEL)
                        .map(|(s, _)| s);
                    (w.clone(), focus_surface)
                });

                if let Some((window, focus_surface)) = window_info {
                    // Raise window and focus it in tiling
                    state.space.raise_element(&window, true);
                    state.tiling.focus_window(&window);

                    if let Some(surface) = focus_surface {
                        keyboard.set_focus(state, Some(surface), serial);
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
                    time: event.time_msec(),
                },
            );
            pointer.frame(state);
        }
        _ => {}
    }
}

/// Convert xkb keysym to a key name string
fn keysym_to_key_name(keysym: smithay::input::keyboard::xkb::Keysym) -> String {
    use smithay::input::keyboard::xkb::Keysym;

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
    }
}

/// Update keyboard focus to match tiling layout's focused window
fn update_focus_from_tiling(state: &mut TomoeState) {
    let serial = smithay::utils::SERIAL_COUNTER.next_serial();
    let keyboard = state.seat.get_keyboard().unwrap();

    if let Some(window) = state.tiling.focused_window() {
        if let Some(toplevel) = window.toplevel() {
            keyboard.set_focus(state, Some(toplevel.wl_surface().clone()), serial);
        }
    } else {
        keyboard.set_focus(state, None, serial);
    }
}
