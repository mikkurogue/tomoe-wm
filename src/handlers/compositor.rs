//! Compositor and buffer handlers

use smithay::{
    backend::renderer::utils::on_commit_buffer_handler,
    desktop::layer_map_for_output,
    reexports::wayland_server::protocol::{wl_buffer, wl_surface::WlSurface},
    wayland::{
        buffer::BufferHandler,
        compositor::{
            get_parent, is_sync_subsurface, with_states, CompositorClientState, CompositorHandler,
            CompositorState,
        },
        shell::xdg::XdgToplevelSurfaceData,
    },
};

use crate::state::{ClientState, TomoeState};

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
        self.handle_layer_surface_commit(surface);

        // Handle popup commits
        self.popups.commit(surface);
    }
}

impl TomoeState {
    /// Handle layer surface commits
    fn handle_layer_surface_commit(&mut self, surface: &WlSurface) {
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
    }
}
