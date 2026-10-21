# Mapa de licencias de Hikaru OpenLive / OpenStudio

Este documento es la referencia unica de "que esta bajo que licencia" para todo
el repositorio, incluyendo los tres crates vendorizados de terceros
(`gpui-pre-patched`, `gpui-pre-linux-patched`, `vst3-host-patched`).

Los textos completos de licencia estan en la raiz del repo (`LICENSE` =
AGPL-3.0, `LICENSE-ES` = traduccion) y en los subdirectorios vendorizados.

---

## 1. El arbol de crates propios

| Crate | Licencia | Rol |
|---|---|---|
| `crates/hikaru_gui` | `AGPL-3.0-or-later` | GUI (egui + wgpu) |
| `crates/hikaru_render` | `AGPL-3.0-or-later` | Render / exportacion |
| `crates/hikaru_audio_drivers` | `LGPL-3.0-or-later` | Drivers de audio |
| `crates/hikaru_project_file` | `LGPL-3.0-or-later` | Formato `.oplproj` |
| `crates/hikaru_openlive_file` | `LGPL-3.0-or-later` | Formato `.oplf` |
| `crates/hikaru_openstudio_file` | `LGPL-3.0-or-later` | Formato `.opsf` |
| `crates/hikaru_core`, `hikaru_transport`, `hikaru_audio_engine`, `hikaru_sequencer`, `hikaru_dsp`, `hikaru_midi`, `hikaru_plugin_host` | (heredan la de la app) | Nucleo del motor |

Los tres crates de formato de archivo son **LGPL-3.0 a proposito**: no dependen
de la app, y son la frontera que permite que un DAW de codigo cerrado lea y
escriba proyectos de Hikaru sin abrir el motor AGPL. Ver
`docs/hikaru-project-files/`.

### Por que "or-later" y no "only"

Los cuatro crates de biblioteca usan `LGPL-3.0-or-later` (y no `-only`) por
tres razones concretas:

1. **El texto de la LGPL-3.0 ya da la opcion de "version posterior".** Su
   seccion 6 dice: *"If the Library as you received it specifies that a
   certain numbered version of the GNU Lesser General Public License 'or any
   later version' applies to it, you have the option of following the terms and
   conditions either of that published version or of any later version"*. La
   FSF ademas publica su propio `COPYING.LESSER` con el boilerplate "either
   version 3 of the License, or (at your option) any later version". O sea,
   `-only` esta peleando con la licencia que dice usar.

2. **Quita friccion sin costo.** Con `-or-later` el receptor elige entre
   {LGPL-3.0, GPL-3.0, AGPL-3.0}. Con `-only` un proyecto GPLv3/AGPLv3
   tendria que renegociar para poder reusar el crate.

3. **No debilita nada.** La opcion mas permisiva del conjunto sigue siendo la
   LGPL-3.0, que es la que habilita el enlace desde un DAW de codigo cerrado:
   la frontera se sostiene igual. Y GPL-3.0/AGPL-3.0 son *mas* exigentes que
   la LGPL-3.0 para una obra combinada, asi que ningun receptor gana escapando
   por ahi.

En resumen: `-or-later` agrega opciones, no resta obligaciones. La GPL-3.0 y
la AGPL-3.0 exigen ademas el fuente completo de la obra combinada, o sea que
son un conjunto de obligaciones *mas grande* que el de la LGPL-3.0, no mas
chico.

### El texto de la LGPL-3.0 esta en el repo

`LICENSE-LGPL-3.0` en la raiz contiene la LGPL-3.0 y la GPL-3 que esta
incorpora por referencia. Hace falta: la LGPL-3.0 seccion 4(b) exige que
quien enlace estas librerias en un programa distribuible acompane el
ejecutable con *"a copy of the GNU GPL and this license document"*. Sin ese
archivo en el repo, un DAW de codigo cerrado que quiera usar el formato
`.oplf` no tendria de donde sacar el texto que tiene que distribuir.

---

## 2. Los tres crates vendorizados

| Directorio | Upstream | Licencia upstream | Oferta de este fork |
|---|---|---|---|
| `gpui-pre-patched` | Zed GPUI (`gpui-pre` 0.3.6) | **Apache-2.0** (irrevocable) | **LGPL-3.0-or-later** |
| `gpui-pre-linux-patched` | Zed `gpui_linux` (`gpui-pre-linux` 0.3.6) | **Apache-2.0** (irrevocable) | **LGPL-3.0-or-later** |
| `vst3-host-patched` | `rust-vst3-host` (`vst3-host` 0.1.1) | **MIT** (irrevocable) | **LGPL-2.1-or-later** |

En los tres casos el `NOTICE` del directorio correspondiente tiene el detalle
completo: who es el autor original, que se relicencio y por que, y que
obligaciones quedan pendientes.

### La regla que decide que version de LGPL va

