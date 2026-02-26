use smithay::{
    backend::{allocator::dmabuf::Dmabuf, renderer::utils::on_commit_buffer_handler},
    delegate_compositor, delegate_data_device, delegate_dmabuf, delegate_layer_shell,
    delegate_output, delegate_seat, delegate_shm, delegate_xdg_decoration, delegate_xdg_shell,
    desktop::{layer_map_for_output, LayerSurface as DesktopLayerSurface, Window},
    input::{
        pointer::{Focus, GrabStartData},
        Seat, SeatHandler, SeatState,
    },
    output::Output,
    reexports::{
        wayland_protocols::xdg::shell::server::xdg_toplevel,
        wayland_server::{
            protocol::{wl_buffer, wl_output, wl_seat, wl_surface::WlSurface},
            Resource,
        },
    },
    utils::Serial,
    wayland::{
        buffer::BufferHandler,
        compositor::{
            get_parent, is_sync_subsurface, with_states, CompositorClientState, CompositorHandler,
            CompositorState,
        },
        dmabuf::{DmabufGlobal, DmabufHandler, DmabufState, ImportNotifier},
        output::OutputHandler,
        selection::{
            data_device::{
                set_data_device_focus, ClientDndGrabHandler, DataDeviceHandler, DataDeviceState,
                ServerDndGrabHandler,
            },
            SelectionHandler,
        },
        shell::{
            wlr_layer::{Layer, LayerSurface, WlrLayerShellHandler, WlrLayerShellState},
            xdg::{
                decoration::XdgDecorationHandler, PopupSurface, PositionerState, ToplevelSurface,
                XdgShellHandler, XdgShellState, XdgToplevelSurfaceData,
            },
        },
        shm::{ShmHandler, ShmState},
    },
};
use tracing::info;

use crate::{
    grabs::{MoveSurfaceGrab, ResizeSurfaceGrab},
    state::{ClientState, TomoeState},
};

// Buffer handler
impl BufferHandler for TomoeState {
    fn buffer_destroyed(&mut self, _buffer: &wl_buffer::WlBuffer) {}
}

// Compositor handler
impl CompositorHandler for TomoeState {
    fn compositor_state(&mut self) -> &mut CompositorState {
        &mut self.compositor_state
    }

    fn client_compositor_state<'a>(
        &self,
        client: &'a smithay::reexports::wayland_server::Client,
    ) -> &'a CompositorClientState {
        &client.get_data::<ClientState>().unwrap().compositor_state
    }

    fn commit(&mut self, surface: &WlSurface) {
        // Handle buffer commits - this is essential for rendering
        on_commit_buffer_handler::<Self>(surface);

        // Handle subsurfaces - find the root surface
        if !is_sync_subsurface(surface) {
            let mut root = surface.clone();
            while let Some(parent) = get_parent(&root) {
                root = parent;
            }

            // Find the window for this surface and call on_commit
            if let Some(window) = self
                .space
                .elements()
                .find(|w| {
                    w.toplevel()
                        .map(|t| t.wl_surface() == &root)
                        .unwrap_or(false)
                })
                .cloned()
            {
                window.on_commit();
            }
        }

        // Handle initial configure for toplevels
        if let Some(window) = self
            .space
            .elements()
            .find(|w| {
                w.toplevel()
                    .map(|t| t.wl_surface() == surface)
                    .unwrap_or(false)
            })
            .cloned()
        {
            let initial_configure_sent = with_states(surface, |states| {
                states
                    .data_map
                    .get::<XdgToplevelSurfaceData>()
                    .unwrap()
                    .lock()
                    .unwrap()
                    .initial_configure_sent
            });

            if !initial_configure_sent {
                window.toplevel().unwrap().send_configure();
            }
        }

        // Handle layer surface commits
        for output in self.space.outputs().cloned().collect::<Vec<_>>() {
            let mut layer_map = layer_map_for_output(&output);

            // Check if this surface belongs to any layer surface
            let layer_surface = layer_map
                .layers()
                .find(|l| l.wl_surface() == surface)
                .cloned();

            if let Some(layer_surface) = layer_surface {
                // Check if this is the initial commit (before first configure)
                let initial_configure_sent = with_states(surface, |states| {
                    states
                        .data_map
                        .get::<smithay::wayland::shell::wlr_layer::LayerSurfaceData>()
                        .map(|data| data.lock().unwrap().initial_configure_sent)
                        .unwrap_or(true)
                });

                // Always arrange on layer surface commits to recalculate geometry
                layer_map.arrange();

                // If initial configure hasn't been sent, send it now
                // arrange() calculates the size but doesn't send configure for initial commit
                if !initial_configure_sent {
                    layer_surface.layer_surface().send_pending_configure();

                    // If this layer surface requests keyboard focus, give it focus
                    // This is important for apps like wofi that need keyboard input
                    if layer_surface.can_receive_keyboard_focus() {
                        let serial = smithay::utils::SERIAL_COUNTER.next_serial();
                        let keyboard = self.seat.get_keyboard().unwrap();
                        keyboard.set_focus(self, Some(surface.clone()), serial);
                        tracing::info!("Layer surface requested keyboard focus, granting focus");
                    }
                }

                // Update tiling for exclusive zones
                drop(layer_map);
                self.update_tiling_for_layer_shells(&output);

                break;
            }
        }

        // Handle popup commits
        self.popups.commit(surface);
    }
}

