//! Standalone window support for testing Dioxus editors outside a DAW.
//!
//! This module provides a way to open a standalone baseview window with the
//! native wgpu surface rendering path, without requiring a plugin host.

use crate::state::DioxusState;
use crate::SharedState;
use baseview::{Size, Window, WindowOpenOptions, WindowScalePolicy};
use dioxus_native::prelude::Element;
use nih_plug::prelude::{GuiContext, ParamPtr, PluginApi, PluginState};
use std::collections::BTreeMap;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

/// A no-op GuiContext for standalone testing.
/// Parameter automation and state are not available outside a DAW.
struct StandaloneGuiContext;

impl GuiContext for StandaloneGuiContext {
    fn plugin_api(&self) -> PluginApi {
        PluginApi::Clap
    }

    fn request_resize(&self) -> bool {
        true
    }

    unsafe fn raw_begin_set_parameter(&self, _param: ParamPtr) {}
    unsafe fn raw_set_parameter_normalized(&self, _param: ParamPtr, _normalized: f32) {}
    unsafe fn raw_end_set_parameter(&self, _param: ParamPtr) {}

    fn get_state(&self) -> PluginState {
        PluginState {
            version: String::new(),
            params: BTreeMap::new(),
            fields: BTreeMap::new(),
        }
    }

    fn set_state(&self, _state: PluginState) {}
}

/// Open a standalone window that renders a Dioxus component using the native
/// wgpu surface path. This blocks until the window is closed.
///
/// This is useful for testing GUI rendering without a DAW host.
///
/// # Arguments
///
/// * `app` - The Dioxus component function to render
/// * `width` - Window width in logical pixels
/// * `height` - Window height in logical pixels
pub fn open_standalone(app: fn() -> Element, width: u32, height: u32) {
    open_standalone_with_state(app, width, height, None);
}

/// Open a standalone window with shared state.
pub fn open_standalone_with_state(
    app: fn() -> Element,
    width: u32,
    height: u32,
    shared_state: Option<SharedState>,
) {
    let dioxus_state = DioxusState::new(move || (width, height));
    let gui_context: Arc<dyn GuiContext> = Arc::new(StandaloneGuiContext);
    let needs_redraw = Arc::new(AtomicBool::new(true));

    Window::open_blocking(
        WindowOpenOptions {
            title: String::from("FTS GUI Test (Native wgpu Surface)"),
            size: Size::new(width as f64, height as f64),
            scale: WindowScalePolicy::ScaleFactor(1.0),
        },
        move |window| {
            // Use the native wgpu surface handler (not softbuffer)
            #[cfg(not(feature = "softbuffer-blit"))]
            {
                crate::window::DioxusWindowHandler::new_with_state(
                    window,
                    app,
                    gui_context.clone(),
                    dioxus_state.clone(),
                    needs_redraw.clone(),
                    shared_state,
                )
            }
            #[cfg(feature = "softbuffer-blit")]
            {
                crate::window_softbuffer::DioxusSoftbufferWindowHandler::new_with_state(
                    window,
                    app,
                    gui_context.clone(),
                    dioxus_state.clone(),
                    needs_redraw.clone(),
                    shared_state,
                )
            }
        },
    );
}
