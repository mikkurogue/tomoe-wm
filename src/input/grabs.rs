//! Pointer grab implementations for window move and resize operations

use smithay::{
    desktop::Window,
    input::{
        pointer::{
            AxisFrame, ButtonEvent, GestureHoldBeginEvent, GestureHoldEndEvent,
            GesturePinchBeginEvent, GesturePinchEndEvent, GesturePinchUpdateEvent,
            GestureSwipeBeginEvent, GestureSwipeEndEvent, GestureSwipeUpdateEvent, GrabStartData,
            MotionEvent, PointerGrab, PointerInnerHandle, RelativeMotionEvent,
        },
        SeatHandler,
    },
    reexports::wayland_protocols::xdg::shell::server::xdg_toplevel,
    utils::{Logical, Point, Size},
};

use crate::state::TomoeState;

/// Grab for moving a window
pub struct MoveSurfaceGrab {
    pub start_data: GrabStartData<TomoeState>,
    pub window: Window,
    pub initial_window_location: Point<i32, Logical>,
}

impl PointerGrab<TomoeState> for MoveSurfaceGrab {
    fn motion(
        &mut self,
        data: &mut TomoeState,
        handle: &mut PointerInnerHandle<'_, TomoeState>,
        _focus: Option<(
            <TomoeState as SeatHandler>::PointerFocus,
            Point<f64, Logical>,
        )>,
        event: &MotionEvent,
    ) {
        // No focus during move
        handle.motion(data, None, event);

        let delta = event.location - self.start_data.location;
        let new_location = self.initial_window_location.to_f64() + delta;
        data.space
            .map_element(self.window.clone(), new_location.to_i32_round(), true);
    }

    fn relative_motion(
        &mut self,
        data: &mut TomoeState,
        handle: &mut PointerInnerHandle<'_, TomoeState>,
        focus: Option<(
            <TomoeState as SeatHandler>::PointerFocus,
            Point<f64, Logical>,
        )>,
        event: &RelativeMotionEvent,
    ) {
        handle.relative_motion(data, focus, event);
    }

    fn button(
        &mut self,
        data: &mut TomoeState,
        handle: &mut PointerInnerHandle<'_, TomoeState>,
        event: &ButtonEvent,
    ) {
        handle.button(data, event);
        if handle.current_pressed().is_empty() {
            // No more buttons pressed, release the grab
            handle.unset_grab(self, data, event.serial, event.time, true);
        }
    }

    fn axis(
        &mut self,
        data: &mut TomoeState,
        handle: &mut PointerInnerHandle<'_, TomoeState>,
        details: AxisFrame,
    ) {
        handle.axis(data, details);
    }

    fn frame(&mut self, data: &mut TomoeState, handle: &mut PointerInnerHandle<'_, TomoeState>) {
        handle.frame(data);
    }

    fn gesture_swipe_begin(
        &mut self,
        data: &mut TomoeState,
        handle: &mut PointerInnerHandle<'_, TomoeState>,
        event: &GestureSwipeBeginEvent,
    ) {
        handle.gesture_swipe_begin(data, event);
    }

    fn gesture_swipe_update(
        &mut self,
        data: &mut TomoeState,
        handle: &mut PointerInnerHandle<'_, TomoeState>,
        event: &GestureSwipeUpdateEvent,
    ) {
        handle.gesture_swipe_update(data, event);
    }

    fn gesture_swipe_end(
        &mut self,
        data: &mut TomoeState,
        handle: &mut PointerInnerHandle<'_, TomoeState>,
        event: &GestureSwipeEndEvent,
    ) {
        handle.gesture_swipe_end(data, event);
    }

    fn gesture_pinch_begin(
        &mut self,
        data: &mut TomoeState,
        handle: &mut PointerInnerHandle<'_, TomoeState>,
        event: &GesturePinchBeginEvent,
    ) {
        handle.gesture_pinch_begin(data, event);
    }

    fn gesture_pinch_update(
        &mut self,
        data: &mut TomoeState,
        handle: &mut PointerInnerHandle<'_, TomoeState>,
        event: &GesturePinchUpdateEvent,
    ) {
        handle.gesture_pinch_update(data, event);
    }

