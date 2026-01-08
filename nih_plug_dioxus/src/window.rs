//! Baseview window handler for Dioxus editors.

use crate::context::ParamContext;
use crate::events::translate_event;
use crate::renderer::Renderer;
use crate::state::DioxusState;
use crate::wgpu_state::WgpuState;

#[cfg(feature = "hot-reload")]
use crate::hot_reload::HotReloadState;

use baseview::{Event, EventStatus, Window, WindowHandler};
use blitz_dom::{Document as _, DocumentConfig};
use blitz_traits::events::MouseEventButtons;
use blitz_traits::shell::{ColorScheme, Viewport};
use dioxus::prelude::*;
use dioxus_native_dom::DioxusDocument;
use futures_util::task::ArcWake;
use nih_plug::prelude::GuiContext;

// Use Modifiers from our events module which handles the version conflict
use crate::events::Modifiers;
use raw_window_handle::{RawDisplayHandle, RawWindowHandle};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Instant;

/// The baseview window handler for Dioxus editors.
pub struct DioxusWindowHandler {
    // Dioxus state
    dioxus_doc: Option<DioxusDocument>,
    app: fn() -> Element,
    animation_start: Instant,

    // Rendering
    wgpu_state: Option<WgpuState>,
    renderer: Option<Renderer>,

    // NIH-plug integration
    gui_context: Arc<dyn GuiContext>,
    dioxus_state: Arc<DioxusState>,
    needs_redraw: Arc<AtomicBool>,

    // Input state
    mouse_pos: (f32, f32),
    mouse_buttons: MouseEventButtons,
    modifiers: Modifiers,

    // Hot reload
    #[cfg(feature = "hot-reload")]
    hot_reload: HotReloadState,

    // Window dimensions
    width: u32,
    height: u32,
    scale_factor: f32,

    // Cached window handles for wgpu surface creation (raw-window-handle 0.6 types)
    window_handle: Option<RawWindowHandle>,
    display_handle: Option<RawDisplayHandle>,
}

impl DioxusWindowHandler {
    /// Create a new window handler.
    ///
    /// The `app` function must be a function pointer (not a closure) because
    /// VirtualDom::new requires `fn() -> Element`.
    pub fn new(
        window: &mut Window,
        app: fn() -> Element,
        gui_context: Arc<dyn GuiContext>,
        dioxus_state: Arc<DioxusState>,
        needs_redraw: Arc<AtomicBool>,
    ) -> Self {
        // Get initial size from the dioxus state
        let (width, height) = dioxus_state.scaled_logical_size();
        let scale_factor = dioxus_state.user_scale_factor() as f32;

        // Get raw window handles using baseview's raw-window-handle 0.5 API
        // and convert them to raw-window-handle 0.6 types for wgpu
        let (window_handle, display_handle) = get_raw_handles_from_baseview(window);

        Self {
            dioxus_doc: None,
            app,
            animation_start: Instant::now(),
            wgpu_state: None,
            renderer: None,
            gui_context,
            dioxus_state,
            needs_redraw,
            mouse_pos: (0.0, 0.0),
            mouse_buttons: MouseEventButtons::empty(),
            modifiers: Modifiers::empty(),
            #[cfg(feature = "hot-reload")]
            hot_reload: HotReloadState::new(),
            width,
            height,
            scale_factor,
            window_handle,
            display_handle,
        }
    }

