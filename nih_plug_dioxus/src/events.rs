//! Event translation from baseview to blitz.

use baseview::{Event, MouseButton, MouseEvent, WindowEvent};
use blitz_traits::events::{BlitzMouseButtonEvent, MouseEventButton, MouseEventButtons, UiEvent};

// Re-export Modifiers from keyboard_types (0.6, matching baseview)
// This is the type we use in window.rs for tracking modifier state
pub use keyboard_types::Modifiers;

// Alias for the 0.7 Modifiers from blitz-traits (via dioxus re-export)
// This is what BlitzMouseButtonEvent expects
type BlitzModifiers = dioxus::prelude::Modifiers;

/// Translate a baseview event to a blitz UiEvent.
pub fn translate_event(
    event: &Event,
    mouse_pos: &mut (f32, f32),
    mouse_buttons: &mut MouseEventButtons,
    modifiers: &mut Modifiers,
) -> Option<UiEvent> {
    match event {
        Event::Mouse(mouse_event) => {
            translate_mouse_event(mouse_event, mouse_pos, mouse_buttons, modifiers)
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
) -> Option<UiEvent> {
    match event {
        MouseEvent::CursorMoved {
            position,
            modifiers: mods,
        } => {
            mouse_pos.0 = position.x as f32;
            mouse_pos.1 = position.y as f32;
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
