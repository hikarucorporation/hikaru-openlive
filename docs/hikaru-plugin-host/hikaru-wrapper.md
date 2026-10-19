# El envoltorio de ventana de los editores (`hikaru-wrapper`)

Módulo aislado en `crates/hikaru_plugin_host/hikaru-wrapper/`. Se declara en
`lib.rs` con `#[path = "../hikaru-wrapper/mod.rs"]` para que el directorio
físico sea ése y no `src/hikaru_wrapper/`: es la carpeta que tiene que poder
auditarse por separado, porque es la que explica por qué el editor de un
plugin no es una ventana del escritorio.

## Qué es

Un *Fruity Wrapper*: la ventana que el plugin abre por su cuenta se convierte
en una ventana **del DAW** —sin decoraciones, fuera del taskbar, atada a la
ventana del DAW— en vez de una aplicación independiente que el gestor de
ventanas baja en cuanto el usuario clickea el DAW.

La API que consume `hikaru_gui`:

| Tipo | Responsabilidad |
|---|---|
| [`HikaruWrapper`] | El ciclo de vida completo: abrir, correr, colocar |
| [`WrapperConfig`] | Qué hints se piden y dónde vive la ventana |
| [`HikaruWrapperWindow`] | La ventana: conexión X11, XID, hints, vida |
| [`WrapperEventLoop`] | El tick de 60 Hz que la mantiene viva |

## Por qué no hay subsurface embedding

El editor de un plugin VST3/CLAP en Linux es una ventana **X11**: JUCE y Qt
usan X11 siempre, y no hay backend Wayland en el VST3 SDK. La sesión del DAW es
**Wayland**, con XWayland al lado.

Un cliente X11 **no se puede empotrar** dentro de una superficie Wayland. No
existe equivalente a `XReparentWindow` entre los dos mundos: XWayland compone
sus clientes como superficies propias, sin relación de árbol con las del
cliente Wayland. Para que el editor fuera un subsurface real habría dos
salidas, y ninguna está al alcance de este módulo:

1. Que el DAW corra bajo X11. GPUI tiene backend X11, así que se podría
   arrancar con `GDK_BACKEND=x11`, y entonces sí: la ventana del editor
   nace como hija de la del DAW en el árbol de X11 y queda dentro. El módulo
   ya tiene ese camino implementado y lo toma solo cuando hay XID del DAW.
2. Que el plugin hable Wayland, lo que depende del plugin, no del host.

Mientras tanto, el envoltorio hace lo que hacen Reaper, Ableton y los
Fruity Wrappers de verdad: que la ventana **parezca** del DAW.

## Cómo se logra: `WrapperStyle::Owned`

Es el modo por defecto y se apoya en tres cosas de X11:

- **`override_redirect`** — la ventana deja de estar gestionada por el gestor
  de ventanas. Sin gestión no hay titlebar, ni bordes, ni botón de cerrar, ni
  entrada en el taskbar, ni alt-tab. El usuario no la mueve ni la redimensiona
  con el ratón: eso es del DAW.
- **`_MOTIF_WM_HINTS`** con las decoraciones en cero, como cinturón para los
  WMs que ignoran `override_redirect` en algunos caminos.
- **Apilado manual.** Como el WM ya no gestiona la ventana, `_NET_WM_STATE_ABOVE`
  deja de existir de hecho, así que el apilado lo hace
  `ConfigureWindow(stack=Above)` en cada tick. Es el motivo por el que
  [`WrapperHintPlan::needs_raising`] devuelve `true` siempre en este modo: acá
  el keep-alive no es un refuerzo, es el único mecanismo.

## Cómo se ubica dentro del DAW: `HostRelation::StrictChild`

"Modal estricta" no es foco bloqueante. El editor toma el foco mientras el
usuario trabaja en él y lo devuelve al DAW cuando sale
(`InputFocus::PARENT`). Es que la ventana del editor está **atada** a la del
DAW:

1. Si el DAW dejó de estar visible (minimizado, tapado por un modal), el
   editor se desmapea. Sin esto el plugin queda flotando solo sobre un
   escritorio donde no hay DAW.
2. Se traduce el cuadro de cliente del DAW a coordenadas de raíz sumándole la
   posición actual del DAW — que puede moverse con el editor abierto.
