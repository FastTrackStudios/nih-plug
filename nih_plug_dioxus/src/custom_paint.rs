//! Custom WGPU paint source support for embedding custom GPU rendering in Dioxus UI.
//!
//! This module provides the `use_wgpu` hook and re-exports the `CustomPaintSource` trait,
//! allowing you to embed custom WGPU-rendered content (like spectrum analyzers, oscilloscopes,
//! or other visualizations) within your Dioxus plugin UI.
//!
//! # Example
//!
//! ```ignore
//! use nih_plug_dioxus::prelude::*;
//!
//! // Implement CustomPaintSource for your renderer
//! struct SpectrumPaintSource { /* ... */ }
//!
//! impl CustomPaintSource for SpectrumPaintSource {
//!     fn resume(&mut self, device_handle: &DeviceHandle) {
//!         // Initialize GPU resources
//!     }
//!     
//!     fn suspend(&mut self) {
//!         // Cleanup when not visible
//!     }
//!     
//!     fn render(&mut self, ctx: CustomPaintCtx<'_>, width: u32, height: u32, scale: f64) -> Option<TextureHandle> {
//!         // Render to texture and return handle
//!     }
//! }
//!
//! #[component]
//! fn SpectrumView() -> Element {
//!     let paint_source = SpectrumPaintSource::new();
//!     let paint_source_id = use_wgpu(move || paint_source);
//!     
//!     rsx!(
//!         canvas { id: "spectrum", "src": paint_source_id }
//!     )
//! }
//! ```

use std::cell::RefCell;
use std::rc::Rc;

use dioxus::prelude::*;
use dioxus_core::use_hook_with_cleanup;

// Re-export types from anyrender_vello for users implementing CustomPaintSource
pub use anyrender_vello::{CustomPaintCtx, CustomPaintSource, TextureHandle};

// Re-export wgpu for users who need direct GPU access
pub use wgpu;

// Re-export DeviceHandle from wgpu_context (used by anyrender_vello)
pub use wgpu_context::DeviceHandle;

/// Register a custom WGPU paint source with the Dioxus renderer.
///
/// This hook creates a custom paint source and registers it with the renderer,
/// returning an ID that can be used as the `src` attribute of a `<canvas>` element.
///
/// The paint source will be:
/// - `resume()`d when the GPU device is available
/// - `render()`ed each frame when visible
/// - `suspend()`ed when the GPU device is lost or the component unmounts
///
/// # Arguments
///
/// * `create_source` - A closure that creates the paint source. This is called once
///   when the hook is first used.
///
/// # Returns
///
/// A unique ID that should be passed to a `<canvas>` element's `src` attribute.
///
/// # Example
///
/// ```ignore
/// let paint_source_id = use_wgpu(|| MyPaintSource::new());
/// rsx!(canvas { "src": paint_source_id })
/// ```
pub fn use_wgpu<T: CustomPaintSource + 'static>(create_source: impl FnOnce() -> T) -> u64 {
    let (_renderer, id) = use_hook_with_cleanup(
        || {
            let renderer = consume_context::<DioxusRenderer>();
            let source = Box::new(create_source());
            let id = renderer.register_custom_paint_source(source);
            (renderer, id)
        },
        |(renderer, id)| {
            renderer.unregister_custom_paint_source(id);
        },
    );

    id
}

/// Wrapper around the renderer that allows registering custom paint sources.
///
/// This is provided as a context to all Dioxus components when using nih_plug_dioxus.
#[derive(Clone)]
pub struct DioxusRenderer {
    inner: Rc<RefCell<RendererInner>>,
}

struct RendererInner {
    paint_sources: Vec<(u64, Box<dyn CustomPaintSource>)>,
    next_id: u64,
}

impl Default for DioxusRenderer {
    fn default() -> Self {
        Self::new()
    }
}

impl DioxusRenderer {
    pub fn new() -> Self {
        Self {
            inner: Rc::new(RefCell::new(RendererInner {
                paint_sources: Vec::new(),
                next_id: 1,
            })),
        }
    }

    /// Register a custom paint source and return its ID.
    pub fn register_custom_paint_source(&self, source: Box<dyn CustomPaintSource>) -> u64 {
        let mut inner = self.inner.borrow_mut();
        let id = inner.next_id;
        inner.next_id += 1;
        inner.paint_sources.push((id, source));
        id
    }

    /// Unregister a custom paint source by ID.
    pub fn unregister_custom_paint_source(&self, id: u64) {
        let mut inner = self.inner.borrow_mut();
        inner.paint_sources.retain(|(source_id, _)| *source_id != id);
    }

    /// Get a mutable reference to a paint source by ID.
    pub fn get_paint_source_mut(&self, id: u64) -> Option<impl std::ops::DerefMut<Target = dyn CustomPaintSource> + '_> {
        let inner = self.inner.borrow_mut();
        // This is a bit awkward because we need to return a guard that holds the borrow
        // For now, we'll use a different approach in the actual renderer
        None::<std::cell::RefMut<'_, dyn CustomPaintSource>>
    }

    /// Resume all paint sources (called when GPU device is available).
    pub fn resume_all(&self, device_handle: &DeviceHandle) {
        let mut inner = self.inner.borrow_mut();
        for (_, source) in &mut inner.paint_sources {
            source.resume(device_handle);
        }
    }

    /// Suspend all paint sources (called when GPU device is lost).
    pub fn suspend_all(&self) {
        let mut inner = self.inner.borrow_mut();
        for (_, source) in &mut inner.paint_sources {
            source.suspend();
        }
    }

    /// Iterate over paint sources for rendering.
    pub fn with_paint_sources<F, R>(&self, f: F) -> R
    where
        F: FnOnce(&mut [(u64, Box<dyn CustomPaintSource>)]) -> R,
    {
        let mut inner = self.inner.borrow_mut();
        f(&mut inner.paint_sources)
    }
}
