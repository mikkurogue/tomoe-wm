//! Cursor management for the compositor
//!
//! Handles loading cursor themes and rendering cursors for the DRM backend.

use smithay::{
    backend::{
        allocator::Fourcc,
        renderer::{
            element::memory::{MemoryRenderBuffer, MemoryRenderBufferRenderElement},
            gles::GlesRenderer,
        },
    },
    input::pointer::CursorImageStatus,
    utils::{Physical, Point, Scale, Transform},
};

/// Default cursor size
const DEFAULT_CURSOR_SIZE: u32 = 24;

/// A simple fallback cursor (white arrow shape)
/// This is a 24x24 ARGB cursor
fn create_fallback_cursor() -> Vec<u8> {
    let size = DEFAULT_CURSOR_SIZE as usize;
    let mut data = vec![0u8; size * size * 4];

    // Draw a simple white arrow cursor with black outline
    // Arrow shape pointing to top-left
    let cursor_shape: [(i32, i32); 12] = [
        (0, 0),
        (0, 16),
        (4, 12),
        (7, 19),
        (9, 18),
        (6, 11),
        (10, 11),
        (0, 0), // Close the shape
        (1, 1),
        (1, 14),
        (5, 10),
        (1, 1),
    ];

    // Fill with a simple triangular cursor
    for y in 0..size {
        for x in 0..size {
            let idx = (y * size + x) * 4;
            let xi = x as i32;
            let yi = y as i32;

            // Simple triangular cursor: x <= y and x + y <= 20
            if xi <= yi && xi + yi <= 20 && yi < 18 {
                // White fill
                data[idx] = 255; // B
                data[idx + 1] = 255; // G
                data[idx + 2] = 255; // R
                data[idx + 3] = 255; // A
            } else if xi <= yi + 1
                && xi + yi <= 21
                && yi < 19
                && (xi == 0 || yi == xi || xi + yi == 20)
            {
                // Black outline
                data[idx] = 0; // B
                data[idx + 1] = 0; // G
                data[idx + 2] = 0; // R
                data[idx + 3] = 255; // A
            }
        }
    }

    data
}

/// Cursor manager handles cursor rendering
pub struct CursorManager {
    /// The current cursor image status
    current_cursor: CursorImageStatus,
    /// Fallback cursor buffer
    fallback_buffer: MemoryRenderBuffer,
    /// Cursor hotspot (relative to top-left of cursor image)
    hotspot: Point<i32, Physical>,
}

impl CursorManager {
    /// Create a new cursor manager
    pub fn new() -> Self {
        let cursor_data = create_fallback_cursor();
        let fallback_buffer = MemoryRenderBuffer::from_slice(
            &cursor_data,
            Fourcc::Argb8888,
            (DEFAULT_CURSOR_SIZE as i32, DEFAULT_CURSOR_SIZE as i32),
            1, // scale
            Transform::Normal,
            None,
        );

        Self {
            current_cursor: CursorImageStatus::default_named(),
            fallback_buffer,
            hotspot: Point::from((0, 0)),
        }
    }

    /// Set the current cursor image
    pub fn set_cursor_image(&mut self, status: CursorImageStatus) {
        self.current_cursor = status;
    }

    /// Get the current cursor status
    pub fn cursor_image(&self) -> &CursorImageStatus {
        &self.current_cursor
    }

    /// Render the cursor at the given position
    /// Returns a render element that can be added to the render list
    pub fn render_cursor<'a>(
        &'a self,
        renderer: &mut GlesRenderer,
        position: Point<f64, smithay::utils::Logical>,
        scale: Scale<f64>,
    ) -> Option<MemoryRenderBufferRenderElement<GlesRenderer>> {
        // For now, always render the fallback cursor
        // TODO: Handle CursorImageStatus::Surface and Named cursors

        match &self.current_cursor {
            CursorImageStatus::Hidden => None,
            _ => {
                // Convert logical position to physical, accounting for hotspot
                let physical_pos: Point<i32, Physical> = (
                    (position.x * scale.x) as i32 - self.hotspot.x,
                    (position.y * scale.y) as i32 - self.hotspot.y,
                )
                    .into();

                MemoryRenderBufferRenderElement::from_buffer(
                    renderer,
                    physical_pos.to_f64(),
                    &self.fallback_buffer,
                    None, // alpha
                    None, // src
                    None, // dst size
                    smithay::backend::renderer::element::Kind::Cursor,
                )
                .ok()
            }
        }
    }
}

impl Default for CursorManager {
    fn default() -> Self {
        Self::new()
    }
}
