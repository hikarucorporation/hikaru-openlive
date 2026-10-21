// crates/hikaru_gui/src/views/hikaru_native_plugins/hikaru_opendms/mod.rs

//! **Hikaru OpenDMS**: el sampler de percusión por pads.
//!
//! # Qué hay adentro
//!
//! - [`open_dms`]: la grilla de pads y el editor grande del sampler. Es el
//!   panel que se abre desde la cadena de módulos y el que se dibuja cuando el
//!   slot del rack tiene un sample cargado.
//! - [`open_dms_sampler`]: el sampler propiamente dicho: envolvente ADSR,
//!   sincronía de tempo, pitch en cents, posición de arranque y corte, y las
//!   slices. También carga los picos de un `.wav` para dibujar la forma de
//!   onda del pad.
//! - [`rack_card`]: la tarjeta compacta que el rack muestra en la tira de
//!   módulos. Antes vivía dentro de `views/dsp_rack.rs` y era el único pedazo
//!   de un plugin nativo incrustado en el rack.
//!
//! # Por qué el sampler está aparte de la grilla
//!
//! La grilla de pads es una **vista**: dibuja, hit-testea y manda comandos al
//! motor. El sampler es el **modelo** de los controles de reproducción. La
//! separación no es cosmética: `open_dms_sampler` es lo que usan el explorador,
//! el editor de clips y la carga de proyectos para leer los picos de un `.wav`,
//! y todo eso es lógica que no necesita GPU ni contexto de render.
//!
//! # La carga de picos
//!
//! [`open_dms_sampler::load_peaks_from_wav`] es la función que traduce un
//! `.wav` de disco en los picos pre-calculados que la grilla dibuja debajo de
//! cada pad. La usan tres lugares distintos del crate (el explorador, el editor
//! de clips y la apertura de proyecto), y por eso vive en el plugin y no en
//! una vista: es **servicio**, no pantalla.

pub mod open_dms;
pub mod open_dms_sampler;
pub mod rack_card;

// Reexports de superficie: el resto de la app usa estos nombres, no la cadena
// de módulos completa.
pub use open_dms::{midi_note_name, DmsPad, OpenDms};
pub use open_dms_sampler::{AdsrEnvelope, DmsSampler};