// XDG Shell handler
impl XdgShellHandler for TomoeState {
    fn xdg_shell_state(&mut self) -> &mut XdgShellState {
        &mut self.xdg_shell_state
    }

    fn new_toplevel(&mut self, surface: ToplevelSurface) {
        info!("New toplevel surface created");

        // Create window
        let window = Window::new_wayland_window(surface);

        // Add to tiling layout (this will configure the window size)
        self.tiling.add_window(window.clone());

        // Map the window in the space at its tiling position
        let positions = self.tiling.calculate_positions();
        for (w, pos) in positions {
            self.space.map_element(w.clone(), pos, false);
        }

        // Set keyboard focus to the new window
        let serial = smithay::utils::SERIAL_COUNTER.next_serial();
        let keyboard = self.seat.get_keyboard().unwrap();
        if let Some(toplevel) = window.toplevel() {
            keyboard.set_focus(self, Some(toplevel.wl_surface().clone()), serial);
        }
    }

    fn new_popup(&mut self, surface: PopupSurface, _positioner: PositionerState) {
        info!("New popup surface created");
        let _ = self.popups.track_popup(surface.into());
    }

    fn grab(&mut self, _surface: PopupSurface, _seat: wl_seat::WlSeat, _serial: Serial) {
        // Popup grab handling
    }

    fn reposition_request(
        &mut self,
        _surface: PopupSurface,
        _positioner: PositionerState,
        _token: u32,
    ) {
        // Popup reposition handling
    }

    fn move_request(&mut self, surface: ToplevelSurface, seat: wl_seat::WlSeat, serial: Serial) {
        tracing::info!("Move request for toplevel, serial={:?}", serial);
        self.handle_move_request(surface, seat, serial);
    }

    fn resize_request(
        &mut self,
        surface: ToplevelSurface,
        seat: wl_seat::WlSeat,
        serial: Serial,
        edges: xdg_toplevel::ResizeEdge,
    ) {
        tracing::info!(
            "Resize request for toplevel, serial={:?}, edges={:?}",
            serial,
            edges
        );
        self.handle_resize_request(surface, seat, serial, edges);
    }

    fn maximize_request(&mut self, surface: ToplevelSurface) {
        info!("Maximize request for toplevel");

        // Get the output size - clone the output geometry first to avoid borrow issues
        let output_geo = self
            .space
            .outputs()
            .next()
            .and_then(|o| self.space.output_geometry(o));

        if let Some(geo) = output_geo {
            surface.with_pending_state(|state| {
                state.size = Some(geo.size);
                state.states.set(xdg_toplevel::State::Maximized);
            });
            surface.send_pending_configure();

            // Move window to origin
            if let Some(window) = self.find_window(&surface) {
                self.space.map_element(window, (0, 0), true);
            }
        }
    }

    fn unmaximize_request(&mut self, surface: ToplevelSurface) {
        info!("Unmaximize request for toplevel");
        surface.with_pending_state(|state| {
            state.states.unset(xdg_toplevel::State::Maximized);
            state.size = None;
        });
        surface.send_pending_configure();
    }

    fn fullscreen_request(
        &mut self,
        surface: ToplevelSurface,
        _output: Option<wl_output::WlOutput>,
    ) {
        info!("Fullscreen request for toplevel");

        // Get the output size - clone the output geometry first to avoid borrow issues
        let output_geo = self
            .space
            .outputs()
            .next()
            .and_then(|o| self.space.output_geometry(o));

        if let Some(geo) = output_geo {
            surface.with_pending_state(|state| {
                state.size = Some(geo.size);
                state.states.set(xdg_toplevel::State::Fullscreen);
            });
            surface.send_pending_configure();

            if let Some(window) = self.find_window(&surface) {
                self.space.map_element(window, (0, 0), true);
            }
        }
    }

