//! Workspace management
//!
//! A workspace contains a set of tiled windows that can be switched as a unit.
//! Inspired by niri's workspace system but simplified for tomoe.

use smithay::{
    backend::renderer::{
        element::{surface::WaylandSurfaceRenderElement, AsRenderElements},
        gles::GlesRenderer,
    },
    desktop::{space::SpaceElement, Window},
    output::Output,
    utils::{Logical, Rectangle, Scale, Size},
};
use std::sync::atomic::{AtomicU64, Ordering};

use super::tiling::TilingLayout;

/// Counter for generating unique workspace IDs
static WORKSPACE_ID_COUNTER: AtomicU64 = AtomicU64::new(1);

/// Unique identifier for a workspace
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct WorkspaceId(u64);

impl WorkspaceId {
    fn next() -> Self {
        Self(WORKSPACE_ID_COUNTER.fetch_add(1, Ordering::Relaxed))
    }

    pub fn get(self) -> u64 {
        self.0
    }
}

/// A workspace containing tiled windows
#[derive(Debug)]
pub struct Workspace {
    /// Unique ID for this workspace
    id: WorkspaceId,

    /// Optional name for this workspace
    name: Option<String>,

    /// The tiling layout for windows in this workspace
    tiling: TilingLayout,

    /// Output this workspace is currently on (if any)
    output: Option<Output>,

    /// View size (output size)
    view_size: Size<f64, Logical>,

    /// Working area (respecting layer shell exclusive zones)
    working_area: Rectangle<f64, Logical>,
}

impl Workspace {
    /// Create a new workspace
    pub fn new(output: &Output, gap: i32, margin: i32, default_width_percent: f64) -> Self {
        let view_size = output_size(output);
        let working_area = compute_working_area(output);

        let mut tiling = TilingLayout::new(gap, margin, default_width_percent);
        tiling.set_output_size(Size::from((view_size.w as i32, view_size.h as i32)));

        if let Some(area) = working_area {
            tiling.set_available_area(area);
        }

        Self {
            id: WorkspaceId::next(),
            name: None,
            tiling,
            output: Some(output.clone()),
            view_size,
            working_area: working_area
                .map(|r| {
                    Rectangle::new(
                        (r.loc.x as f64, r.loc.y as f64).into(),
                        (r.size.w as f64, r.size.h as f64).into(),
                    )
                })
                .unwrap_or_else(|| Rectangle::new((0., 0.).into(), view_size)),
        }
    }

    /// Create a new workspace with a name
    pub fn new_named(
        name: String,
        output: &Output,
        gap: i32,
        margin: i32,
        default_width_percent: f64,
    ) -> Self {
        let mut ws = Self::new(output, gap, margin, default_width_percent);
        ws.name = Some(name);
        ws
    }

    /// Get the workspace ID
    pub fn id(&self) -> WorkspaceId {
        self.id
    }

    /// Get the workspace name
    pub fn name(&self) -> Option<&str> {
        self.name.as_deref()
    }

    /// Set the workspace name
    pub fn set_name(&mut self, name: Option<String>) {
        self.name = name;
    }

    /// Get the current output
    pub fn output(&self) -> Option<&Output> {
        self.output.as_ref()
    }

    /// Set the output for this workspace
    pub fn set_output(&mut self, output: Option<Output>) {
        if let Some(ref out) = output {
            self.view_size = output_size(out);
            self.tiling.set_output_size(Size::from((
                self.view_size.w as i32,
                self.view_size.h as i32,
            )));

            if let Some(area) = compute_working_area(out) {
                self.tiling.set_available_area(area);
                self.working_area = Rectangle::new(
                    (area.loc.x as f64, area.loc.y as f64).into(),
                    (area.size.w as f64, area.size.h as f64).into(),
                );
            }
        }
        self.output = output;
    }

    /// Update working area (call when layer shells change)
    pub fn update_working_area(&mut self) {
        if let Some(ref output) = self.output {
            if let Some(area) = compute_working_area(output) {
                self.tiling.set_available_area(area);
                self.working_area = Rectangle::new(
                    (area.loc.x as f64, area.loc.y as f64).into(),
                    (area.size.w as f64, area.size.h as f64).into(),
                );
            }
        }
    }

    /// Add a window to this workspace
    pub fn add_window(&mut self, window: Window) {
        // Notify window it entered this output
        if let Some(ref output) = self.output {
            window.output_enter(output, window.bbox());
        }

        self.tiling.add_window(window);
    }

    /// Remove a window from this workspace
    pub fn remove_window(&mut self, window: &Window) -> bool {
        // Notify window it left this output
        if let Some(ref output) = self.output {
            window.output_leave(output);
        }

        self.tiling.remove_window(window)
    }

    /// Get the focused window
    pub fn focused_window(&self) -> Option<&Window> {
        self.tiling.focused_window()
    }

    /// Focus the next window
    pub fn focus_next(&mut self) {
        self.tiling.focus_next();
    }

    /// Focus the previous window
    pub fn focus_prev(&mut self) {
        self.tiling.focus_prev();
    }

    /// Focus a specific window
    pub fn focus_window(&mut self, window: &Window) {
        self.tiling.focus_window(window);
    }

    /// Check if workspace contains a window
    pub fn contains(&self, window: &Window) -> bool {
        self.tiling.contains(window)
    }

    /// Get all windows in this workspace
    pub fn windows(&self) -> &[Window] {
        self.tiling.windows()
    }

    /// Get window count
    pub fn window_count(&self) -> usize {
        self.tiling.window_count()
    }

    /// Check if workspace is empty
    pub fn is_empty(&self) -> bool {
        self.tiling.window_count() == 0
    }

    /// Get the tiling layout (for position calculations)
    pub fn tiling(&self) -> &TilingLayout {
        &self.tiling
    }

    /// Get mutable access to tiling layout
    pub fn tiling_mut(&mut self) -> &mut TilingLayout {
        &mut self.tiling
    }

    /// Reconfigure all windows (e.g., after resize)
    pub fn reconfigure_all(&mut self) {
        self.tiling.reconfigure_all();
    }

    /// Render workspace elements
    pub fn render_elements<'a>(
        &'a self,
        renderer: &mut GlesRenderer,
        scale: Scale<f64>,
    ) -> Vec<WaylandSurfaceRenderElement<GlesRenderer>> {
        let mut elements = Vec::new();

        // Get window positions from tiling layout
        let positions = self.tiling.calculate_positions();

        // Render each window at its position
        for (window, pos) in positions {
            let loc = pos.to_physical_precise_round(scale);
            let window_elements: Vec<WaylandSurfaceRenderElement<GlesRenderer>> =
                window.render_elements(renderer, loc, scale, 1.0);
            elements.extend(window_elements);
        }

        elements
    }
}

/// Get the size of an output
fn output_size(output: &Output) -> Size<f64, Logical> {
    let mode = output.current_mode().expect("Output has no mode");
    let transform = output.current_transform();
    let size = transform.transform_size(mode.size);
    Size::from((size.w as f64, size.h as f64))
}

/// Compute the working area for an output (respecting layer shell exclusive zones)
fn compute_working_area(output: &Output) -> Option<Rectangle<i32, Logical>> {
    use smithay::desktop::layer_map_for_output;

    let layer_map = layer_map_for_output(output);
    let zone = layer_map.non_exclusive_zone();

    // non_exclusive_zone returns the area not reserved by exclusive layer surfaces
    Some(zone)
}
