//! Event translation from baseview to blitz.

use baseview::{Event, MouseButton, MouseEvent, WindowEvent};
use blitz_traits::events::{BlitzMouseButtonEvent, MouseEventButton, MouseEventButtons, UiEvent};

// Re-export Modifiers from keyboard_types (0.6, matching baseview)
// This is the type we use in window.rs for tracking modifier state
pub use keyboard_types::Modifiers;

// Alias for the 0.7 Modifiers from blitz-traits (via dioxus re-export)
// This is what BlitzMouseButtonEvent expects
type BlitzModifiers = dioxus_native::prelude::Modifiers;

/// Translate a baseview event to a blitz UiEvent.
///
/// `viewport_size` is `(width, height)` in physical pixels. When a mouse button
/// is held (drag in progress), coordinates are clamped to the viewport so that
/// Blitz hit-testing still finds the overlay element even when the OS cursor is
/// outside the plugin window.
pub fn translate_event(
    event: &Event,
    mouse_pos: &mut (f32, f32),
    mouse_buttons: &mut MouseEventButtons,
    modifiers: &mut Modifiers,
    viewport_size: (u32, u32),
) -> Option<UiEvent> {
    match event {
        Event::Mouse(mouse_event) => {
            translate_mouse_event(mouse_event, mouse_pos, mouse_buttons, modifiers, viewport_size)
        }
        // Skip keyboard events for now due to keyboard_types version mismatch
        // between baseview (0.6) and blitz-traits (0.7)
        Event::Keyboard(_keyboard_event) => None,
        Event::Window(WindowEvent::Focused) => None,
        Event::Window(WindowEvent::Unfocused) => None,
        Event::Window(WindowEvent::Resized(_)) => None, // Handled separately
        Event::Window(WindowEvent::WillClose) => None,
    }
}

fn translate_mouse_event(
    event: &MouseEvent,
    mouse_pos: &mut (f32, f32),
    mouse_buttons: &mut MouseEventButtons,
    _modifiers: &Modifiers,
    viewport_size: (u32, u32),
) -> Option<UiEvent> {
    // When a mouse button is held, clamp coordinates to the viewport so that
    // Blitz hit-testing still routes the event to the correct element (e.g. a
    // full-viewport drag overlay). Without this, out-of-bounds coordinates
    // cause hit() to return None and the drag event is lost.
    let clamp = |x: f32, y: f32, buttons: &MouseEventButtons| -> (f32, f32) {
        if buttons.is_empty() {
            (x, y)
        } else {
            let max_x = (viewport_size.0 as f32 - 1.0).max(0.0);
            let max_y = (viewport_size.1 as f32 - 1.0).max(0.0);
            (x.clamp(0.0, max_x), y.clamp(0.0, max_y))
        }
    };

    match event {
        MouseEvent::CursorMoved {
            position,
            modifiers: mods,
        } => {
            let (cx, cy) = clamp(position.x as f32, position.y as f32, mouse_buttons);
            mouse_pos.0 = cx;
            mouse_pos.1 = cy;
            Some(UiEvent::MouseMove(BlitzMouseButtonEvent {
                x: mouse_pos.0,
                y: mouse_pos.1,
                button: MouseEventButton::Main,
                buttons: *mouse_buttons,
                mods: convert_modifiers(*mods),
            }))
        }
        MouseEvent::ButtonPressed {
            button,
            modifiers: mods,
        } => {
            let blitz_button = translate_mouse_button(*button);
            *mouse_buttons |= MouseEventButtons::from(blitz_button);
            Some(UiEvent::MouseDown(BlitzMouseButtonEvent {
                x: mouse_pos.0,
                y: mouse_pos.1,
                button: blitz_button,
                buttons: *mouse_buttons,
                mods: convert_modifiers(*mods),
            }))
        }
        MouseEvent::ButtonReleased {
            button,
            modifiers: mods,
        } => {
            let blitz_button = translate_mouse_button(*button);
            *mouse_buttons &= !MouseEventButtons::from(blitz_button);
            Some(UiEvent::MouseUp(BlitzMouseButtonEvent {
                x: mouse_pos.0,
                y: mouse_pos.1,
                button: blitz_button,
                buttons: *mouse_buttons,
                mods: convert_modifiers(*mods),
            }))
        }
        // Wheel events are not directly supported as UiEvent in blitz-traits 0.2
        MouseEvent::WheelScrolled { .. } => None,
        MouseEvent::CursorEntered => None,
        MouseEvent::CursorLeft => None,
        // Drag events - not currently translated to blitz events
        MouseEvent::DragEntered { .. } => None,
        MouseEvent::DragMoved { .. } => None,
        MouseEvent::DragLeft => None,
        MouseEvent::DragDropped { .. } => None,
    }
}

fn translate_mouse_button(button: MouseButton) -> MouseEventButton {
    match button {
        MouseButton::Left => MouseEventButton::Main,
        MouseButton::Right => MouseEventButton::Secondary,
        MouseButton::Middle => MouseEventButton::Auxiliary,
        MouseButton::Back => MouseEventButton::Fourth,
        MouseButton::Forward => MouseEventButton::Fifth,
        MouseButton::Other(_) => MouseEventButton::Main,
    }
}

/// Convert baseview/keyboard_types 0.6 Modifiers to blitz-traits/keyboard_types 0.7 Modifiers
/// Both types have the same bitflags values, so we can convert via the underlying bits.
fn convert_modifiers(mods: keyboard_types::Modifiers) -> BlitzModifiers {
    // The bitflags have the same values in both versions, so we can safely transmute
    // ALT = 1, CONTROL = 2, SHIFT = 4, META = 8, etc.
    BlitzModifiers::from_bits_truncate(mods.bits())
}
