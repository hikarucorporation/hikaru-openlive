// crates/hikaru_gui/src/views/hikaru_native_plugins/hikaru_openwavetable/mod.rs

//! **Hikaru OpenWavetable**: el sintetizador de wavetable.
//!
//! # Qué hay adentro
//!
//! - [`open_wavetable`]: el editor visual del sintetizador, el que se dibuja en
//!   la tarjeta del rack y en la cadena de módulos. Trae la pila 3D de ciclos,
//!   los knobs de morph y de índice, y la envolvente.
//! - [`wavetable_io`]: la lectura de archivos `.wav` de disco. Vive aparte
//!   porque **no dibuja nada**: es lógica pura, y por eso se puede testear sin
//!   levantar la GPU.
//! - `open_wavetable_tests`: los tests de interacción del knob `WT POS`.
//!
//! # Por qué el editor y la IO están separados
//!
//! No es una cuestión de tamaño de archivo. El editor importa `gpui_kit::*`
//! (un glob) y necesita el contexto de `wgpu` para dibujar la pila 3D; la IO no
//! necesita ninguna de las dos cosas. Mezclarlas en un solo archivo hacía que
//! hasta el test más tonto del módulo tuviera que atravesar el motor gráfico.
//!
//! # El glob y los tests
//!
//! Los tests de este plugin **no** pueden vivir dentro de `open_wavetable.rs`
//! como un `#[cfg(test)] mod tests`. Ese módulo hace `use gpui_kit::*`, y al
//! combinar ese glob con el `use super::*` que el harness de `#[test]` inyecta,
//! el compilador entra en una recursión de expansión que no termina: sube el
//! límite de recursión de 128 a 512, a 1024, y pide 2048, siempre sin
//! converger.
//!
//! Es un problema del compilador, no del código, y subir el `recursion_limit` no
//! lo arregla. Lo que funciona es sacar los tests a un módulo hermano que
//! importa lo que necesita **por nombre**. Por eso `open_wavetable_tests` es un
//! módulo hermano y no un submódulo de [`open_wavetable`]: al ser hermano, no
//! hereda el glob que dispara la recursión, y compila al toque.
//!
//! `wavetable_io.rs` ya tenía tests y nunca falló, y es la prueba de que el
//! diagnóstico es el glob y no el hecho de testear este crate: ese módulo no
//! importa `gpui_kit::*`.

pub mod open_wavetable;
pub mod wavetable_io;

#[cfg(test)]
mod open_wavetable_tests;

// Reexports de superficie: el resto de la app usa estos nombres, no la cadena
// de módulos completa.
pub use open_wavetable::WavetableEditor;
pub use wavetable_io::Wavetable;
