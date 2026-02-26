//! Layout management
//!
//! The Layout is the top-level manager that coordinates monitors and workspaces.
//! It handles multi-monitor setups, window placement, and focus management.

use smithay::{
    backend::renderer::{element::surface::WaylandSurfaceRenderElement, gles::GlesRenderer},
    desktop::Window,
    output::Output,
    utils::Scale,
};
use std::collections::HashMap;

use super::monitor::Monitor;
use super::workspace::WorkspaceId;

/// Top-level layout manager
#[derive(Debug)]
pub struct Layout {
    /// Monitors indexed by output name
    monitors: HashMap<String, Monitor>,

    /// Name of the currently active monitor
    active_monitor: Option<String>,

    /// Name of the primary monitor (first one added, receives orphaned workspaces)
    primary_monitor: Option<String>,

    /// Layout configuration
    gap: i32,
    margin: i32,
    default_width_percent: f64,
}

impl Layout {
    /// Create a new empty layout
    pub fn new(gap: i32, margin: i32, default_width_percent: f64) -> Self {
        Self {
            monitors: HashMap::new(),
            active_monitor: None,
            primary_monitor: None,
            gap,
            margin,
            default_width_percent,
        }
    }

    /// Add a new output/monitor to the layout
    pub fn add_output(&mut self, output: Output) {
        let name = output.name();

        // Create monitor with one workspace
        let monitor = Monitor::new(output, self.gap, self.margin, self.default_width_percent);

        self.monitors.insert(name.clone(), monitor);

        // Set as primary if first monitor
        if self.primary_monitor.is_none() {
            self.primary_monitor = Some(name.clone());
        }

        // Set as active if no active monitor
        if self.active_monitor.is_none() {
            self.active_monitor = Some(name);
        }
    }

    /// Remove an output/monitor from the layout
    /// Returns the windows that were on that monitor (they need to be moved elsewhere)
    pub fn remove_output(&mut self, output: &Output) -> Vec<Window> {
        let name = output.name();
        let mut orphaned_windows = Vec::new();

        if let Some(monitor) = self.monitors.remove(&name) {
            // Collect all windows from all workspaces
            for workspace in monitor.workspaces() {
                orphaned_windows.extend(workspace.windows().iter().cloned());
            }

            // Update active monitor if needed
            if self.active_monitor.as_ref() == Some(&name) {
                self.active_monitor = self.monitors.keys().next().cloned();
            }

            // Update primary monitor if needed
            if self.primary_monitor.as_ref() == Some(&name) {
                self.primary_monitor = self.monitors.keys().next().cloned();
            }
        }

        orphaned_windows
    }

    /// Get a monitor by output
    pub fn monitor_for_output(&self, output: &Output) -> Option<&Monitor> {
        self.monitors.get(&output.name())
    }

    /// Get a mutable monitor by output
    pub fn monitor_for_output_mut(&mut self, output: &Output) -> Option<&mut Monitor> {
        self.monitors.get_mut(&output.name())
    }

    /// Get the active monitor
    pub fn active_monitor(&self) -> Option<&Monitor> {
        self.active_monitor
            .as_ref()
            .and_then(|name| self.monitors.get(name))
    }

    /// Get mutable active monitor
    pub fn active_monitor_mut(&mut self) -> Option<&mut Monitor> {
        self.active_monitor
            .as_ref()
            .and_then(|name| self.monitors.get_mut(name))
    }

    /// Get the primary monitor
    pub fn primary_monitor(&self) -> Option<&Monitor> {
        self.primary_monitor
            .as_ref()
            .and_then(|name| self.monitors.get(name))
    }

    /// Set the active monitor by output
    pub fn set_active_monitor(&mut self, output: &Output) {
        let name = output.name();
        if self.monitors.contains_key(&name) {
            self.active_monitor = Some(name);
        }
    }

    /// Get all monitors
    pub fn monitors(&self) -> impl Iterator<Item = &Monitor> {
        self.monitors.values()
    }

    /// Get all monitors mutably
    pub fn monitors_mut(&mut self) -> impl Iterator<Item = &mut Monitor> {
        self.monitors.values_mut()
    }

    /// Add a window to the active monitor's active workspace
    pub fn add_window(&mut self, window: Window) {
        if let Some(monitor) = self.active_monitor_mut() {
            monitor.add_window(window);
        }
    }

