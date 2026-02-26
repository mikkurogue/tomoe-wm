//! Wayland protocol handlers
//!
//! This module contains all the Smithay handler implementations and delegate macros
//! for the various Wayland protocols supported by Tomoe.

mod compositor;
mod layer_shell;
mod output;
mod seat;
mod xdg_shell;

use smithay::{
    delegate_compositor, delegate_data_device, delegate_dmabuf, delegate_layer_shell,
    delegate_output, delegate_seat, delegate_shm, delegate_xdg_decoration, delegate_xdg_shell,
};

use crate::state::TomoeState;

// Delegate macros - these wire up the protocol dispatching to our handler implementations
delegate_compositor!(TomoeState);
delegate_xdg_shell!(TomoeState);
delegate_xdg_decoration!(TomoeState);
delegate_shm!(TomoeState);
delegate_seat!(TomoeState);
delegate_data_device!(TomoeState);
delegate_output!(TomoeState);
delegate_dmabuf!(TomoeState);
delegate_layer_shell!(TomoeState);
