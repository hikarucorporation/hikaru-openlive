
# Hikaru OpenSpectral Phaser — Layout de Interfaz & DSP Rack Card

Adaptación soberana de la interfaz del UZU al sistema de tarjetas nativas en GPUI / DSP Rack de Hikaru OpenLive.

## 📐 ASCII Layout: Tarjeta del DSP Rack (Expandida)

```

┌────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────┐
│ [●] 01 Hikaru OpenSpectral Phaser                                                                              [ ▼ ]   │
├───────────────────────────────────────────────────────┬────────────────────────────────────────────────────────────────┤
│                                                       │ FREQ / CUTOFF (Hz / Oct)                                       │
│                                                       │                                                                │     
│                                                       │                      [ 1x | 5x ] | [ Keep Bass ] | [ Bypass ]  │   │                                                       │                                                                │
│          [ DISPLAY / FFT SCOPE VIEW ]                 │ [ HZ ] ◄════════════════[ O ]═══════════════════ [ OCT ]       │
│                                                       ├────────────────────────────────────────────────────────────────┤
│       ~._     _.·´¯`·.¸_    .-¯-.     _.._            │ [ Bypass: Off ]                    [ Blocksize: 1024 ]         │
│          `·.,·´          `·.,·     `·.,·    `·.,·     ├────────────────────────────────────────────────────────────────┤
│                                                       │ CONTROLES DE DSP & MODULACIÓN (Perillas & Knobs)               │
│       (Render espectral en tiempo real                │                                                                │
│        mapeando los biquads en cascada)               │    ┌─────────┐     ┌─────────┐     ┌─────────┐                 │
│                                                       │    │   (O)   │     │   (O)   │     │   (O)   │                 │
│                                                       │    │  Width  │     │ Offset  │     │  Depth  │                 │
│                                                       │    │  0.50   │     │  0.00   │     │  0.00   │                 │
│                                                       │    └─────────┘     └─────────┘     └─────────┘                 │
│                                                       │                                                                │
│                                                       │    ┌─────────┐     ┌─────────┐     ┌─────────┐                 │
│                                                       │    │   (O)   │     │   (O)   │     │   (O)   │                 │
│                                                       │    │  Speed  │     │   Mix   │     │  Blur   │                 │
│                                                       │    │  0.50   │     │  1.00   │     │  0.00   │                 │
│                                                       │    └─────────┘     └─────────┘     └─────────┘                 │
├───────────────────────────────────────────────────────┴────────────────────────────────────────────────────────────────┤
│ PARÁMETROS (11) • Bloque nativo Rust + GPUI Kit                                                             [ Editor ] │
└────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────┘

```

## 🎛️ Mapeo de Parámetros (Struct en Rust)

| # | ID VST3 / Interno | Nombre UI | Tipo | Rango / Opciones | Default | Descripción del DSP |
|---|---|---|---|---|---|---|
| 0 | `width` | Width | Knob (Normal) | 0..100 | 50.0 | Ancho estéreo / dispersión de fase |
| 1 | `offset` | Offset | Knob (Central / Bipolar) | -100..+100 | 0.0 | Desplazamiento de polos en cascada |
| 2 | `depth` | Depth | Knob (Normal) | 0..100 | 0.0 | Profundidad de modulación del phaser |
| 3 | `speed` | Speed | Knob (Central / Bipolar) | -100..+100 | 0.0 | Velocidad del LFO interno |
| 4 | `mix` | Mix | Knob (Normal) | 0..100 | 100.0 | Dry/Wet balance |
| 5 | `blur` | Blur | Knob (Normal) | 0..100 | 0.0 | Difuminado espectral / suavizado FFT |
| 6 | `hz_oct` | Hz/Oct | Knob (Central / Bipolar) | -100..+100 | 0.0 | Frecuencia central / mapeo de octavas |
| 7 | `rate_multiplier` | 1x/5x | Toggle | 0 (Off) / 1 (On) | 0 | Multiplicador de escala temporal para la velocidad |
| 8 | `keep_bass` | Keep Bass | Toggle | 0 (Off) / 1 (On) | 0 | Protección de frecuencias graves (preserva sub-bajos) |
| 9 | `blocksize` | Blocksize | Multiselect | 512, 1024, 2048, 4096, 8192, 16384 | 1024 | Resolución de ventana FFT para el espectro |
| 10 | `bypass` | Bypass | Toggle | 0 (Off) / 1 (On) | 0 | Paso directo de señal (apaga/prende el efecto) |