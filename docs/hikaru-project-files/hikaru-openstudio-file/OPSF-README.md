# Hikaru OpenStudio File (`.opsf`) - Documentación.

El formato `.opsf` es el esquema de archivo de proyecto de Hikaru OpenStudio. El archivo `.opsf` contiene la estructura de la sesión de rendimiento y la información de configuración del modo de reproducción de la Playlist/Timeline.

La Playlist/Timeline es el equivalente al FL Studio o REAPER, pero con la diferencia de que en ambos casos el modo de reproducción es un clip de audio, mientras que en Hikaru OpenStudio el modo de reproducción es una sesión de rendimiento.

```powershell
PS C:\Users\Tomoyo Sakurai\Hikaru Corporation\Hikaru OpenLive
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
  "format_version": "1.0.0",
  "engine": {
    "mode": "OpenStudio",
    "version": "2.22.2",
    "license": "GNU AGPLv3"
  },
  "metadata": {
    "title": "Tomoyo Sakurai - Yakumo's Squizofrenia",
    "artist": "Tomoyo Sakurai",
    "genre": "Future Bass / Melodic Dubstep / J-EDM",
    "created_at": "2026-10-09T20:52:00Z",
    "modified_at": "2026-10-09T20:52:00Z"
  },
  "transport": {
    "bpm": 160.0,
    "time_signature": {
      "numerator": 4,
      "denominator": 4
    },
    "key_signature": "B Minor",
    "launch_quantization": "1/1",
    "tap_tempo_enabled": true
  },
  "stats": {
    "total_scenes": 160,
    "total_tracks": 20,
    "total_audio_clips": 50,
    "total_midi_clips": 15
  },
  "plugins": {
    "vst3": [
      { "name": "Serum 1", "count": 10 },
      { "name": "OTT", "count": 15 },
      { "name": "Chroma", "count": 20 },
      { "name": "Vital", "count": 15 }
    ],
    "clap": [
      { "name": "LSP Compressor", "count": 10 },
      { "name": "LSP Reverb", "count": 15 },
      { "name": "LSP EQ", "count": 20 },
      { "name": "LSP Delay", "count": 15 }
    ],
    "native": [
      { "name": "Hikaru OpenWavetable", "count": 10 },
      { "name": "Hikaru OpenDMS", "count": 45 },
      { "name": "Hikaru OpenModulation", "count": 15 }
    ]
  },
  "timeline": {
    "tracks": [
      {
        "id": 1,
        "name": "Track 1",
        "type": "audio",
        "muted": false,
        "solo": false,
        "clips": [
          { "type": "audio", "source": "Drum Loop 160BPM.wav", "start_beat": 0.0, "length_beats": 16.0 }
        ]
      },
      {
        "id": 2,
        "name": "Track 2",
        "type": "audio",
        "muted": false,
        "solo": false,
        "clips": [
          { "type": "audio", "source": "Future Bass Drop Loop - 160BPM - B minor.wav", "start_beat": 16.0, "length_beats": 32.0 }
        ]
      },
      {
        "id": 3,
        "name": "Track 3",
        "type": "midi",
        "muted": false,
        "solo": false,
        "clips": [
          { "type": "midi", "source": "Hihat Pattern 01 - 160BPM.mid", "start_beat": 0.0, "length_beats": 16.0 }
        ]
      },
      {
        "id": 4,
        "name": "Track 4",
        "type": "audio",
        "muted": false,
        "solo": false,
        "clips": [
          { "type": "audio", "source": "FX 160BPM.wav", "start_beat": 0.0, "length_beats": 8.0 }
        ]
      },
      {
        "id": 5,
        "name": "Track 5",
        "type": "audio",
        "muted": false,
        "solo": false,
        "clips": [
          { "type": "audio", "source": "Buildup Snare Roll Loop 160BPM.wav", "start_beat": 8.0, "length_beats": 8.0 }
        ]
      },
      {
        "id": 6,
        "name": "Track 6",
        "type": "audio",
        "muted": false,
        "solo": false,
        "clips": [
          { "type": "audio", "source": "FX 160BPM.wav", "start_beat": 16.0, "length_beats": 8.0 }
        ]
      },
      {
        "id": 7,
        "name": "Track 7",
        "type": "audio",
        "muted": false,
        "solo": false,
        "clips": [
          { "type": "audio", "source": "Dubstep Drum Loop 160BPM.wav", "start_beat": 48.0, "length_beats": 32.0 }
        ]
      },
      {
        "id": 8,
        "name": "Track 8",
        "type": "audio",
        "muted": false,
        "solo": false,
        "clips": [
          { "type": "audio", "source": "Dubstep Bass Drop Loop - 160BPM - B minor.wav", "start_beat": 48.0, "length_beats": 32.0 }
        ]
      },
      {
        "id": 9,
        "name": "Track 9",
        "type": "audio",
        "muted": false,
        "solo": false,
        "clips": [
          { "type": "audio", "source": "FX 160BPM.wav", "start_beat": 48.0, "length_beats": 8.0 }
        ]
      },
      {
        "id": 10,
        "name": "Track 10",
        "type": "midi",
        "muted": false,
        "solo": false,
        "clips": [
          { "type": "midi", "source": "Hihat Pattern 02 - 160BPM.mid", "start_beat": 48.0, "length_beats": 16.0 }
        ]
      }
    ]
  }
}

```