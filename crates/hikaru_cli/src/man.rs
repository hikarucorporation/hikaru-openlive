// La página de manual del programa, embebida en el binario.
// SPDX-License-Identifier: AGPL-3.0-or-later
// crates/hikaru_cli/src/man.rs
//
// =========================================================================
// POR QUÉ LA PÁGINA ESTÁ EMBEBIDA
// =========================================================================
//
// La página de manual (`man hikaru_openlive`) es un archivo de texto que
// vive FUERA del binario: es lo que `/usr/share/man/man1` guarda. Pero el
// binario tiene que saber qué está documentando, por dos razones:
//
//   1. El `build.rs` copia la página a `$OUT_DIR/man/` para el empaquetado
//      (ver `crates/hikaru_cli/build.rs`), y deja la ruta en la variable de
//      entorno `HIKARU_MAN_PAGE_PATH`. Acá se lee con `include_str!`.
//
//   2. Los tests pueden compararla con la gramática REAL del programa: si
//      `clap` agrega un subcomando y nadie actualiza la página, el test
//      falla (ver `crates/hikaru_cli/tests/man_page.rs`).
//
// El `include_str!` no puede fallar: el `build.rs` SIEMPRE escribe un
// archivo en `$OUT_DIR/man/` (o la página real, o un marcador de posición
// vacío). Un build sin la página no rompe la compilación: la página es
// documentación, no código.

/// La página de manual del binario, embebida en el binario.
///
/// Es el fuente en groff/troff (el `.1`), no el texto ya renderizado: el
/// renderizado lo hace `man(1)`.
pub const MAN_PAGE: &str = include_str!(env!("HIKARU_MAN_PAGE_PATH"));

/// El nombre del binario, y el de la página de manual.
pub const MAN_NAME: &str = "hikaru_openlive";

/// ¿Hay una página de manual de verdad en esta compilación?
///
/// Da `false` cuando el `build.rs` no encontró el archivo y tuvo que
/// escribir el marcador de posición.
pub fn is_available() -> bool {
    !MAN_PAGE.trim().is_empty()
}

/// La línea `.TH` de la página: la que dice el nombre, la sección, la fecha
/// y la versión.
///
/// Es la primera línea que no es un comentario, y es la que `man` usa para
/// el pie de página.
pub fn header() -> &'static str {
    MAN_PAGE
        .lines()
        .find(|line| line.starts_with(".TH "))
        .unwrap_or_default()
}

/// La versión del producto que documenta la página.
///
/// Sale de la línea `.TH`: si el `build.rs` avisó de que la página no
/// menciona la versión del crate, esta función devuelve la que dice la
/// página (que no es la del binario) y el consumidor puede detectarlo.
pub fn documented_version() -> &'static str {
    // La línea `.TH` tiene la versión en el quinto campo, entre comillas:
    //   .TH HIKARU_OPENLIVE 1 "2026-10-21" "Hikaru OpenLive 2.22.2" "User Commands"
    //
    // El campo va entre comillas Y tiene espacios adentro ("Hikaru OpenLive
    // 2.22.2"), así que no se puede partir con `split_whitespace`: hay que
    // partir respetando las comillas.
    let campos = campos_con_comillas(header());

    campos
        .get(4)
        .map(|campo| campo.trim_matches('"'))
        .unwrap_or_default()
}

/// Parte una línea en campos, respetando las comillas dobles.
///
/// Un campo entre comillas puede tener espacios: `"Hikaru OpenLive 2.22.2"`
/// es UN campo, no tres.
fn campos_con_comillas(linea: &str) -> Vec<&str> {
    let mut campos = Vec::new();
    let bytes = linea.as_bytes();
    let mut inicio = 0;
    let mut i = 0;
    let mut en_comillas = false;

    while i < bytes.len() {
        match bytes[i] {
            b'"' => en_comillas = !en_comillas,
            b' ' if !en_comillas => {
                if i > inicio {
                    campos.push(&linea[inicio..i]);
                }
                inicio = i + 1;
            }
            _ => {}
        }
        i += 1;
    }

    if inicio < linea.len() {
        campos.push(&linea[inicio..]);
    }

    campos
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn la_pagina_embebida_no_esta_vacia() {
        assert!(!MAN_PAGE.trim().is_empty());
    }

    #[test]
    fn la_pagina_tiene_su_cabecera() {
        assert!(header().starts_with(".TH HIKARU_OPENLIVE 1"));
    }

    #[test]
    fn la_version_documentada_es_la_del_crate() {
        assert_eq!(
            documented_version(),
            format!("Hikaru OpenLive {}", env!("CARGO_PKG_VERSION"))
        );
    }
}
