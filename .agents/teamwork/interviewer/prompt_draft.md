Implementar MIDI Clips y un Piano Roll al estilo Bitwig (copia 1 a 1) para producción en Hikaru OpenLive, utilizando los assets 3D / modo inmediato en egui + wgpu + wgsl/naga de Lemon Studio.

Working directory: /home/alex/Documentos/Codigo_Fuente/hikaru-workspace/hikaru-openlive
Integrity mode: development

## Material de Referencia / Reference Material
- Video de referencia: `/home/alex/Documentos/Codigo_Fuente/hikaru-workspace/hikaru-openlive/.opencode/references/bitwig_5.1.7/screenshots/video/bitwig-comportamiento-piano-roll-1.mp4`
- Guías de ingeniería inversa y diseño frontend:
  - `.agents/skills/reverse-engineer-anything/SKILL.md`
  - `.agents/skills/frontend-design/SKILL.md`
  - `.agents/skills/impeccable/SKILL.md`

## Requirements / Requisitos

### R1. Integración de MIDI Clips
Implementar MIDI Clips para producción dentro de Hikaru OpenLive que puedan crearse, organizarse, dispararse y loopearse tanto en la vista Arranger como en la vista Matrix, manteniendo paridad visual y de comportamiento 1 a 1 con el flujo de trabajo de Bitwig Studio demostrado en `bitwig-comportamiento-piano-roll-1.mp4`.

### R2. Editor Piano Roll 1 a 1 estilo Bitwig
Implementar la vista del Piano Roll en modo inmediato utilizando egui + wgpu + wgsl/naga, integrando los assets de Lemon Studio presentes en `crates/hikaru_gui`. El editor debe soportar creación, selección, desplazamiento, redimensionamiento de duración, edición de velocidad de notas, ajuste magnético a la grilla (grid snapping) y navegación fluida por la línea de tiempo (zoom y paneo horizontal y vertical), tal como se muestra en el video de referencia.

### R3. Integración con la Arquitectura del Motor y Secuenciador
Integrar las estructuras de datos de MIDI clips y eventos de notas del piano roll con el secuenciador, el motor de reproducción y la persistencia de proyectos de Hikaru OpenLive, garantizando que no haya regresiones ni roturas en las funcionalidades de audio existentes.

## Verification Resources / Recursos de Verificación
- Video de referencia: `/home/alex/Documentos/Codigo_Fuente/hikaru-workspace/hikaru-openlive/.opencode/references/bitwig_5.1.7/screenshots/video/bitwig-comportamiento-piano-roll-1.mp4`
- Pruebas existentes en el workspace: `cargo test --workspace` y `cargo check --workspace`

## Acceptance Criteria / Criterios de Aceptación

### Pruebas Automatizadas y Compilación
- [ ] El código compila limpiamente sin errores en todos los crates del workspace ejecutando `cargo check --workspace`.
- [ ] La suite de pruebas del workspace pasa exitosamente con `cargo test --workspace`.
- [ ] Se añaden pruebas unitarias y de integración que cubren la creación de clips MIDI, serialización/deserialización y manipulación de eventos de notas (insertar, borrar, mover, redimensionar).

### Paridad Funcional y Visual (Referencia de Video)
- [ ] Al abrir un clip MIDI se muestra el editor Piano Roll en modo inmediato cargado con las notas correspondientes a dicho clip.
- [ ] Las notas en el piano roll se pueden crear, seleccionar, arrastrar/mover en tono (pitch) y tiempo, cambiar de longitud (redimensionar) y eliminar.
- [ ] El carril de velocidad (velocity lane) muestra con precisión el valor de velocidad de cada nota y permite su edición interactiva.
- [ ] La navegación en la línea de tiempo (zoom y paneo/scroll horizontal y vertical) y el ajuste a la grilla responden exactamente como se demuestra en el video `bitwig-comportamiento-piano-roll-1.mp4`.
- [ ] El renderizado en modo inmediato / 3D se ejecuta a través de egui + wgpu + shaders wgsl integrando los assets de Lemon Studio en `crates/hikaru_gui`.
