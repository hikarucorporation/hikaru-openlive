# Patches locales sobre `gpui-pre-linux` 0.3.6

Requisito de la Apache-2.0 seccion 4(b): "You must cause any modified files
to carry prominent notices stating that You changed the files" + declarar los
cambios.

Estado: el codigo upstream de Zed Industries sigue bajo **Apache-2.0** (grant
irrevocable, `LICENSE-APACHE` intacto). Este fork se ofrece bajo
**LGPL-3.0-or-later** por sublicencia (Apache-2.0 seccion 2). Ver `NOTICE`
para el detalle legal.

## Diff contra el upstream

Comparado contra el `.crate` publicado en crates.io (`gpui-pre-linux` 0.3.6,
snapshot de `zed@bcf6582`):

```
src/linux/platform.rs          |  4 ++
src/linux/wayland.rs           |  6 +++
src/linux/wayland/client.rs    | 33 +++++++++++++++++
src/linux/x11/client.rs        |  7 +++
4 files changed, 50 insertions(+), 1 deletion(-)
```

## Inventario

Este crate implementa los brazos por backend de `CursorStyle::Hidden`, la
variante que se agrega en `gpui-pre-patched/src/platform.rs`. Como
`CursorStyle` es un enum cerrado y los `match` son exhaustivos, cada backend
que lo matchea necesita su brazo o la compilacion falla.

### `src/linux/wayland.rs` — rama `to_shape()`

`wp_cursor_shape_v1` no tiene forma "hidden" (el enum del protocolo es fijo),
asi que el brazo devuelve `Shape::Default`. El brazo existe solo para que el
match siga siendo exhaustivo: el cursor oculto real se emite por
`wl_pointer.set_cursor(serial, None, ..)` en `client.rs`.

### `src/linux/platform.rs` — rama `cursor_style_to_icon_names()`

En X11 el cursor oculto se resuelve en `X11ClientState::get_cursor_icon`,
que devuelve el cursor vacio ya cacheado; esta lista vacia nunca llega a
recorrerse.

### `src/linux/x11/client.rs` — `get_cursor_icon()`

El cursor oculto reusa el cursor vacio que el backend ya crea para
`hide_cursor_until_mouse_moves`. Es el mismo objeto X11, asi que no hace falta
construir uno nuevo.

### `src/linux/wayland/client.rs` — tres puntos

1. `set_cursor_style()`: emite el cursor oculto por el mecanismo del protocolo
   core `wl_pointer.set_cursor` con buffer `None`, que es como Wayland
   representa "sin cursor". Va **antes** de la rama de `cursor-shape-v1`,
   porque ese protocolo no tiene forma oculta.

2. `restore_cursor_after_hide()`: si el estilo vigente es "oculto", restaurar
   significa volver a ocultar. Sin esta rama, un
   `hide_cursor_until_mouse_moves` previo (p. ej. al escribir en un input de
   texto) dejaria el puntero visible al mover el mouse.

3. `MouseEnter` (re-entrada a la ventana): re-emite el cursor oculto. Sin
   esto, re-entrar a la ventana durante un gesto con cursor oculto lo volveria
   visible.

## Nota al actualizar gpui-pre-linux

Si sube la version de `gpui-pre-linux`, hay que volver a aplicar este parche
sobre las fuentes nuevas y regenerar este archivo (diff contra el `.crate`
nuevo). Ademas hay que re-aplicar el parche de `gpui-pre-patched` (la variante
del enum) antes de compilar, porque los dos crates dependen uno del otro.
