// El empaquetado de la página de manual del binario `hikaru_openlive`.
// SPDX-License-Identifier: AGPL-3.0-or-later
// crates/hikaru_cli/build.rs
//
// =========================================================================
// POR QUÉ UN `build.rs` PARA UNA PÁGINA DE MANUAL
// =========================================================================
//
// La página de manual (`man hikaru_openlive`) es la documentación que el
// sistema muestra cuando alguien escribe `man hikaru_openlive`. Si la
// página vive en el repositorio pero nunca llega a `/usr/share/man/man1`,
// el comando da "No manual entry" y la herramienta parece no existir.
//
// Este `build.rs` hace las tres cosas que hacen falta para que eso no
// pase:
//
//   1. COPIA la página a `$OUT_DIR/man/`, que es el lugar donde un
//      empaquetador (cargo-dist, un .deb, un ports de FreeBSD) busca los
//      archivos que van con el binario.
//   2. EXPONE la página al código con `cargo:rustc-env`, para que la crate
//      la embeba (`include_str!`) y el binario sepa qué se está
//      documentando.
//   3. INSTALA la página si el usuario lo pide explícitamente, con
//      `HIKARU_INSTALL_MAN=1` (en el directorio del usuario, sin root) o
//      `HIKARU_INSTALL_MAN=system` (en `/usr/share/man/man1`).
//
// Por omisión NO se instala nada: un `cargo build` no tiene por qué tocar
// `/usr/share`. Instalar es una decisión del que empaqueta, no del que
// compila.

use std::path::{Path, PathBuf};

/// El nombre del binario, y el de la página de manual.
const MAN_NAME: &str = "hikaru_openlive";

/// La variable de entorno que pide instalar la página durante el build.
///
/// Valores aceptados:
///
///   * (sin definir) o `0`: no se instala nada (es lo por omisión).
///   * `1`, `yes`, `user`: se instala en el directorio del usuario
///     (`$XDG_DATA_HOME/man/man1` o `~/.local/share/man/man1`).
///   * `system`: se instala en `/usr/share/man/man1` (hace falta root).
const INSTALL_ENV: &str = "HIKARU_INSTALL_MAN";

/// La variable de entorno que pisa el directorio de destino.
///
/// Apunta al directorio `man1` donde va la página (por ejemplo
/// `/usr/share/man/man1`).
const MAN_DIR_ENV: &str = "HIKARU_MAN_DIR";

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-env-changed={}", INSTALL_ENV);
    println!("cargo:rerun-if-env-changed={}", MAN_DIR_ENV);

    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let out_dir = PathBuf::from(std::env::var("OUT_DIR").expect("cargo define OUT_DIR"));

    // --- 1. Dónde está la página -----------------------------------------
    let source = locate_source(&manifest_dir);

    // --- 2. La copia empaquetable ----------------------------------------
    // Siempre se escribe un archivo en `$OUT_DIR/man/`: o la página real, o
    // un marcador de posición. Así el `include_str!` del crate nunca falta
    // (un build sin la página NO puede romperse: la página es
    // documentación, no código).
    let staged = out_dir.join("man").join(format!("{MAN_NAME}.1"));
    if let Some(parent) = staged.parent() {
        std::fs::create_dir_all(parent).expect("no se pudo crear el directorio de la página");
    }

    match &source {
        Some(path) => {
            std::fs::copy(path, &staged).expect("no se pudo copiar la página de manual");
            println!("cargo:rerun-if-changed={}", path.display());
        }
        None => {
            println!(
                "cargo:warning=No se encontró la página de manual de {MAN_NAME}: \
                 el crate se compiló sin ella (mirá crates/hikaru_cli/man/)"
            );
            std::fs::write(&staged, PLACEHOLDER).expect("no se pudo escribir el marcador");
        }
    }

    // La ruta del archivo empaquetado la consume el crate (`include_str!`).
    println!("cargo:rustc-env=HIKARU_MAN_PAGE_PATH={}", staged.display());

    // --- 3. La versión de la página tiene que ser la del crate ------------
    check_version(&source);

    // --- 4. La instalación (sólo si se pide) ------------------------------
    install(&staged);
}

/// El contenido del marcador de posición: una página vacía pero válida.
///
/// Tiene que ser una página de manual que no dé error si alguien la renderiza
/// por accidente.
const PLACEHOLDER: &str = concat!(
    ".\\\" Página de manual no disponible en esta compilación.\n",
    ".TH HIKARU_OPENLIVE 1\n",
    ".SH NAME\n",
    "hikaru_openlive \\- DAW de performance en vivo y arreglo\n",
);

