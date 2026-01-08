//! Editor state management for Dioxus editors.

use crossbeam::atomic::AtomicCell;
use nih_plug::params::persist::PersistentField;
use serde::{Deserialize, Serialize};
use std::fmt::Debug;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

/// State for a `nih_plug_dioxus` editor.
///
/// This tracks the window size, scale factor, and whether the editor is currently open.
/// The state can be persisted with the plugin's parameters using the `#[persist]` attribute.
///
/// # Example
///
/// ```ignore
/// #[derive(Params)]
/// struct MyParams {
///     #[persist = "editor-state"]
///     editor_state: Arc<DioxusState>,
///
///     #[id = "gain"]
///     gain: FloatParam,
/// }
///
/// impl Default for MyParams {
///     fn default() -> Self {
///         Self {
///             editor_state: DioxusState::new(|| (400, 300)),
///             gain: FloatParam::new("Gain", 1.0, FloatRange::Linear { min: 0.0, max: 2.0 }),
///         }
///     }
/// }
/// ```
#[derive(Serialize, Deserialize)]
pub struct DioxusState {
    /// A function that returns the window's current size in logical pixels.
    #[serde(skip, default = "empty_size_fn")]
    size_fn: Box<dyn Fn() -> (u32, u32) + Send + Sync>,

    /// A scale factor applied on top of any system HiDPI scaling.
    #[serde(with = "nih_plug::params::persist::serialize_atomic_cell")]
    scale_factor: AtomicCell<f64>,

    /// Whether the editor window is currently open.
    #[serde(skip)]
    open: AtomicBool,
}

fn empty_size_fn() -> Box<dyn Fn() -> (u32, u32) + Send + Sync> {
    Box::new(|| (0, 0))
}

impl Debug for DioxusState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let (width, height) = (self.size_fn)();
        f.debug_struct("DioxusState")
            .field("size_fn", &format!("<fn> ({}, {})", width, height))
            .field("scale_factor", &self.scale_factor)
            .field("open", &self.open)
            .finish()
    }
}

impl DioxusState {
    /// Create a new editor state with a size function.
    ///
    /// The size function should return the window's logical size in pixels.
    /// This can be a static size or computed based on plugin state.
    ///
    /// # Example
    ///
    /// ```ignore
    /// // Static size
    /// let state = DioxusState::new(|| (400, 300));
    ///
    /// // Dynamic size based on some state
    /// let expanded = Arc::new(AtomicBool::new(false));
    /// let state = DioxusState::new(move || {
    ///     if expanded.load(Ordering::Relaxed) {
    ///         (800, 600)
    ///     } else {
    ///         (400, 300)
    ///     }
    /// });
    /// ```
    pub fn new(size_fn: impl Fn() -> (u32, u32) + Send + Sync + 'static) -> Arc<Self> {
        Arc::new(Self {
            size_fn: Box::new(size_fn),
            scale_factor: AtomicCell::new(1.0),
            open: AtomicBool::new(false),
        })
    }

    /// Create a new editor state with a custom default scale factor.
    ///
    /// This scale factor is applied on top of any system HiDPI scaling.
    pub fn new_with_default_scale_factor(
        size_fn: impl Fn() -> (u32, u32) + Send + Sync + 'static,
        default_scale_factor: f64,
    ) -> Arc<Self> {
        Arc::new(Self {
            size_fn: Box::new(size_fn),
            scale_factor: AtomicCell::new(default_scale_factor),
            open: AtomicBool::new(false),
        })
    }

    /// Returns the window size in logical pixels after applying the user scale factor.
    pub fn scaled_logical_size(&self) -> (u32, u32) {
        let (width, height) = self.inner_logical_size();
        let scale = self.scale_factor.load();
        (
            (width as f64 * scale).round() as u32,
            (height as f64 * scale).round() as u32,
        )
    }

    /// Returns the window size in logical pixels before applying the user scale factor.
    pub fn inner_logical_size(&self) -> (u32, u32) {
        (self.size_fn)()
    }

    /// Get the user scale factor.
    pub fn user_scale_factor(&self) -> f64 {
        self.scale_factor.load()
    }

    /// Set the user scale factor.
    pub fn set_user_scale_factor(&self, factor: f64) {
        self.scale_factor.store(factor);
    }

    /// Returns whether the editor window is currently open.
    pub fn is_open(&self) -> bool {
        self.open.load(Ordering::Acquire)
    }

    /// Set the open state (internal use).
    pub(crate) fn set_open(&self, open: bool) {
        self.open.store(open, Ordering::Release);
    }
}

impl<'a> PersistentField<'a, DioxusState> for Arc<DioxusState> {
    fn set(&self, new_value: DioxusState) {
        self.scale_factor.store(new_value.scale_factor.load());
    }

    fn map<F, R>(&self, f: F) -> R
    where
        F: Fn(&DioxusState) -> R,
    {
        f(self)
    }
}
