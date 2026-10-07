// Versión del producto: fuente única de verdad.
// GNU AGPLv3
// crates/hikaru_gui/src/version.rs
//
// =========================================================================
// POR QUÉ ESTE MÓDULO
// =========================================================================
//
// La versión aparece en tres lugares que hoy divergían entre sí:
// la cabecera `created_by` de los archivos `.oplf`/`.opsf`, la pantalla About y
// la variable de entorno en runtime. Si se hardcodea en cada uno, el día que se
// sube a `2.23.0` los `.oplf` siguen diciendo `2.22.2` y nadie se entera.
//
// Así que el número vive UNA vez, en `[workspace.package].version` del
// `Cargo.toml` raíz, y desde acá sale por `CARGO_PKG_VERSION`. La regla del
// formato (OPLF-README.md §3.1) lo exige explícitamente: prohibido escribir
// `2.22.2` a mano en el código.

/// Versión del producto (`2.22.2`), leída del `Cargo.toml` del workspace.
///
/// Es la que va en la cabecera `created_by` de los `.oplf`/`.opsf`:
/// `format!("Hikaru OpenLive {}", VERSION)`.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");