// Copyright (C) Hikaru Corporation - 2026
// GNU Lesser General Public License v3
// crates/hikaru_gui/build.rs

//! Script de construcción del binario del DAW.
//!
//! # Por qué hace falta
//!
//! `hikaru_plugin_host` interpone `dlsym` para poder desactivar XInput2
//! en el proceso (ver `hikaru_plugin_host::xlib`): sin eso, los editores de
//! plugin hechos con JUCE 7 en adelante se quedan **sin ratón** bajo
//! XWayland, porque el toolkit descarta los eventos del núcleo de X11
//! esperando los de XI2 que XWayland nunca entrega.
//!
//! El interposor no sirve de nada si el binario no lo **exporta**: los
//! plugins referencian `dlsym` con versión (`dlsym@GLIBC_2.34`), y una
//! definición sin versión no satisface una referencia versionada. El
//! enlazador se encarga de eso con un script de versiones, que tiene que
//! aplicársele al **binario final** (a las librerías no: ahí no hay nada
//! que exportar).
//!
//! El script va en el crate del plugin host porque es donde vive el
//! interposor; este `build.rs` sólo lo aplica.

use std::path::PathBuf;

fn main() {
    println!("cargo:rerun-if-changed=../hikaru_plugin_host/hikaru-dlsym.map");

    // El script de versiones vive en el crate del plugin host, que es donde
    // está el interposor. La ruta es relativa al crate del binario.
    let map = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../hikaru_plugin_host/hikaru-dlsym.map");

    if !map.exists() {
        println!(
            "cargo:warning=No se encontró el script de versiones {:?}: \
             los editores de plugin con JUCE 7+ se quedarán sin ratón bajo XWayland",
            map
        );
        return;
    }

    println!("cargo:rustc-link-arg=-Wl,--version-script={}", map.display());
}
