// Los tests de la página de manual.
// SPDX-License-Identifier: AGPL-3.0-or-later
// crates/hikaru_cli/tests/man_page.rs
//
// =========================================================================
// POR QUÉ HAY UN TEST DE DOCUMENTACIÓN
// =========================================================================
//
// Una página de manual es código que se ejecuta cuando alguien escribe
// `man hikaru_openlive`: si está desactualizada, muestra una gramática que
// el programa ya no tiene, y el usuario pierde el tiempo con `--mcp list`
// cuando el comando real es `mcp list`.
//
// La forma barata de que eso no pase es COMPARAR la página con la gramática
// real, que es la que arma `clap`. Estos tests hacen exactamente eso:
//
//   1. La página tiene las secciones que tiene que tener.
//   2. La página menciona TODOS los subcomandos, las banderas largas y las
//      cortas que acepta el programa (sacados de `clap`, no escritos a
//      mano: si `clap` agrega algo, el test lo exige en la página).
//   3. La página documenta la versión del crate.
//   4. El espejo de `docs/` está sincronizado con el archivo canónico.
//   5. La página no tiene avisos de `groff` (si el sistema tiene groff).

use std::path::PathBuf;
use std::process::Command;

use clap::CommandFactory;

use hikaru_cli::cli::Cli;
use hikaru_cli::man::{self, MAN_PAGE};

/// La raíz del repositorio (el crate está dos niveles más abajo).
fn raiz() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
}

/// El archivo canónico de la página de manual.
fn canonico() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("man")
        .join(format!("{}.1", man::MAN_NAME))
}

/// El espejo de la página en la carpeta de documentación.
fn espejo() -> PathBuf {
    raiz()
        .join("docs")
        .join("man")
        .join(format!("{}.1", man::MAN_NAME))
}

/// El texto de la página sin las escapes de roff.
///
/// En el fuente, un guion se escribe `\-` y una negrita `\fB`. Para buscar
/// "--json" o "inspect" no nos interesa el formato: se quita el `\` y se
/// busca en el texto plano.
fn plano() -> String {
    MAN_PAGE.chars().filter(|c| *c != '\\').collect()
}

/// ¿El texto tiene la palabra como palabra entera?
///
/// Evita los falsos positivos: "cli" no debería aparecer sólo porque el texto
/// dice "clientes", ni "add" porque dice "Agrega".
fn contiene_palabra(texto: &str, palabra: &str) -> bool {
    let limite = |c: Option<char>| {
        c.map(|c| !c.is_alphanumeric() && c != '_' && c != '-')
            .unwrap_or(true)
    };

    texto.match_indices(palabra).any(|(indice, _)| {
        let antes = texto[..indice].chars().next_back();
        let despues = texto[indice + palabra.len()..].chars().next();
        limite(antes) && limite(despues)
    })
}

/// La gramática real del programa: subcomandos, banderas largas y cortas.
struct Gramatica {
    subcomandos: Vec<String>,
    banderas_largas: Vec<String>,
    banderas_cortas: Vec<char>,
}

/// Camina el árbol de comandos de `clap` y junte todo lo que el programa
/// acepta.
fn gramatica() -> Gramatica {
    fn camina(
        comando: &clap::Command,
        subcomandos: &mut Vec<String>,
        largas: &mut Vec<String>,
        cortas: &mut Vec<char>,
    ) {
        for sub in comando.get_subcommands() {
            subcomandos.push(sub.get_name().to_string());
            camina(sub, subcomandos, largas, cortas);
        }
        for arg in comando.get_arguments() {
            if let Some(long) = arg.get_long() {
                largas.push(format!("--{long}"));
            }
            if let Some(short) = arg.get_short() {
                cortas.push(short);
            }
        }
    }

    let mut g = Gramatica {
        subcomandos: Vec::new(),
        banderas_largas: Vec::new(),
        banderas_cortas: Vec::new(),
    };
    camina(&Cli::command(), &mut g.subcomandos, &mut g.banderas_largas, &mut g.banderas_cortas);
    g
}

// =========================================================================
// LAS SECCIONES
// =========================================================================

#[test]
fn la_pagina_tiene_las_secciones_que_tiene_que_tener() {
    for seccion in [
        ".TH HIKARU_OPENLIVE 1",
        ".SH NAME",
        ".SH SYNOPSIS",
        ".SH DESCRIPTION",
        ".SH OPTIONS",
        ".SH COMMANDS",
        ".SH MCP PORTS",
        ".SH ENVIRONMENT",
        ".SH FILES",
        ".SH EXIT STATUS",
        ".SH EXAMPLES",
        ".SH LICENSE",
        ".SH SEE ALSO",
    ] {
        assert!(
            MAN_PAGE.contains(seccion),
            "a la página de manual le falta la sección `{seccion}`"
        );
    }
}

#[test]
fn la_pagina_documenta_la_licencia_correcta() {
    assert!(MAN_PAGE.contains("AGPLv3"));
    assert!(MAN_PAGE.contains("LGPLv3"));
}

// =========================================================================
// LA GRAMÁTICA
// =========================================================================

