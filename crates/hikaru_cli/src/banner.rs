// El banner oficial de la consola.
// SPDX-License-Identifier: AGPL-3.0-or-later
// crates/hikaru_cli/src/banner.rs
//
// =========================================================================
// POR QUÉ EL BANNER ESTÁ EN UN MÓDULO PROPIO
// =========================================================================
//
// El banner es la primera cosa que ve el usuario (y la primera que ve el
// agente de IA al hacer un `hikaru_openlive --help`). Va en su propio módulo
// porque lo usan TRES caminos distintos: la ayuda, la consola interactiva y
// el error de uso. Si estuviera escrito en tres lugares, cambiar una línea
// del texto significaría buscarla en tres.
//
// El texto es el del diseño de la especificación (docs/hikaru-openlive-cli/),
// con los datos del HOST rellenos con lo que se puede detectar sin linkear
// nada: el sistema operativo, la arquitectura y la cantidad de núcleos. La
// GPU y el backend de audio los announce la GUI, que sí los puede ver.
//
// =========================================================================
// EL COLOR
// =========================================================================
//
// `colored` desactiva el color solo cuando la salida no es una terminal, así
// que el mismo código sirve para un pipe a un archivo o a otro proceso: el
// banner de un `hikaru_openlive --help > salida.txt` no tiene escapes ANSI.

use colored::Colorize;

/// Ancho de la caja del banner.
const WIDTH: usize = 102;

/// Las dos líneas de licencia del producto.
const LINE_APP: &str =
    "  > Hikaru OpenLive CLI | Hikaru Corporation (C) 2026 | GNU Affero General Public License v3 (AGPLv3)";
const LINE_FORMAT: &str =
    "  > Hikaru OpenLive File Format (.oplf) | GNU Lesser General Public License v3.0 (LGPLv3)";

/// El banner oficial, con la caja y los datos del host.
pub fn banner() -> String {
    let mut out = String::new();

    out.push_str(&border());
    out.push_str(&format!("{}\n", LINE_APP.bold().cyan()));
    out.push_str(&format!("{}\n", LINE_FORMAT.bold().cyan()));
    out.push_str(&border());
    out.push('\n');

    out.push_str(&format!(
        "  > {}: {}\n",
        "Hikaru OpenLive CLI Version".bold(),
        crate::version::VERSION
    ));
    out.push_str(&format!(
        "  > {}: {}\n",
        "Hikaru OpenLive File Format Version".bold(),
        crate::version::format_version_label()
    ));
    out.push('\n');

    for (key, value) in host_info() {
        out.push_str(&format!("  > {}: {}\n", key.bold(), value));
    }
    out.push('\n');
    out.push_str(&format!(
        "  > {}: {}\n",
        "Hikaru VST3/CLAP Host Version".bold(),
        crate::version::VERSION
    ));
    out.push_str(&format!(
        "  > {}: {}\n",
        "Hikaru Audio Backend".bold(),
        "CPAL (ALSA / PulseAudio)".dimmed()
    ));

    out
}

/// La línea de ayuda que se muestra cuando los argumentos no se entienden.
pub fn hint_uso() -> String {
    format!(
        "{}\n{}\n{}",
        "Los argumentos no se entendieron.".red().bold(),
        "hikaru_openlive --help",
        "hikaru_openlive new <archivo.oplf|archivo.opsf> | inspect <ruta> | validate <ruta> | --mcp | --cli"
    )
}

/// La caja de arriba y abajo del banner.
fn border() -> String {
    format!("{}\n", "=".repeat(WIDTH))
}

/// Los datos del host, que se llenan solos.
///
/// Se detectan con la biblioteca estándar: nada de `nvidia-smi` ni de
/// `lspci`, que no están en todas las máquinas y que además tardarían una
/// eternidad en una sesión de agente.
pub fn host_info() -> Vec<(String, String)> {
    let mut info = Vec::new();

    info.push((
        "Host OS".to_string(),
        format!(
            "{} {}",
            std::env::consts::OS,
            std::env::consts::FAMILY
        )
        .to_string(),
    ));

    info.push((
        "Host Architecture".to_string(),
        std::env::consts::ARCH.to_string(),
    ));

    info.push(("Host Kernel".to_string(), kernel_release()));

    let cores = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(0);
    info.push(("Host CPU Cores".to_string(), cores.to_string()));

    info
}

/// La versión del kernel, si se puede leer.
///
/// En Linux está en `/proc/sys/kernel/osrelease`. En los demás sistemas no
/// hay una forma portable: se dice que no se pudo saber, en vez de inventar
/// un número.
fn kernel_release() -> String {
    #[cfg(target_os = "linux")]
    {
        if let Ok(release) = std::fs::read_to_string("/proc/sys/kernel/osrelease") {
            let release = release.trim();
            if !release.is_empty() {
                return release.to_string();
            }
        }
    }
    "desconocido".to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn el_banner_menciona_la_version_del_producto_y_la_del_formato() {
        let banner = banner();
        assert!(banner.contains(crate::version::VERSION));
        assert!(banner.contains("Hikaru OpenLive File Format Version"));
        assert!(banner.contains("AGPL"));
        assert!(banner.contains("LGPL"));
    }

    #[test]
    fn el_banner_no_trae_escapes_ansi_cuando_la_salida_no_es_terminal() {
        // En los tests la salida no es una terminal, así que `colored` no
        // colorea: el banner tiene que ser texto plano y parseable.
        let banner = banner();
        assert!(!banner.contains('\u{1b}'), "el banner trajo escapes ANSI");
    }

    #[test]
    fn la_caja_del_banner_es_del_ancho_correcto() {
        let line = border();
        let ancho = line.trim_end_matches('\n').chars().count();
        assert_eq!(ancho, WIDTH);
    }

    #[test]
    fn el_host_reporta_al_menos_el_sistema_operativo() {
        let info = host_info();
        assert!(!info.is_empty());
        assert!(info[0].0.contains("OS"));
    }
}
