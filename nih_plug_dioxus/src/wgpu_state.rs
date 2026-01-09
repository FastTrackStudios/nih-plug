//! WGPU device and surface management.

use pollster::FutureExt;
use raw_window_handle::{
    DisplayHandle, HandleError, HasDisplayHandle, HasWindowHandle, RawDisplayHandle,
    RawWindowHandle, WindowHandle,
};
use std::sync::Arc;

/// Holds the WGPU instance, device, queue, and surface configuration.
pub struct WgpuState {
    pub instance: wgpu::Instance,
    pub surface: wgpu::Surface<'static>,
    pub device: Arc<wgpu::Device>,
    pub queue: Arc<wgpu::Queue>,
    pub config: wgpu::SurfaceConfiguration,
}

impl WgpuState {
    /// Create a new WGPU state from raw window handles.
    ///
    /// This takes raw handles instead of HasWindowHandle/HasDisplayHandle traits
    /// to work around the raw-window-handle version mismatch between baseview (0.5)
    /// and wgpu (0.6).
    pub fn new_from_raw(
        window_handle: RawWindowHandle,
        display_handle: RawDisplayHandle,
        width: u32,
        height: u32,
    ) -> Self {
        // Create the instance
        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
            backends: wgpu::Backends::all(),
            ..Default::default()
        });

        // Create a wrapper that provides the raw-window-handle 0.6 traits
        let wrapper = RawHandleWrapper {
            window: window_handle,
            display: display_handle,
        };

        // Create the surface
        let surface = unsafe {
            instance
                .create_surface_unsafe(
                    wgpu::SurfaceTargetUnsafe::from_window(&wrapper)
                        .expect("Failed to create surface target"),
                )
                .expect("Failed to create surface")
        };

        // Request an adapter
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::LowPower,
                force_fallback_adapter: false,
                compatible_surface: Some(&surface),
            })
            .block_on()
            .expect("Failed to find an appropriate adapter");

        // Create the device and queue
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: Some("nih_plug_dioxus device"),
                required_features: wgpu::Features::empty(),
                required_limits: wgpu::Limits::default(),
                memory_hints: wgpu::MemoryHints::Performance,
                trace: wgpu::Trace::Off,
            })
            .block_on()
            .expect("Failed to create device");

        let device = Arc::new(device);
        let queue = Arc::new(queue);

        // Configure the surface
        // Use NON-sRGB format because Vello outputs linear RGB values, and we copy
        // them directly to the surface. Using a non-sRGB surface means the values
        // are displayed as-is without additional gamma correction.
        //
        // Note: This means CSS colors (which are specified in sRGB) need to be
        // converted to linear RGB by the rendering pipeline (Blitz/Vello).
        let surface_caps = surface.get_capabilities(&adapter);
        let format = surface_caps
            .formats
            .iter()
            .find(|f| !f.is_srgb())
            .copied()
            .unwrap_or(surface_caps.formats[0]);

        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            width: width.max(1),
            height: height.max(1),
            present_mode: wgpu::PresentMode::AutoVsync,
            alpha_mode: surface_caps.alpha_modes[0],
            view_formats: vec![],
            desired_maximum_frame_latency: 2,
        };

        surface.configure(&device, &config);

        Self {
            instance,
            surface,
            device,
            queue,
            config,
        }
    }

    /// Resize the surface.
    pub fn resize(&mut self, width: u32, height: u32) {
        if width > 0 && height > 0 {
            self.config.width = width;
            self.config.height = height;
            self.surface.configure(&self.device, &self.config);
        }
    }

    /// Get the surface format.
    pub fn format(&self) -> wgpu::TextureFormat {
        self.config.format
    }
}

/// Wrapper to provide raw-window-handle 0.6 traits from raw handles.
struct RawHandleWrapper {
    window: RawWindowHandle,
    display: RawDisplayHandle,
}

impl HasWindowHandle for RawHandleWrapper {
    fn window_handle(&self) -> Result<WindowHandle<'_>, HandleError> {
        // SAFETY: The handles are valid for the lifetime of the window
        Ok(unsafe { WindowHandle::borrow_raw(self.window) })
    }
}

impl HasDisplayHandle for RawHandleWrapper {
    fn display_handle(&self) -> Result<DisplayHandle<'_>, HandleError> {
        // SAFETY: The handles are valid for the lifetime of the window
        Ok(unsafe { DisplayHandle::borrow_raw(self.display) })
    }
}
