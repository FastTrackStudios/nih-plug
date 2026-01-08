//! WGPU REAPER Demo Plugin
//!
//! A demonstration plugin showcasing Dioxus rendering with:
//! - Animated windowed GUI using Vello/WGPU
//! - REAPER embedded UI support (TCP/MCP)
//! - Custom WGPU canvas elements (future)
//!
//! This plugin is a simple gain effect with an animated UI to demonstrate
//! the rendering capabilities of nih_plug_dioxus.

use atomic_float::AtomicF32;
use nih_plug::prelude::*;
use nih_plug_dioxus::prelude::*;
use std::sync::atomic::Ordering;
use std::sync::Arc;

mod editor;

/// Peak meter decay time in milliseconds
const PEAK_METER_DECAY_MS: f64 = 150.0;

/// The main plugin struct
pub struct WgpuReaperDemo {
    params: Arc<DemoParams>,
    /// Peak meter decay weight (calculated from sample rate)
    peak_meter_decay_weight: f32,
    /// Current peak level (shared with UI)
    peak_meter: Arc<AtomicF32>,
}

/// Plugin parameters
#[derive(Params)]
pub struct DemoParams {
    /// Editor state (window size, etc.)
    #[persist = "editor-state"]
    pub editor_state: Arc<DioxusState>,

    /// Main gain parameter
    #[id = "gain"]
    pub gain: FloatParam,

    /// Animation speed (for demo purposes)
    #[id = "anim_speed"]
    pub anim_speed: FloatParam,
}

impl Default for WgpuReaperDemo {
    fn default() -> Self {
        Self {
            params: Arc::new(DemoParams::default()),
            peak_meter_decay_weight: 1.0,
            peak_meter: Arc::new(AtomicF32::new(util::MINUS_INFINITY_DB)),
        }
    }
}

impl Default for DemoParams {
    fn default() -> Self {
        Self {
            editor_state: editor::default_state(),

            gain: FloatParam::new(
                "Gain",
                util::db_to_gain(0.0),
                FloatRange::Skewed {
                    min: util::db_to_gain(-30.0),
                    max: util::db_to_gain(30.0),
                    factor: FloatRange::gain_skew_factor(-30.0, 30.0),
                },
            )
            .with_smoother(SmoothingStyle::Logarithmic(50.0))
            .with_unit(" dB")
            .with_value_to_string(formatters::v2s_f32_gain_to_db(2))
            .with_string_to_value(formatters::s2v_f32_gain_to_db()),

            anim_speed: FloatParam::new(
                "Animation Speed",
                1.0,
                FloatRange::Linear { min: 0.1, max: 3.0 },
            )
            .with_unit("x")
            .with_value_to_string(formatters::v2s_f32_rounded(2)),
        }
    }
}

impl Plugin for WgpuReaperDemo {
    const NAME: &'static str = "WGPU REAPER Demo";
    const VENDOR: &'static str = "FastTrackStudio";
    const URL: &'static str = "https://github.com/FastTrackStudios/FastTrackStudio";
    const EMAIL: &'static str = "info@example.com";
    const VERSION: &'static str = env!("CARGO_PKG_VERSION");

    const AUDIO_IO_LAYOUTS: &'static [AudioIOLayout] = &[
        AudioIOLayout {
            main_input_channels: NonZeroU32::new(2),
            main_output_channels: NonZeroU32::new(2),
            ..AudioIOLayout::const_default()
        },
        AudioIOLayout {
            main_input_channels: NonZeroU32::new(1),
            main_output_channels: NonZeroU32::new(1),
            ..AudioIOLayout::const_default()
        },
    ];

    const SAMPLE_ACCURATE_AUTOMATION: bool = true;

    type SysExMessage = ();
    type BackgroundTask = ();

    fn params(&self) -> Arc<dyn Params> {
        self.params.clone()
    }

    fn editor(&mut self, _async_executor: AsyncExecutor<Self>) -> Option<Box<dyn Editor>> {
        editor::create(
            self.params.clone(),
            self.peak_meter.clone(),
            self.params.editor_state.clone(),
        )
    }

    // TODO: Enable embedded editor once DioxusDocument threading issues are resolved
    // The DioxusDocument type is not Send+Sync, which is required by EmbeddedEditor.
    // We'll need a different approach - perhaps rendering on the audio thread or
    // using a message-passing architecture.
    //
    // fn embedded_editor(&mut self) -> Option<Arc<dyn EmbeddedEditor>> {
    //     Some(Arc::new(DioxusEmbeddedEditor::new(
    //         self.params.editor_state.clone(),
    //         editor::App,
    //     )))
    // }

    fn initialize(
        &mut self,
        _audio_io_layout: &AudioIOLayout,
        buffer_config: &BufferConfig,
        _context: &mut impl InitContext<Self>,
    ) -> bool {
        // Calculate peak meter decay weight based on sample rate
        self.peak_meter_decay_weight = 0.25f64
            .powf((buffer_config.sample_rate as f64 * PEAK_METER_DECAY_MS / 1000.0).recip())
            as f32;

        true
    }

    fn process(
        &mut self,
        buffer: &mut Buffer,
        _aux: &mut AuxiliaryBuffers,
        _context: &mut impl ProcessContext<Self>,
    ) -> ProcessStatus {
        for channel_samples in buffer.iter_samples() {
            let mut amplitude = 0.0;
            let num_samples = channel_samples.len();

            let gain = self.params.gain.smoothed.next();
            for sample in channel_samples {
                *sample *= gain;
                amplitude += *sample;
            }

            // Only compute peak meter when UI is open
            if self.params.editor_state.is_open() {
                amplitude = (amplitude / num_samples as f32).abs();
                let current_peak_meter = self.peak_meter.load(Ordering::Relaxed);
                let new_peak_meter = if amplitude > current_peak_meter {
                    amplitude
                } else {
                    current_peak_meter * self.peak_meter_decay_weight
                        + amplitude * (1.0 - self.peak_meter_decay_weight)
                };

                self.peak_meter.store(new_peak_meter, Ordering::Relaxed);
            }
        }

        ProcessStatus::Normal
    }
}

impl ClapPlugin for WgpuReaperDemo {
    const CLAP_ID: &'static str = "com.fasttrackstudio.wgpu-reaper-demo";
    const CLAP_DESCRIPTION: Option<&'static str> =
        Some("Demo plugin showcasing Dioxus rendering with WGPU and REAPER embedded UI");
    const CLAP_MANUAL_URL: Option<&'static str> = Some(Self::URL);
    const CLAP_SUPPORT_URL: Option<&'static str> = None;
    const CLAP_FEATURES: &'static [ClapFeature] = &[
        ClapFeature::AudioEffect,
        ClapFeature::Stereo,
        ClapFeature::Mono,
        ClapFeature::Utility,
    ];
}

impl Vst3Plugin for WgpuReaperDemo {
    const VST3_CLASS_ID: [u8; 16] = *b"WgpuReaperDemoAA";
    const VST3_SUBCATEGORIES: &'static [Vst3SubCategory] =
        &[Vst3SubCategory::Fx, Vst3SubCategory::Tools];
}

nih_export_clap!(WgpuReaperDemo);
nih_export_vst3!(WgpuReaperDemo);