    /// Initialize the Dioxus document and rendering state.
    fn initialize(&mut self) {
        let (Some(window_handle), Some(display_handle)) = (self.window_handle, self.display_handle)
        else {
            nih_plug::nih_error!("Cannot initialize: missing window handles");
            return;
        };

        // Create wgpu state using the stored raw handles
        let wgpu_state =
            WgpuState::new_from_raw(window_handle, display_handle, self.width, self.height);

        // Create renderer
        let renderer = Renderer::new(&wgpu_state.device);

        // Create the Dioxus virtual DOM
        let vdom = VirtualDom::new(self.app);

        // Create viewport
        let viewport = Viewport::new(
            self.width,
            self.height,
            self.scale_factor,
            ColorScheme::Light,
        );

        // Create the Dioxus document
        let mut dioxus_doc = DioxusDocument::new(
            vdom,
            DocumentConfig {
                viewport: Some(viewport),
                ..Default::default()
            },
        );

        // Provide ParamContext to the Dioxus component tree
        let param_context = ParamContext::new(self.gui_context.clone(), self.needs_redraw.clone());

        dioxus_doc.vdom.in_scope(ScopeId::ROOT, move || {
            provide_context(param_context);
        });

        // Initial build
        dioxus_doc.initial_build();
        dioxus_doc.resolve(0.0);

        self.wgpu_state = Some(wgpu_state);
        self.renderer = Some(renderer);
        self.dioxus_doc = Some(dioxus_doc);

        // Connect to hot reload server
        #[cfg(feature = "hot-reload")]
        self.hot_reload.connect();
    }

    /// Get the current animation time in seconds.
    fn animation_time(&self) -> f64 {
        self.animation_start.elapsed().as_secs_f64()
    }
}

impl WindowHandler for DioxusWindowHandler {
    fn on_frame(&mut self, _window: &mut Window) {
        // Initialize on first frame
        if self.wgpu_state.is_none() {
            self.initialize();
        }

        // Get animation time upfront before any mutable borrows
        let animation_time = self.animation_start.elapsed().as_secs_f64();
        let needs_redraw = self.needs_redraw.clone();
        let scale_factor = self.scale_factor;
        let width = self.width;
        let height = self.height;

        let Some(doc) = &mut self.dioxus_doc else {
            return;
        };
        let Some(wgpu_state) = &self.wgpu_state else {
            return;
        };
        let Some(renderer) = &mut self.renderer else {
            return;
        };

        // Handle hot reload messages
        #[cfg(feature = "hot-reload")]
        self.hot_reload.process_messages(doc);

        // Create a waker that triggers redraw
        let waker = futures_util::task::waker(Arc::new(RedrawWaker(needs_redraw.clone())));

        // Poll the virtual DOM
        let cx = std::task::Context::from_waker(&waker);
        doc.poll(Some(cx));

        // Resolve layout with animation time
        doc.resolve(animation_time);

        // Render
        renderer.render(wgpu_state, doc, scale_factor, width, height);

        // Reset redraw flag
        self.needs_redraw.store(false, Ordering::Relaxed);
    }

    fn on_event(&mut self, _window: &mut Window, event: Event) -> EventStatus {
        match &event {
            Event::Window(baseview::WindowEvent::Resized(info)) => {
                self.width = info.physical_size().width as u32;
                self.height = info.physical_size().height as u32;
                self.scale_factor = info.scale() as f32;

                if let Some(doc) = &mut self.dioxus_doc {
                    doc.set_viewport(Viewport::new(
                        self.width,
                        self.height,
                        self.scale_factor,
                        ColorScheme::Light,
                    ));
                }
                if let Some(wgpu_state) = &mut self.wgpu_state {
                    wgpu_state.resize(self.width, self.height);
                }
                self.needs_redraw.store(true, Ordering::Relaxed);
                return EventStatus::Captured;
            }
            _ => {}
        }

        // Translate and dispatch event to Dioxus
        if let Some(doc) = &mut self.dioxus_doc {
            if let Some(ui_event) = translate_event(
                &event,
                &mut self.mouse_pos,
                &mut self.mouse_buttons,
                &mut self.modifiers,
            ) {
                doc.handle_ui_event(ui_event);
                self.needs_redraw.store(true, Ordering::Relaxed);
                return EventStatus::Captured;
            }
        }

        EventStatus::Ignored
    }
}

/// Waker that sets a flag to trigger a redraw.
struct RedrawWaker(Arc<AtomicBool>);

impl ArcWake for RedrawWaker {
    fn wake_by_ref(arc_self: &Arc<Self>) {
        arc_self.0.store(true, Ordering::Relaxed);
    }
}