3. Se limita el cuadro al tamaño del DAW y se aplica, **pero sólo si cambió**:
   un `ConfigureWindow` por tick serían 60 viajes al servidor por ventana para
   no hacer nada.

El DAW dice dónde va el editor con [`HostFrame`], en coordenadas de cliente, y
lo cambia en cualquier momento con
[`HikaruWrapper::set_host_frame`] o [`WrapperCommand::SetHostFrame`].

### El caso Wayland

En Wayland el DAW es una superficie Wayland: no hay XID, así que no hay nada a
lo que colgarse. El cuadro se calcula igual (el DAW lo manda), pero la ventana
del editor es top-level en la raíz de XWayland y su posición se recalcula en
cada tick. El resultado es una ventana sin adornos que sigue al DAW por
coincidencia de coordenadas, no por relación de árbol.

[`HikaruWrapper::is_child_of_daw`] dice en runtime en cuál de los dos casos se
está, y el rack lo muestra para que el estado sea verificable y no una
suposición.

## Cómo está partido

La regla del módulo es que **la política es pura y testeable sin servidor de
display**. Todo lo que se *decide* vive en funciones sin X11; lo único que
habla con el servidor es la ejecución de esas decisiones.

| Fichero | Responsabilidad | ¿Habla con X11? |
|---|---|---|
| `config.rs` | Qué hints se piden | no |
| `input.rs` | Qué eventos se seleccionan, cuándo se activa la ventana | no |
| `keep_alive.rs` | Cada cuánto se re-afirma el stacking | no |
| `x11.rs` | Escribir hints, activar, enfocar, redirigir entrada | sí |
| `frame.rs` | Geometría del marco y hit-test (puro) | no |
| `window.rs` | La ventana: conexión, XID, vida, marco | sí |
| `event_loop.rs` | El tick de 60 Hz | vía `window` |

## Los hints de ventana

Se escriben **antes** del `map`, que es cuando el WM lee `_NET_WM_WINDOW_TYPE`,
`_NET_WM_STATE` y `WM_HINTS` y decide el resto de la vida de la ventana.

1. **`WM_TRANSIENT_FOR`** → la ventana del DAW. El WM la apila con su padre,
   la minimiza con él y le devuelve el foco al cerrarlo. Sólo existe si hay un
   XID de padre: en una sesión **Wayland pura** la ventana del DAW es una
   superficie Wayland y no hay XID que emparentar.
2. **`_NET_WM_STATE_ABOVE`** → siempre encima. Es el único mecanismo que
   funciona en XWayland, así que es el que sostiene el comportamiento cuando
   el DAW corre en Wayland.
3. **`_NET_WM_WINDOW_TYPE_UTILITY`** → tool-window del DAW, no otra
   aplicación: sin entrada propia en el taskbar, sin maximizarse, se cierra
   junto al padre.
4. **`WM_HINTS.input = True`** → la ventana *quiere* teclado.

El 2 se aplica una vez y a partir de ahí manda el WM: cada vez que el usuario
clickea el DAW, el gestor vuelve a subir su ventana y deja el editor detrás.
Por eso `keep_alive.rs` lo re-afirma cada 750 ms con
`ConfigureWindow(stack=Above)`, que **reordena el stacking sin activar la
ventana**. La activación (`_NET_ACTIVE_WINDOW`) ocurre **una sola vez, al
abrir**: hacerlo periódicamente le robaría el foco al DAW mientras el usuario
arrastra un fader.

## El tick

`WrapperEventLoop::run` corre a 60 Hz (`TickBudget`) y en cada tick hace
**siempre** las mismas cinco cosas, en orden:

1. Drena los comandos del rack (`SetParam`, `Close`, `Raise`, `Focus`).
2. Llama a `PluginInstance::process()` **siempre**, no sólo cuando hay audio
   nuevo.
3. Llama a `PluginInstance::gui_tick()`.
4. Drena los eventos de X11, incluida la redirección de entrada.
5. Re-afirma el stacking y devuelve el feedback de parámetros.

El paso 2 es la mitad de por qué el display del editor se movía. Antes sólo se
llamaba `process()` cuando la cinta de audio del motor (`hikaru_plugin_host::tap`)
entregaba un bloque, y esa cinta está muda con el transporte parado. Como
`process()` es lo único que drena el anillo de control de VST3
(`RtControl::set_parameter` sólo encola) y lo único que dispara
`start_processing`, los parámetros del rack se acumulaban sin aplicarse y el
analizador del editor se quedaba en *"Play audio to see the spectrum"*
aunque el audio se oyera perfecto en la cadena.

