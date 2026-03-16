//! Baseview window handler for Dioxus editors.
//!
//! This module provides the standard wgpu surface-based window handler.
//! For Linux/XWayland compatibility, use the `softbuffer-blit` feature which
//! renders with wgpu to an offscreen texture and blits via softbuffer.

// This module is only used when softbuffer-blit is NOT enabled
#![cfg(not(feature = "softbuffer-blit"))]

use crate::SharedState;
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
use crossbeam::channel::{Receiver, Sender, unbounded};
use dioxus_native::DioxusDocument;
use dioxus_native::prelude::*;
use futures_util::task::ArcWake;
use nih_plug::prelude::GuiContext;

// Use Modifiers from our events module which handles the version conflict
use crate::events::Modifiers;
use raw_window_handle::{RawDisplayHandle, RawWindowHandle};
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;

/// Messages sent from Dioxus components to the window handler.
/// Used for document operations like injecting stylesheets.
enum DocumentMessage {
    CreateHeadElement {
        name: String,
        attributes: Vec<(String, String)>,
        contents: Option<String>,
    },
}

/// Proxy for document operations from Dioxus components.
/// Implements `dioxus::document::Document` to enable `document::Style` etc.
#[derive(Clone)]
pub struct DocumentProxy {
    sender: Sender<DocumentMessage>,
}

impl DocumentProxy {
    fn new(sender: Sender<DocumentMessage>) -> Self {
        Self { sender }
    }

    fn create_head_element(
        &self,
        name: &str,
        attributes: &[(&str, String)],
        contents: Option<String>,
    ) {
        let _ = self.sender.send(DocumentMessage::CreateHeadElement {
            name: name.to_string(),
            attributes: attributes
                .iter()
                .map(|(k, v)| (k.to_string(), v.clone()))
                .collect(),
            contents,
        });
    }
}

impl document::Document for DocumentProxy {
    fn eval(&self, js: String) -> document::Eval {
        // No-op for native - we don't support JS eval
        document::NoOpDocument.eval(js)
    }

    fn set_title(&self, title: String) {
        self.create_head_element("title", &[], Some(title));
    }

    fn create_meta(&self, props: document::MetaProps) {
        self.create_head_element("meta", &props.attributes(), None);
    }

    fn create_script(&self, props: document::ScriptProps) {
        self.create_head_element("script", &props.attributes(), props.script_contents().ok());
    }

    fn create_style(&self, props: document::StyleProps) {
        self.create_head_element("style", &props.attributes(), props.style_contents().ok());
    }

    fn create_link(&self, props: document::LinkProps) {
        self.create_head_element("link", &props.attributes(), None);
    }

    fn create_head_component(&self) -> bool {
        true
    }
}

/// The baseview window handler for Dioxus editors using standard wgpu surface.
pub struct DioxusWindowHandler {
    // Dioxus state
    dioxus_doc: Option<DioxusDocument>,
    app: fn() -> Element,
    animation_start: Instant,

    // Rendering - standard wgpu surface mode
    wgpu_state: Option<WgpuState>,
    renderer: Option<Renderer>,

    // NIH-plug integration
    gui_context: Arc<dyn GuiContext>,
    dioxus_state: Arc<DioxusState>,
    needs_redraw: Arc<AtomicBool>,

    // Shared UI state (injected into Dioxus context)
    shared_state: Option<SharedState>,

    // Document message channel (for document::Style etc.)
    doc_message_receiver: Option<Receiver<DocumentMessage>>,

    // Input state
    mouse_pos: (f32, f32),
    mouse_buttons: MouseEventButtons,
    modifiers: Modifiers,

    // Hot reload
    #[cfg(feature = "hot-reload")]
    hot_reload: HotReloadState,