#[test]
fn la_pagina_documenta_todos_los_subcomandos() {
    let texto = plano();
    let g = gramatica();

    assert!(
        !g.subcomandos.is_empty(),
        "la gramática no tiene subcomandos: el test no probaría nada"
    );

    for sub in &g.subcomandos {
        assert!(
            contiene_palabra(&texto, sub),
            "la página de manual no menciona el subcomando `{sub}`"
        );
    }
}

#[test]
fn la_pagina_documenta_todas_las_banderas_largas() {
    let texto = plano();
    let g = gramatica();

    for bandera in &g.banderas_largas {
        assert!(
            contiene_palabra(&texto, bandera),
            "la página de manual no menciona la bandera `{bandera}`"
        );
    }
}

#[test]
fn la_pagina_documenta_todas_las_banderas_cortas() {
    let texto = plano();
    let g = gramatica();

    for bandera in &g.banderas_cortas {
        let bandera = format!("-{bandera}");
        assert!(
            contiene_palabra(&texto, &bandera),
            "la página de manual no menciona la bandera corta `{bandera}`"
        );
    }
}

#[test]
fn la_pagina_documenta_las_acciones_de_los_puertos_mcp() {
    let texto = plano();

    for accion in [
        "list", "status", "enable", "disable", "add", "remove", "transport",
    ] {
        assert!(
            contiene_palabra(&texto, accion),
            "la página de manual no menciona la acción de puerto MCP `{accion}`"
        );
    }

    for transporte in ["unix", "stdio", "server-flags"] {
        assert!(
            contiene_palabra(&texto, transporte),
            "la página de manual no menciona el transporte `{transporte}`"
        );
    }
}

#[test]
fn la_pagina_documenta_las_variables_de_entorno() {
    for variable in ["HIKARU_MCP_CONFIG", "XDG_CONFIG_HOME", "XDG_RUNTIME_DIR"] {
        assert!(
            MAN_PAGE.contains(variable),
            "la página de manual no menciona la variable de entorno `{variable}`"
        );
    }
}

// =========================================================================
// LA VERSIÓN
// =========================================================================

#[test]
fn la_pagina_documenta_la_version_del_crate() {
    assert_eq!(
        man::documented_version(),
        format!("Hikaru OpenLive {}", env!("CARGO_PKG_VERSION"))
    );
}

// =========================================================================
// EL ESPEJO DE `docs/`
// =========================================================================

#[test]
fn el_espejo_de_docs_esta_sincronizado() {
    let canonico = canonico();
    let espejo = espejo();

    if !espejo.exists() {
        // El crate puede estar solo (vendorizado, copiado a otro repo): en
        // ese caso el espejo no tiene por qué estar.
        return;
    }

    let origen = std::fs::read_to_string(&canonico).expect("no se pudo leer la página canónica");
    let copia = std::fs::read_to_string(&espejo).expect("no se pudo leer el espejo de docs");

    assert_eq!(
        origen,
        copia,
        "el espejo de docs está desfasado: corré `make man-sync`"
    );
}

// =========================================================================
// EL GROFF
// =========================================================================

#[test]
fn la_pagina_no_tiene_avisos_de_groff() {
    let ruta = canonico();

    let salida = match Command::new("groff")
        .args(["-K", "utf8", "-man", "-Tutf8", "-ww", "-z"])
        .arg(&ruta)
        .output()
    {
        Ok(salida) => salida,
        // Sin groff en el sistema no hay nada que verificar: el test no
        // puede fallar por una herramienta que no está instalada.
        Err(_) => return,
    };

    let avisos = String::from_utf8_lossy(&salida.stderr);
    assert!(
        avisos.trim().is_empty(),
        "groff se quejó de la página de manual:\n{avisos}"
    );
}

#[test]
fn la_pagina_se_renderiza_sin_errores() {
    let ruta = canonico();

    let salida = match Command::new("groff")
        .args(["-K", "utf8", "-man", "-Tutf8"])
        .arg(&ruta)
        .output()
    {
        Ok(salida) => salida,
        Err(_) => return,
    };

    let texto = sin_escapes(&String::from_utf8_lossy(&salida.stdout));
    assert!(texto.contains("HIKARU_OPENLIVE(1)"));
    assert!(texto.contains("hikaru_openlive"));
    assert!(!String::from_utf8_lossy(&salida.stderr).contains("error"));
}

/// Saca los escapes de color de la salida de groff.
///
/// `groff -Tutf8` mete secuencias ANSI en el texto (las del negrita y el
/// subrayado), que parten las palabras: "HIKARU_OPENLIVE(1)" sale como
/// "HIKARU_OPENLIVE\e[24m(1)".
fn sin_escapes(texto: &str) -> String {
    let mut limpio = String::with_capacity(texto.len());
    let mut chars = texto.chars();

    while let Some(c) = chars.next() {
        if c != '\u{1b}' {
            limpio.push(c);
            continue;
        }
        // Escapada: se come lo que viene hasta la letra final.
        for c in chars.by_ref() {
            if c.is_ascii_alphabetic() {
                break;
            }
        }
    }

    limpio
}