    fn gesture_pinch_end(
        &mut self,
        data: &mut TomoeState,
        handle: &mut PointerInnerHandle<'_, TomoeState>,
        event: &GesturePinchEndEvent,
    ) {
        handle.gesture_pinch_end(data, event);
    }

    fn gesture_hold_begin(
        &mut self,
        data: &mut TomoeState,
        handle: &mut PointerInnerHandle<'_, TomoeState>,
        event: &GestureHoldBeginEvent,
    ) {
        handle.gesture_hold_begin(data, event);
    }

    fn gesture_hold_end(
        &mut self,
        data: &mut TomoeState,
        handle: &mut PointerInnerHandle<'_, TomoeState>,
        event: &GestureHoldEndEvent,
    ) {
        handle.gesture_hold_end(data, event);
    }

    fn start_data(&self) -> &GrabStartData<TomoeState> {
        &self.start_data
    }

    fn unset(&mut self, _data: &mut TomoeState) {}
}

/// Check if resize edge includes left
fn has_left(edges: xdg_toplevel::ResizeEdge) -> bool {
    matches!(
        edges,
        xdg_toplevel::ResizeEdge::Left
            | xdg_toplevel::ResizeEdge::TopLeft
            | xdg_toplevel::ResizeEdge::BottomLeft
    )
}

/// Check if resize edge includes right
fn has_right(edges: xdg_toplevel::ResizeEdge) -> bool {
    matches!(
        edges,
        xdg_toplevel::ResizeEdge::Right
            | xdg_toplevel::ResizeEdge::TopRight
            | xdg_toplevel::ResizeEdge::BottomRight
    )
}

/// Check if resize edge includes top
fn has_top(edges: xdg_toplevel::ResizeEdge) -> bool {
    matches!(
        edges,
        xdg_toplevel::ResizeEdge::Top
            | xdg_toplevel::ResizeEdge::TopLeft
            | xdg_toplevel::ResizeEdge::TopRight
    )
}

/// Check if resize edge includes bottom
fn has_bottom(edges: xdg_toplevel::ResizeEdge) -> bool {
    matches!(
        edges,
        xdg_toplevel::ResizeEdge::Bottom
            | xdg_toplevel::ResizeEdge::BottomLeft
            | xdg_toplevel::ResizeEdge::BottomRight
    )
}

/// Grab for resizing a window
pub struct ResizeSurfaceGrab {
    pub start_data: GrabStartData<TomoeState>,
    pub window: Window,
    pub edges: xdg_toplevel::ResizeEdge,
    pub initial_window_location: Point<i32, Logical>,
    pub initial_window_size: Size<i32, Logical>,
    pub last_window_size: Size<i32, Logical>,
}

impl PointerGrab<TomoeState> for ResizeSurfaceGrab {
    fn motion(
        &mut self,
        data: &mut TomoeState,
        handle: &mut PointerInnerHandle<'_, TomoeState>,
        _focus: Option<(
            <TomoeState as SeatHandler>::PointerFocus,
            Point<f64, Logical>,
        )>,
        event: &MotionEvent,
    ) {
        // No focus during resize
        handle.motion(data, None, event);

        let delta = (event.location - self.start_data.location).to_i32_round::<i32>();

        let mut new_window_width = self.initial_window_size.w;
        let mut new_window_height = self.initial_window_size.h;

        if has_left(self.edges) {
            new_window_width = i32::max(1, self.initial_window_size.w - delta.x);
        } else if has_right(self.edges) {
            new_window_width = i32::max(1, self.initial_window_size.w + delta.x);
        }

        if has_top(self.edges) {
            new_window_height = i32::max(1, self.initial_window_size.h - delta.y);
        } else if has_bottom(self.edges) {
            new_window_height = i32::max(1, self.initial_window_size.h + delta.y);
        }

        let new_size = Size::from((new_window_width, new_window_height));

        if new_size != self.last_window_size {
            if let Some(toplevel) = self.window.toplevel() {
                toplevel.with_pending_state(|state| {
                    state.size = Some(new_size);
                });
                toplevel.send_pending_configure();
            }
            self.last_window_size = new_size;
        }
    }