    fn unfullscreen_request(&mut self, surface: ToplevelSurface) {
        info!("Unfullscreen request for toplevel");
        surface.with_pending_state(|state| {
            state.states.unset(xdg_toplevel::State::Fullscreen);
            state.size = None;
        });
        surface.send_pending_configure();
    }

    fn minimize_request(&mut self, surface: ToplevelSurface) {
        info!("Minimize request for toplevel");
        // For now just unmap the window
        if let Some(window) = self.find_window(&surface) {
            self.space.unmap_elem(&window);
        }
    }

    fn toplevel_destroyed(&mut self, surface: ToplevelSurface) {
        info!("Toplevel surface destroyed");
        if let Some(window) = self.find_window(&surface) {
            // Remove from tiling layout
            self.tiling.remove_window(&window);
            self.space.unmap_elem(&window);

            // Update positions for remaining windows
            let positions = self.tiling.calculate_positions();
            for (w, pos) in positions {
                self.space.map_element(w.clone(), pos, false);
            }

            // Update focus to the new focused window in tiling
            let serial = smithay::utils::SERIAL_COUNTER.next_serial();
            let keyboard = self.seat.get_keyboard().unwrap();
            if let Some(focused) = self.tiling.focused_window() {
                if let Some(toplevel) = focused.toplevel() {
                    keyboard.set_focus(self, Some(toplevel.wl_surface().clone()), serial);
                }
            } else {
                keyboard.set_focus(self, None, serial);
            }
        }
    }

    fn popup_destroyed(&mut self, _surface: PopupSurface) {}
}

impl TomoeState {
    /// Find a window by its toplevel surface
    pub fn find_window(&self, surface: &ToplevelSurface) -> Option<Window> {
        self.space
            .elements()
            .find(|w| {
                w.toplevel()
                    .map(|t| t.wl_surface() == surface.wl_surface())
                    .unwrap_or(false)
            })
            .cloned()
    }

    /// Handle move request from client
    fn handle_move_request(
        &mut self,
        surface: ToplevelSurface,
        seat: wl_seat::WlSeat,
        serial: Serial,
    ) {
        let seat = Seat::from_resource(&seat).unwrap();
        let wl_surface = surface.wl_surface().clone();

        let pointer = match seat.get_pointer() {
            Some(p) => p,
            None => return,
        };

        // Try to get grab start data, or create our own
        let start_data = if let Some(data) = check_grab(&seat, &wl_surface, serial) {
            data
        } else {
            // Create start data from current pointer state
            tracing::debug!("Creating fallback grab start data");
            let location = pointer.current_location();
            GrabStartData {
                focus: pointer.current_focus().map(|f| (f, location)),
                button: 0x110, // BTN_LEFT
                location,
            }
        };

        if let Some(window) = self.find_window(&surface) {
            let initial_window_location = self.space.element_location(&window).unwrap_or_default();

            let grab = MoveSurfaceGrab {
                start_data,
                window,
                initial_window_location,
            };

            pointer.set_grab(self, grab, serial, Focus::Clear);
            tracing::debug!("Move grab set successfully");
        }
    }

    /// Handle resize request from client
    fn handle_resize_request(
        &mut self,
        surface: ToplevelSurface,
        seat: wl_seat::WlSeat,
        serial: Serial,
        edges: xdg_toplevel::ResizeEdge,
    ) {
        let seat = Seat::from_resource(&seat).unwrap();
        let wl_surface = surface.wl_surface().clone();

        let pointer = match seat.get_pointer() {
            Some(p) => p,
            None => return,
        };

        // Try to get grab start data, or create our own
        let start_data = if let Some(data) = check_grab(&seat, &wl_surface, serial) {
            data
        } else {
            // Create start data from current pointer state
            tracing::debug!("Creating fallback grab start data for resize");
            let location = pointer.current_location();
            GrabStartData {
                focus: pointer.current_focus().map(|f| (f, location)),
                button: 0x110, // BTN_LEFT
                location,
            }
        };

        if let Some(window) = self.find_window(&surface) {
            let initial_window_location = self.space.element_location(&window).unwrap_or_default();
            let initial_window_size = window.geometry().size;

            surface.with_pending_state(|state| {
                state.states.set(xdg_toplevel::State::Resizing);
            });
            surface.send_pending_configure();

            let grab = ResizeSurfaceGrab {
                start_data,
                window,
                edges,
                initial_window_location,
                initial_window_size,
                last_window_size: initial_window_size,
            };

            pointer.set_grab(self, grab, serial, Focus::Clear);
            tracing::debug!("Resize grab set successfully");
        }
    }

