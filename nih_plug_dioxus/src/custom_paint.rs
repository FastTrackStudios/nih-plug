//! Scene overlay support for embedding custom GPU-rendered content in Dioxus UI.
//!
//! This module provides the `use_scene_overlay` hook and re-exports the `SceneOverlay` trait
//! from the renderer, allowing you to embed custom vello-rendered content (like EQ graphs,
//! spectrum analyzers, oscilloscopes) within your Dioxus plugin UI.
//!
//! Scene overlays paint directly into the main vello Scene after the Dioxus DOM is rendered,
//! giving full access to vello's 2D vector graphics (anti-aliased curves, gradients, glow effects).
//!
//! # Element-relative positioning
//!
//! Overlays can be positioned relative to a DOM element by calling `set_rect()` on the
//! returned `OverlayHandle`. The overlay's `paint()` receives element-local coordinates
//! (width/height of the rect), while the renderer handles the window-space transform.
//!
//! # Example
//!
//! ```ignore
//! use nih_plug_dioxus::prelude::*;
//! use nih_plug_dioxus::custom_paint::{use_scene_overlay, SceneOverlay};
//!
//! struct MyOverlay { /* shared state */ }
//!
//! impl SceneOverlay for MyOverlay {
//!     fn paint(&mut self, scene: &mut vello::Scene, width: u32, height: u32, scale: f64) {
//!         // width/height are the overlay rect dimensions, NOT the full window.
//!         // Paint in element-local coordinates (0,0 = top-left of overlay rect).
//!     }
//! }
//!
//! #[component]
//! fn MyView() -> Element {
//!     let overlay = use_scene_overlay(|| MyOverlay::new());
//!     // Position the overlay at (10, 50) with size 800x400 CSS pixels
//!     overlay.set_rect(10.0, 50.0, 800.0, 400.0);
//!     rsx!(div { style: "width:100%; height:100%;", /* interaction handling */ })
//! }
//! ```

use std::cell::RefCell;
use std::rc::Rc;

use dioxus_native::prelude::dioxus_core::use_hook_with_cleanup;
use dioxus_native::prelude::*;
use vello::kurbo::{Affine, Rect};

// Re-export the SceneOverlay trait and vello types for users
pub use crate::renderer::SceneOverlay;
pub use vello;

/// Rectangle in CSS (logical) pixels, relative to the window origin.
#[derive(Clone, Debug, Default)]
pub struct OverlayRect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

/// Internal entry for a registered overlay.
struct OverlayEntry {
    id: u64,
    overlay: Box<dyn SceneOverlay>,
    /// Element-relative rect (CSS pixels). `None` = paint in full window space.
    rect: Option<OverlayRect>,
}

/// Handle to the renderer's overlay registry.
///
/// Provided as a Dioxus context so that `use_scene_overlay` can register overlays
/// at component mount time. The actual rendering happens in the Renderer's render loop.
#[derive(Clone)]
pub struct OverlayRegistry {
    inner: Rc<RefCell<OverlayRegistryInner>>,
}

struct OverlayRegistryInner {
    entries: Vec<OverlayEntry>,
    next_id: u64,
}

impl Default for OverlayRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl OverlayRegistry {
    pub fn new() -> Self {
        Self {
            inner: Rc::new(RefCell::new(OverlayRegistryInner {
                entries: Vec::new(),
                next_id: 1,
            })),
        }
    }

    /// Register a scene overlay and return its unique ID.
    pub fn register(&self, overlay: Box<dyn SceneOverlay>) -> u64 {
        let mut inner = self.inner.borrow_mut();
        let id = inner.next_id;
        inner.next_id += 1;
        inner.entries.push(OverlayEntry {
            id,
            overlay,
            rect: None,
        });
        id
    }

    /// Unregister a scene overlay by ID.
    pub fn unregister(&self, id: u64) {
        let mut inner = self.inner.borrow_mut();
        inner.entries.retain(|e| e.id != id);
    }

    /// Update the overlay's position and size (CSS pixels, window-relative).
    ///
    /// When set, the renderer will:
    /// 1. Apply a translate+scale transform so the overlay paints in element-local coords
    /// 2. Clip to the rect boundaries
    /// 3. Pass the rect width/height (not window size) to `paint()`
    pub fn set_rect(&self, id: u64, rect: OverlayRect) {
        let mut inner = self.inner.borrow_mut();
        if let Some(entry) = inner.entries.iter_mut().find(|e| e.id == id) {
            entry.rect = Some(rect);
        }
    }

    /// Paint all registered overlays into the scene.
    /// Called by the Renderer each frame.
    pub fn paint_all(
        &self,
        scene: &mut vello::Scene,
        width: u32,
        height: u32,
        scale: f64,
    ) {
        let mut inner = self.inner.borrow_mut();
        for entry in &mut inner.entries {
            if let Some(rect) = &entry.rect {
                // Skip zero-size rects
                if rect.width < 1.0 || rect.height < 1.0 {
                    continue;
                }

                // Transform: position at (rect.x, rect.y) in window, scaled by display factor
                let transform =
                    Affine::translate((rect.x * scale, rect.y * scale)) * Affine::scale(scale);

                // Clip to the overlay rect (in element-local CSS coords, pre-transform)
                let clip = Rect::new(0.0, 0.0, rect.width, rect.height);
                scene.push_clip_layer(transform, &clip);

                // Paint in element-local coordinates
                entry.overlay.paint(
                    scene,
                    rect.width as u32,
                    rect.height as u32,
                    scale,
                );

                scene.pop_layer();
            } else {
                // No rect set — paint in full window space (legacy mode)
                entry.overlay.paint(scene, width, height, scale);
            }
        }
    }
}

/// Handle returned by `use_scene_overlay` for controlling an overlay's position.
#[derive(Clone)]
pub struct OverlayHandle {
    registry: OverlayRegistry,
    id: u64,
}

impl OverlayHandle {
    /// Set the overlay's position and size in CSS (logical) pixels, relative to the window.
    ///
    /// Call this each render to keep the overlay positioned correctly as the layout changes.
    pub fn set_rect(&self, x: f64, y: f64, width: f64, height: f64) {
        self.registry
            .set_rect(self.id, OverlayRect { x, y, width, height });
    }
}

/// Register a scene overlay that renders custom vello content each frame.
///
/// Returns an `OverlayHandle` that you MUST call `set_rect()` on each render
/// to position the overlay within the window.
///
/// The overlay's `paint()` method receives element-local coordinates —
/// (0,0) is the top-left of the rect, and width/height are the rect dimensions.
///
/// The overlay is automatically unregistered when the component unmounts.
///
/// # Arguments
///
/// * `create_overlay` - A closure that creates the overlay. Called once on first render.
pub fn use_scene_overlay<T: SceneOverlay>(create_overlay: impl FnOnce() -> T) -> OverlayHandle {
    let (registry, id) = use_hook_with_cleanup(
        || {
            let registry = consume_context::<OverlayRegistry>();
            let overlay = Box::new(create_overlay());
            let id = registry.register(overlay);
            (registry, id)
        },
        |(registry, id)| {
            registry.unregister(id);
        },
    );
    OverlayHandle {
        registry: registry.clone(),
        id,
    }
}