    fn relative_motion(
        &mut self,
        data: &mut TomoeState,
        handle: &mut PointerInnerHandle<'_, TomoeState>,
        focus: Option<(
            <TomoeState as SeatHandler>::PointerFocus,
            Point<f64, Logical>,
        )>,
        event: &RelativeMotionEvent,
    ) {
        handle.relative_motion(data, focus, event);
    }

    fn button(
        &mut self,
        data: &mut TomoeState,
        handle: &mut PointerInnerHandle<'_, TomoeState>,
        event: &ButtonEvent,
    ) {
        handle.button(data, event);
        if handle.current_pressed().is_empty() {
            // No more buttons pressed, finish resize
            if let Some(toplevel) = self.window.toplevel() {
                toplevel.with_pending_state(|state| {
                    state.states.unset(xdg_toplevel::State::Resizing);
                });
                toplevel.send_pending_configure();
            }

            // Update window location if resizing from top or left
            let geometry = self.window.geometry();
            let mut new_location = data
                .space
                .element_location(&self.window)
                .unwrap_or_default();

            if has_left(self.edges) {
                new_location.x =
                    self.initial_window_location.x + (self.initial_window_size.w - geometry.size.w);
            }
            if has_top(self.edges) {
                new_location.y =
                    self.initial_window_location.y + (self.initial_window_size.h - geometry.size.h);
            }

            data.space
                .map_element(self.window.clone(), new_location, true);

            handle.unset_grab(self, data, event.serial, event.time, true);
        }
    }

    fn axis(
        &mut self,
        data: &mut TomoeState,
        handle: &mut PointerInnerHandle<'_, TomoeState>,
        details: AxisFrame,
    ) {
        handle.axis(data, details);
    }

    fn frame(&mut self, data: &mut TomoeState, handle: &mut PointerInnerHandle<'_, TomoeState>) {
        handle.frame(data);
    }

    fn gesture_swipe_begin(
        &mut self,
        data: &mut TomoeState,
        handle: &mut PointerInnerHandle<'_, TomoeState>,
        event: &GestureSwipeBeginEvent,
    ) {
        handle.gesture_swipe_begin(data, event);
    }

    fn gesture_swipe_update(
        &mut self,
        data: &mut TomoeState,
        handle: &mut PointerInnerHandle<'_, TomoeState>,
        event: &GestureSwipeUpdateEvent,
    ) {
        handle.gesture_swipe_update(data, event);
    }

    fn gesture_swipe_end(
        &mut self,
        data: &mut TomoeState,
        handle: &mut PointerInnerHandle<'_, TomoeState>,
        event: &GestureSwipeEndEvent,
    ) {
        handle.gesture_swipe_end(data, event);
    }

    fn gesture_pinch_begin(
        &mut self,
        data: &mut TomoeState,
        handle: &mut PointerInnerHandle<'_, TomoeState>,
        event: &GesturePinchBeginEvent,
    ) {
        handle.gesture_pinch_begin(data, event);
    }

    fn gesture_pinch_update(
        &mut self,
        data: &mut TomoeState,
        handle: &mut PointerInnerHandle<'_, TomoeState>,
        event: &GesturePinchUpdateEvent,
    ) {
        handle.gesture_pinch_update(data, event);
    }

    fn gesture_pinch_end(
        &mut self,
        data: &mut TomoeState,
        handle: &mut PointerInnerHandle<'_, TomoeState>,
        event: &GesturePinchEndEvent,
    ) {
        handle.gesture_pinch_end(data, event);
    }

    fn gesture_hold_begin(
        &mut self,
        data: &mut TomoeState,
        handle: &mut PointerInnerHandle<'_, TomoeState>,
        event: &GestureHoldBeginEvent,
    ) {
        handle.gesture_hold_begin(data, event);
    }

    fn gesture_hold_end(
        &mut self,
        data: &mut TomoeState,
        handle: &mut PointerInnerHandle<'_, TomoeState>,
        event: &GestureHoldEndEvent,
    ) {
        handle.gesture_hold_end(data, event);
    }

    fn start_data(&self) -> &GrabStartData<TomoeState> {
        &self.start_data
    }

    fn unset(&mut self, _data: &mut TomoeState) {}
}
