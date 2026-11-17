# Paridad con Bitwig Studio 5.1.7

Este directorio es la **fuente de verdad** del gráfico de paridad de Hikaru
OpenLive frente a **Bitwig Studio 5.1.7** (revisión `143754`). Todo lo que
alimenta el SVG vive acá: el catálogo de features, el generador y la evidencia
de la auditoría.

## Archivos

| Archivo | Qué es |
|---|---|
| [`features_bitwig.json`](features_bitwig.json) | Base de datos de paridad: 12 áreas y 398 features con estado (`done`, `in_progress`, `planned`), evidencia de código y bugs abiertos. |
| [`generate_status.py`](generate_status.py) | Generador autocontenido (sólo stdlib de Python). Lee el JSON y produce `status.svg`; refresca el `summary` del JSON para que nunca se desincronicen. |
| [`status.svg`](status.svg) | Squarified treemap oscuro de 680×480, con leyenda, porcentaje global, barras de estado y badges de bugs. Es el archivo que muestra el `README.md` raíz. |
| [`bitwig-5.1.7-parity-audit.md`](bitwig-5.1.7-parity-audit.md) | Auditoría completa: instalación local de Bitwig, catálogo de 118 dispositivos y 43 moduladores, crates de Hikaru y metodología. |
| `README.md` | Este índice. |

## Estado actual

- **148 / 398** features `done` → **37 % de paridad**.
- **23** en `in_progress`.
- **227** `planned`.
- **1** bug abierto ([issue #1](https://github.com/hikarucorporation/hikaru-openlive/issues/1), mapeado a `Build & Platform`).

## Regenerar

Desde cualquier directorio:

```bash
python3 docs/bitwig-parity/generate_status.py
```

El script resuelve `features_bitwig.json` y `status.svg` relativos a su propia
ubicación, así que no depende del `cwd`. También se pueden pasar rutas
explícitas:

```bash
python3 docs/bitwig-parity/generate_status.py ruta/features.json ruta/status.svg
```

Para verificar que el JSON y el SVG están al día sin escribir nada:

```bash
python3 docs/bitwig-parity/generate_status.py --check
```

## Editar la base de paridad

Cada área tiene esta forma:

```json
{
  "id": "clip-launcher",
  "name": "Clip Launcher",
  "bitwig_reference": "Clip Launcher, scenes, clip slots and launch quantization (Bitwig Studio 5.1.7).",
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
- Después de editar, correr el generador: recalcula el bloque `summary` y
  redibuja `status.svg`.

## Automatización

El workflow [`.github/workflows/update-status.yml`](../../.github/workflows/update-status.yml)
se ejecuta en cada push a `main` (y manualmente con `workflow_dispatch`). Corre
el generador, valida el resultado y commitea `status.svg` +
`features_bitwig.json` si cambiaron.

## Criterio de medición

El porcentaje es `done / total` sobre las features rastreadas, y el área de
cada tile es proporcional a la cantidad de features del área. Los 118
dispositivos y los 43 moduladores de Bitwig se cuentan uno por uno; las
equivalencias con dispositivos de Hikaru son funcionales, no binarias. Los
detalles y límites están en
[`bitwig-5.1.7-parity-audit.md`](bitwig-5.1.7-parity-audit.md).