El paso 3 es la otra mitad, y sólo importa para CLAP: la extensión `timer`
deja que el plugin registre temporizadores y espere que el host los dispare.
Un editor que invalida su display desde `on_timer` no vuelve a pintar nunca si
el host no llama. VST3 no tiene un `idle()` llamado por el host en el SDK que
usa Hikaru, así que ahí la implementación es un no-op deliberado.

## El marco: Hikaru como gestor de ventanas

Con `override_redirect` la ventana sale de la gestión del gestor de ventanas
del escritorio, y con ella se va el marco entero. Eso fue exactamente lo que
se reportó: el plugin dibujaba dentro del DAW pero **pegado como una
pegatina**, sin título, sin bordes y sin forma de moverlo ni cerrarlo.

Para arreglarlo, Hikaru tiene que hacer de gestor de ventanas él mismo. Son
tres piezas:

1. **`frame.rs` — la geometría** (pura, testeable sin display). Dice dónde va
   el editor dentro de la ventana, qué parte del puntero corresponde a qué, y
   qué cursor va en cada zona.
2. **`x11.rs::Paint` — el pintado.** Un `Gcontext` y una fuente del servidor,
   creados una vez por ventana. Rectángulos para los bordes, una "X" de dos
   líneas para el botón de cerrar, tres diagonales para el tirador.
3. **`window.rs` — los gestos.** Arrastrar, redimensionar y cerrar.

### Reparto del puntero

El orden del hit-test **es** la corrección:

| Zona | Qué hace | Quién lo recibe |
|---|---|---|
| Botón de cerrar | Cierra | El marco |
| Título | Arrastra la ventana | El marco |
| Tirador / bordes | Redimensiona | El marco |
| Interior | Lo que sea | **El plugin** |

El botón de cerrar va primero a propósito: vive **dentro** del titlebar, así
que si el título ganara el clic de cerrarla movería la ventana en vez de
cerrarla.

### El borde superior no redimensiona

Es una decisión, no un accidente. Si el borde superior redimensionara, la
franja de título quedaría partida y no se podría mover la ventana arrastrándola
—que es lo único que el usuario haría con ella—. El título arrastra y punto,
igual que en Windows, macOS y el Fruity Wrapper de FL Studio. Se redimensiona
desde abajo, los lados y el tirador.

### El editor se coloca dentro del hueco

El plugin cree que ocupa toda la ventana que se le pasó, así que hay que
moverla y redimensionarla a mano al hueco que deja el marco. Y cuando el DAW
pide un tamaño, la ventana se agranda **por los insets**, para que el editor
reciba lo que pidió y no `pedido − marco`: si no, se descuadra contra su
propio layout.

`vertical_inset()` es `título + borde de abajo`, **no** `título + dos bordes`:
el borde superior vive dentro del titlebar y contarlo dos veces dejaba la
ventana 3 px más alta que el marco que se come. Hay un test que lo fija.

### Fuera de la pantalla del DAW

`clamp_to_host` impide arrastrar el editor hasta media pantalla y dejarlo
pegado al borde con el DAW detrás, que es justo lo que un gestor de ventanas
modal no dejaría.

### Repintar

El marco se repinta en cada `Expose` y en cada cambio de tamaño. Sin eso el
titlebar se borra mientras se arrastra la ventana y queda un rectángulo del
color del escritorio.

### Cuando no hay fuente

El título se dibuja con una fuente del servidor X11 (`fixed`). Si el servidor
no la tiene, el marco se dibuja igual pero sin texto, y el envoltorio avisa
por log. Degradar es preferible a no dibujar el marco entero.

### Plugins que ya traen su propio titlebar

`FrameMetrics::frameless()` los deja sin marco. Envolverlos en otro se ve
doble, y es el caso de varios editores profesionales.

## La entrada

Una ventana incrustada tiene **dos** ventanas: la del envoltorio, que crea el
host, y la del plugin, que crea el editor con su propio toolkit (JUCE, Qt,
web…) y es la que recibe los eventos.

El contenedor se creaba seleccionando `EXPOSURE | KEY_PRESS |
STRUCTURE_NOTIFY`: **ni un bit de puntero**. Tres consecuencias, en orden de
gravedad:

