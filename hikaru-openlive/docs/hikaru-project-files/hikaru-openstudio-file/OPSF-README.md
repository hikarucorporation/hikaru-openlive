## Hikaru OpenStudio File (`.opsf`) - Documentación.

El formato `.opsf` es el esquema de archivo de proyecto de Hikaru OpenStudio. El archivo `.opsf` contiene la estructura lineal de la Playlist/Timeline para la producción y arreglo de estudio (equivalente a FL Studio o REAPER), manejando eventos basados en ticks y control de transporte estándar.

# Ejemplo de Archivo (`.opsf`) en Windows 10 21H2 (x64) con Hikaru OpenStudio 2.22.2

```powershell
PS C:\Users\Tomoyo Sakurai\Hikaru Corporation\Hikaru OpenLive

DIR C:\Users\Tomoyo Sakurai\Hikaru Corporation\Hikaru OpenLive

hikaru_openlive new my_project.opsf

```

```json

/* 
==========================================================================================
  Hikaru OpenLive CLI | Hikaru Corporation (C) 2026 | GNU AGPLv3
  Hikaru OpenStudio File Format (.opsf) | GNU Lesser General Public License v3.0 (LGPLv3)
==========================================================================================

  --------------------------------------------

  Hikaru OpenLive CLI Version: 2.22.2
  Hikaru OpenStudio File Format Version: 1.0

  --------------------------------------------

  Shrine Host OS: "Windows 10 21H2 (x64)"
  Shrine Host Kernel Version: 10.0.19044
  Shrine Host Kernel Architecture: x86_64
  Shrine Host CPU: Intel(R) Core(TM) i3-2500K CPU @ 3.30GHz
  Shrine Host CPU Cores: 2 | Threads: 4
  Shrine Host GPU: Intel(R) UHD Graphics 630
  Shrine Host GPU Memory: 4 GB

  --------------------------------------------

  Hikaru VST3/CLAP Host Version: 2.22.2

  --------------------------------------------

  File Name: "Tomoyo Sakurai - Yakumo's Squizofrenia.opsf"
  Artist(s): "Tomoyo Sakurai"
  Genre: Future Bass / Melodic Dubstep / J-EDM
  BPM: 160
  Tonal: B Minor
  Time Signature: 4/4
  Launch Quantization: 1/1
  Tap Tempo Enabled: true
  Scenes Number: 160
  Tracks Number: 20

  --------------------------------------------

  ============================================
  VST3 Devices in Total Project: 15 
  ============================================
  
  {
    "Serum 1" : x10,
    "OTT" : x15,
    "Chroma" : x20,
    "Vital" : x15,
  }

  ============================================
  CLAP Devices in Total Project: 15
  ============================================

  {
    "LSP Compressor" : x10,
    "LSP Reverb" : x15,
    "LSP EQ" : x20,
    "LSP Delay" : x15,
  }

  ============================================
  Hikaru Native Plugins in Total Project: 15
  ============================================
  
  {
    "Hikaru OpenWavetable" : x10,
    "Hikaru OpenDMS" : x45,
    "Hikaru OpenModulation" : x15,
  }

  ============================================
  Audio Clips in Total Project: 50
  MIDI Clips in Total Project: 15
  ============================================

  --------------------------------------------

  ============================================
  Tracks in Total Project: 20
  ============================================

  {
    "Track 1" : ("Drum Loop 160BPM"),
    "Track 2" : (Future Bass Drop Loop - 160BPM - B minor),
    "Track 3" : ("Hihat Pattern 01 - 160BPM.mid"),
    "Track 4" : ("FX 160BPM"),
    "Track 5" : ("Buildup Snare Roll Loop 160BPM"),
    "Track 6" : ("FX 160BPM"),
    "Track 7" : ("Dubstep Drum Loop 160BPM"),
    "Track 8" : ("Dubstep Bass Drop Loop - 160BPM - B minor"),
    "Track 9" : ("FX 160BPM"),
    "Track 10" : ("Hihat Pattern 02 - 160BPM.mid"),
  }

  --------------------------------------------

*/

{
  "$schema": "hikaru-openstudio-file/opsf-v1.json",
  "format_version": 1,
  "created_by": "Hikaru OpenStudio 2.22.2",
  "engine_mode": "OpenStudio",
  "metadata": {
    "title": "Tomoyo Sakurai - Yakumo's Squizofrenia",
    "artist": "Tomoyo Sakurai",
    "genre": "Future Bass / Melodic Dubstep / J-EDM",
    "key": "B Minor",
    "stats": {
      "total_scenes": 160,
      "total_tracks": 20,
      "audio_clips": 50,
      "midi_clips": 15
    }
  },
  "transport": {
    "bpm": 160.0,
    "beats_per_bar": 4,
    "beat_division": 4,
    "loop_start_ticks": 0,
    "loop_end_ticks": 30720
  },
  "playlist": {
    "ppqn": 96,
    "clips": [
      {
        "idx": 1,
        "clip": {
          "type": "audio",
          "name": "Drum Loop 160BPM",
          "source": "Drum Loop 160BPM.wav",
          "start_tick": 0,
          "duration_ticks": 3072,
          "muted": false,
          "solo": false
        }
      },
      {
        "idx": 2,
        "clip": {
          "type": "audio",
          "name": "Future Bass Drop Loop",
          "source": "Future Bass Drop Loop - 160BPM - B minor.wav",
          "start_tick": 3072,
          "duration_ticks": 6144,
          "muted": false,
          "solo": false
        }
      },
      {
        "idx": 3,
        "clip": {
          "type": "midi",
          "name": "Hihat Pattern 01",
          "source": "Hihat Pattern 01 - 160BPM.mid",
          "start_tick": 0,
          "duration_ticks": 3072,
          "muted": false,
          "solo": false
        }
      }
    ]
  }
}

```