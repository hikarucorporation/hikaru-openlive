// crates/hikaru_gui/src/views/mod.rs

//! Las **vistas** de Hikaru OpenLive: todo lo que se dibuja en la ventana.
//!
//! # Cómo está organizado
//!
//! Acá viven las pantallas de la aplicación (la playlist, el mezclador, el
//! piano roll, el explorador, el rack de DSP, ...) y, en su propia carpeta, los
//! paneles de los **plugins nativos**:
//!
//! - [`hikaru_native_plugins`]: los instrumentos y efectos que son parte de la
//!   app (Hikaru OpenWavetable, Hikaru OpenDMS, OpenEQ3). Cada uno tiene su
//!   carpeta con su propio `mod.rs`.
//!
//! Todo lo que vive acá es **vista**: dibuja, hit-testea y manda comandos al
//! motor de audio a través de `crate::audio_proxy`. El estado de la sesión no
//! vive en las vistas sino en `crate::app`.

pub mod header;
pub mod footer;
pub mod matrix;
pub mod mixer;
pub mod controls;
pub mod util;
pub mod dsp_rack;
pub mod playlist;
pub mod menu_bar;
pub mod about;
pub mod branding;
pub mod arranger_view;
pub mod explorer;
pub mod audio_settings;
pub mod clipboard;
pub mod waveform;
pub mod clip_editor;
pub mod piano_roll;
pub mod external_plugins_settings;
pub mod mcp_port_settings;
pub mod control_panel;
pub mod modal;
pub mod hikaru_native_plugins;
mod live_canvas;

pub use live_canvas::LiveCanvas;
