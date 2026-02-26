//! wlr-layer-shell handler for panels, bars, and overlay surfaces

use smithay::{
    desktop::{layer_map_for_output, LayerSurface as DesktopLayerSurface},
    output::Output,
    reexports::wayland_server::protocol::wl_output,
    wayland::shell::wlr_layer::{Layer, LayerSurface, WlrLayerShellHandler, WlrLayerShellState},
};
use tracing::info;

use crate::state::TomoeState;

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

impl TomoeState {
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