La FSF ([license-compatibility.html](https://www.gnu.org/licenses/license-compatibility.html)):

> "Any GNU license, version N, subsumes the Apache 2.0 license provided N is
> at least 3."

Y de la [lista de licencias](https://www.gnu.org/licenses/license-list.html),
sobre la Apache-2.0:

> "This is a free software license, compatible with version 3 of the GNU GPL.
> Please note that this license is not compatible with GPL version 2, because
> it has some requirements that are not in that GPL version. These include
> certain patent termination and indemnification provisions."

De ahi sale todo:

- La **LGPL-2.1** es familia GPLv2. La Apache-2.0 **no** es compatible con la
  GPLv2, asi que la LGPL-2.1 **no puede** absorber codigo Apache-2.0. Por eso
  los dos crates de gpui quedaron en **LGPL-3.0-or-later**, que si es
  familia GPLv3.
- La **MIT**, en cambio, si es compatible con la GPLv2 y con la GPLv3 (la
  lista de la FSF dice que las licencias compatibles son compatibles con
  ambas salvo que se indique lo contrario). Por eso `vst3-host` **si** pudo
  quedar en **LGPL-2.1-or-later**, que es exactamente lo pedido.

La diferencia no es casual: sale de la licencia upstream de cada crate.

### `vst3-host-patched` -- de MIT a LGPL-2.1-or-later

La MIT es permisiva y concede el derecho de "sublicense" ("to use, copy,
modify, merge, publish, distribute, sublicense, and/or sell copies of the
Software"), con la unica condicion de conservar el aviso de copyright
original. Eso se cumple: el aviso MIT de Helge Sverre esta intacto en
`LICENSE-MIT`, al lado del texto LGPL-2.1 en `LICENSE`.

Se eligio `LGPL-2.1-or-later` y no `LGPL-2.1-only` a proposito: la app es
AGPL-3.0 (familia GPLv3) y `LGPL-2.1-only` es codigo GPLv2-only, que no
puede formar parte de una obra AGPL-3.0.

Pendiente para el futuro: la LGPL-2.1 seccion 4 exige que quien distribuye la
libreria en forma ejecutable permita relinkear contra una version modificada.
Ver la seccion "Obligaciones pendientes" del `NOTICE` de `vst3-host-patched`.

### `gpui-pre-patched` y `gpui-pre-linux-patched` -- de Apache-2.0 a LGPL-3.0-or-later

La Apache-2.0 seccion 2 concede el derecho de "sublicense", asi que el
upstream puede sublicenciarse: este fork ofrece la obra completa bajo
LGPL-3.0-or-later, y la LGPL-3 (familia GPLv3) si absorbe los terminos de la
Apache-2.0.

Lo que NO se puede es dejar el `license` en `Apache-2.0` y pretender que
alcanza: la grant de Apache-2.0 es **irrevocable** y sigue disponible para
cualquiera que reciba una copia. La sublicencia **agrega** una offerta, no
quita ninguna. Por eso:

- `LICENSE-APACHE` se conserva intacto (lo exige la Apache-2.0 seccion 4(a)),
- el `NOTICE` y el `PATCHES.md` documentan los cambios (secciones 4(b) y 4(c)),
- y el `license` del `Cargo.toml` dice `LGPL-3.0-or-later`, que es la
  licencia bajo la cual se distribuye este fork.

### Lo que esto protege y lo que no

- **No** protege el codigo upstream de Zed ni el de Helge Sverre: eso no es
  tuyo, y la grant original sigue viva para cualquiera. No tenes ni el
  derecho ni el interes de "protegerlo".
- **Si** establece que este fork, tal como se distribuye, es copyleft, y que
  el parche propio de Hikaru va con esos terminos.
- Y lo que de verdad protege el codigo propio es la **AGPL-3.0 de la
  aplicacion**: cubre el conjunto completo que se distribuye, parches
  incluidos. Eso ya estaba hecho antes de este cambio.

Bajo MIT y Apache-2.0, las modificaciones propias **ya se pueden mantener
privadas** si uno quiere: ambas licencias permiten usar el derivado en
software propietario. O sea, relicenciar a LGPL no "protege" nada que antes
estuviera desprotegido, y a cambio impone obligaciones nuevas (publicar el
fuente de la libreria, permitir relinkeo).

Y relicenciar codigo de terceros tampoco "cierra" la puerta: el upstream
puede publicar una version nueva bajo la licencia que quiera, y la licencia
del fork no le impide a uno reusar el codigo original.

---

## 3. Checklist para publicar

- [ ] El `LICENSE` de la raiz (AGPL-3.0) acompana al binario.
- [ ] El codigo fuente completo de la aplicacion se publica (obligacion AGPL-3.0
      seccion 13, y en especial para uso en red/servicio).
- [ ] Si el binario enlaza cualquiera de los crates LGPL-3.0-or-later
      (`hikaru_project_file`, `hikaru_openlive_file`, `hikaru_openstudio_file`,
      `hikaru_audio_drivers`), el `LICENSE-LGPL-3.0` de la raiz acompana al
      binario: la LGPL-3.0 seccion 4(b) exige entregar "a copy of the GNU GPL
      and this license document" con la obra combinada.
- [ ] `vst3-host-patched/`: `LICENSE` (LGPL-2.1 + GPL-2), `LICENSE-MIT` y
      `NOTICE` acompanan al binario. Si se publica un binario cerrado que
      enlace este crate, hay que cumplir la seccion 4 de la LGPL-2.1
      (relinkeo): publicar los object files o el mecanismo de enlace
      compartido, o el codigo fuente de la libreria con la oferta de
      relinkeo. Ver el `NOTICE` de ese directorio.
- [ ] `gpui-pre-patched/` y `gpui-pre-linux-patched/`: `LICENSE` (LGPL-3.0 +
      GPL-3), `LICENSE-APACHE`, `NOTICE` y `PATCHES.md` acompanan al binario
      (requisito de Apache-2.0 seccion 4).
- [ ] Si se re-vendoriza cualquiera de los tres crates: volver a copiar
      `LICENSE` / `LICENSE-MIT` / `LICENSE-APACHE` / `NOTICE` y volver a setear
      el campo `license` del `Cargo.toml`:
      - `vst3-host` -> `LGPL-2.1-or-later`
      - `gpui-pre` -> `LGPL-3.0-or-later`
      - `gpui-pre-linux` -> `LGPL-3.0-or-later`