1. No hay forma de recuperar un gesto que la vista hija dejó sin tratar, así
   que `input.rs` selecciona **todo** el puntero
   (`ButtonPress`, `ButtonRelease`, `ButtonMotion`, `MotionNotify`,
   `EnterWindow`, `LeaveWindow`) y `x11.rs` reenvía a la vista lo que caiga
   dentro de ella, traduciendo las coordenadas.
2. Sin `WM_HINTS.input`, hay WMs (KWin entre ellos) que asumen que una
   tool-window no quiere teclado y **no le dan el foco** aunque el usuario la
   clickee. El click se ve pero el control de abajo no entra en modo edición.
3. El foco se repara con `SetInputFocus` **sólo si el puntero está sobre la
   ventana**, para no robarle el foco al DAW mientras el usuario trabaja.

El reenvío sólo se activa como respaldo: una vista que selecciona sus propios
eventos los recibe directamente y nunca pasan por el contenedor. Hay un caso
que hay que tratar con cuidado y es el que fija
`InputDisposition::Native`: si la vista seleccionó `MotionNotify` pero no
`ButtonPress`, los movimientos le llegan igual y suben al contenedor con las
coordenadas **ya en el espacio de la vista**. Reenviarlas las volvería a
desplazar y el puntero aparecería corrido por el offset de la vista.

## Intercambio bidireccional de parámetros

```text
       knob del rack                      editor del plugin
             |                                     ^
             |  WrapperCommand::SetParam           |
             v                                     |
      +--------------------------------------------+
      |              WrapperEventLoop               |
      +--------------------------------------------+
             |                                     ^
             +--> ParamFeedback (Editor u Host) ----+
```

`ParamFeedback::origin` distingue las dos fuentes:

- `Editor` — el plugin lo cambió por su cuenta (el usuario usó su editor).
- `Host` — el rack se lo mandó y el editor lo aceptó.

La distinción existe para que el hilo principal pueda ignorar un eco
desfasado del host sobre un knob que el usuario está arrastrando en ese
momento, y para que el bucle no reenvíe como `Editor` el eco de su propio
`SetParam` (lo que provocaría un ciclo rack → editor → rack en cada knob).

## Apagado

El orden importa y lo fija el `Drop` de `HikaruWrapperWindow`:

1. `detach` — soltar `WM_TRANSIENT_FOR` **antes** de destruir. Sin esto el WM
   conserva una entrada para una ventana que ya no existe, que es la fuga de
   handles clásica de XWayland y se acumula con cada abrir/cerrar.
2. `hide_gui` — el editor se desengancha mientras su padre sigue vivo.
3. `destroy_window`, y la conexión cae al salir de la función, en último lugar.

Las ventanas se crean **sin mapear** y sólo se muestran cuando el editor ya
está embebido, así que un plugin que falla al cargar no deja nunca una
ventana vacía en pantalla.

## Tests

- `crates/hikaru_plugin_host/tests/wrapper_contract.rs` — el contrato que
  consume `hikaru_gui`: nombres, defaults y la política de activación.
- `crates/hikaru_plugin_host/tests/wrapper_pump.rs` — el bombeo del plugin
  con y sin audio, y el ciclo de parámetros en ambos sentidos.
- Tests unitarios junto a cada módulo, todos sin servidor de display.

[`HikaruWrapper`]: ../crates/hikaru_plugin_host/hikaru-wrapper/wrapper.rs
[`WrapperConfig`]: ../crates/hikaru_plugin_host/hikaru-wrapper/config.rs
[`WrapperEventLoop`]: ../crates/hikaru_plugin_host/hikaru-wrapper/event_loop.rs
[`HikaruWrapperWindow`]: ../crates/hikaru_plugin_host/hikaru-wrapper/window.rs
[`HostFrame`]: ../crates/hikaru_plugin_host/hikaru-wrapper/config.rs
[`HikaruWrapper::set_host_frame`]: ../crates/hikaru_plugin_host/hikaru-wrapper/wrapper.rs
[`WrapperCommand::SetHostFrame`]: ../crates/hikaru_plugin_host/hikaru-wrapper/event_loop.rs
[`WrapperHintPlan::needs_raising`]: ../crates/hikaru_plugin_host/hikaru-wrapper/config.rs
[`HikaruWrapper::is_child_of_daw`]: ../crates/hikaru_plugin_host/hikaru-wrapper/wrapper.rs