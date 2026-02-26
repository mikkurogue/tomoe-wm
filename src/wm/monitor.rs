//! Monitor management
//!
//! A monitor represents a physical output with multiple workspaces.
//! Each monitor has its own set of workspaces that can be switched independently.

use smithay::{
    backend::renderer::{element::surface::WaylandSurfaceRenderElement, gles::GlesRenderer},
    desktop::Window,
    output::Output,
    utils::{Logical, Scale, Size},
};

use super::workspace::{Workspace, WorkspaceId};

/// Per-output state managing workspaces
#[derive(Debug)]
pub struct Monitor {
    /// The output this monitor represents
    output: Output,

    /// Workspaces on this monitor (always at least one)
    workspaces: Vec<Workspace>,

    /// Index of the active workspace
    active_workspace_idx: usize,

    /// ID of the previously active workspace (for "go back" functionality)
    previous_workspace_id: Option<WorkspaceId>,

    /// Layout configuration
    gap: i32,
    margin: i32,
    default_width_percent: f64,
}

impl Monitor {
    /// Create a new monitor with one empty workspace
    pub fn new(output: Output, gap: i32, margin: i32, default_width_percent: f64) -> Self {
        let workspace = Workspace::new(&output, gap, margin, default_width_percent);

        Self {
            output,
            workspaces: vec![workspace],
            active_workspace_idx: 0,
            previous_workspace_id: None,
            gap,
            margin,
            default_width_percent,
        }
    }

    /// Get the output
    pub fn output(&self) -> &Output {
        &self.output
    }

    /// Get the active workspace
    pub fn active_workspace(&self) -> &Workspace {
        &self.workspaces[self.active_workspace_idx]
    }

    /// Get mutable access to the active workspace
    pub fn active_workspace_mut(&mut self) -> &mut Workspace {
        &mut self.workspaces[self.active_workspace_idx]
    }

    /// Get the active workspace index
    pub fn active_workspace_idx(&self) -> usize {
        self.active_workspace_idx
    }

    /// Get all workspaces
    pub fn workspaces(&self) -> &[Workspace] {
        &self.workspaces
    }

    /// Get workspace count
    pub fn workspace_count(&self) -> usize {
        self.workspaces.len()
    }

    /// Get a workspace by index
    pub fn workspace(&self, idx: usize) -> Option<&Workspace> {
        self.workspaces.get(idx)
    }

    /// Get a mutable workspace by index
    pub fn workspace_mut(&mut self, idx: usize) -> Option<&mut Workspace> {
        self.workspaces.get_mut(idx)
    }

    /// Find a workspace by ID
    pub fn workspace_by_id(&self, id: WorkspaceId) -> Option<&Workspace> {
        self.workspaces.iter().find(|ws| ws.id() == id)
    }

    /// Find workspace index by ID
    pub fn workspace_idx_by_id(&self, id: WorkspaceId) -> Option<usize> {
        self.workspaces.iter().position(|ws| ws.id() == id)
    }

    /// Switch to a workspace by index
    pub fn switch_to_workspace(&mut self, idx: usize) -> bool {
        if idx >= self.workspaces.len() || idx == self.active_workspace_idx {
            return false;
        }

        self.previous_workspace_id = Some(self.workspaces[self.active_workspace_idx].id());
        self.active_workspace_idx = idx;
        true
    }

    /// Switch to the next workspace
    pub fn switch_to_next_workspace(&mut self) -> bool {
        let next_idx = if self.active_workspace_idx + 1 < self.workspaces.len() {
            self.active_workspace_idx + 1
        } else {
            0 // Wrap around
        };
        self.switch_to_workspace(next_idx)
    }

    /// Switch to the previous workspace
    pub fn switch_to_prev_workspace(&mut self) -> bool {
        let prev_idx = if self.active_workspace_idx > 0 {
            self.active_workspace_idx - 1
        } else {
            self.workspaces.len() - 1 // Wrap around
        };
        self.switch_to_workspace(prev_idx)
    }

    /// Switch back to the previously active workspace
    pub fn switch_to_previous(&mut self) -> bool {
        if let Some(prev_id) = self.previous_workspace_id {
            if let Some(idx) = self.workspace_idx_by_id(prev_id) {
                return self.switch_to_workspace(idx);
            }
        }
        false
    }

    /// Create a new workspace and optionally switch to it
    pub fn create_workspace(&mut self, switch_to: bool) -> WorkspaceId {
        let workspace = Workspace::new(
            &self.output,
            self.gap,
            self.margin,
            self.default_width_percent,
        );
        let id = workspace.id();
        self.workspaces.push(workspace);

        if switch_to {
            self.switch_to_workspace(self.workspaces.len() - 1);
        }

        id
    }

