//! The Dioxus editor for the WGPU REAPER Demo plugin.
//!
//! This demonstrates a Dioxus UI with Tailwind CSS styling.

use atomic_float::AtomicF32;
use nih_plug::prelude::Editor;
use nih_plug_dioxus::prelude::*;
use std::sync::Arc;

use crate::DemoParams;

/// Creates the default editor state with a fixed size.
pub fn default_state() -> Arc<DioxusState> {
    DioxusState::new(|| (400, 300))
}

/// Creates the Dioxus editor for the plugin.
pub fn create(
    _params: Arc<DemoParams>,
    _peak_meter: Arc<AtomicF32>,
    editor_state: Arc<DioxusState>,
) -> Option<Box<dyn Editor>> {
    create_dioxus_editor(editor_state, App)
}

/// The main app component.
#[component]
pub fn App() -> Element {
    // For now, use static values - animation will be added later
    let pulse = 0.7_f32;
    let rotate = 45.0_f64;
    let bounce = 10_i32;
    let hue = 220_i32;

    rsx! {
        style { {TAILWIND_CSS} }
        
        div {
            class: "dark w-full h-full bg-background text-foreground flex flex-col",
            
            // Header with animated gradient
            header {
                class: "p-4 text-center border-b border-border",
                style: "background: linear-gradient({hue}deg, hsl({hue}, 70%, 20%), hsl({((hue + 60) % 360)}deg, 70%, 15%))",
                
                h1 {
                    class: "text-xl font-bold text-white",
                    "WGPU REAPER Demo"
                }
                p {
                    class: "text-sm text-white/70 mt-1",
                    "Dioxus + Vello + WGPU"
                }
            }
            
            // Main content
            div {
                class: "flex-1 p-4 flex flex-col gap-4",
                
                // Animated visualization area
                div {
                    class: "flex-1 rounded-lg border border-border bg-card overflow-hidden relative",
                    
                    // Animated circles
                    div {
                        class: "absolute inset-0 flex items-center justify-center",
                        
                        // Outer rotating ring
                        div {
                            class: "w-32 h-32 rounded-full border-4 border-primary/30",
                            style: "transform: rotate({rotate}deg)",
                        }
                        
                        // Inner pulsing circle
                        div {
                            class: "absolute w-20 h-20 rounded-full bg-primary",
                            style: "opacity: {pulse}; transform: scale({0.8 + pulse * 0.4})",
                        }
                        
                        // Center dot
                        div {
                            class: "absolute w-4 h-4 rounded-full bg-white",
                        }
                    }
                    
                    // Bouncing indicator
                    div {
                        class: "absolute left-4 bottom-4 w-3 h-3 rounded-full bg-green-500",
                        style: "transform: translateY(-{bounce}px)",
                    }
                    
                    // Status text
                    div {
                        class: "absolute top-2 right-2 text-xs text-muted-foreground font-mono",
                        "WGPU Demo"
                    }
                }
                
                // Controls section
                div {
                    class: "space-y-3",
                    
                    // Gain control placeholder
                    div {
                        class: "flex items-center gap-3",
                        label {
                            class: "text-sm font-medium w-24",
                            "Gain"
                        }
                        div {
                            class: "flex-1 h-2 bg-muted rounded-full overflow-hidden",
                            div {
                                class: "h-full bg-primary transition-all",
                                style: "width: 50%",
                            }
                        }
                        span {
                            class: "text-sm text-muted-foreground w-16 text-right font-mono",
                            "0.0 dB"
                        }
                    }
                    
                    // Animation speed placeholder
                    div {
                        class: "flex items-center gap-3",
                        label {
                            class: "text-sm font-medium w-24",
                            "Anim Speed"
                        }
                        div {
                            class: "flex-1 h-2 bg-muted rounded-full overflow-hidden",
                            div {
                                class: "h-full bg-primary transition-all",
                                style: "width: 33%",
                            }
                        }
                        span {
                            class: "text-sm text-muted-foreground w-16 text-right font-mono",
                            "1.0x"
                        }
                    }
                }
            }
            
            // Footer
            footer {
                class: "p-2 text-center text-xs text-muted-foreground border-t border-border",
                "FastTrackStudio - nih_plug_dioxus demo"
            }
        }
    }
}
