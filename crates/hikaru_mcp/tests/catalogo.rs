// crates/hikaru_mcp/tests/catalogo.rs
// SPDX-License-Identifier: AGPL-3.0-or-later

//! El catálogo y el handler tienen que estar de acuerdo.
//!
//! El catálogo es lo que el agente ve en `tools/list`. Si declara una
//! herramienta que el handler no sabe despachar, el agente la llama y se
//! come un error de protocolo. Estos tests comparan las dos listas.

use std::collections::HashSet;

use hikaru_mcp::catalog;

/// Los nombres de las herramientas que el handler despacha.
///
/// Tiene que coincidir con el catálogo: si se agrega una herramienta hay que
/// agregarla en los dos lados, y este test avisa.
fn herramientas_despachadas() -> HashSet<&'static str> {
    [
        "list_plugins",
        "plugin_info",
        "plugin_parameters",
        "set_parameter",
        "inspect_plugin_wrapper",
        "decompile_plugin_stub",
        "plugin_symbols",
        "audit_project",
        "project_info",
    ]
    .into_iter()
    .collect()
}

#[test]
fn el_catalogo_y_el_despacho_declaran_lo_mismo() {
    let catalogo: HashSet<&str> = catalog::tools().iter().map(|t| t.name).collect();
    let despacho = herramientas_despachadas();

    let solo_catalogo: Vec<_> = catalogo.difference(&despacho).collect();
    let solo_despacho: Vec<_> = despacho.difference(&catalogo).collect();

    assert!(
        solo_catalogo.is_empty(),
        "el catálogo declara herramientas que el handler no despacha: {solo_catalogo:?}"
    );
    assert!(
        solo_despacho.is_empty(),
        "el handler despacha herramientas que el catálogo no declara: {solo_despacho:?}"
    );
}

#[test]
fn toda_herramienta_tiene_descripcion_y_argumentos_con_nombre() {
    for herramienta in catalog::tools() {
        assert!(
            !herramienta.description.trim().is_empty(),
            "la herramienta '{}' no tiene descripción",
            herramienta.name
        );

        for parametro in &herramienta.params {
            assert!(
                !parametro.name.is_empty(),
                "la herramienta '{}' tiene un argumento sin nombre",
                herramienta.name
            );
            assert!(
                !parametro.description.trim().is_empty(),
                "el argumento '{}' de '{}' no tiene descripción",
                parametro.name,
                herramienta.name
            );
        }
    }
}

#[test]
fn los_recursos_tienen_esquema_y_descripcion() {
    for recurso in catalog::resources() {
        assert!(
            recurso.uri.starts_with("hikaru://"),
            "el recurso '{}' no usa el esquema hikaru://",
            recurso.uri
        );
        assert_eq!(recurso.mime_type, "application/json");
        assert!(!recurso.description.trim().is_empty());
    }
}

#[test]
fn los_prompts_declaran_sus_argumentos() {
    for prompt in catalog::prompts() {
        assert!(!prompt.description.trim().is_empty());
        for (nombre, descripcion, _) in &prompt.arguments {
            assert!(!nombre.is_empty());
            assert!(!descripcion.trim().is_empty());
        }
    }
}