    /// Update tiling layout to respect layer shell exclusive zones
    pub fn update_tiling_for_layer_shells(&mut self, output: &Output) {
        let layer_map = layer_map_for_output(output);
        let non_exclusive = layer_map.non_exclusive_zone();
        drop(layer_map);

        // Update tiling layout with the available area (after layer shells reserve their space)
        self.tiling.set_available_area(non_exclusive);
        self.tiling.reconfigure_all();

        // Update window positions
        let positions = self.tiling.calculate_positions();
        for (window, pos) in positions {
            self.space.map_element(window.clone(), pos, false);
        }
    }
}

/// Check if a grab should be initiated - returns start data for the grab
fn check_grab(
    seat: &Seat<TomoeState>,
    surface: &WlSurface,
    serial: Serial,
) -> Option<GrabStartData<TomoeState>> {
    let pointer = seat.get_pointer()?;

    // Check if there's an active grab (ClickGrab from button press)
    // The serial should match the button press that initiated this request
    if !pointer.has_grab(serial) {
        tracing::debug!("Grab check failed: no grab for serial {:?}", serial);
        // Fall back to checking if there's any active grab
        if !pointer.is_grabbed() {
            tracing::debug!("Grab check failed: pointer not grabbed at all");
            return None;
        }
    }

    let start_data = pointer.grab_start_data();
    if start_data.is_none() {
        tracing::debug!("Grab check failed: no grab start data");
        return None;
    }
    let start_data = start_data.unwrap();

    tracing::debug!(
        "Grab start_data: focus={:?}, location={:?}",
        start_data.focus.is_some(),
        start_data.location
    );

    // If there's no focus in the grab, we can still proceed
    // (the grab might have been created without focus tracking)
    if let Some((focus_surface, _)) = start_data.focus.as_ref() {
        // Check the focus surface is from the same client as the requesting surface
        if !focus_surface.id().same_client_as(&surface.id()) {
            tracing::debug!("Grab check failed: surface client mismatch");
            return None;
        }
    }

    Some(start_data)
}

// XDG Decoration handler
impl XdgDecorationHandler for TomoeState {
    fn new_decoration(&mut self, toplevel: ToplevelSurface) {
        use smithay::reexports::wayland_protocols::xdg::decoration::zv1::server::zxdg_toplevel_decoration_v1::Mode;

        // Request client-side decorations (we don't render server-side decorations yet)
        // TODO: Implement server-side decorations and make this configurable
        toplevel.with_pending_state(|state| {
            state.decoration_mode = Some(Mode::ClientSide);
        });
        toplevel.send_pending_configure();
    }

    fn request_mode(
        &mut self,
        toplevel: ToplevelSurface,
        mode: smithay::reexports::wayland_protocols::xdg::decoration::zv1::server::zxdg_toplevel_decoration_v1::Mode,
    ) {
        use smithay::reexports::wayland_protocols::xdg::decoration::zv1::server::zxdg_toplevel_decoration_v1::Mode;

        // For now, always force client-side decorations
        // TODO: Make this configurable
        let _ = mode; // Ignore requested mode
        toplevel.with_pending_state(|state| {
            state.decoration_mode = Some(Mode::ClientSide);
        });
        toplevel.send_pending_configure();
    }

    fn unset_mode(&mut self, toplevel: ToplevelSurface) {
        use smithay::reexports::wayland_protocols::xdg::decoration::zv1::server::zxdg_toplevel_decoration_v1::Mode;

        toplevel.with_pending_state(|state| {
            state.decoration_mode = Some(Mode::ClientSide);
        });
        toplevel.send_pending_configure();
    }
}

// Seat handler
impl SeatHandler for TomoeState {
    type KeyboardFocus = WlSurface;
    type PointerFocus = WlSurface;
    type TouchFocus = WlSurface;

    fn seat_state(&mut self) -> &mut SeatState<Self> {
        &mut self.seat_state
    }

    fn focus_changed(&mut self, seat: &Seat<Self>, focused: Option<&WlSurface>) {
        let dh = &self.display_handle;
        let focus = focused.and_then(|s| dh.get_client(s.id()).ok());
        set_data_device_focus(dh, seat, focus);
    }

    fn cursor_image(
        &mut self,
        _seat: &Seat<Self>,
        _image: smithay::input::pointer::CursorImageStatus,
    ) {
    }
}

// Selection handler (required for DataDeviceHandler)
impl SelectionHandler for TomoeState {
    type SelectionUserData = ();
}

