//! Winit backend for running as a nested compositor
//!
//! This backend allows running Tomoe inside another Wayland or X11 compositor,
//! which is useful for development and testing.

use smithay::{
    backend::{
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

use crate::input::{handle_input, WinitInputEvent};
use crate::state::TomoeState;

/// Initialize the winit backend
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
            make: "tomoe".into(),
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

    // Add output to the layout system (creates a monitor with one workspace)
    state.layout.add_output(output.clone());

    // Set initial tiling layout size (legacy, keep for compatibility)
    state
        .tiling
        .set_output_size(Size::from((initial_size.w as i32, initial_size.h as i32)));

    let mut damage_tracker = OutputDamageTracker::from_output(&output);
    let start_time = state.start_time;

    event_loop
        .handle()
        .insert_source(winit_event_loop, move |event, _, state| match event {
            WinitEvent::Resized { size, .. } => {
                handle_resize(state, &output, size);
            }
            WinitEvent::Focus(focused) => {
                handle_focus_change(state, focused);
            }
            WinitEvent::Input(event) => {
                handle_input(state, WinitInputEvent(event));
            }
            WinitEvent::Redraw => {
                render_frame(
                    state,
                    &mut backend,
                    &output,
                    &mut damage_tracker,
                    start_time,
                );
            }
            WinitEvent::CloseRequested => {
                state.running = false;
            }
        })?;

    Ok(())
}

/// Handle window resize events
fn handle_resize(
    state: &mut TomoeState,
    output: &Output,
    size: Size<i32, smithay::utils::Physical>,
) {
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
    let positions = state.tiling.calculate_positions();
    for (window, pos) in positions {
        state.space.map_element(window.clone(), pos, false);
    }
}

/// Handle compositor window focus changes
fn handle_focus_change(state: &mut TomoeState, focused: bool) {
    if !focused {
        // When the compositor window loses focus, clear client keyboard focus
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
                keyboard.set_focus(state, Some(toplevel.wl_surface().clone()), serial);
            }
        }
    }
}

/// Render a frame
fn render_frame(
    state: &mut TomoeState,
    backend: &mut winit::WinitGraphicsBackend<GlesRenderer>,
    output: &Output,
    damage_tracker: &mut OutputDamageTracker,
    start_time: std::time::Instant,
) {
    let (renderer, mut framebuffer) = backend.bind().unwrap();

    // Collect layer surface render elements
    let layer_map = layer_map_for_output(output);
    let mut layer_elements: Vec<WaylandSurfaceRenderElement<GlesRenderer>> = Vec::new();

    // Get output scale for rendering
    let output_scale = Scale::from(output.current_scale().fractional_scale());

    // Render layer surfaces in order: Background, Bottom, Top, Overlay
    for layer in [
        WlrLayer::Background,
        WlrLayer::Bottom,
        WlrLayer::Top,
        WlrLayer::Overlay,
    ] {
        for layer_surface in layer_map.layers_on(layer) {
            if let Some(geo) = layer_map.layer_geometry(layer_surface) {
                let loc = geo.loc.to_physical_precise_round(output_scale);
                let elements: Vec<WaylandSurfaceRenderElement<GlesRenderer>> =
                    layer_surface.render_elements(renderer, loc, output_scale, 1.0);
                layer_elements.extend(elements);
            }
        }
    }
    drop(layer_map);

    let render_res = render_output::<_, WaylandSurfaceRenderElement<GlesRenderer>, _, _>(
        output,
        renderer,
        &mut framebuffer,
        1.0,
        0,
        [&state.space],
        &layer_elements,
        damage_tracker,
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
            output,
            start_time.elapsed(),
            Some(std::time::Duration::ZERO),
            |_, _| Some(output.clone()),
        );
    });

    // Send frame callbacks to layer surfaces
    let layer_map = layer_map_for_output(output);
    for layer_surface in layer_map.layers() {
        layer_surface.send_frame(
            output,
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
