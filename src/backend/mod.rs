//! Backend implementations for different display servers
//!
//! Currently supports:
//! - Winit (nested compositor for development/testing)

pub mod winit;

pub use winit::init_winit;