/// Get raw window handles from baseview Window, converting from raw-window-handle 0.5
/// to raw-window-handle 0.6 types.
///
/// Baseview uses raw-window-handle 0.5 which has different types than 0.6.
/// We need to manually extract the raw pointers and reconstruct them as 0.6 types.
fn get_raw_handles_from_baseview(
    window: &Window,
) -> (Option<RawWindowHandle>, Option<RawDisplayHandle>) {
    // Use baseview's HasRawWindowHandle trait (0.5) to get the raw handles
    use raw_window_handle_05::HasRawDisplayHandle as HasRawDisplayHandle05;
    use raw_window_handle_05::HasRawWindowHandle as HasRawWindowHandle05;

    // Get the 0.5 raw handles
    let window_handle_05 = window.raw_window_handle();
    let display_handle_05 = window.raw_display_handle();

    // Convert 0.5 types to 0.6 types by extracting the raw data
    let window_handle_06 = convert_window_handle_05_to_06(window_handle_05);
    let display_handle_06 = convert_display_handle_05_to_06(display_handle_05);

    (window_handle_06, display_handle_06)
}

/// Convert a raw-window-handle 0.5 RawWindowHandle to 0.6
fn convert_window_handle_05_to_06(
    handle: raw_window_handle_05::RawWindowHandle,
) -> Option<RawWindowHandle> {
    use std::num::NonZeroIsize;
    use std::ptr::NonNull;

    match handle {
        #[cfg(target_os = "macos")]
        raw_window_handle_05::RawWindowHandle::AppKit(h) => {
            let mut handle =
                raw_window_handle::AppKitWindowHandle::new(NonNull::new(h.ns_view as *mut _)?);
            Some(RawWindowHandle::AppKit(handle))
        }
        #[cfg(target_os = "windows")]
        raw_window_handle_05::RawWindowHandle::Win32(h) => {
            let handle =
                raw_window_handle::Win32WindowHandle::new(NonZeroIsize::new(h.hwnd as isize)?);
            Some(RawWindowHandle::Win32(handle))
        }
        #[cfg(target_os = "linux")]
        raw_window_handle_05::RawWindowHandle::Xcb(h) => {
            let handle =
                raw_window_handle::XcbWindowHandle::new(NonZeroIsize::new(h.window as isize)?);
            Some(RawWindowHandle::Xcb(handle))
        }
        #[cfg(target_os = "linux")]
        raw_window_handle_05::RawWindowHandle::Xlib(h) => {
            let handle = raw_window_handle::XlibWindowHandle::new(h.window as u32);
            Some(RawWindowHandle::Xlib(handle))
        }
        _ => None,
    }
}

/// Convert a raw-window-handle 0.5 RawDisplayHandle to 0.6
fn convert_display_handle_05_to_06(
    handle: raw_window_handle_05::RawDisplayHandle,
) -> Option<RawDisplayHandle> {
    use std::ptr::NonNull;

    match handle {
        #[cfg(target_os = "macos")]
        raw_window_handle_05::RawDisplayHandle::AppKit(_) => Some(RawDisplayHandle::AppKit(
            raw_window_handle::AppKitDisplayHandle::new(),
        )),
        #[cfg(target_os = "windows")]
        raw_window_handle_05::RawDisplayHandle::Windows(_) => Some(RawDisplayHandle::Windows(
            raw_window_handle::WindowsDisplayHandle::new(),
        )),
        #[cfg(target_os = "linux")]
        raw_window_handle_05::RawDisplayHandle::Xcb(h) => {
            let connection = NonNull::new(h.connection as *mut _);
            let handle = raw_window_handle::XcbDisplayHandle::new(connection, h.screen);
            Some(RawDisplayHandle::Xcb(handle))
        }
        #[cfg(target_os = "linux")]
        raw_window_handle_05::RawDisplayHandle::Xlib(h) => {
            let display = NonNull::new(h.display as *mut _);
            let handle = raw_window_handle::XlibDisplayHandle::new(display, h.screen);
            Some(RawDisplayHandle::Xlib(handle))
        }
        _ => None,
    }
}
