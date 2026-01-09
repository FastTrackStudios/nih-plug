//! Vello renderer integration.

use crate::wgpu_state::WgpuState;
#[cfg(feature = "softbuffer-blit")]
use crate::wgpu_offscreen::WgpuOffscreenState;
use anyrender_vello::VelloScenePainter;
use blitz_dom::Document as _;
use blitz_paint::paint_scene;
use dioxus_native::DioxusDocument;
use vello::{
    RenderParams, Renderer as VelloRenderer, RendererOptions, Scene, peniko::color::AlphaColor,
};
use wgpu::util::TextureBlitter;

/// Manages Vello rendering to a wgpu surface.
pub struct Renderer {
    vello_renderer: VelloRenderer,
    scene: Scene,
    // Intermediate texture for Vello rendering (compute shader output, must be Rgba8Unorm)
    target_texture: Option<wgpu::Texture>,
    /// View for Vello to render into (linear Rgba8Unorm for compute shader)
    target_view: Option<wgpu::TextureView>,
    blitter: Option<TextureBlitter>,
    last_width: u32,
    last_height: u32,
}

impl Renderer {
    /// Create a new renderer.
    pub fn new(device: &wgpu::Device) -> Self {
        let vello_renderer = VelloRenderer::new(
            device,
            RendererOptions {
                use_cpu: false,
                antialiasing_support: vello::AaSupport::all(),
                num_init_threads: None,
                pipeline_cache: None,
            },
        )
        .expect("Failed to create Vello renderer");

        Self {
            vello_renderer,
            scene: Scene::new(),
            target_texture: None,
            target_view: None,
            blitter: None,
            last_width: 0,
            last_height: 0,
        }
    }

    /// Ensure the intermediate texture is the right size.
    fn ensure_target(
        &mut self,
        device: &wgpu::Device,
        surface_format: wgpu::TextureFormat,
        width: u32,
        height: u32,
    ) {
        if self.last_width != width || self.last_height != height || self.target_texture.is_none() {
            // Create intermediate texture for vello (compute shader output)
            // Vello requires Rgba8Unorm with STORAGE_BINDING for its compute shaders.
            // The blitter will handle any necessary format conversion when copying to the surface.
            let target_texture = device.create_texture(&wgpu::TextureDescriptor {
                label: Some("vello target"),
                size: wgpu::Extent3d {
                    width,
                    height,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                usage: wgpu::TextureUsages::STORAGE_BINDING | wgpu::TextureUsages::TEXTURE_BINDING,
                format: wgpu::TextureFormat::Rgba8Unorm,
                view_formats: &[],
            });

            // View for Vello to render into (must be linear Rgba8Unorm for compute shader)
            let target_view = target_texture.create_view(&wgpu::TextureViewDescriptor::default());

            // Create blitter to copy from intermediate to surface
            // Note: TextureBlitter does a simple copy without gamma correction.
            // Since we're using a non-sRGB surface format (selected in wgpu_state.rs),
            // the colors will be interpreted as linear, which matches Vello's output.
            let blitter = TextureBlitter::new(device, surface_format);

            self.target_texture = Some(target_texture);
            self.target_view = Some(target_view);
            self.blitter = Some(blitter);
            self.last_width = width;
            self.last_height = height;
        }
    }

    /// Render the document to the surface.
    pub fn render(
        &mut self,
        wgpu_state: &WgpuState,
        doc: &DioxusDocument,
        scale: f32,
        width: u32,
        height: u32,
    ) {
        // Get the next frame
        let frame = match wgpu_state.surface.get_current_texture() {
            Ok(frame) => frame,
            Err(wgpu::SurfaceError::Outdated) => {
                // Surface is outdated, skip this frame
                return;
            }
            Err(e) => {
                eprintln!("Failed to get next frame: {:?}", e);
                return;
            }
        };

        // Ensure we have the right sized intermediate texture
        self.ensure_target(&wgpu_state.device, wgpu_state.format(), width, height);

        let target_view = self.target_view.as_ref().expect("Target view not created");
        let blitter = self.blitter.as_ref().expect("Blitter not created");

        // Clear and paint the scene
        self.scene.reset();
        paint_scene(
            &mut VelloScenePainter::new(&mut self.scene),
            &*doc.inner(),
            scale as f64,
            width,
            height,
        );

        // Render to the intermediate texture (using linear view for Vello compute shader)
        self.vello_renderer
            .render_to_texture(
                &wgpu_state.device,
                &wgpu_state.queue,
                &self.scene,
                target_view,
                &RenderParams {
                    // Transparent background - let CSS provide the actual background color
                    base_color: AlphaColor::TRANSPARENT,
                    width,
                    height,
                    antialiasing_method: vello::AaConfig::Msaa16,
                },
            )
            .expect("Failed to render");

        // Blit from intermediate texture to surface
        let surface_view = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());

        let mut encoder =
            wgpu_state
                .device
                .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("blit encoder"),
                });

        blitter.copy(&wgpu_state.device, &mut encoder, target_view, &surface_view);

        wgpu_state.queue.submit(std::iter::once(encoder.finish()));
        frame.present();
    }

    /// Render the document to an offscreen texture (for softbuffer blit).
    #[cfg(feature = "softbuffer-blit")]
    pub fn render_offscreen(
        &mut self,
        wgpu_state: &WgpuOffscreenState,
        doc: &DioxusDocument,
        scale: f32,
        width: u32,
        height: u32,
    ) {
        // Ensure we have the right sized intermediate texture
        self.ensure_target(&wgpu_state.device, wgpu_state.format(), width, height);

        let target_view = self.target_view.as_ref().expect("Target view not created");
        let blitter = self.blitter.as_ref().expect("Blitter not created");

        // Clear and paint the scene
        self.scene.reset();
        paint_scene(
            &mut VelloScenePainter::new(&mut self.scene),
            &*doc.inner(),
            scale as f64,
            width,
            height,
        );

        // Render to the intermediate texture (using linear view for Vello compute shader)
        self.vello_renderer
            .render_to_texture(
                &wgpu_state.device,
                &wgpu_state.queue,
                &self.scene,
                target_view,
                &RenderParams {
                    // Transparent background - let CSS provide the actual background color
                    base_color: AlphaColor::TRANSPARENT,
                    width,
                    height,
                    antialiasing_method: vello::AaConfig::Msaa16,
                },
            )
            .expect("Failed to render");

        // Blit from intermediate texture to offscreen render texture
        let mut encoder =
            wgpu_state
                .device
                .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("offscreen blit encoder"),
                });

        blitter.copy(&wgpu_state.device, &mut encoder, target_view, &wgpu_state.render_texture_view);

        wgpu_state.queue.submit(std::iter::once(encoder.finish()));
    }
}