    /// Add a window to a specific output
    pub fn add_window_to_output(&mut self, window: Window, output: &Output) {
        if let Some(monitor) = self.monitor_for_output_mut(output) {
            monitor.add_window(window);
        }
    }

    /// Remove a window from the layout (searches all monitors)
    pub fn remove_window(&mut self, window: &Window) -> bool {
        for monitor in self.monitors.values_mut() {
            if monitor.remove_window(window) {
                return true;
            }
        }
        false
    }

    /// Find which monitor and workspace contains a window
    pub fn find_window(&self, window: &Window) -> Option<(&Monitor, usize)> {
        for monitor in self.monitors.values() {
            if let Some((ws_idx, _)) = monitor.find_window(window) {
                return Some((monitor, ws_idx));
            }
        }
        None
    }

    /// Get the focused window on the active monitor
    pub fn focused_window(&self) -> Option<&Window> {
        self.active_monitor().and_then(|m| m.focused_window())
    }

    /// Focus the next window on the active monitor
    pub fn focus_next(&mut self) {
        if let Some(monitor) = self.active_monitor_mut() {
            monitor.focus_next();
        }
    }

    /// Focus the previous window on the active monitor
    pub fn focus_prev(&mut self) {
        if let Some(monitor) = self.active_monitor_mut() {
            monitor.focus_prev();
        }
    }

    /// Focus a specific window (finds it across all monitors)
    pub fn focus_window(&mut self, window: &Window) {
        // Find which monitor has this window
        let monitor_name = self
            .monitors
            .iter()
            .find(|(_, m)| m.find_window(window).is_some())
            .map(|(name, _)| name.clone());

        if let Some(name) = monitor_name {
            // Set that monitor as active
            self.active_monitor = Some(name.clone());
            // Focus the window
            if let Some(monitor) = self.monitors.get_mut(&name) {
                monitor.focus_window(window);
            }
        }
    }

    /// Switch to the next workspace on the active monitor
    pub fn switch_to_next_workspace(&mut self) -> bool {
        if let Some(monitor) = self.active_monitor_mut() {
            return monitor.switch_to_next_workspace();
        }
        false
    }

    /// Switch to the previous workspace on the active monitor
    pub fn switch_to_prev_workspace(&mut self) -> bool {
        if let Some(monitor) = self.active_monitor_mut() {
            return monitor.switch_to_prev_workspace();
        }
        false
    }

    /// Switch to a specific workspace by index on the active monitor
    pub fn switch_to_workspace(&mut self, idx: usize) -> bool {
        if let Some(monitor) = self.active_monitor_mut() {
            return monitor.switch_to_workspace(idx);
        }
        false
    }

    /// Create a new workspace on the active monitor
    pub fn create_workspace(&mut self, switch_to: bool) -> Option<WorkspaceId> {
        self.active_monitor_mut()
            .map(|m| m.create_workspace(switch_to))
    }

    /// Move the focused window to a specific workspace
    pub fn move_focused_to_workspace(&mut self, target_idx: usize) -> bool {
        if let Some(monitor) = self.active_monitor_mut() {
            if let Some(window) = monitor.focused_window().cloned() {
                return monitor.move_window_to_workspace(&window, target_idx);
            }
        }
        false
    }

    /// Update working areas for all monitors (call when layer shells change)
    pub fn update_working_areas(&mut self) {
        for monitor in self.monitors.values_mut() {
            monitor.update_working_areas();
        }
    }

    /// Reconfigure all windows on all monitors
    pub fn reconfigure_all(&mut self) {
        for monitor in self.monitors.values_mut() {
            monitor.reconfigure_all();
        }
    }

    /// Render elements for a specific output
    pub fn render_elements_for_output(
        &self,
        output: &Output,
        renderer: &mut GlesRenderer,
        scale: Scale<f64>,
    ) -> Vec<WaylandSurfaceRenderElement<GlesRenderer>> {
        if let Some(monitor) = self.monitor_for_output(output) {
            monitor.render_elements(renderer, scale)
        } else {
            Vec::new()
        }
    }

    /// Get active workspace index for an output
    pub fn active_workspace_idx(&self, output: &Output) -> Option<usize> {
        self.monitor_for_output(output)
            .map(|m| m.active_workspace_idx())
    }

    /// Get workspace count for an output
    pub fn workspace_count(&self, output: &Output) -> Option<usize> {
        self.monitor_for_output(output).map(|m| m.workspace_count())
    }
}
