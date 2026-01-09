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
        // Create the instance - use Vulkan on Linux
        #[cfg(target_os = "linux")]
        let backends = wgpu::Backends::VULKAN;
        #[cfg(not(target_os = "linux"))]
        let backends = wgpu::Backends::all();

        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
            backends,
            flags: wgpu::InstanceFlags::from_build_config(),
            ..Default::default()
        });

        // Create the surface using RawHandle directly (not from_window)
        // This gives us more control over exactly what handles are passed
        let surface = unsafe {
            instance
                .create_surface_unsafe(wgpu::SurfaceTargetUnsafe::RawHandle {
                    raw_display_handle: display_handle,
                    raw_window_handle: window_handle,
                })
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

        // Debug: log adapter and surface capabilities
        nih_plug::nih_log!("[WGPU] Adapter: {:?}", adapter.get_info());
        nih_plug::nih_log!("[WGPU] Adapter supports surface: {}", adapter.is_surface_supported(&surface));
        nih_plug::nih_log!("[WGPU] Surface formats: {:?}", surface_caps.formats);
        nih_plug::nih_log!("[WGPU] Surface alpha modes: {:?}", surface_caps.alpha_modes);
        nih_plug::nih_log!("[WGPU] Surface present modes: {:?}", surface_caps.present_modes);

        if surface_caps.formats.is_empty() {
            nih_plug::nih_error!("[WGPU] No surface formats available - surface may be invalid");
        }

        let format = surface_caps
            .formats
            .iter()
            .find(|f| !f.is_srgb())
            .copied()
            .unwrap_or_else(|| {
                if surface_caps.formats.is_empty() {
                    nih_plug::nih_error!("[WGPU] Using fallback format Bgra8Unorm");
                    wgpu::TextureFormat::Bgra8Unorm
                } else {
                    surface_caps.formats[0]
                }
            });

        // Prefer Inherit alpha mode for better compatibility, especially on XWayland
        let alpha_mode = if surface_caps.alpha_modes.contains(&wgpu::CompositeAlphaMode::Inherit) {
            wgpu::CompositeAlphaMode::Inherit
        } else if surface_caps.alpha_modes.contains(&wgpu::CompositeAlphaMode::Opaque) {
            wgpu::CompositeAlphaMode::Opaque
        } else {
            surface_caps.alpha_modes[0]
        };

        // Use Fifo (vsync) for reliability
        let present_mode = if surface_caps.present_modes.contains(&wgpu::PresentMode::Fifo) {
            wgpu::PresentMode::Fifo
        } else {
            surface_caps.present_modes[0]
        };

        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            width: width.max(1),
            height: height.max(1),
            present_mode,
            alpha_mode,
            view_formats: vec![],
            desired_maximum_frame_latency: 2,
        };

        nih_plug::nih_log!("[WGPU] Configuring surface: {}x{}, format={:?}, alpha={:?}, present={:?}",
            config.width, config.height, config.format, config.alpha_mode, config.present_mode);

        // On Linux/XWayland, surface configuration can fail due to timing issues
        // or XWayland compatibility problems. We'll try a few times with a small delay.
        #[cfg(target_os = "linux")]
        {
            let mut attempts = 0;
            const MAX_ATTEMPTS: u32 = 3;
            loop {
                // Try to configure - this pushes errors to the device's error scope
                device.push_error_scope(wgpu::ErrorFilter::Validation);
                surface.configure(&device, &config);

                let error = device.pop_error_scope().block_on();
                if error.is_none() {
                    nih_plug::nih_log!("[WGPU] Surface configured successfully on attempt {}", attempts + 1);
                    break;
                }

                attempts += 1;
                if attempts >= MAX_ATTEMPTS {
                    nih_plug::nih_error!("[WGPU] Surface configuration failed after {} attempts: {:?}", attempts, error);
                    // Try one more time without error scope - let it panic if it still fails
                    surface.configure(&device, &config);
                    break;
                }

                nih_plug::nih_warn!("[WGPU] Surface configuration attempt {} failed, retrying...", attempts);
                std::thread::sleep(std::time::Duration::from_millis(100));
            }
        }

        #[cfg(not(target_os = "linux"))]
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
