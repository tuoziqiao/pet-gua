//! Input adaptation shared by the island and its controls.
use eframe::egui::{Event, InputState, MouseWheelUnit};

/// Reconstruct the unsmoothed vertical scroll in logical points.
pub(crate) fn raw_scroll_y(input: &InputState) -> f32 {
    input
        .events
        .iter()
        .filter_map(|event| match event {
            Event::MouseWheel {
                unit,
                delta,
                modifiers,
                ..
            } if !modifiers.shift => {
                let scale = match unit {
                    MouseWheelUnit::Point => 1.0,
                    MouseWheelUnit::Line => 40.0,
                    MouseWheelUnit::Page => input.viewport_rect().height(),
                };
                Some(delta.y * scale)
            }
            _ => None,
        })
        .sum()
}
