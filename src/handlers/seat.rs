//! Seat, data device, and selection handlers

use smithay::{
    input::{Seat, SeatHandler, SeatState},
    reexports::wayland_server::{protocol::wl_surface::WlSurface, Resource},
    wayland::selection::{
        data_device::{
            set_data_device_focus, ClientDndGrabHandler, DataDeviceHandler, DataDeviceState,
            ServerDndGrabHandler,
        },
        SelectionHandler,
    },
};

use crate::state::TomoeState;

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
