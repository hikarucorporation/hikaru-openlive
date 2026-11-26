# Paridad con Bitwig Studio 5.3.13

Este directorio es la **fuente de verdad** del gráfico de paridad de Hikaru
OpenLive frente a **Bitwig Studio 5.3.13** (revisión `161983`). Todo lo que
alimenta el SVG vive acá: el catálogo de features, el generador y la evidencia
de la auditoría.

## Archivos

| Archivo | Qué es |
|---|---|
| [`features_bitwig.json`](features_bitwig.json) | Base de datos de paridad: 19 áreas y 692 features con estado (`done`, `in_progress`, `planned`), evidencia de código y bugs abiertos. |
| [`generate_status.py`](generate_status.py) | Generador autocontenido (sólo stdlib de Python). Lee el JSON y produce `status.svg` y `parity-detail.svg`; refresca el `summary` del JSON y la tabla de paridad de la auditoría para que nunca se desincronicen. |
| [`status.svg`](status.svg) | Squarified treemap oscuro de 680×480, con leyenda, porcentaje global, barras de estado y badges de bugs. Es el archivo que muestra el `README.md` raíz. |
| [`parity-detail.svg`](parity-detail.svg) | Póster de 1400 px de ancho con las 692 features nombradas, agrupadas por área y ordenadas **en progreso → hecho → pendiente**, con barra por área, resumen superior y pie con la fuente. Se abre a tamaño completo desde el treemap. |
| [`bitwig-5.3.13-parity-audit.md`](bitwig-5.3.13-parity-audit.md) | Auditoría completa: instalación local de Bitwig, catálogo de 150 dispositivos, 43 moduladores y 228 módulos de Grid, crates de Hikaru y metodología. |
| `README.md` | Este índice. |

## Estado actual

- **329 / 692** features `done` → **48 % de paridad**.
- **16** en `in_progress`.
- **347** `planned`.
- **1** bug abierto ([issue #1](https://github.com/hikarucorporation/hikaru-openlive/issues/1), mapeado a `Build & Platform`).
- El detalle feature por feature está en [`parity-detail.svg`](parity-detail.svg): 692 chips con nombre, estado y evidencia.

## Regenerar

Desde cualquier directorio:

```bash
python3 docs/bitwig-parity/generate_status.py
```

El script resuelve `features_bitwig.json`, `status.svg`, `parity-detail.svg` y
`bitwig-5.3.13-parity-audit.md` relativos a su propia ubicación, así que no
depende del `cwd`. También se pueden pasar rutas explícitas:

```bash
python3 docs/bitwig-parity/generate_status.py ruta/features.json ruta/status.svg ruta/parity-detail.svg ruta/audit.md
```

Para verificar que el JSON, la auditoría y ambos SVG están al día sin escribir
nada:

```bash
python3 docs/bitwig-parity/generate_status.py --check
```

### Modo watch (mientras editás el catálogo)

Para que las cifras y el póster se actualicen solos en cada guardado del JSON:

```bash
python3 docs/bitwig-parity/generate_status.py --watch
```

Queda corriendo, detecta el cambio de `features_bitwig.json` y regenera el
`summary`, los dos SVG y la tabla de la auditoría en ~150 ms. Imprime una línea
por actualización con el porcentaje y los conteos. `--interval=0.5` ajusta el
sondeo (por defecto 0.25 s). Si el editor deja el JSON a medio escribir, avisa
y reintenta en el siguiente guardado. Se corta con `Ctrl+C`.

> **Ojo:** el porcentaje sale de los `status` del JSON. Copiar menús en
> `crates/hikaru_gui` no mueve la paridad por sí solo: hay que marcar la feature
> como `done` y agregarle `evidence` (o pasar la a `in_progress`). El watch
> acelera el reflejo, no inventa el estado.

## Editar la base de paridad

Cada área tiene esta forma:

```json
{
  "id": "clip-launcher",
  "name": "Clip Launcher",
  "bitwig_reference": "Clip Launcher, scenes, clip slots and launch quantization (Bitwig Studio 5.3.13).",
  "bugs": 0,
  "features": [
    {
      "id": "cl-01",
      "name": "Session matrix grid (tracks x scenes)",
      "status": "done",
      "evidence": "crates/hikaru_gui/src/views/matrix.rs"
    }
  ]
}
```

- `status` sólo admite `done`, `in_progress` o `planned`.
- `evidence` es una ruta relativa a la raíz del repo; apunta al código que
  respalda la feature.
- `bugs` es el número de issues abiertos de esa área.
- Después de editar, correr el generador: recalcula el bloque `summary`, la
  tabla de la auditoría y redibuja `status.svg` y `parity-detail.svg`.

## Automatización

El workflow [`.github/workflows/update-status.yml`](../../.github/workflows/update-status.yml)
se ejecuta en cada push a `main` (y manualmente con `workflow_dispatch`). Corre
el generador, valida el resultado y commitea `status.svg`, `parity-detail.svg`,
`bitwig-5.3.13-parity-audit.md` + `features_bitwig.json` si cambiaron.

## Criterio de medición

El porcentaje es `done / total` sobre las features rastreadas, y el área de
cada tile es proporcional a la cantidad de features del área. Para que las
áreas chicas (como `Automation`, con 4 features) se puedan leer, el treemap
aplica un piso de legibilidad de 22 features por tile; ese piso sólo cambia el
tamaño dibujado, nunca los conteos ni el porcentaje. Los 118
dispositivos y los 43 moduladores de Bitwig se cuentan uno por uno; las
equivalencias con dispositivos de Hikaru son funcionales, no binarias. En el
póster, cada chip lleva el nombre completo y la ruta de `evidence` en su
`<title>`, y los grupos de estado se ordenan en progreso → hecho → pendiente.
Los detalles y límites están en
[`bitwig-5.3.13-parity-audit.md`](bitwig-5.3.13-parity-audit.md).
