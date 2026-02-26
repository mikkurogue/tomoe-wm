mod config;
mod grabs;
mod handlers;
mod state;
mod winit;
mod wm;

use smithay::reexports::{calloop::EventLoop, wayland_server::Display};
use tracing::info;

use crate::config::Config;
use crate::state::TomoeState;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize logging
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    info!("Starting tomoe-wm");

    // Load configuration
    let config = Config::load();
    info!("Config loaded from {:?}", Config::config_path());

    // Create event loop
    let mut event_loop: EventLoop<TomoeState> = EventLoop::try_new()?;

    // Create Wayland display
    let display: Display<TomoeState> = Display::new()?;

    // Initialize compositor state
    let mut state = TomoeState::new(&mut event_loop, display, config);

    // Initialize winit backend
    winit::init_winit(&mut event_loop, &mut state)?;

    // Set WAYLAND_DISPLAY for child processes
    std::env::set_var("WAYLAND_DISPLAY", &state.socket_name);
    info!("Listening on WAYLAND_DISPLAY={:?}", state.socket_name);

    // Run startup commands
    state.run_startup_commands();

    // Run event loop
    event_loop.run(None, &mut state, |state| {
        // This callback runs once per loop iteration
        if !state.running {
            state.loop_signal.stop();
        }
    })?;

    info!("tomoe-wm exiting");
    Ok(())
}
