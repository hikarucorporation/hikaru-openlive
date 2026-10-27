# Cuentas Hikaru: Identidad Local y Metadata en Hikaru OpenLive / OpenStudio

## 1. ¿Qué es una Cuenta Hikaru?
Una **Cuenta Hikaru** es un perfil de usuario local almacenado directamente en la máquina del productor (en formato JSON/TOML liviano dentro de los directorios de configuración del DAW). 

A diferencia del software comercial que te ata a cuentas en la nube obligatorias, las Cuentas Hikaru están diseñadas para funcionar de manera **completamente offline y local**, sirviendo para dos propósitos fundamentales:
1. **Personalización y Configuración:** Guardar preferencias de interfaz, atajos de teclado (*keybindings*), mapeos MIDI y rutas predeterminadas de samples.
2. **Identidad en Colaboración LAN:** Distinguir a los usuarios conectados en una sesión multijugador local o colaboraciones por red local.

---

## 2. Estructura del Perfil Local
Cada cuenta almacena información básica y directa, sin requerir registros en servidores externos:

- `username`: Alias único dentro de la red local (ej. `miko_dsp`, `miku-dayo`).
- `first_name` / `last_name`: Nombre y apellido (opcional, para créditos del proyecto).
- `preferences`: Objeto de configuración local (tema visual, tamaño de buffer predeterminado, etc.).
- `device_signature`: Identificador único de hardware/sesión para evitar conflictos de IP en la LAN.

Ejemplo de estructura interna (`account.json`):
```json
{
  "username": "uchiyamada_kobayashi",
  "first_name": "Kobayashi",
  "last_name": "Uchiyamada",
  "preferences": {
    "theme": "dark_energy_core",
    "default_sample_rate": 44100,
    "buffer_size": 256
  }
}

```

---

## 3. Integración con la Metadata del Proyecto (`.oplf`)

Para que los proyectos colaborativos tengan trazabilidad real, cada modificación guardada en el archivo `.oplf` (JSON) lleva la firma del usuario o agente que la realizó:

* **Autoría de Clips y Pistas:** En la Session Matrix o en la Playlist (OpenStudio), cada clip o patrón MIDI almacena el metadato `created_by: "uchiyamada_kobayashi"`.
* **Historial de Cambios (Delta Tracking):** Cuando un colaborador ajusta una automatización o mueve un clip, el delta enviado por socket LAN incluye la firma de su Cuenta Hikaru, permitiendo ver quién metió mano en qué parte del arreglo.

---

## 4. Uso en el Modo Multijugador LAN

Cuando se inicia una sesión local en red:

1. El **Host** publica la lista de participantes activos basada en las Cuentas Hikaru conectadas.
2. Cada participante ve en pantalla el cursor o el indicador de color asignado a la cuenta del otro usuario (ej. *Kobayashi está editando el DSP Rack de la Pista 2*).
3. Si un **Agente MCP** se conecta a la sesión, se le asigna un perfil especial de cuenta (ej. `agent:hikaru_mcp_assistant`) para que sus sugerencias queden debidamente registradas en el historial del proyecto sin pisar las decisiones del productor humano.
