//! Backend implementations for different display servers
//!
//! Supports:
//! - Winit: nested compositor for development/testing (runs inside Wayland/X11)
//! - Udev: native DRM backend for running from TTY as standalone compositor

pub mod udev;
pub mod winit;

pub use udev::init_udev;
pub use winit::init_winit;

use std::env;

/// Backend type selection
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BackendType {
    /// Winit backend - runs nested inside another Wayland or X11 compositor
    Winit,
    /// Udev/DRM backend - runs natively from a TTY
    Udev,
}

impl BackendType {
    /// Auto-detect the appropriate backend based on environment
    ///
    /// - If WAYLAND_DISPLAY or DISPLAY is set, use Winit (nested mode)
    /// - Otherwise, use Udev (native DRM mode)
    pub fn auto_detect() -> Self {
        if env::var("WAYLAND_DISPLAY").is_ok() || env::var("DISPLAY").is_ok() {
            tracing::info!("Detected existing display server, using Winit backend (nested mode)");
            BackendType::Winit
        } else {
            tracing::info!("No display server detected, using Udev backend (native DRM mode)");
            BackendType::Udev
        }
    }
}