/// Busca el archivo de la página de manual.
///
/// Se mira primero el directorio del crate (`man/`), que es el CANÓNICO y el
/// que viaja con el crate; después el espejo de `docs/`, para el caso de que
/// alguien compile el crate con la documentación pero sin el `man/` propio.
fn locate_source(manifest_dir: &Path) -> Option<PathBuf> {
    let candidates = [
        manifest_dir.join("man").join(format!("{MAN_NAME}.1")),
        manifest_dir
            .join("..")
            .join("..")
            .join("docs")
            .join("man")
            .join(format!("{MAN_NAME}.1")),
    ];

    candidates.into_iter().find(|path| path.is_file())
}

/// Avisa (sin romper el build) si la página documenta otra versión.
///
/// La versión de la página está en la línea `.TH`, en el quinto campo (después
/// del nombre, la sección y la fecha). No es un error fatal: un árbol de
/// trabajo a medio cambiar no debería dejar de compilar. Pero conviene que
/// quede a la vista.
fn check_version(source: &Option<PathBuf>) {
    let Some(path) = source else { return };

    let Ok(text) = std::fs::read_to_string(path) else {
        return;
    };

    let version = env!("CARGO_PKG_VERSION");
    let th_line = text
        .lines()
        .find(|line| line.starts_with(".TH "))
        .unwrap_or_default();

    if !th_line.contains(version) {
        println!(
            "cargo:warning=La página de manual de {MAN_NAME} no menciona la versión {version} \
             (la línea .TH dice: {th_line:?}). Actualizala con `make man-sync`."
        );
    }
}

/// Instala la página si el usuario lo pidió.
///
/// Un fallo acá NO rompe el build: la página es documentación y un `cargo
/// build` que se cae porque no se pudo copiar un `.1` a `/usr/share` sería
/// una molestia. El aviso de cargo queda a la vista igual.
fn install(staged: &Path) {
    let mode = std::env::var(INSTALL_ENV).unwrap_or_default();
    let mode = mode.trim();

    if mode.is_empty() || matches!(mode, "0" | "no" | "false") {
        return;
    }

    // El destino se puede pisar con `HIKARU_MAN_DIR` (lo usa el
    // empaquetador, que ya sabe dónde van los archivos).
    let target_dir = match std::env::var(MAN_DIR_ENV) {
        Ok(dir) if !dir.trim().is_empty() => PathBuf::from(dir),
        _ => match mode {
            "1" | "yes" | "true" | "user" => user_man_dir(),
            "system" => PathBuf::from("/usr/share/man/man1"),
            other => {
                println!(
                    "cargo:warning=Valor inválido para {INSTALL_ENV}: {other:?}. \
                     Se esperaba 1 (usuario) o system."
                );
                return;
            }
        },
    };

    let target = target_dir.join(format!("{MAN_NAME}.1"));

    if let Err(err) = std::fs::create_dir_all(&target_dir) {
        println!(
            "cargo:warning=No se pudo crear {} para instalar la página de manual: {err}",
            target_dir.display()
        );
        return;
    }

    match std::fs::copy(staged, &target) {
        Ok(_) => {
            // Va como `cargo:warning` y no como un `eprintln!` porque cargo
            // esconde la salida del script de build salvo que se compile con
            // `-vv`: con esto el que pidió la instalación la ve siempre.
            println!(
                "cargo:warning=Página de manual de {MAN_NAME} instalada en {}",
                target.display()
            );
        }
        Err(err) => {
            println!(
                "cargo:warning=No se pudo instalar la página de manual en {}: {err}",
                target.display()
            );
        }
    }
}

/// El directorio `man1` del usuario.
///
/// Es `$XDG_DATA_HOME/man/man1` si `XDG_DATA_HOME` está definido, y
/// `$HOME/.local/share/man/man1` si no (que es lo que dice el estándar XDG
/// y lo que la mayoría de las distribuciones mira).
fn user_man_dir() -> PathBuf {
    let base = match std::env::var("XDG_DATA_HOME") {
        Ok(dir) if !dir.trim().is_empty() => PathBuf::from(dir),
        _ => {
            let home = std::env::var("HOME").unwrap_or_else(|_| "/".to_string());
            PathBuf::from(home).join(".local").join("share")
        }
    };

    base.join("man").join("man1")
}
