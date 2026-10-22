# `Atención Gordos Dev Github: Este es un borrador, no está completo HDPs.`
---

# Hikaru OpenLive CLI - Documentación.

El ejecutable CLI de `hikaru_openlive` permite inspeccionar, validar, crear y manipular proyectos `.oplf` y `.opsf` directamente desde la terminal sin necesidad de inicializar la interfaz gráfica de usuario (GUI).

---

# Input

```bash

FreeBSD 14.5 CURRENT
user: chen
password: ****

yukamo-shrinebsd@server:~$ hikaru_openlive --cli

======================================================================================================|
  > Hikaru OpenLive CLI | Hikaru Corporation (C) 2026 | GNU Affero General Public License v3 (AGPLv3) |
  > Hikaru OpenLive File Format (.oplf) | GNU Lesser General Public License v3.0 (LGPLv3)             |
======================================================================================================|

  > Hikaru OpenLive CLI Version: 2.22.2
  > Hikaru OpenLive File Format Version: 1.0

  > Shrine Host OS: "FreeBSD"
  > Shrine Host Kernel Version: 14.5-CURRENT
  > Shrine Host Kernel Architecture: amd64
  > Shrine Host CPU: AMD Ryzen Threadripper 3970X
  > Shrine Host CPU Cores: 32 | Threads: 64
  > Shrine Host GPU: NVIDIA GeForce 4070 TI Super 16GB VRAM
  > Shrine Host GPU Memory: 128 GB

  > Hikaru VST3/CLAP Host Version: 2.22.2

  > Hikaru Audio Backend: JACK (ALSA/JACK Backend Disabled)

  --------------------------------------------------------------------------------------------------
    > hikaru_openlive --help | Hikaru OpenLive CLI Help

    > hikaru_openlive new <".oplf"/".opsf"> [OPTIONS] 
      > Creates a new project file (.oplf/.opsf) with default template.
      [OPTIONS] is optional, is not obligatory.
      > Example: `hikaru_openlive new my-project.oplf`

    > hikaru_openlive metadata <PATH> [OPTIONS] 
      > Shows project metadata, clips, tracks, BPM, Key Scale, etc.
      [OPTIONS] is optional, is not obligatory.

      {
          "name" : "my-project",
          "format" : "OpenLive",
          "schema_version" : "2.22.2",
          "created_by" : "Hikaru OpenLive 2.22.2",
          "engine_mode" : "OpenLive",
          "metadata" : {
              "title" : "My Project",
              "artist" : "My Name",
              "genre" : "Electronic",
              "key" : "C Minor",
              "stats" : {
                  "total_scenes" : 1,
                  "total_tracks" : 1,
                  "audio_clips" : 0,
                  "midi_clips" : 0,
                  // Más metadatos adicionales...
      } // Fin del código de metadatos

  --------------------------------------------------------------------------------------------------

[Hikaru OpenLive] Iniciando el editor de proyecto...
[Hikaru OpenLive] Cargando proyecto...
[Hikaru OpenLive] Proyecto cargado.
[Hikaru OpenLive] Iniciando la GUI...
```

## `hikaru_openlive --help`

```bash
hikaru_openlive --help

## El resto de Boludeces...

```


---

## 1. Comandos Principales

### `new`
Inicializa un nuevo proyecto `.oplf` o `.opsf` con plantillas predeterminadas.
```bash
# Crear un nuevo proyecto OpenLive con plantilla por defecto
hikaru_openlive new mi_track.oplf --template default

# Crear un proyecto OpenStudio vacío
hikaru_openlive new mi_album.opsf --empty

```

### `inspect` / `info`

Muestra metadatos del proyecto, estructuras de clips, pistas, BPM y versión del esquema sin cargar la interfaz de audio.

```bash
hikaru_openlive inspect mi_track.oplf

```

**Ejemplo de salida:**

```text
[Hikaru Project Summary]
Format: OpenLive (.oplf)
Schema Version: 2.22.2
BPM: 128.0 | Launch Quantization: 1 Bar
Session Matrix: 4 Tracks x 8 Scenes
Live Tracks: [Drums, Bass, Synth, FX]

```

### `validate` / `check`

Realiza un diagnóstico de integridad sobre archivos `.oplf` / `.opsf`. Verifica que los recursos indexados (muestras de audio, referencias de instrumentos) existan y no estén corruptos.

```bash
hikaru_openlive validate mi_track.oplf

```

### `convert`

Permite migrar o exportar datos entre formatos cuando sea compatible.

```bash
hikaru_openlive convert mi_track.oplf --export-matrix-json

```

---

## 2. Argumentos de Lanzamiento de la GUI

La interfaz CLI también actúa como punto de entrada (entrypoint) para levantar la GUI con flags de diagnóstico:

* `hikaru_openlive --gui` : Inicia la interfaz gráfica habitual.
* `hikaru_openlive --no-audio` : Inicia la GUI desactivando el backend de ALSA/JACK (ideal para debugging en servidores sin placa de sonido).
* `hikaru_openlive --project <PATH>` : Carga un archivo `.oplf` / `.opsf` directamente al abrir la GUI.

---

## 3. La Página de Manual (`man`)

El binario tiene su página de manual nativa, escrita en groff/troff, para que
`man hikaru_openlive` funcione en cualquier sistema Unix/BSD/Linux:

```bash
man hikaru_openlive
```

* **Archivo canónico**: [`crates/hikaru_cli/man/hikaru_openlive.1`](../../crates/hikaru_cli/man/hikaru_openlive.1)
  (es el que viaja con el crate y el que copia el `build.rs` a `$OUT_DIR/man/`
  para el empaquetado).
* **Espejo en la documentación**: [`docs/man/hikaru_openlive.1`](../man/hikaru_openlive.1)
  (se mantiene sincronizado con `make man-sync`).

### Instalarla

```bash
# En el directorio del usuario (no hace falta root):
make man-install-user

# En el sistema (hace falta root; PREFIX es /usr/local por omisión):
sudo make man-install PREFIX=/usr
```

Después de la instalación de usuario, `man` la encuentra si
`$HOME/.local/share/man` está en el `MANPATH` (en la mayoría de las
distribuciones ya lo está):

```bash
MANPATH=$HOME/.local/share/man man hikaru_openlive
```

El `build.rs` del crate también la instala si se lo pide explícitamente:

```bash
HIKARU_INSTALL_MAN=1 cargo build --release --bin hikaru_openlive
# (HIKARU_INSTALL_MAN=system la instala en /usr/share/man/man1)
```

### Verificarla

```bash
make man-check      # groff no tiene que quejarse de nada
make man-render     # el renderizado en la terminal
man ./docs/man/hikaru_openlive.1
```

La página no se desactualiza sola: `crates/hikaru_cli/tests/man_page.rs` la
compara con la gramática real de `clap` y falla si hay un subcomando, una
bandera o una acción de puerto MCP que la página no menciona.

---

## 4. Integración en Crate Architecture

Se sugiere implementar una caja independiente o un binario secundario en el workspace de Rust:

* `crates/hikaru_cli/`
* Dependencias recomendadas: `clap` (parsing de CLI), `serde_json` (inspección de estados), `colored` (formato de salida de consola).
