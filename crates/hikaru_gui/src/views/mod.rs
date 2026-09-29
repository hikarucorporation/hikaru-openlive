pub mod header;
pub mod footer;
pub mod matrix;
pub mod mixer;
pub mod controls;
pub mod util;
pub mod wavetable_io;
pub mod dsp_rack;
pub mod open_wavetable;
#[cfg(test)]
mod open_wavetable_tests;
pub mod playlist;
pub mod menu_bar;
pub mod about;
pub mod arranger_view;
pub mod explorer;
pub mod audio_settings;
pub mod clipboard;
pub mod waveform;
pub mod clip_editor;
pub mod piano_roll;
pub mod external_plugins_settings;
pub mod open_dms;
pub mod open_dms_sampler;
mod live_canvas;

pub use live_canvas::LiveCanvas;
