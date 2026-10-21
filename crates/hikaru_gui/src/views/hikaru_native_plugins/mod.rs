// crates/hikaru_gui/src/views/hikaru_native_plugins/mod.rs

//! Vistas de los **plugins nativos** de Hikaru OpenLive.
//!
//! # Qué es un plugin nativo acá
//!
//! Un plugin nativo es un instrumento o un efecto que **vive dentro de la app**:
//! su DSP está en `hikaru_dsp` y su panel visual está en este árbol. No hay
//! binario que escanear, ni carpeta de presets que leer, ni proceso externo que
//! mantener vivo: el rack lo instancia y lo dibuja.
//!
//! La diferencia con un plugin externo (VST3/CLAP) no es el hecho de que uno
//! suene y el otro no, sino de **quién manda**: el externo trae su propio
//! editor y sus propios parámetros, y la app los hospeda; el nativo se dibuja
//! con los widgets de `gpui_kit` que ya usa el resto del DAW, así que se ve
//! igual que el resto de la interfaz.
//!
//! # Por qué un subdirectorio por plugin
//!
//! Antes de este refactor los archivos de los plugins vivían sueltos en
//! `views/`, mezclados con las vistas de la aplicación (`playlist.rs`,
//! `mixer.rs`, `piano_roll.rs`, ...). Eso obligaba a que cualquiera que tocara
//! un plugin tuviera que parsear el `mod.rs` de `views/` para saber si
//! `open_dms.rs` era una vista de la app o el panel de un instrumento.
//!
//! Ahora cada plugin nativo es **una carpeta con su propio `mod.rs`**, y su
//! superficie pública se reexporta acá:
//!
//! ```text
//! views/hikaru_native_plugins/
//! ├── mod.rs                  <- este archivo: el índice del subsistema
//! ├── hikaru_openwavetable/   <- instrumento: Hikaru OpenWavetable
//! │   ├── mod.rs
//! │   ├── open_wavetable.rs      (el editor de la wavetable)
//! │   ├── open_wavetable_tests.rs (tests del knob `WT POS`)
//! │   └── wavetable_io.rs        (lectura de `.wav` de disco)
//! ├── hikaru_opendms/         <- instrumento: Hikaru OpenDMS
//! │   ├── mod.rs
//! │   ├── open_dms.rs            (la grilla de pads)
//! │   ├── open_dms_sampler.rs    (el sampler y su envolvente ADSR)
//! │   └── rack_card.rs           (la tarjeta compacta del rack)
//! └── hikaru_openeq3/         <- efecto: OpenEQ3
//!     └── mod.rs
//! ```
//!
//! La regla es que **un plugin nativo no puede depender de otro** salvo por
//! Items que de verdad le pertenecen: `hikaru_openwavetable` y `hikaru_opendms`
//! son islands, y `hikaru_openeq3` no depende de ninguno. Lo que sí puede (y
//! debe) usar es la infraestructura común de la app: `crate::app`,
//! `crate::audio_proxy`, `crate::render` y las piezas de UI compartidas de
//! `views::controls` y `views::util`.
//!
//! # El catálogo y el rack
//!
//! El catálogo de plugins que el rack ofrece al usuario vive en
//! `views::dsp_rack::PLUGIN_CATALOG`, y `views::dsp_rack` es quien decide qué
//! panel se dibuja en cada tarjeta según el nombre del slot. Ese `match` es el
//! **único** punto donde el rack conoce a los plugins nativos: para el resto de
//! la app, un nativo es sólo un `DspSlot` con un nombre.

pub mod hikaru_opendms;
pub mod hikaru_openeq3;
pub mod hikaru_openwavetable;

// Reexports de superficie. Los que el resto de la app usa por nombre;
// dejarlos acá evita repetir la cadena completa
// `hikaru_native_plugins::hikaru_openwavetable::open_wavetable::...` en cada
// call site, que es largo y propaga el nombre de la carpeta a todo el crate.
pub use hikaru_opendms::open_dms::{midi_note_name, DmsPad, OpenDms};
pub use hikaru_opendms::open_dms_sampler::{AdsrEnvelope, DmsSampler};
pub use hikaru_openwavetable::open_wavetable::WavetableEditor;
pub use hikaru_openwavetable::wavetable_io::Wavetable;
