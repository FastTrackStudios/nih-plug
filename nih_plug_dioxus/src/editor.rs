//! The [`Editor`] trait implementation for Dioxus editors.

use crate::state::DioxusState;
use crate::window::DioxusWindowHandler;
use baseview::{Size, WindowHandle, WindowOpenOptions, WindowScalePolicy};
use crossbeam::atomic::AtomicCell;
use dioxus::prelude::Element;
use nih_plug::prelude::{Editor, GuiContext, ParentWindowHandle};
use std::any::Any;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

/// An [`Editor`] implementation that renders a Dioxus UI using the Blitz/Vello renderer.
pub struct DioxusEditor {
    pub(crate) state: Arc<DioxusState>,
    pub(crate) app: fn() -> Element,
    pub(crate) scaling_factor: AtomicCell<Option<f32>>,
    pub(crate) needs_redraw: Arc<AtomicBool>,
}

impl DioxusEditor {
    pub fn new(state: Arc<DioxusState>, app: fn() -> Element) -> Self {
        Self {
            state,
            app,
            // On macOS, we use the system scaling factor
            #[cfg(target_os = "macos")]
            scaling_factor: AtomicCell::new(None),
            #[cfg(not(target_os = "macos"))]
            scaling_factor: AtomicCell::new(Some(1.0)),
            needs_redraw: Arc::new(AtomicBool::new(false)),
        }
    }
}

impl Editor for DioxusEditor {
    fn spawn(
        &self,
        parent: ParentWindowHandle,
        context: Arc<dyn GuiContext>,
    ) -> Box<dyn Any + Send> {
        let (width, height) = self.state.inner_logical_size();
        let scaling_factor = self.scaling_factor.load();

        let app = self.app;
        let gui_context = context.clone();
        let dioxus_state = self.state.clone();
        let needs_redraw = self.needs_redraw.clone();

        let window = baseview::Window::open_parented(
            &RwhAdapter(parent),
            WindowOpenOptions {
                title: String::from("Plugin Editor"),
                size: Size::new(width as f64, height as f64),
                scale: scaling_factor
                    .map(|f| WindowScalePolicy::ScaleFactor(f as f64))
                    .unwrap_or(WindowScalePolicy::SystemScaleFactor),
            },
            move |window| {
                DioxusWindowHandler::new(
                    window,
                    app,
                    gui_context.clone(),
                    dioxus_state.clone(),
                    needs_redraw.clone(),
                )
            },
        );

        self.state.set_open(true);
        Box::new(DioxusEditorHandle {
            state: self.state.clone(),
            window,
        })
    }

    fn size(&self) -> (u32, u32) {
        self.state.scaled_logical_size()
    }

    fn set_scale_factor(&self, factor: f32) -> bool {
        // Don't allow scale factor changes while the editor is open
        if self.state.is_open() {
            return false;
        }
        self.scaling_factor.store(Some(factor));
        true
    }

    fn param_value_changed(&self, _id: &str, _normalized_value: f32) {
        self.needs_redraw.store(true, Ordering::Relaxed);
    }

    fn param_modulation_changed(&self, _id: &str, _modulation_offset: f32) {
        self.needs_redraw.store(true, Ordering::Relaxed);
    }

    fn param_values_changed(&self) {
        self.needs_redraw.store(true, Ordering::Relaxed);
    }
}

/// Handle returned from `Editor::spawn()` that closes the window when dropped.
struct DioxusEditorHandle {
    state: Arc<DioxusState>,
    window: WindowHandle,
}

// The window handle contains raw pointers
unsafe impl Send for DioxusEditorHandle {}

impl Drop for DioxusEditorHandle {
    fn drop(&mut self) {
        self.state.set_open(false);
        self.window.close();
    }
}

/// Adapter to convert nih_plug's `ParentWindowHandle` to raw-window-handle 0.5 traits
/// (which is what baseview expects).
struct RwhAdapter(ParentWindowHandle);

// Implement raw-window-handle 0.5 traits for baseview compatibility
unsafe impl raw_window_handle_05::HasRawWindowHandle for RwhAdapter {
    fn raw_window_handle(&self) -> raw_window_handle_05::RawWindowHandle {
        match self.0 {
            ParentWindowHandle::X11Window(window) => {
                let mut handle = raw_window_handle_05::XcbWindowHandle::empty();
                handle.window = window as u32;
                raw_window_handle_05::RawWindowHandle::Xcb(handle)
            }
            ParentWindowHandle::AppKitNsView(ns_view) => {
                let mut handle = raw_window_handle_05::AppKitWindowHandle::empty();
                handle.ns_view = ns_view;
                raw_window_handle_05::RawWindowHandle::AppKit(handle)
            }
            ParentWindowHandle::Win32Hwnd(hwnd) => {
                let mut handle = raw_window_handle_05::Win32WindowHandle::empty();
                handle.hwnd = hwnd;
                raw_window_handle_05::RawWindowHandle::Win32(handle)
            }
        }
    }
}

unsafe impl raw_window_handle_05::HasRawDisplayHandle for RwhAdapter {
    fn raw_display_handle(&self) -> raw_window_handle_05::RawDisplayHandle {
        match self.0 {
            ParentWindowHandle::X11Window(_) => {
                // For X11, we need a display connection, but we don't have one
                // from the parent handle. Use an empty XCB display handle.
                let handle = raw_window_handle_05::XcbDisplayHandle::empty();
                raw_window_handle_05::RawDisplayHandle::Xcb(handle)
            }
            ParentWindowHandle::AppKitNsView(_) => {
                let handle = raw_window_handle_05::AppKitDisplayHandle::empty();
                raw_window_handle_05::RawDisplayHandle::AppKit(handle)
            }
            ParentWindowHandle::Win32Hwnd(_) => {
                let handle = raw_window_handle_05::WindowsDisplayHandle::empty();
                raw_window_handle_05::RawDisplayHandle::Windows(handle)
            }
        }
    }
}
