//! XDG shell and decoration handlers

use smithay::{
    desktop::Window,
    input::{
        pointer::{Focus, GrabStartData},
        Seat,
    },
    reexports::{
        wayland_protocols::xdg::shell::server::xdg_toplevel,
        wayland_server::{
            protocol::{wl_output, wl_seat, wl_surface::WlSurface},
            Resource,
        },
    },
    utils::Serial,
    wayland::shell::xdg::{
        decoration::XdgDecorationHandler, PopupSurface, PositionerState, ToplevelSurface,
        XdgShellHandler, XdgShellState,
    },
};
use tracing::info;

use crate::input::grabs::{MoveSurfaceGrab, ResizeSurfaceGrab};
use crate::state::TomoeState;

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
}

/// Check if a grab should be initiated - returns start data for the grab
pub fn check_grab(
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
