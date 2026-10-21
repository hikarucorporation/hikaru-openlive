# Patches locales sobre `gpui-pre` 0.3.6

Requisito de la Apache-2.0 seccion 4(b): "You must cause any modified files
to carry prominent notices stating that You changed the files" + declarar los
cambios.

Estado: el codigo upstream de Zed Industries sigue bajo **Apache-2.0** (grant
irrevocable, `LICENSE-APACHE` intacto). Este fork se ofrece bajo
**LGPL-3.0-or-later** por sublicencia (Apache-2.0 seccion 2). Ver `NOTICE`
para el detalle legal.

## Diff contra el upstream

Comparado contra el `.crate` publicado en crates.io (`gpui-pre` 0.3.6,
snapshot de `zed@bcf6582`):

```
src/platform.rs   | 14 ++++++++++++++
1 file changed, 14 insertions(+)
```

## Inventario

### `src/platform.rs` — nueva variante `CursorStyle::Hidden`

Agrega la variante `Hidden` al enum `CursorStyle`, con doc comment que
explica el uso y las dos implementaciones por backend.

- Es un enum **cerrado** y todos los `match` sobre el son exhaustivos, asi que
  agregar la variante obliga a tocar cada backend que lo matchea. Por eso el
  crate se vendoriza en vez de depender de crates.io: en `gpui-pre-linux`
  estan los brazos correspondientes (ver su `PATCHES.md`).

Motivo: poder ocultar el puntero durante gestos continuos de arrastre
(middle-drag de la Playlist), donde un cursor visible distrae y compite con
el feedback en pantalla.

Restricciones que el patch deja documentadas:

- En Wayland se implementa con el mecanismo del protocolo core
  `wl_pointer.set_cursor(serial, None, 0, 0)`.
- En X11 se resuelve con el cursor vacio ya cacheado por el backend.
- **No** usar `Platform::hide_cursor_until_mouse_moves()` para esto: ademas de
  ser inaccesible desde la app, restaura el cursor en cada evento `Motion`,
  justo lo contrario de lo que hace falta al arrastrar.

## Nota al actualizar gpui-pre

Si sube la version de `gpui-pre`, hay que volver a aplicar este parche sobre
las fuentes nuevas y regenerar este archivo (diff contra el `.crate` nuevo).
El parche vive en `src/platform.rs`; los brazos por backend viven en
`gpui-pre-linux-patched`.
