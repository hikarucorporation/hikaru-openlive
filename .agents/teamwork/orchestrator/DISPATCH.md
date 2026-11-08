# Dispatch Log

## 2026-11-07T23:57:04Z

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
- R1. Integración de MIDI Clips (creación, organización, disparo y loop en Arranger y Matrix, paridad 1 a 1 Bitwig).
- R2. Editor Piano Roll 1 a 1 estilo Bitwig (egui + wgpu + wgsl/naga con assets Lemon Studio en `crates/hikaru_gui`, edición de notas, pitch, tiempo, duración, velocity lane, grid snapping, zoom/pan H y V).
- R3. Integración con la Arquitectura del Motor y Secuenciador (estructuras de datos MIDI, secuenciador, motor de reproducción, persistencia, sin regresiones de audio).
