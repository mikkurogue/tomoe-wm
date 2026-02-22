//! Scrolling tiling layout similar to niri
//!
//! Windows are arranged horizontally in a row, and the view can be scrolled
//! left/right to show different windows. The focused window is always visible.

use smithay::{
    desktop::Window,
    utils::{Logical, Point, Size},
};

/// Manages the scrolling tiling layout
#[derive(Debug)]
pub struct TilingLayout {
    /// All windows in order (left to right)
    windows: Vec<Window>,
    /// Index of the focused window
    focus_index: Option<usize>,
    /// Current scroll offset (positive = scrolled right, showing windows on the left)
    scroll_offset: i32,
    /// Gap between windows
    gap: i32,
    /// Outer margin
    margin: i32,
    /// Default window width as percentage of output width
    default_width_percent: f64,
    /// Output size
    output_size: Size<i32, Logical>,
}

impl TilingLayout {
    pub fn new(gap: i32, margin: i32, default_width_percent: f64) -> Self {
        Self {
            windows: Vec::new(),
            focus_index: None,
            scroll_offset: 0,
            gap,
            margin,
            default_width_percent,
            output_size: Size::from((1920, 1080)), // Default, updated when output is known
        }
    }

    /// Set the output size for layout calculations
    pub fn set_output_size(&mut self, size: Size<i32, Logical>) {
        self.output_size = size;
    }

    /// Add a new window to the layout (appends to the right)
    pub fn add_window(&mut self, window: Window) {
        // Configure the window size
        let window_width = self.calculate_window_width();
        let window_height = self.output_size.h - 2 * self.margin;

        if let Some(toplevel) = window.toplevel() {
            toplevel.with_pending_state(|state| {
                state.size = Some(Size::from((window_width, window_height)));
            });
            toplevel.send_pending_configure();
        }

        self.windows.push(window);

        // Focus the new window
        self.focus_index = Some(self.windows.len() - 1);

        // Scroll to show the new window
        self.scroll_to_focused();
    }

    /// Remove a window from the layout
    pub fn remove_window(&mut self, window: &Window) -> bool {
        if let Some(idx) = self.windows.iter().position(|w| w == window) {
            self.windows.remove(idx);

            // Adjust focus index
            if self.windows.is_empty() {
                self.focus_index = None;
            } else if let Some(focus_idx) = self.focus_index {
                if focus_idx >= self.windows.len() {
                    self.focus_index = Some(self.windows.len() - 1);
                } else if idx < focus_idx {
                    self.focus_index = Some(focus_idx - 1);
                }
            }

            self.scroll_to_focused();
            return true;
        }
        false
    }

    /// Get the currently focused window
    pub fn focused_window(&self) -> Option<&Window> {
        self.focus_index.and_then(|idx| self.windows.get(idx))
    }

    /// Focus the next window (to the right)
    pub fn focus_next(&mut self) {
        if self.windows.is_empty() {
            return;
        }

        self.focus_index = Some(match self.focus_index {
            Some(idx) if idx + 1 < self.windows.len() => idx + 1,
            Some(_) => 0, // Wrap around
            None => 0,
        });

        self.scroll_to_focused();
    }

    /// Focus the previous window (to the left)
    pub fn focus_prev(&mut self) {
        if self.windows.is_empty() {
            return;
        }

        self.focus_index = Some(match self.focus_index {
            Some(0) => self.windows.len() - 1, // Wrap around
            Some(idx) => idx - 1,
            None => 0,
        });

        self.scroll_to_focused();
    }

    /// Focus a specific window
    pub fn focus_window(&mut self, window: &Window) {
        if let Some(idx) = self.windows.iter().position(|w| w == window) {
            self.focus_index = Some(idx);
            self.scroll_to_focused();
        }
    }

    /// Scroll the view left (show more windows on the left)
    pub fn scroll_left(&mut self) {
        let scroll_amount = self.calculate_window_width() / 2;
        self.scroll_offset = (self.scroll_offset - scroll_amount).max(0);
    }

    /// Scroll the view right (show more windows on the right)
    pub fn scroll_right(&mut self) {
        let scroll_amount = self.calculate_window_width() / 2;
        let max_scroll = self.calculate_max_scroll();
        self.scroll_offset = (self.scroll_offset + scroll_amount).min(max_scroll);
    }

    /// Calculate window positions and return them
    /// Returns (window, position) pairs
    pub fn calculate_positions(&self) -> Vec<(&Window, Point<i32, Logical>)> {
        let mut positions = Vec::new();
        let window_width = self.calculate_window_width();
        let mut x = self.margin - self.scroll_offset;

        for window in &self.windows {
            let y = self.margin;
            positions.push((window, Point::from((x, y))));
            x += window_width + self.gap;
        }

        positions
    }

    /// Get all windows
    pub fn windows(&self) -> &[Window] {
        &self.windows
    }

    /// Check if a window is in the layout
    pub fn contains(&self, window: &Window) -> bool {
        self.windows.contains(window)
    }

    /// Get window count
    pub fn window_count(&self) -> usize {
        self.windows.len()
    }

    /// Calculate the default window width
    fn calculate_window_width(&self) -> i32 {
        let available_width = self.output_size.w - 2 * self.margin;
        (available_width as f64 * self.default_width_percent) as i32
    }

    /// Calculate the maximum scroll offset
    fn calculate_max_scroll(&self) -> i32 {
        if self.windows.is_empty() {
            return 0;
        }

        let window_width = self.calculate_window_width();
        let total_width =
            self.windows.len() as i32 * window_width + (self.windows.len() as i32 - 1) * self.gap;
        let visible_width = self.output_size.w - 2 * self.margin;

        (total_width - visible_width).max(0)
    }

    /// Scroll to ensure the focused window is visible
    fn scroll_to_focused(&mut self) {
        let Some(focus_idx) = self.focus_index else {
            return;
        };

        let window_width = self.calculate_window_width();
        let visible_width = self.output_size.w - 2 * self.margin;

        // Calculate the focused window's position (without scroll)
        let window_start = focus_idx as i32 * (window_width + self.gap);
        let window_end = window_start + window_width;

        // Adjust scroll to ensure the window is visible
        if window_start < self.scroll_offset {
            // Window is off the left edge, scroll left
            self.scroll_offset = window_start;
        } else if window_end > self.scroll_offset + visible_width {
            // Window is off the right edge, scroll right
            self.scroll_offset = window_end - visible_width;
        }

        // Clamp scroll offset
        self.scroll_offset = self.scroll_offset.clamp(0, self.calculate_max_scroll());
    }

    /// Reconfigure all windows with current layout settings
    pub fn reconfigure_all(&mut self) {
        let window_width = self.calculate_window_width();
        let window_height = self.output_size.h - 2 * self.margin;

        for window in &self.windows {
            if let Some(toplevel) = window.toplevel() {
                toplevel.with_pending_state(|state| {
                    state.size = Some(Size::from((window_width, window_height)));
                });
                toplevel.send_pending_configure();
            }
        }
    }
}
