use smithay::{
    backend::allocator::dmabuf::Dmabuf,
    desktop::{PopupManager, Space, Window},
    input::{keyboard::XkbConfig, Seat, SeatState},
    reexports::{
        calloop::{generic::Generic, EventLoop, Interest, LoopSignal, Mode, PostAction},
        wayland_server::{
            backend::{ClientData, ClientId, DisconnectReason},
            Display, DisplayHandle,
        },
    },
    wayland::{
        compositor::{CompositorClientState, CompositorState},
        dmabuf::{DmabufGlobal, DmabufState},
        output::OutputManagerState,
        selection::data_device::DataDeviceState,
        shell::{
            wlr_layer::WlrLayerShellState,
            xdg::{decoration::XdgDecorationState, XdgShellState},
        },
        shm::ShmState,
        socket::ListeningSocketSource,
    },
};
use std::{ffi::OsString, sync::Arc, time::Instant};

use crate::config::Config;
use crate::tiling::TilingLayout;

pub struct TomoeState {
    pub display_handle: DisplayHandle,
    pub socket_name: OsString,
    pub loop_signal: LoopSignal,
    pub running: bool,
    pub start_time: Instant,

    // Configuration
    pub config: Config,

    // Window management
    pub space: Space<Window>,
    pub popups: PopupManager,
    pub tiling: TilingLayout,

    // Protocol state
    pub compositor_state: CompositorState,
    pub xdg_shell_state: XdgShellState,
    pub xdg_decoration_state: XdgDecorationState,
    pub shm_state: ShmState,
    pub output_manager_state: OutputManagerState,
    pub seat_state: SeatState<Self>,
    pub data_device_state: DataDeviceState,
    pub dmabuf_state: DmabufState,
    pub dmabuf_global: Option<DmabufGlobal>,
    pub layer_shell_state: WlrLayerShellState,

    pub seat: Seat<Self>,

    // For dmabuf import validation
    pub dmabuf_imported: Option<Dmabuf>,
}

impl TomoeState {
    pub fn new(event_loop: &mut EventLoop<Self>, display: Display<Self>, config: Config) -> Self {
        let dh = display.handle();

        // Initialize protocol states
        let compositor_state = CompositorState::new::<Self>(&dh);
        let xdg_shell_state = XdgShellState::new::<Self>(&dh);
        let xdg_decoration_state = XdgDecorationState::new::<Self>(&dh);
        let shm_state = ShmState::new::<Self>(&dh, vec![]);
        let output_manager_state = OutputManagerState::new_with_xdg_output::<Self>(&dh);
        let data_device_state = DataDeviceState::new::<Self>(&dh);
        let dmabuf_state = DmabufState::new();
        let layer_shell_state = WlrLayerShellState::new::<Self>(&dh);

        // Initialize seat (input devices)
        let mut seat_state = SeatState::new();
        let mut seat = seat_state.new_wl_seat(&dh, "seat-0");

        // Build XkbConfig from config, using empty strings for system defaults
        let xkb_config = XkbConfig {
            rules: config.keyboard.rules.as_deref().unwrap_or(""),
            model: config.keyboard.model.as_deref().unwrap_or(""),
            layout: config.keyboard.layout.as_deref().unwrap_or(""),
            variant: config.keyboard.variant.as_deref().unwrap_or(""),
            options: config.keyboard.options.clone(),
        };

        tracing::info!(
            "Keyboard config: layout={:?}, variant={:?}, options={:?}",
            config.keyboard.layout,
            config.keyboard.variant,
            config.keyboard.options
        );

        seat.add_keyboard(
            xkb_config,
            config.keyboard.repeat_delay,
            config.keyboard.repeat_rate,
        )
        .expect("Failed to add keyboard");
        seat.add_pointer();

        // Setup wayland socket
        let socket_name = Self::init_wayland_listener(display, event_loop);

        // Initialize tiling layout
        let tiling = TilingLayout::new(
            config.general.gap,
            config.general.margin,
            config.tiling.default_window_width,
        );

        Self {
            display_handle: dh,
            socket_name,
            loop_signal: event_loop.get_signal(),
            running: true,
            start_time: Instant::now(),
            config,
            space: Space::default(),
            popups: PopupManager::default(),
            tiling,
            compositor_state,
            xdg_shell_state,
            xdg_decoration_state,
            shm_state,
            output_manager_state,
            seat_state,
            data_device_state,
            dmabuf_state,
            dmabuf_global: None,
            layer_shell_state,
            seat,
            dmabuf_imported: None,
        }
    }

    fn init_wayland_listener(
        display: Display<TomoeState>,
        event_loop: &mut EventLoop<TomoeState>,
    ) -> OsString {
        // Create a listening socket
        let listening_socket = ListeningSocketSource::new_auto().expect("Failed to create socket");
        let socket_name = listening_socket.socket_name().to_os_string();

        // Insert the socket source into the event loop
        event_loop
            .handle()
            .insert_source(listening_socket, move |client_stream, _, state| {
                // Accept the client connection
                state
                    .display_handle
                    .insert_client(client_stream, Arc::new(ClientState::default()))
                    .expect("Failed to insert client");
            })
            .expect("Failed to init wayland listener");

        // Insert the display source into the event loop
        event_loop
            .handle()
            .insert_source(
                Generic::new(display, Interest::READ, Mode::Level),
                |_, display, state| {
                    // SAFETY: we don't drop the display
                    unsafe {
                        display.get_mut().dispatch_clients(state).unwrap();
                    }
                    Ok(PostAction::Continue)
                },
            )
            .expect("Failed to init display source");

        socket_name
    }

    /// Run startup commands from config
    pub fn run_startup_commands(&self) {
        for cmd in &self.config.on_start {
            tracing::info!("Running startup command: {}", cmd);
            if let Err(e) = Self::spawn_command(cmd, &self.socket_name) {
                tracing::warn!("Failed to run startup command '{}': {}", cmd, e);
            }
        }
    }

    /// Spawn a command with the correct WAYLAND_DISPLAY set
    pub fn spawn_command(
        command: &str,
        socket_name: &OsString,
    ) -> Result<(), Box<dyn std::error::Error>> {
        use std::process::Command;

        let parts: Vec<&str> = command.split_whitespace().collect();
        if parts.is_empty() {
            return Err("Empty command".into());
        }

        let mut cmd = Command::new(parts[0]);
        if parts.len() > 1 {
            cmd.args(&parts[1..]);
        }

        cmd.env("WAYLAND_DISPLAY", socket_name);

        // Spawn detached
        cmd.spawn()?;

        Ok(())
    }
}

#[derive(Default)]
pub struct ClientState {
    pub compositor_state: CompositorClientState,
}

impl ClientData for ClientState {
    fn initialized(&self, _client_id: ClientId) {}
    fn disconnected(&self, _client_id: ClientId, _reason: DisconnectReason) {}
}
