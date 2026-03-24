//! CLAP gain-adjustment-metering draft extension support.
//!
//! This implements the `clap.gain-adjustment-metering/0` extension, which allows hosts
//! like REAPER to read gain reduction from compressor/limiter plugins.

use std::ffi::CStr;

use clap_sys::plugin::clap_plugin;

/// The extension ID string.
pub const CLAP_EXT_GAIN_ADJUSTMENT_METERING: &CStr =
    unsafe { CStr::from_bytes_with_nul_unchecked(b"clap.gain-adjustment-metering/0\0") };

/// The plugin-side vtable for the gain adjustment metering extension.
#[repr(C)]
pub struct ClapPluginGainAdjustmentMetering {
    /// Returns current gain adjustment in dB.
    /// Negative = gain reduction (compressor/limiter).
    /// Positive = gain expansion (expander).
    /// Zero = no processing.
    /// Called on the audio thread.
    pub get: Option<unsafe extern "C" fn(plugin: *const clap_plugin) -> f64>,
}

// Safety: The vtable contains only a function pointer and is used from the audio thread.
unsafe impl Send for ClapPluginGainAdjustmentMetering {}
unsafe impl Sync for ClapPluginGainAdjustmentMetering {}
