//! Dioxus Native GUI support for NIH-plug.
//!
//! This crate provides a native GPU-rendered UI framework for audio plugins
//! using Dioxus with the Blitz rendering engine (Vello + wgpu).
//!
//! # Features
//!
//! - **Windowed Editor**: Full Dioxus UI in a resizable window (using baseview)
//! - **Embedded Editor**: Render Dioxus UI to REAPER's TCP/MCP inline display
//! - **Custom WGPU**: Embed custom GPU-rendered content (spectrum analyzers, etc.)
//!
//! # Example - Windowed Editor
//!
//! ```ignore
//! use nih_plug_dioxus::prelude::*;
//!
//! fn editor(
//!     params: Arc<MyParams>,
//!     editor_state: Arc<DioxusState>,
//! ) -> Option<Box<dyn Editor>> {
//!     create_dioxus_editor(editor_state, App)
//! }
//!
//! #[component]
//! fn App() -> Element {
//!     rsx! {
//!         div { class: "plugin-ui",
//!             h1 { "My Plugin" }
//!         }
//!     }
//! }
//! ```
//!
//! # Example - Embedded Editor (REAPER TCP/MCP)
//!
//! ```ignore
//! use nih_plug_dioxus::embedded::DioxusEmbeddedEditor;
//!
//! impl Plugin for MyPlugin {
//!     fn embedded_editor(&mut self) -> Option<Arc<dyn EmbeddedEditor>> {
//!         Some(Arc::new(DioxusEmbeddedEditor::new(
//!             self.dioxus_state.clone(),
//!             App,
//!         )))
//!     }
//! }
//! ```
//!
//! # Example - Custom WGPU Canvas
//!
//! ```ignore
//! use nih_plug_dioxus::prelude::*;
//! use nih_plug_dioxus::custom_paint::{use_wgpu, CustomPaintSource};
//!
//! #[component]
//! fn SpectrumView() -> Element {
//!     let paint_source = SpectrumPaintSource::new();
//!     let canvas_id = use_wgpu(move || paint_source);
//!     
//!     rsx!(
//!         canvas { id: "spectrum", "src": canvas_id }
//!     )
//! }
//! ```

#![allow(clippy::type_complexity)]

use std::sync::Arc;

use dioxus::prelude::Element;
use nih_plug::prelude::Editor;

// Re-export dioxus for convenience
pub use dioxus;

// Public modules
pub mod assets;
pub mod context;
pub mod custom_paint;
#[cfg(feature = "embedded")]
pub mod embedded;
pub mod widgets;

// Internal modules
mod editor;
mod events;
#[cfg(feature = "hot-reload")]
mod hot_reload;
mod renderer;
mod state;
mod wgpu_state;
mod window;

pub use context::{use_param, use_param_context, use_param_normalized, ParamContext};
pub use state::DioxusState;

/// Compiled Tailwind CSS with shadcn/lumen-blocks theme variables.
///
/// This CSS is compiled at build time from `tailwind.css` using the Tailwind v4 CLI.
/// It includes:
/// - Full Tailwind v4 utility classes (tree-shaken to only used classes)
/// - CSS custom properties for light/dark themes (shadcn-compatible)
/// - Theme configuration for lumen-blocks components
///
/// To use this CSS, inject it via a document::Style element in your app component:
/// ```ignore
/// use nih_plug_dioxus::TAILWIND_CSS;
///
/// fn App() -> Element {
///     rsx! {
///         document::Style { {TAILWIND_CSS} }
///         div { class: "dark bg-background text-foreground",
///             // Your app content...
///         }
///     }
/// }
/// ```
///
/// For dark mode, add the `dark` class to a parent element.
pub const TAILWIND_CSS: &str = include_str!(concat!(env!("OUT_DIR"), "/tailwind.compiled.css"));

/// Alias for TAILWIND_CSS (legacy name)
pub const THEME_CSS: &str = TAILWIND_CSS;

/// Prelude module for convenient imports
pub mod prelude {
    pub use crate::context::{use_param, use_param_context, use_param_normalized, ParamContext};
    pub use crate::create_dioxus_editor;
    pub use crate::custom_paint::{use_wgpu, CustomPaintCtx, CustomPaintSource, DeviceHandle, TextureHandle};
    #[cfg(feature = "embedded")]
    pub use crate::embedded::DioxusEmbeddedEditor;
    pub use crate::state::DioxusState;
    pub use crate::widgets::*;
    pub use crate::TAILWIND_CSS;
    pub use crate::THEME_CSS;
    pub use dioxus::prelude::*;
}

/// Create a Dioxus-based editor for a NIH-plug plugin.
///
/// # Arguments
///
/// * `state` - The editor state, which tracks window size and open status
/// * `app` - The Dioxus component function that renders the UI (must be `fn() -> Element`)
///
/// # Example
///
/// ```ignore
/// use nih_plug_dioxus::prelude::*;
///
/// fn create_editor(params: Arc<MyParams>, state: Arc<DioxusState>) -> Option<Box<dyn Editor>> {
///     create_dioxus_editor(state, App)
/// }
///
/// #[component]
/// fn App() -> Element {
///     rsx! {
///         div {
///             h1 { "My Plugin" }
///         }
///     }
/// }
/// ```
///
/// # Note
///
/// The app component must be a function pointer (`fn() -> Element`), not a closure.
/// This is a limitation of Dioxus's VirtualDom. To pass data to your component,
/// use Dioxus contexts (via `provide_context` and `use_context`).
///
/// For plugin parameters, use the `use_param_context()` hook which is automatically
/// provided to all components.
pub fn create_dioxus_editor(
    state: Arc<DioxusState>,
    app: fn() -> Element,
) -> Option<Box<dyn Editor>> {
    Some(Box::new(editor::DioxusEditor::new(state, app)))
}
