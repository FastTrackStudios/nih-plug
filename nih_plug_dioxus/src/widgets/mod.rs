//! Pre-built widgets for common plugin UI elements.
//!
//! These widgets integrate with NIH-plug's parameter system through the
//! `ParamContext` provided by `create_dioxus_editor`.

mod param_slider;
mod peak_meter;

pub use param_slider::ParamSlider;
pub use peak_meter::PeakMeter;