    // Window dimensions in PHYSICAL pixels (for wgpu surface and Blitz viewport)
    width: u32,
    height: u32,
    // System scale factor (from resize events)
    scale_factor: f32,
    // Whether we've received a resize event with the actual scale factor
    received_resize: bool,

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
        Self::new_with_state(window, app, gui_context, dioxus_state, needs_redraw, None)
    }

    /// Create a new window handler with shared state.
    ///
    /// The shared state will be injected into the Dioxus context and available
    /// via `use_context::<SharedState>()` in components.
    pub fn new_with_state(
        window: &mut Window,
        app: fn() -> Element,
        gui_context: Arc<dyn GuiContext>,
        dioxus_state: Arc<DioxusState>,
        needs_redraw: Arc<AtomicBool>,
        shared_state: Option<SharedState>,
    ) -> Self {
        // Get initial logical size from the dioxus state (this is what we asked for)
        let (logical_width, logical_height) = dioxus_state.inner_logical_size();

        // On macOS, we use SystemScaleFactor which means we don't know the actual
        // scale until we get a resize event. Default to 1.0 but this will be updated.
        // We estimate 2.0 for Retina displays as a reasonable starting point.
        #[cfg(target_os = "macos")]
        let scale_factor = 2.0f32; // Retina default
        #[cfg(not(target_os = "macos"))]
        let scale_factor = 1.0f32;

        // Get raw window handles using baseview's raw-window-handle 0.5 API
        // and convert them to raw-window-handle 0.6 types for wgpu
        let (window_handle, display_handle) = get_raw_handles_from_baseview(window);

        // Debug: log what handles we got
        nih_plug::nih_log!("[HANDLES] window: {:?}, display: {:?}",
            window_handle.as_ref().map(|h| format!("{:?}", h)),
            display_handle.as_ref().map(|h| format!("{:?}", h)));

        // Calculate initial physical size (will be corrected on first resize event)
        let physical_width = (logical_width as f32 * scale_factor) as u32;
        let physical_height = (logical_height as f32 * scale_factor) as u32;

        Self {
            dioxus_doc: None,
            app,
            animation_start: Instant::now(),
            wgpu_state: None,
            renderer: None,
            gui_context,
            dioxus_state,
            needs_redraw,
            shared_state,
            doc_message_receiver: None,
            mouse_pos: (0.0, 0.0),
            mouse_buttons: MouseEventButtons::empty(),
            modifiers: Modifiers::empty(),
            #[cfg(feature = "hot-reload")]
            hot_reload: HotReloadState::new(),
            // Store PHYSICAL dimensions - updated on resize events
            width: physical_width,
            height: physical_height,
            scale_factor,
            received_resize: false,
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

        // self.width and self.height are already in PHYSICAL pixels
        let physical_width = self.width.max(1);
        let physical_height = self.height.max(1);

        nih_plug::nih_log!(
            "[INIT] physical: {}x{}, scale: {}",
            physical_width,
            physical_height,
            self.scale_factor
        );

        // Create wgpu state using physical size for the GPU surface
        let wgpu_state = WgpuState::new_from_raw(
            window_handle,
            display_handle,
            physical_width,
            physical_height,
        );

        // Create renderer
        let renderer = Renderer::new(&wgpu_state.device);

        // Create the Dioxus virtual DOM
        let vdom = VirtualDom::new(self.app);

        // Create viewport with PHYSICAL size and scale factor
        let viewport = Viewport::new(
            self.width,
            self.height,
            self.scale_factor,
            ColorScheme::Light,
        );

        nih_plug::nih_log!(
            "[VIEWPORT] Creating DioxusDocument with viewport: {}x{} physical, scale={}",
            self.width, self.height, self.scale_factor
        );

        // Create the Dioxus document
        let mut dioxus_doc = DioxusDocument::new(
            vdom,
            DocumentConfig {
                viewport: Some(viewport),
                ..Default::default()
            },
        );

        // Create channel for document messages (for document::Style etc.)
        let (doc_sender, doc_receiver) = unbounded();

        // Provide contexts to the Dioxus component tree
        let param_context = ParamContext::new(self.gui_context.clone(), self.needs_redraw.clone());
        let shared_state = self.shared_state.take();
        let dioxus_state_for_context = self.dioxus_state.clone();

        // Create DocumentProxy for document::Style support
        let doc_proxy = DocumentProxy::new(doc_sender);
        let doc_proxy_rc = Rc::new(doc_proxy);

        dioxus_doc.vdom.in_scope(ScopeId::ROOT, move || {
            // Provide DocumentProxy as Document for document::Style
            provide_context(doc_proxy_rc as Rc<dyn document::Document>);

            // Provide ParamContext for parameter bindings
            provide_context(param_context);

            // Inject DioxusState so ResizeHandle can access it
            provide_context(dioxus_state_for_context);

            // Inject shared state if provided
            if let Some(state) = shared_state {
                provide_context(state);
            }
        });

        // Initial build - this may queue document::Style messages
        dioxus_doc.initial_build();

        // Process any document messages that were queued during initial_build()
        // This is CRITICAL - CSS must be added to the stylist BEFORE resolve()
        while let Ok(msg) = doc_receiver.try_recv() {
            match msg {
                DocumentMessage::CreateHeadElement {
                    name,
                    attributes,
                    contents,
                } => {
                    let attrs: Vec<(String, String)> = attributes;
                    dioxus_doc.create_head_element(&name, &attrs, &contents);
                }
            }
        }

        // Store the receiver for processing messages during on_frame
        self.doc_message_receiver = Some(doc_receiver);

        // Now resolve layout (CSS is already added)
        dioxus_doc.inner_mut().resolve(0.0);

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
    fn on_frame(&mut self, window: &mut Window) {
        // Initialize after receiving the first resize event (which gives us the actual scale factor)
        // On macOS with SystemScaleFactor, we need to wait for this to get the HiDPI scale
        if self.wgpu_state.is_none() {
            if self.received_resize {
                self.initialize();
            } else {
                // Skip this frame, wait for resize event
                return;
            }
        }

        // Check for pending resize request from the UI (UI provides LOGICAL size)
        if let Some((new_logical_width, new_logical_height)) =
            self.dioxus_state.take_pending_resize()
        {
            nih_plug::nih_log!(
                "[RESIZE] Pending resize: {}x{} logical (current physical: {}x{})",
                new_logical_width,
                new_logical_height,
                self.width,
                self.height
            );

            // Sanity check - don't resize to crazy values (in logical pixels)
            if new_logical_width > 4096
                || new_logical_height > 4096
                || new_logical_width < 100
                || new_logical_height < 100
            {
                nih_plug::nih_warn!(
                    "[RESIZE] Ignoring invalid size: {}x{}",
                    new_logical_width,
                    new_logical_height
                );
            } else {
                // Resize the window (baseview takes logical size)
                window.resize(baseview::Size::new(
                    new_logical_width as f64,
                    new_logical_height as f64,
                ));

                // Calculate physical size
                let new_physical_width = (new_logical_width as f32 * self.scale_factor) as u32;
                let new_physical_height = (new_logical_height as f32 * self.scale_factor) as u32;

                // Update our tracked PHYSICAL size
                self.width = new_physical_width;
                self.height = new_physical_height;

                // Update the stored size in DioxusState (logical for persistence)
                self.dioxus_state
                    .set_size(new_logical_width, new_logical_height);

                // Notify the host that the window size changed
                self.gui_context.request_resize();

                // Update document viewport with PHYSICAL size
                if let Some(doc) = &mut self.dioxus_doc {
                    doc.inner_mut().set_viewport(Viewport::new(
                        new_physical_width,
                        new_physical_height,
                        self.scale_factor,
                        ColorScheme::Light,
                    ));
                }

                // Resize wgpu surface with physical size
                if let Some(wgpu_state) = &mut self.wgpu_state {
                    wgpu_state.resize(new_physical_width, new_physical_height);
                }

                self.needs_redraw.store(true, Ordering::Relaxed);
            }
        }

        // Check for host-driven resize (set_size from host — no callback to host)
        if let Some((new_logical_width, new_logical_height)) =
            self.dioxus_state.take_pending_host_resize()
        {
            let new_physical_width = (new_logical_width as f32 * self.scale_factor) as u32;
            let new_physical_height = (new_logical_height as f32 * self.scale_factor) as u32;

            // Resize the baseview window (needed for the child NSView to match)
            window.resize(baseview::Size::new(
                new_logical_width as f64,
                new_logical_height as f64,
            ));

            self.width = new_physical_width;
            self.height = new_physical_height;

            self.dioxus_state
                .set_size(new_logical_width, new_logical_height);

            // NOTE: Do NOT call gui_context.request_resize() here — the host
            // is already driving this resize, calling back would create a loop.

            if let Some(doc) = &mut self.dioxus_doc {
                doc.inner_mut().set_viewport(Viewport::new(
                    new_physical_width,
                    new_physical_height,
                    self.scale_factor,
                    ColorScheme::Light,
                ));
            }

            if let Some(wgpu_state) = &mut self.wgpu_state {
                wgpu_state.resize(new_physical_width, new_physical_height);
            }

            self.needs_redraw.store(true, Ordering::Relaxed);
        }

        // Get animation time upfront before any mutable borrows
        let animation_time = self.animation_start.elapsed().as_secs_f64();
        let needs_redraw = self.needs_redraw.clone();
        let scale_factor = self.scale_factor;

        // self.width and self.height are already in physical pixels
        // Cap at 4096 to avoid Vello's texture size limits
        // See: https://github.com/linebender/vello/issues/680
        const MAX_RENDER_SIZE: u32 = 4096;
        let physical_width = self.width.min(MAX_RENDER_SIZE);
        let physical_height = self.height.min(MAX_RENDER_SIZE);

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

        // Process any pending document messages (e.g., dynamically added styles)
        if let Some(receiver) = &self.doc_message_receiver {
            while let Ok(msg) = receiver.try_recv() {
                match msg {
                    DocumentMessage::CreateHeadElement {
                        name,
                        attributes,
                        contents,
                    } => {
                        let attrs: Vec<(String, String)> = attributes;
                        doc.create_head_element(&name, &attrs, &contents);
                    }
                }
            }
        }

        // Create a waker that triggers redraw
        let waker = futures_util::task::waker(Arc::new(RedrawWaker(needs_redraw.clone())));

        // Poll the virtual DOM
        let cx = std::task::Context::from_waker(&waker);
        doc.poll(Some(cx));

        // Resolve layout with animation time
        doc.inner_mut().resolve(animation_time);

        // Log viewport info periodically
        static FRAME_COUNT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let frame = FRAME_COUNT.fetch_add(1, Ordering::Relaxed);
        if frame % 300 == 0 {
            let inner = doc.inner();
            let vp = inner.viewport();
            nih_plug::nih_log!(
                "[FRAME {}] viewport: {}x{} hidpi={} zoom={}, render: {}x{}",
                frame, vp.window_size.0, vp.window_size.1,
                vp.hidpi_scale, vp.zoom, physical_width, physical_height
            );
        }

        // Render at physical size
        renderer.render(
            wgpu_state,
            doc,
            scale_factor,
            physical_width,
            physical_height,
        );

        // Reset redraw flag
        self.needs_redraw.store(false, Ordering::Relaxed);
    }

    fn on_event(&mut self, _window: &mut Window, event: Event) -> EventStatus {
        match &event {
            Event::Window(baseview::WindowEvent::Resized(info)) => {
                // Use PHYSICAL size for wgpu and Blitz viewport
                let physical_size = info.physical_size();
                self.width = physical_size.width as u32;
                self.height = physical_size.height as u32;
                self.scale_factor = info.scale() as f32;
                self.received_resize = true;

                nih_plug::nih_log!(
                    "[RESIZE EVENT] physical: {}x{}, logical: {}x{}, scale: {}",
                    self.width,
                    self.height,
                    info.logical_size().width,
                    info.logical_size().height,
                    self.scale_factor
                );

                // Update the stored size in DioxusState (for persistence) using logical size
                let logical_size = info.logical_size();
                self.dioxus_state
                    .set_size(logical_size.width as u32, logical_size.height as u32);

                // Update viewport with PHYSICAL size (this is how Blitz expects it)
                if let Some(doc) = &mut self.dioxus_doc {
                    doc.inner_mut().set_viewport(Viewport::new(
                        self.width,
                        self.height,
                        self.scale_factor,
                        ColorScheme::Light,
                    ));
                }

                // Resize wgpu surface with physical size
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
                // Debug log for mouse events with hit testing info
                match &ui_event {
                    blitz_traits::events::UiEvent::MouseDown(e) => {
                        nih_plug::nih_log!("[CLICK] MouseDown at ({}, {})", e.x, e.y);
                        // Try to get hit test info
                        let inner = doc.inner();
                        if let Some(hit) = inner.hit(e.x, e.y) {
                            if let Some(node) = inner.get_node(hit.node_id) {
                                let tag = node
                                    .element_data()
                                    .map(|ed| ed.name.local.as_ref())
                                    .unwrap_or("?");
                                // Log all attributes to debug
                                let attrs: Vec<String> = node
                                    .element_data()
                                    .map(|ed| {
                                        ed.attrs()
                                            .iter()
                                            .map(|a| {
                                                format!(
                                                    "{}={}",
                                                    a.name.local,
                                                    a.value.chars().take(20).collect::<String>()
                                                )
                                            })
                                            .collect()
                                    })
                                    .unwrap_or_default();
                                nih_plug::nih_log!(
                                    "[HIT] Node {} tag={} attrs=[{}]",
                                    hit.node_id,
                                    tag,
                                    attrs.join(", ")
                                );
                            }
                        }
                    }
                    blitz_traits::events::UiEvent::MouseUp(e) => {
                        nih_plug::nih_log!("[CLICK] MouseUp at ({}, {})", e.x, e.y);
                    }
                    blitz_traits::events::UiEvent::MouseMove(e) => {
                        // Log hover only occasionally to avoid spam (every ~50 pixels of movement)
                        static LAST_LOG: std::sync::atomic::AtomicU32 =
                            std::sync::atomic::AtomicU32::new(0);
                        let pos_hash = ((e.x as u32) / 50) * 1000 + ((e.y as u32) / 50);
                        let last = LAST_LOG.load(std::sync::atomic::Ordering::Relaxed);
                        if pos_hash != last {
                            LAST_LOG.store(pos_hash, std::sync::atomic::Ordering::Relaxed);
                            let inner = doc.inner();
                            if let Some(hit) = inner.hit(e.x, e.y) {
                                if let Some(node) = inner.get_node(hit.node_id) {
                                    let tag = node
                                        .element_data()
                                        .map(|ed| ed.name.local.as_ref())
                                        .unwrap_or("?");
                                    let class = node
                                        .element_data()
                                        .and_then(|ed| {
                                            ed.attrs()
                                                .iter()
                                                .find(|a| a.name.local.as_ref() == "class")
                                        })
                                        .map(|a| a.value.chars().take(30).collect::<String>())
                                        .unwrap_or_default();
                                    nih_plug::nih_log!(
                                        "[HOVER] ({:.0}, {:.0}) -> Node {} tag={} class={}",
                                        e.x,
                                        e.y,
                                        hit.node_id,
                                        tag,
                                        class
                                    );
                                }
                            }
                        }
                    }
                    _ => {}
                }
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

/// Get raw window handles from baseview Window using raw-window-handle 0.6 API.
///
/// Our forked baseview uses raw-window-handle 0.6 directly, so we can just use
/// the HasWindowHandle and HasDisplayHandle traits.
fn get_raw_handles_from_baseview(
    window: &Window,
) -> (Option<RawWindowHandle>, Option<RawDisplayHandle>) {
    use raw_window_handle::{HasDisplayHandle, HasWindowHandle};

    // Get the 0.6 handles directly from baseview
    let window_handle = window.window_handle().ok().map(|h| h.as_raw());
    let display_handle = window.display_handle().ok().map(|h| h.as_raw());

    // Debug: log the raw handles
    nih_plug::nih_log!("[RAW HANDLES] window: {:?}, display: {:?}",
        window_handle, display_handle);

    (window_handle, display_handle)
}
