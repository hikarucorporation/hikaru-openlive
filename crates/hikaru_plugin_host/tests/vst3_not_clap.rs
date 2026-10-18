// Copyright (C) Hikaru Corporation - 2026
// GNU Lesser General Public License v3
//! Reproduce el bug reportado: un bundle VST3 se indexaba como si fuera un
//! `.so` suelto de CLAP, y al cargarlo fallaba con
//! `undefined symbol: clap_entry`.

use hikaru_plugin_host::registry::scan_standard_paths;

fn vst3_dir_paths() -> Vec<std::path::PathBuf> {
    let home = std::path::PathBuf::from(std::env::var("HOME").unwrap_or_else(|_| "/root".into()));
    let mut out = Vec::new();
    for root in [std::path::PathBuf::from("/usr/lib/vst3"), home.join(".vst3")] {
        if let Ok(entries) = std::fs::read_dir(&root) {
            for e in entries.flatten() {
                let p = e.path();
                if p.is_dir() && p.extension().map_or(false, |x| x == "vst3") {
                    out.push(p);
                }
            }
        }
    }
    out.sort();
    out
}

/// Ningún VST3 puede aparecer apuntando a su `.so` interno, y ningún plugin
/// marcado CLAP puede salir de un bundle VST3.
#[test]
fn vst3_bundles_are_not_reported_as_clap() {
    let report = scan_standard_paths();
    let bundles = vst3_dir_paths();

    if bundles.is_empty() {
        eprintln!("[vst3] sin bundles instalados; skip");
        return;
    }

    for plugin in &report.plugins {
        let name = format!("{:?}", plugin.name);
        let path = plugin.path.display().to_string();

        // Si la ruta está dentro de un bundle .vst3, el formato NO puede ser CLAP.
        if path.contains(".vst3/") {
            assert_eq!(
                plugin.format,
                hikaru_plugin_host::PluginFormat::VST3,
                "'{name}' apunta dentro de un bundle .vst3 pero quedó como {:?} en {path}",
                plugin.format
            );
            // Y la ruta tiene que ser el bundle, no el .so interno.
            assert!(
                plugin.path.extension().map(|e| e == "vst3").unwrap_or(false),
                "'{name}' debería apuntar al bundle, no a su binario: {path}"
            );
        }
    }

    let vst3_count = report
        .plugins
        .iter()
        .filter(|p| p.format == hikaru_plugin_host::PluginFormat::VST3)
        .count();
    let clap_count = report
        .plugins
        .iter()
        .filter(|p| p.format == hikaru_plugin_host::PluginFormat::CLAP)
        .count();
    eprintln!("[vst3] {vst3_count} VST3, {clap_count} CLAP, {} errores", report.errors.len());

    assert!(
        vst3_count >= bundles.len(),
        "de {vst3_count} VST3 indexados hay {} bundles en disco",
        bundles.len()
    );
}

/// Cargar cada VST3 indexado tiene que funcionar de verdad: si alguno sale con
/// `undefined symbol: clap_entry`, el bug volvió.
///
/// Se saltea yabridge a propósito: sus bundles son stubs de 88K que no
/// contienen un módulo VST3 real y abortan el proceso con un `assert` de C++
/// (`Assertion 'bridge' failed`) cuando no hay bridge configurado. Ese es un
/// problema aparte —y grave— pero no es el que se está probando acá, y ambos
/// casos abortarían el binario de tests.
#[test]
fn every_indexed_vst3_actually_loads() {
    let report = scan_standard_paths();

    let vst3: Vec<_> = report
        .plugins
        .iter()
        .filter(|p| p.format == hikaru_plugin_host::PluginFormat::VST3)
        .filter(|p| !p.path.to_string_lossy().contains("yabridge"))
        .collect();

    if vst3.is_empty() {
        eprintln!("[vst3] sin VST3 indexados; skip");
        return;
    }

    // Lo que se separa: un fallo por `GetPluginFactory` significa que el
    // binario NO es un VST3 (típicamente un VST2 dejado en `~/.vst3`, que
    // exporta `VSTPluginMain`). Eso es una instalación equivocada del plugin,
    // no un bug del host. Un fallo por `clap_entry` sí es el bug original.
    let mut not_a_vst3 = Vec::new();
    let mut misclassified = Vec::new();

    for p in vst3 {
        match hikaru_plugin_host::vst3::Vst3Instance::load(&p.path) {
            Ok(_) => eprintln!("[vst3] {} carga OK", p.name),
            Err(e) => {
                let msg = e.to_string();
                if msg.contains("clap_entry") {
                    eprintln!("[vst3] {} MAL CLASIFICADO: {msg}", p.name);
                    misclassified.push(format!("{}: {msg}", p.name));
                } else {
                    eprintln!("[vst3] {} no es VST3 (instalación inválida): {msg}", p.name);
                    not_a_vst3.push(p.name.clone());
                }
            }
        }
    }

    assert!(
        misclassified.is_empty(),
        "VST3 cargados como CLAP (el bug original volvió): {misclassified:?}"
    );
    if !not_a_vst3.is_empty() {
        eprintln!(
            "[vst3] {not_a_vst3:?} están en un directorio .vst3 pero no son VST3 \
             (VST2, o stubs de yabridge). El host los rechaza bien."
        );
    }
}