    /// Create a new named workspace
    pub fn create_named_workspace(&mut self, name: String, switch_to: bool) -> WorkspaceId {
        let workspace = Workspace::new_named(
            name,
            &self.output,
            self.gap,
            self.margin,
            self.default_width_percent,
        );
        let id = workspace.id();
        self.workspaces.push(workspace);

        if switch_to {
            self.switch_to_workspace(self.workspaces.len() - 1);
        }

        id
    }

    /// Remove an empty workspace (cannot remove the last one)
    pub fn remove_workspace(&mut self, idx: usize) -> bool {
        // Don't remove if it's the only workspace or has windows
        if self.workspaces.len() <= 1 {
            return false;
        }
        if let Some(ws) = self.workspaces.get(idx) {
            if !ws.is_empty() {
                return false;
            }
        } else {
            return false;
        }

        self.workspaces.remove(idx);

        // Adjust active workspace index
        if self.active_workspace_idx >= self.workspaces.len() {
            self.active_workspace_idx = self.workspaces.len() - 1;
        } else if idx < self.active_workspace_idx {
            self.active_workspace_idx -= 1;
        }

        true
    }

    /// Clean up empty workspaces (keeping at least one and the active one)
    pub fn cleanup_empty_workspaces(&mut self) {
        if self.workspaces.len() <= 1 {
            return;
        }

        // Collect indices of empty workspaces to remove (excluding active)
        let to_remove: Vec<usize> = self
            .workspaces
            .iter()
            .enumerate()
            .filter(|(i, ws)| *i != self.active_workspace_idx && ws.is_empty())
            .map(|(i, _)| i)
            .collect();

        // Remove from back to front to preserve indices
        for idx in to_remove.into_iter().rev() {
            if self.workspaces.len() > 1 {
                self.workspaces.remove(idx);
                if idx < self.active_workspace_idx {
                    self.active_workspace_idx -= 1;
                }
            }
        }
    }

    /// Add a window to the active workspace
    pub fn add_window(&mut self, window: Window) {
        self.active_workspace_mut().add_window(window);
    }

    /// Remove a window from any workspace on this monitor
    pub fn remove_window(&mut self, window: &Window) -> bool {
        for workspace in &mut self.workspaces {
            if workspace.remove_window(window) {
                return true;
            }
        }
        false
    }

    /// Find which workspace contains a window
    pub fn find_window(&self, window: &Window) -> Option<(usize, &Workspace)> {
        self.workspaces
            .iter()
            .enumerate()
            .find(|(_, ws)| ws.contains(window))
    }

    /// Move a window to a specific workspace
    pub fn move_window_to_workspace(&mut self, window: &Window, target_idx: usize) -> bool {
        if target_idx >= self.workspaces.len() {
            return false;
        }

        // Find and remove from current workspace
        let mut found_window = None;
        for workspace in &mut self.workspaces {
            if workspace.contains(window) {
                // Clone the window before removing
                let win = window.clone();
                workspace.remove_window(&win);
                found_window = Some(win);
                break;
            }
        }

        // Add to target workspace
        if let Some(win) = found_window {
            self.workspaces[target_idx].add_window(win);
            true
        } else {
            false
        }
    }

    /// Update all workspaces when output changes
    pub fn update_output(&mut self, output: Output) {
        self.output = output.clone();
        for workspace in &mut self.workspaces {
            workspace.set_output(Some(output.clone()));
        }
    }

    /// Update working areas for all workspaces (e.g., when layer shells change)
    pub fn update_working_areas(&mut self) {
        for workspace in &mut self.workspaces {
            workspace.update_working_area();
        }
    }

    /// Reconfigure all windows on all workspaces
    pub fn reconfigure_all(&mut self) {
        for workspace in &mut self.workspaces {
            workspace.reconfigure_all();
        }
    }

    /// Render elements for the active workspace
    pub fn render_elements(
        &self,
        renderer: &mut GlesRenderer,
        scale: Scale<f64>,
    ) -> Vec<WaylandSurfaceRenderElement<GlesRenderer>> {
        self.workspaces[self.active_workspace_idx].render_elements(renderer, scale)
    }

    /// Get the focused window on the active workspace
    pub fn focused_window(&self) -> Option<&Window> {
        self.active_workspace().focused_window()
    }

    /// Focus the next window on the active workspace
    pub fn focus_next(&mut self) {
        self.active_workspace_mut().focus_next();
    }

    /// Focus the previous window on the active workspace
    pub fn focus_prev(&mut self) {
        self.active_workspace_mut().focus_prev();
    }

    /// Focus a specific window (finds it across workspaces)
    pub fn focus_window(&mut self, window: &Window) {
        // Find the workspace containing the window
        if let Some((idx, _)) = self.find_window(window) {
            // Switch to that workspace if needed
            if idx != self.active_workspace_idx {
                self.switch_to_workspace(idx);
            }
            // Focus the window
            self.active_workspace_mut().focus_window(window);
        }
    }
}
