## El enfoque correcto: Tarjetas Fijas Horizontales:
En un DAW profesional, el DSP Rack es una sola tira horizontal fija (por ejemplo, de 200px a 240px de altura total) donde cada dispositivo es un módulo/tarjeta contiguo:

---

```
Session Matrix:

--------------------------------------------------------------------------------------------------
DSP Rack : [ Track 1 | Scene 1 ]

----------------[--------------------------] [------------------------] [-------------------------
                │ [ 01 OpenWavetable  [ON] ] │ [ 02 OpenEQ3      [ON] ] │ 
                │ ┌────────────────────────┐ │ ┌──────────────────────┐ │
    [ + Slot ]  │ │  [ VISOR 3D WGPU ]     │ │ │  [ Low | Mid | Hi ]  │ │   [ + ]
                │ └────────────────────────┘ │ └──────────────────────┘ │
                │  (O) WT POS   < 113/256 >  │  (O) Gain   (O) Freq     │
----------------[--------------------------] [------------------------] [-------------------------         


```


```
Arranger View:

----------------------------------------------------------------------------------------------
DSP Rack : [ Scene 1 | Track 1 ]

----------------[--------------------------] [------------------------] [-------------------------
                │ [ 01 OpenWavetable  [ON] ] │ [ 02 OpenEQ3      [ON] ] │ 
                │ ┌────────────────────────┐ │ ┌──────────────────────┐ │
    [ + Slot ]  │ │  [ VISOR 3D WGPU ]     │ │ │  [ Low | Mid | Hi ]  │ │   [ + ]
                │ └────────────────────────┘ │ └──────────────────────┘ │
                │  (O) WT POS   < 113/256 >  │  (O) Gain   (O) Freq     │
----------------[--------------------------] [------------------------] [-------------------------
```
---

Estas tarjetas se pueden mover con el mouse, haciendo click con ellas y arrastrandolas al lugar que quieras dentro del **DSP Rack**.

En caso de querer removerl las tarjetas seleccionadas, hacés click a una de ellas y simplemente oprimís **`"Suprimir"`** y listo. Lo mismo para copiar/pegar aunque diferente:

```
Arranger View:

----------------------------------------------------------------------------------------------
DSP Rack : [ Scene 1 | Track 1 ]

----------------[--------------------------]      [------------------------] [-------------------------
                │ [ 01 OpenWavetable  [ON]        │ [ 02 OpenEQ3      [ON] ] │ 
                │ ┌────────────────────────┐      │ ┌──────────────────────┐ │
    [ + Slot ]  │ │  [ VISOR 3D WGPU ]     │  ➕  │ │  [ Low | Mid | Hi ]  │ │   [ + ]
                │ └────────────────────────┘      │ └──────────────────────┘ │
                │  (O) WT POS   < 113/256 >       │  (O) Gain   (O) Freq     │
----------------[--------------------------]      [------------------------] [-------------------------
                                             
                                              ⬆️

                                [`*lugar o casillero a ser copiado*`]

```
---

En caso de abrir el **Hikaru OpenLive** con un proyecto completamente **vacío**, el **DSP Rack** se vé así

```
Session Matrix:

--------------------------------------------------------------------------------------------------
DSP Rack : [ Track 1 | Scene 1 ]

--------------------------------------------------------------------------------------------------
                │ 
                │
    [ + Slot ]  │ Inserte sus dispositivos acá.
                │
                │
--------------------------------------------------------------------------------------------------     


```

En caso de presionar el botón de `[ + ]` con **Click Izquierdo o Derecho**

```
Session Matrix:

--------------------------------------------------------------------------------------------------
DSP Rack : [ Track 1 | Scene 1 ]

--------------------------------------------------------------------------------------------------
                │                               Efectos: 
                │                               1. OpenEQ3
    [ + Slot ]  │ **(-> Menú Conceptual ->)**   Generadores:
                │                               1. Hikaru OpenWavetable
                │
--------------------------------------------------------------------------------------------------     


```