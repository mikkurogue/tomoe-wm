use smithay::{
    backend::allocator::dmabuf::Dmabuf,
    desktop::{layer_map_for_output, PopupManager, Space, Window, WindowSurfaceType},
    input::{keyboard::XkbConfig, Seat, SeatState},
    reexports::{
        calloop::{generic::Generic, EventLoop, Interest, LoopSignal, Mode, PostAction},
        wayland_server::{
            backend::{ClientData, ClientId, DisconnectReason},
            protocol::wl_surface::WlSurface,
            Display, DisplayHandle,
        },
    },
    utils::{Logical, Point},
    wayland::{
        compositor::{CompositorClientState, CompositorState},
        dmabuf::{DmabufGlobal, DmabufState},
        output::OutputManagerState,
        selection::data_device::DataDeviceState,
        shell::{
            wlr_layer::{Layer as WlrLayer, WlrLayerShellState},
            xdg::{decoration::XdgDecorationState, XdgShellState},
        },
        shm::ShmState,
        socket::ListeningSocketSource,
    },
};
use std::{ffi::OsString, sync::Arc, time::Instant};

use crate::backend::udev::UdevData;
use crate::config::Config;
use crate::cursor::CursorManager;
use crate::wm::tiling::TilingLayout;
use crate::wm::Layout;

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
    pub layout: Layout,

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

    // Backend-specific data
    pub udev_data: Option<UdevData>,

    // Cursor management (for DRM backend software cursor)
    pub cursor_manager: CursorManager,
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

        // Initialize layout manager (will be populated when outputs are added)
        let layout = Layout::new(
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
            layout,
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
            udev_data: None,
            cursor_manager: CursorManager::new(),
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

    /// Find the surface under a position, checking layer surfaces first (top to bottom),
    /// then windows. Returns the surface and its location.
    pub fn surface_under(
        &self,
        pos: Point<f64, Logical>,
    ) -> Option<(WlSurface, Point<i32, Logical>)> {
        let output = self.space.outputs().next()?;
        let layer_map = layer_map_for_output(output);

        // Check layer surfaces from top to bottom (Overlay, Top, Bottom, Background)
        // But for pointer interaction, we only care about Overlay and Top (above windows)
        for layer in [WlrLayer::Overlay, WlrLayer::Top] {
            for layer_surface in layer_map.layers_on(layer) {
                if let Some(geo) = layer_map.layer_geometry(layer_surface) {
                    let surface_loc = geo.loc;
                    let pos_in_surface = pos - surface_loc.to_f64();

                    if let Some((surface, offset)) =
                        layer_surface.surface_under(pos_in_surface, WindowSurfaceType::ALL)
                    {
                        return Some((surface, surface_loc + offset));
                    }
                }
            }
        }

        // No need to hold the layer_map anymore
        drop(layer_map);

        // Check windows in the space
        if let Some((window, window_loc)) = self.space.element_under(pos) {
            let pos_in_window = pos - window_loc.to_f64();
            if let Some((surface, offset)) =
                window.surface_under(pos_in_window, WindowSurfaceType::ALL)
            {
                return Some((surface, window_loc + offset));
            }
        }

        // Check bottom layer surfaces (below windows)
        let layer_map = layer_map_for_output(output);
        for layer in [WlrLayer::Bottom, WlrLayer::Background] {
            for layer_surface in layer_map.layers_on(layer) {
                if let Some(geo) = layer_map.layer_geometry(layer_surface) {
                    let surface_loc = geo.loc;
                    let pos_in_surface = pos - surface_loc.to_f64();

                    if let Some((surface, offset)) =
                        layer_surface.surface_under(pos_in_surface, WindowSurfaceType::ALL)
                    {
                        return Some((surface, surface_loc + offset));
                    }
                }
            }
        }

        None
    }

    /// Find the focus target under a position.
    /// Returns:
    /// - The surface for pointer focus
    /// - An optional surface for keyboard focus (only if the target can receive keyboard focus)
    pub fn focus_target_under(
        &mut self,
        pos: Point<f64, Logical>,
    ) -> Option<(WlSurface, Option<WlSurface>)> {
        let output = self.space.outputs().next()?.clone();
        let layer_map = layer_map_for_output(&output);

        // Check layer surfaces from top to bottom (Overlay, Top)
        for layer in [WlrLayer::Overlay, WlrLayer::Top] {
            for layer_surface in layer_map.layers_on(layer) {
                if let Some(geo) = layer_map.layer_geometry(layer_surface) {
                    let surface_loc = geo.loc;
                    let pos_in_surface = pos - surface_loc.to_f64();

                    if let Some((surface, _offset)) =
                        layer_surface.surface_under(pos_in_surface, WindowSurfaceType::ALL)
                    {
                        // Check if this layer surface can receive keyboard focus
                        let keyboard_focus = if layer_surface.can_receive_keyboard_focus() {
                            Some(layer_surface.wl_surface().clone())
                        } else {
                            None
                        };
                        return Some((surface, keyboard_focus));
                    }
                }
            }
        }

        drop(layer_map);

        // Check windows in the space
        if let Some((window, window_loc)) = self.space.element_under(pos) {
            let pos_in_window = pos - window_loc.to_f64();
            if let Some((surface, _offset)) =
                window.surface_under(pos_in_window, WindowSurfaceType::ALL)
            {
                // Raise window and focus it in tiling
                let window = window.clone();
                self.space.raise_element(&window, true);
                self.tiling.focus_window(&window);

                // Windows can always receive keyboard focus
                let keyboard_focus = window.toplevel().map(|t| t.wl_surface().clone());
                return Some((surface, keyboard_focus));
            }
        }

        // Check bottom layer surfaces (Bottom, Background)
        let layer_map = layer_map_for_output(&output);
        for layer in [WlrLayer::Bottom, WlrLayer::Background] {
            for layer_surface in layer_map.layers_on(layer) {
                if let Some(geo) = layer_map.layer_geometry(layer_surface) {
                    let surface_loc = geo.loc;
                    let pos_in_surface = pos - surface_loc.to_f64();

                    if let Some((surface, _offset)) =
                        layer_surface.surface_under(pos_in_surface, WindowSurfaceType::ALL)
                    {
                        let keyboard_focus = if layer_surface.can_receive_keyboard_focus() {
                            Some(layer_surface.wl_surface().clone())
                        } else {
                            None
                        };
                        return Some((surface, keyboard_focus));
                    }
                }
            }
        }

        None
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