// Data device handler
impl DataDeviceHandler for TomoeState {
    fn data_device_state(&self) -> &DataDeviceState {
        &self.data_device_state
    }
}

impl ClientDndGrabHandler for TomoeState {}
impl ServerDndGrabHandler for TomoeState {}

// SHM handler
impl ShmHandler for TomoeState {
    fn shm_state(&self) -> &ShmState {
        &self.shm_state
    }
}

// DMA-BUF handler
impl DmabufHandler for TomoeState {
    fn dmabuf_state(&mut self) -> &mut DmabufState {
        &mut self.dmabuf_state
    }

    fn dmabuf_imported(
        &mut self,
        _global: &DmabufGlobal,
        dmabuf: Dmabuf,
        notifier: ImportNotifier,
    ) {
        // For now, accept all dmabufs - in a real compositor you'd validate with the renderer
        self.dmabuf_imported = Some(dmabuf);
        let _ = notifier.successful::<TomoeState>();
    }
}

// Output handler
impl OutputHandler for TomoeState {}

// Layer shell handler
impl WlrLayerShellHandler for TomoeState {
    fn shell_state(&mut self) -> &mut WlrLayerShellState {
        &mut self.layer_shell_state
    }

    fn new_layer_surface(
        &mut self,
        surface: LayerSurface,
        output: Option<wl_output::WlOutput>,
        layer: Layer,
        namespace: String,
    ) {
        info!(
            "New layer surface: namespace={}, layer={:?}",
            namespace, layer
        );

        // Get the output for this layer surface
        let output = output
            .as_ref()
            .and_then(Output::from_resource)
            .or_else(|| self.space.outputs().next().cloned());

        let Some(output) = output else {
            tracing::warn!("No output available for layer surface");
            return;
        };

        // Wrap in desktop LayerSurface
        let desktop_surface = DesktopLayerSurface::new(surface, namespace.clone());

        // Get the layer map for this output and insert the surface
        let mut layer_map = layer_map_for_output(&output);

        // Map the layer surface
        if let Err(e) = layer_map.map_layer(&desktop_surface) {
            tracing::error!("Failed to map layer surface: {:?}", e);
            return;
        }

        // Arrange all layers - this calculates positions and handles exclusive zones
        layer_map.arrange();

        // Get the non-exclusive zone (area available for windows after layer shells reserve space)
        let non_exclusive = layer_map.non_exclusive_zone();
        drop(layer_map);

        // Update tiling layout to respect layer shell exclusive zones
        self.update_tiling_for_layer_shells(&output);

        info!(
            "Layer surface mapped, non-exclusive zone: {:?}",
            non_exclusive
        );
    }

    fn layer_destroyed(&mut self, surface: LayerSurface) {
        info!("Layer surface destroyed");

        // Find the output this layer surface was on and remove it
        let mut found_output = None;
        let mut had_keyboard_focus = false;

        for output in self.space.outputs() {
            let mut layer_map = layer_map_for_output(output);
            // We need to find the desktop layer surface that wraps this wlr surface
            let desktop_surface = layer_map
                .layers()
                .find(|l| l.layer_surface() == &surface)
                .cloned();

            if let Some(desktop_surface) = desktop_surface {
                // Check if this layer surface had keyboard focus capability
                had_keyboard_focus = desktop_surface.can_receive_keyboard_focus();
                layer_map.unmap_layer(&desktop_surface);
                found_output = Some(output.clone());
                break;
            }
        }

        // Update tiling layout after layer surface is removed
        if let Some(output) = found_output {
            self.update_tiling_for_layer_shells(&output);
        }

        // If the destroyed layer surface could receive keyboard focus,
        // restore focus to the tiled focused window
        if had_keyboard_focus {
            let serial = smithay::utils::SERIAL_COUNTER.next_serial();
            let keyboard = self.seat.get_keyboard().unwrap();
            if let Some(focused) = self.tiling.focused_window() {
                if let Some(toplevel) = focused.toplevel() {
                    keyboard.set_focus(self, Some(toplevel.wl_surface().clone()), serial);
                    tracing::info!(
                        "Restored keyboard focus to tiled window after layer surface destroyed"
                    );
                }
            } else {
                keyboard.set_focus(self, None, serial);
            }
        }
    }
}

// Delegate macros
delegate_compositor!(TomoeState);
delegate_xdg_shell!(TomoeState);
delegate_xdg_decoration!(TomoeState);
delegate_shm!(TomoeState);
delegate_seat!(TomoeState);
delegate_data_device!(TomoeState);
delegate_output!(TomoeState);
delegate_dmabuf!(TomoeState);
delegate_layer_shell!(TomoeState);
