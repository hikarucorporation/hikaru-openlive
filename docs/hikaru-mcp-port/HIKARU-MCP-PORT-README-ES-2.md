## 1.1 Mapeo Arquitectural (Análoga al Modelo "USB de la IA")

Siguiendo el estándar abierto del Model Context Protocol (MCP), la topología de Hikaru OpenLive se divide en tres capas bien definidas:


```

+-------------------------------------------------------+
|                      MCP HOST                         |
|         (OpenCode / Claude Code / Agente CLI)         |
+-------------------------------------------------------+
                           |
           ( Estándar MCP  /  JSON-RPC  )
                           v
+-------------------------------------------------------+
|                      MCP SERVER                       |
|         (`hikaru_mcp` + Plugins VST3/CLAP / REA)      |
|  * Funciona como un "Hub USB" de instrumentación DSP  |
+-------------------------------------------------------+

```

- **El Host (OpenCode):** Orquesta las peticiones, interpreta el código y toma decisiones de ingeniería inversa.
- **El Servidor (Hikaru OpenLive + Stubs de Plugins):** Expone los "endpoints" y recursos del DAW y de los plugins propietarios (UZU, Serum, etc.) como si fueran múltiples dispositivos conectados a un puerto universal, permitiendo inyectar telemetría y extraer código legible en Rust sin fricciones
