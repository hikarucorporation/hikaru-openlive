// La versión del producto, vista desde la consola.
// SPDX-License-Identifier: AGPL-3.0-or-later
// crates/hikaru_cli/src/version.rs
//
// =========================================================================
// POR QUÉ OTRO `version.rs` Y NO EL DE LA GUI
// =========================================================================
//
// La GUI tiene el suyo en `hikaru_gui/src/version.rs`. Este NO puede ser el
// mismo: `hikaru_gui` linkea GPUI, y la consola tiene que poder correr sin
// ventana (ver el doc comment del `lib.rs`).
//
// Los dos leen el MISMO número: `[workspace.package].version` del `Cargo.toml`
// raíz. No son dos versiones que puedan divergir: son el mismo campo del
// workspace, leído desde dos crates distintos. Por eso el `created_by` de un
// `.oplf` escrito por la consola dice lo mismo que el que escribe la GUI.

/// Versión del producto (`2.22.2`), leída del `Cargo.toml` del workspace.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// La cabecera `created_by` de un proyecto de OpenLive.
pub fn created_by_openlive() -> String {
    format!("Hikaru OpenLive {}", VERSION)
}

/// La cabecera `created_by` de un proyecto de OpenStudio.
pub fn created_by_openstudio() -> String {
    format!("Hikaru OpenStudio {}", VERSION)
}

/// La versión del formato de archivo, tal como la muestra el banner.
///
/// El número del FORMATO (`format_version`, que es 1) y el del PRODUCTO (este)
/// son cosas distintas: el primero es el contrato del archivo y el segundo
/// la versión del programa que lo escribió. El banner muestra los dos.
pub fn format_version_label() -> String {
    format!("{}.0", hikaru_project_file::FORMAT_VERSION)
}
