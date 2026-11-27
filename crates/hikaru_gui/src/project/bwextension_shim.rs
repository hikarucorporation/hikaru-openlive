// Lector e integrador clean-room de extensiones de controlador `.bwextension` de Bitwig Studio.
// GNU AGPLv3
// crates/hikaru_gui/src/project/bwextension_shim.rs
//
// Implementa las 3 features de paridad `.bwextension`:
// - bex-01: Extension archive & manifest inspector (.bwextension)
// - bex-02: Controller surface & MIDI mapping bridge
// - bex-03: Built-in vendor controller profile parity

use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::path::Path;

use crate::app::AppState;
use crate::project::dawproject::{build_zip_store, read_zip_entries};
use crate::views::remote_controls::{
    apply_hardware_midi_cc, assign_hardware_cc_bank, RemoteControlsState,
};

/// Extensión del formato de paquete de controlador nativo de Bitwig Studio.
pub const BWEXTENSION_EXTENSION: &str = "bwextension";

const CONTROLLER_SERVICE_PATH: &str =
    "META-INF/services/com.bitwig.extension.controller.ControllerExtensionDefinition";

/// Devuelve `true` si la ruta corresponde a un archivo `.bwextension`.
pub fn is_bwextension_file(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|ext| ext.eq_ignore_ascii_case(BWEXTENSION_EXTENSION))
        .unwrap_or(false)
}

// =========================================================================
// bex-03: Built-in vendor controller profile parity
// =========================================================================

/// Perfil de controlador hardware de fabricante equivalente al catálogo nativo de Bitwig.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VendorControllerProfile {
    pub id: &'static str,
    pub vendor: &'static str,
    pub model_family: &'static str,
    pub keywords: &'static [&'static str],
    pub pad_grid: Option<(u8, u8)>,
    pub encoders: u8,
    pub faders: u8,
    pub transport_buttons: bool,
    pub default_cc_base: u8,
    pub default_midi_channel: u8,
}

/// Catálogo *clean-room* con las 17+ familias de controladores soportadas en Bitwig Studio 5.3.13.
pub const BUILTIN_VENDOR_PROFILES: &[VendorControllerProfile] = &[
    VendorControllerProfile {
        id: "novation-launchpad",
        vendor: "Novation",
        model_family: "Launchpad / Launchkey / SL MkIII",
        keywords: &["novation", "launchpad", "launchkey", "slmk", "circuit"],
        pad_grid: Some((8, 8)),
        encoders: 8,
        faders: 8,
        transport_buttons: true,
        default_cc_base: 21,
        default_midi_channel: 0,
    },
    VendorControllerProfile {
        id: "ableton-push",
        vendor: "Ableton",
        model_family: "Push / Push 2 / Push 3",
        keywords: &["ableton", "push"],
        pad_grid: Some((8, 8)),
        encoders: 8,
        faders: 0,
        transport_buttons: true,
        default_cc_base: 71,
        default_midi_channel: 0,
    },
    VendorControllerProfile {
        id: "akai-apc",
        vendor: "Akai",
        model_family: "APC / MPK / Force",
        keywords: &["akai", "apc", "mpk", "lpd", "midimix"],
        pad_grid: Some((8, 5)),
        encoders: 8,
        faders: 8,
        transport_buttons: true,
        default_cc_base: 16,
        default_midi_channel: 0,
    },
    VendorControllerProfile {
        id: "arturia-keylab",
        vendor: "Arturia",
        model_family: "KeyLab / MiniLab / BeatStep",
        keywords: &["arturia", "keylab", "minilab", "beatstep", "keystep"],
        pad_grid: Some((8, 2)),
        encoders: 9,
        faders: 9,
        transport_buttons: true,
        default_cc_base: 74,
        default_midi_channel: 0,
    },
    VendorControllerProfile {
        id: "native-instruments",
        vendor: "Native Instruments",
        model_family: "Komplete Kontrol / Maschine",
        keywords: &["nativeinstruments", "native instruments", "komplete", "maschine"],
        pad_grid: Some((4, 4)),
        encoders: 8,
        faders: 0,
        transport_buttons: true,
        default_cc_base: 14,
        default_midi_channel: 0,
    },
    VendorControllerProfile {
        id: "nektar-panorama",
        vendor: "Nektar",
        model_family: "Panorama / Impact",
        keywords: &["nektar", "panorama", "impact", "aura"],
        pad_grid: Some((4, 4)),
        encoders: 8,
        faders: 9,
        transport_buttons: true,
        default_cc_base: 12,
        default_midi_channel: 0,
    },
    VendorControllerProfile {
        id: "korg-nano",
        vendor: "Korg",
        model_family: "nanoKONTROL / Keystage / PadKONTROL",
        keywords: &["korg", "nanokontrol", "nanopad", "keystage", "taktile"],
        pad_grid: Some((4, 4)),
        encoders: 8,
        faders: 8,
        transport_buttons: true,
        default_cc_base: 16,
        default_midi_channel: 0,
    },
    VendorControllerProfile {
        id: "behringer-xtouch",
        vendor: "Behringer",
        model_family: "X-Touch / BCF2000 / BCR2000",
        keywords: &["behringer", "xtouch", "x-touch", "bcf", "bcr"],
        pad_grid: None,
        encoders: 8,
        faders: 8,
        transport_buttons: true,
        default_cc_base: 16,
        default_midi_channel: 0,
    },
    VendorControllerProfile {
        id: "mackie-mcu",
        vendor: "Mackie",
        model_family: "Mackie Control Universal (MCU)",
        keywords: &["mackie", "mcu", "controluniversal"],
        pad_grid: None,
        encoders: 8,
        faders: 8,
        transport_buttons: true,
        default_cc_base: 16,
        default_midi_channel: 0,
    },
    VendorControllerProfile {
        id: "presonus-faderport",
        vendor: "PreSonus",
        model_family: "FaderPort / ATOM",
        keywords: &["presonus", "faderport", "atom"],
        pad_grid: Some((4, 4)),
        encoders: 8,
        faders: 8,
        transport_buttons: true,
        default_cc_base: 14,
        default_midi_channel: 0,
    },
    VendorControllerProfile {
        id: "allen-heath",
        vendor: "Allen & Heath",
        model_family: "Xone:K2",
        keywords: &["allen", "xone"],
        pad_grid: Some((4, 4)),
        encoders: 6,
        faders: 4,
        transport_buttons: false,
        default_cc_base: 12,
        default_midi_channel: 14,
    },
    VendorControllerProfile {
        id: "elektron",
        vendor: "Elektron",
        model_family: "Digitakt / Analog / Syntakt",
        keywords: &["elektron", "digitakt", "digitone", "syntakt"],
        pad_grid: Some((8, 2)),
        encoders: 8,
        faders: 0,
        transport_buttons: true,
        default_cc_base: 70,
        default_midi_channel: 0,
    },
    VendorControllerProfile {
        id: "expressive-e",
        vendor: "Expressive E",
        model_family: "Osmose / Touche",
        keywords: &["expressive", "osmose", "touche"],
        pad_grid: None,
        encoders: 8,
        faders: 0,
        transport_buttons: false,
        default_cc_base: 74,
        default_midi_channel: 0,
    },
    VendorControllerProfile {
        id: "faderfox",
        vendor: "Faderfox",
        model_family: "EC4 / UC4 / PC4",
        keywords: &["faderfox", "ec4", "uc4", "pc4"],
        pad_grid: None,
        encoders: 16,
        faders: 8,
        transport_buttons: false,
        default_cc_base: 16,
        default_midi_channel: 0,
    },
    VendorControllerProfile {
        id: "icon-pro",
        vendor: "iCON",
        model_family: "Platform M+ / QCon",
        keywords: &["icon", "qcon", "platform"],
        pad_grid: None,
        encoders: 8,
        faders: 8,
        transport_buttons: true,
        default_cc_base: 16,
        default_midi_channel: 0,
    },
    VendorControllerProfile {
        id: "keith-mcmillen",
        vendor: "Keith McMillen",
        model_family: "QuNeo / QuNexus / SoftStep",
        keywords: &["mcmillen", "quneo", "qunexus", "softstep"],
        pad_grid: Some((4, 4)),
        encoders: 8,
        faders: 4,
        transport_buttons: true,
        default_cc_base: 20,
        default_midi_channel: 0,
    },
    VendorControllerProfile {
        id: "m-audio",
        vendor: "M-Audio",
        model_family: "Oxygen / Axiom / Hammer",
        keywords: &["maudio", "m-audio", "oxygen", "axiom"],
        pad_grid: Some((4, 2)),
        encoders: 8,
        faders: 8,
        transport_buttons: true,
        default_cc_base: 70,
        default_midi_channel: 0,
    },
    VendorControllerProfile {
        id: "roland",
        vendor: "Roland",
        model_family: "A-PRO / TR / Fantom",
        keywords: &["roland", "a-pro", "apro", "fantom"],
        pad_grid: Some((4, 2)),
        encoders: 8,
        faders: 8,
        transport_buttons: true,
        default_cc_base: 16,
        default_midi_channel: 0,
    },
    VendorControllerProfile {
        id: "roli",
        vendor: "ROLI",
        model_family: "Seaboard / Lightpad Block",
        keywords: &["roli", "seaboard", "lightpad"],
        pad_grid: Some((5, 5)),
        encoders: 8,
        faders: 0,
        transport_buttons: false,
        default_cc_base: 74,
        default_midi_channel: 0,
    },
    VendorControllerProfile {
        id: "yamaha-steinberg",
        vendor: "Yamaha / Steinberg",
        model_family: "CC121 / Montage / MODX",
        keywords: &["yamaha", "steinberg", "cc121", "montage", "modx"],
        pad_grid: None,
        encoders: 8,
        faders: 8,
        transport_buttons: true,
        default_cc_base: 16,
        default_midi_channel: 0,
    },
    VendorControllerProfile {
        id: "bitwig-controllers-bundle",
        vendor: "Bitwig",
        model_family: "Controllers Bundle / Generic MIDI",
        keywords: &["controllers", "generic", "bitwig"],
        pad_grid: Some((8, 8)),
        encoders: 8,
        faders: 8,
        transport_buttons: true,
        default_cc_base: 21,
        default_midi_channel: 0,
    },
];

/// Resuelve el perfil de fabricante integrado a partir de vendor, título o clases de servicio.
pub fn resolve_vendor_profile(
    vendor: Option<&str>,
    title: &str,
    extension_classes: &[String],
) -> Option<&'static VendorControllerProfile> {
    let mut haystack = String::new();
    if let Some(v) = vendor {
        haystack.push_str(&v.to_ascii_lowercase());
        haystack.push(' ');
    }
    haystack.push_str(&title.to_ascii_lowercase());
    for cls in extension_classes {
        haystack.push(' ');
        haystack.push_str(&cls.to_ascii_lowercase());
    }

    // Priorizar perfiles específicos de hardware antes del bundle genérico de Bitwig.
    for profile in BUILTIN_VENDOR_PROFILES
        .iter()
        .filter(|p| p.id != "bitwig-controllers-bundle")
    {
        if profile.keywords.iter().any(|kw| haystack.contains(kw)) {
            return Some(profile);
        }
    }

    BUILTIN_VENDOR_PROFILES
        .iter()
        .find(|p| p.id == "bitwig-controllers-bundle" && p.keywords.iter().any(|kw| haystack.contains(kw)))
}

// =========================================================================
// bex-01: Extension archive & manifest inspector (.bwextension)
// =========================================================================

/// Metadatos extraídos de un paquete `.bwextension` (ZIP/JAR).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BwExtensionManifest {
    pub title: String,
    pub vendor: String,
    pub version: String,
    pub api_version: Option<String>,
    pub extension_classes: Vec<String>,
    pub class_count: usize,
    pub resource_count: usize,
    pub total_entries: usize,
    pub properties: BTreeMap<String, String>,
}

/// Tipo de control de superficie mapeado hacia Hikaru OpenLive.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SurfaceControlKind {
    EncoderCc,
    FaderCc,
    PadNote,
    TransportButton,
}

/// Destino funcional dentro de Hikaru OpenLive.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SurfaceControlTarget {
    RemoteControlSlot(u8),
    TrackVolume(u8),
    SessionPad(u8, u8),
    TransportPlayStop,
    TransportRecord,
}

/// Binding individual entre un mensaje MIDI hardware y una acción/parámetro de Hikaru.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SurfaceControlBinding {
    pub label: String,
    pub kind: SurfaceControlKind,
    pub midi_channel: u8,
    pub cc_or_note: u8,
    pub target: SurfaceControlTarget,
}

/// Puente de superficie y mapeo MIDI generado para un `.bwextension`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ControllerSurfaceMapping {
    pub controller_name: String,
    pub vendor: String,
    pub cc_bank_base: u8,
    pub midi_channel: u8,
    pub pad_grid: Option<(u8, u8)>,
    pub encoders: u8,
    pub faders: u8,
    pub transport_buttons: bool,
    pub bindings: Vec<SurfaceControlBinding>,
}

/// Reporte completo de inspección y puente de un `.bwextension`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BwExtensionReport {
    pub manifest: BwExtensionManifest,
    pub matched_profile: Option<&'static VendorControllerProfile>,
    pub surface_mapping: ControllerSurfaceMapping,
}

/// Resumen tras activar un `.bwextension` en el estado vivo de `AppState`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BwExtensionActivationSummary {
    pub controller_name: String,
    pub vendor: String,
    pub cc_bank_base: u8,
    pub bound_remote_slots: usize,
    pub total_bindings: usize,
}

fn parse_manifest_mf(raw: &str) -> BTreeMap<String, String> {
    let mut unfolded = Vec::<String>::new();
    for line in raw.lines() {
        let trimmed_end = line.trim_end_matches('\r');
        if let Some(rest) = trimmed_end.strip_prefix(' ') {
            if let Some(last) = unfolded.last_mut() {
                last.push_str(rest);
            }
        } else if !trimmed_end.trim().is_empty() {
            unfolded.push(trimmed_end.to_string());
        }
    }
    let mut map = BTreeMap::new();
    for line in unfolded {
        if let Some((k, v)) = line.split_once(':') {
            map.insert(k.trim().to_string(), v.trim().to_string());
        }
    }
    map
}

fn parse_properties_entries(entries: &HashMap<String, Vec<u8>>) -> BTreeMap<String, String> {
    let mut props = BTreeMap::new();
    let mut keys: Vec<&String> = entries
        .keys()
        .filter(|k| k.ends_with(".properties"))
        .collect();
    keys.sort();
    for key in keys {
        if let Some(bytes) = entries.get(key) {
            let text = String::from_utf8_lossy(bytes);
            for line in text.lines() {
                let line = line.trim();
                if line.is_empty() || line.starts_with('#') || line.starts_with('!') {
                    continue;
                }
                if let Some((k, v)) = line.split_once('=').or_else(|| line.split_once(':')) {
                    props.insert(k.trim().to_string(), v.trim().to_string());
                }
            }
        }
    }
    props
}

fn humanize_class_name(cls: &str) -> String {
    let short = cls.rsplit('.').next().unwrap_or(cls);
    short
        .strip_suffix("ExtensionDefinition")
        .or_else(|| short.strip_suffix("Definition"))
        .unwrap_or(short)
        .to_string()
}

/// Inspecciona los bytes de un paquete `.bwextension` (archivo ZIP/JAR) en modo *clean-room*.
pub fn inspect_bwextension_bytes(
    bytes: &[u8],
    fallback_name: &str,
) -> Result<BwExtensionReport, String> {
    let entries = read_zip_entries(bytes)?;
    if entries.is_empty() {
        return Err("el paquete .bwextension está vacío".to_string());
    }

    let manifest_map = entries
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case("META-INF/MANIFEST.MF"))
        .map(|(_, v)| parse_manifest_mf(&String::from_utf8_lossy(v)))
        .unwrap_or_default();

    let props = parse_properties_entries(&entries);

    let mut extension_classes = Vec::new();
    for (name, data) in &entries {
        if name.eq_ignore_ascii_case(CONTROLLER_SERVICE_PATH)
            || (name.starts_with("META-INF/services/") && name.contains("ControllerExtensionDefinition"))
        {
            for line in String::from_utf8_lossy(data).lines() {
                let trimmed = line.split('#').next().unwrap_or("").trim();
                if !trimmed.is_empty() && !extension_classes.contains(&trimmed.to_string()) {
                    extension_classes.push(trimmed.to_string());
                }
            }
        }
    }
    extension_classes.sort();

    let class_count = entries.keys().filter(|k| k.ends_with(".class")).count();
    let total_entries = entries.len();
    let resource_count = total_entries.saturating_sub(class_count);

    let title = manifest_map
        .get("Implementation-Title")
        .or_else(|| manifest_map.get("Bundle-Name"))
        .or_else(|| props.get("name"))
        .or_else(|| props.get("extension.name"))
        .cloned()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| {
            if let Some(first_cls) = extension_classes.first() {
                humanize_class_name(first_cls)
            } else {
                fallback_name.to_string()
            }
        });

    let vendor_raw = manifest_map
        .get("Implementation-Vendor")
        .or_else(|| manifest_map.get("Bundle-Vendor"))
        .or_else(|| props.get("vendor"))
        .or_else(|| props.get("author"))
        .cloned()
        .filter(|s| !s.is_empty());

    let matched_profile = resolve_vendor_profile(
        vendor_raw.as_deref(),
        &title,
        &extension_classes,
    );

    let vendor = vendor_raw
        .or_else(|| matched_profile.map(|p| p.vendor.to_string()))
        .unwrap_or_else(|| "Bitwig / 3rd Party".to_string());

    let version = manifest_map
        .get("Implementation-Version")
        .or_else(|| manifest_map.get("Bundle-Version"))
        .or_else(|| props.get("version"))
        .cloned()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "1.0".to_string());

    let api_version = manifest_map
        .get("Bitwig-API-Version")
        .or_else(|| props.get("apiVersion"))
        .cloned();

    let manifest = BwExtensionManifest {
        title,
        vendor,
        version,
        api_version,
        extension_classes,
        class_count,
        resource_count,
        total_entries,
        properties: props,
    };

    let surface_mapping = build_surface_mapping(&manifest, matched_profile);

    Ok(BwExtensionReport {
        manifest,
        matched_profile,
        surface_mapping,
    })
}

/// Carga e inspecciona un archivo `.bwextension` desde disco.
pub fn inspect_bwextension_file(path: &Path) -> Result<BwExtensionReport, String> {
    let bytes = fs::read(path).map_err(|e| format!("no se pudo leer .bwextension: {e}"))?;
    let fallback = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("Controller Extension");
    inspect_bwextension_bytes(&bytes, fallback)
}

// =========================================================================
// bex-02: Controller surface & MIDI mapping bridge
// =========================================================================

/// Construye el mapeo de superficie MIDI hacia Hikaru OpenLive a partir del manifest y perfil.
pub fn build_surface_mapping(
    manifest: &BwExtensionManifest,
    profile: Option<&'static VendorControllerProfile>,
) -> ControllerSurfaceMapping {
    let (pad_grid, encoders, faders, transport_buttons, cc_bank_base, midi_channel) =
        if let Some(p) = profile {
            (
                p.pad_grid,
                p.encoders,
                p.faders,
                p.transport_buttons,
                p.default_cc_base,
                p.default_midi_channel,
            )
        } else {
            (Some((8, 4)), 8, 8, true, 21, 0)
        };

    let mut bindings = Vec::new();
    for i in 0..encoders.min(8) {
        bindings.push(SurfaceControlBinding {
            label: format!("Remote Knob {}", i + 1),
            kind: SurfaceControlKind::EncoderCc,
            midi_channel,
            cc_or_note: cc_bank_base.saturating_add(i).min(127),
            target: SurfaceControlTarget::RemoteControlSlot(i),
        });
    }
    for i in 0..faders.min(8) {
        bindings.push(SurfaceControlBinding {
            label: format!("Track {} Volume", i + 1),
            kind: SurfaceControlKind::FaderCc,
            midi_channel,
            cc_or_note: 70_u8.saturating_add(i).min(127),
            target: SurfaceControlTarget::TrackVolume(i),
        });
    }
    if let Some((cols, rows)) = pad_grid {
        for r in 0..rows.min(8) {
            for c in 0..cols.min(8) {
                let note = 36_u8.saturating_add(r * 8 + c).min(127);
                bindings.push(SurfaceControlBinding {
                    label: format!("Session Pad ({},{})", c + 1, r + 1),
                    kind: SurfaceControlKind::PadNote,
                    midi_channel,
                    cc_or_note: note,
                    target: SurfaceControlTarget::SessionPad(c, r),
                });
            }
        }
    }
    if transport_buttons {
        bindings.push(SurfaceControlBinding {
            label: "Transport Play/Stop".to_string(),
            kind: SurfaceControlKind::TransportButton,
            midi_channel,
            cc_or_note: 118,
            target: SurfaceControlTarget::TransportPlayStop,
        });
        bindings.push(SurfaceControlBinding {
            label: "Transport Record".to_string(),
            kind: SurfaceControlKind::TransportButton,
            midi_channel,
            cc_or_note: 119,
            target: SurfaceControlTarget::TransportRecord,
        });
    }

    ControllerSurfaceMapping {
        controller_name: manifest.title.clone(),
        vendor: manifest.vendor.clone(),
        cc_bank_base,
        midi_channel,
        pad_grid,
        encoders,
        faders,
        transport_buttons,
        bindings,
    }
}

/// Vincula el banco de 8 encoders CC de un `.bwextension` sobre un `RemoteControlsState`.
pub fn apply_bwextension_to_remotes(
    remotes: &mut RemoteControlsState,
    report: &BwExtensionReport,
) -> usize {
    assign_hardware_cc_bank(remotes, report.surface_mapping.cc_bank_base);
    8
}

/// Activa el puente de superficie `.bwextension` sobre `AppState`, vinculando el banco de 8 CC
/// de `RemoteControls` en Proyecto, Pista seleccionada y Dispositivo seleccionado.
pub fn apply_bwextension_to_app(
    app: &mut AppState,
    report: &BwExtensionReport,
) -> BwExtensionActivationSummary {
    let base_cc = report.surface_mapping.cc_bank_base;
    let mut bound_remote_slots = apply_bwextension_to_remotes(&mut app.project_remotes, report);

    let tr_idx = app.safe_track_index();
    let sl_idx = app.selected_slot_index;
    if let Some(tr) = app.current_tracks_mut().get_mut(tr_idx) {
        bound_remote_slots += apply_bwextension_to_remotes(&mut tr.remotes, report);
        if let Some(dev) = tr.effects.get_mut(sl_idx) {
            bound_remote_slots += apply_bwextension_to_remotes(&mut dev.remotes, report);
        }
    }
    app.project_dirty = true;

    BwExtensionActivationSummary {
        controller_name: report.surface_mapping.controller_name.clone(),
        vendor: report.surface_mapping.vendor.clone(),
        cc_bank_base: base_cc,
        bound_remote_slots,
        total_bindings: report.surface_mapping.bindings.len(),
    }
}

/// Enruta un mensaje MIDI CC entrante usando el puente `.bwextension` activo hacia `RemoteControls`
/// y controles de superficie de pista.
pub fn route_bwextension_midi_cc(
    app: &mut AppState,
    mapping: &ControllerSurfaceMapping,
    cc: u8,
    value_0_to_127: u8,
) -> usize {
    let mut handled = apply_hardware_midi_cc(app, cc, value_0_to_127);
    let norm = (value_0_to_127.min(127) as f32) / 127.0;
    for binding in &mapping.bindings {
        if binding.kind == SurfaceControlKind::FaderCc && binding.cc_or_note == cc {
            if let SurfaceControlTarget::TrackVolume(track_i) = binding.target {
                if let Some(tr) = app.current_tracks_mut().get_mut(track_i as usize) {
                    tr.volume = norm;
                    handled += 1;
                }
            }
        }
    }
    handled
}

/// Construye un archivo `.bwextension` sintético válido (ZIP/JAR) para pruebas y exportación.
pub fn build_synthetic_bwextension_zip(
    title: &str,
    vendor: &str,
    version: &str,
    service_classes: &[&str],
) -> Vec<u8> {
    let manifest = format!(
        "Manifest-Version: 1.0\r\nImplementation-Title: {title}\r\nImplementation-Vendor: {vendor}\r\nImplementation-Version: {version}\r\n"
    );
    let services = service_classes.join("\n");
    let dummy_class = b"\xCA\xFE\xBA\xBE\x00\x00\x00\x3D";
    build_zip_store(&[
        ("META-INF/MANIFEST.MF", manifest.as_bytes()),
        (CONTROLLER_SERVICE_PATH, services.as_bytes()),
        ("com/example/ControllerExtension.class", dummy_class),
    ])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bex01_inspects_bwextension_zip_manifest_and_services() {
        let zip = build_synthetic_bwextension_zip(
            "Launchpad Pro Mk3",
            "Novation",
            "2.1.0",
            &["com.bitwig.extensions.controllers.novation.launchpad_pro_mk3.LaunchpadProMk3ExtensionDefinition"],
        );
        let report = inspect_bwextension_bytes(&zip, "LaunchpadProMk3").expect("inspect bwextension");
        assert_eq!(report.manifest.title, "Launchpad Pro Mk3");
        assert_eq!(report.manifest.vendor, "Novation");
        assert_eq!(report.manifest.version, "2.1.0");
        assert_eq!(report.manifest.extension_classes.len(), 1);
        assert_eq!(report.manifest.class_count, 1);
        assert!(report.manifest.resource_count >= 2);
    }

    #[test]
    fn bex03_resolves_builtin_vendor_controller_profiles() {
        let profile = resolve_vendor_profile(
            Some("Ableton"),
            "Push 3",
            &["com.bitwig.extensions.controllers.ableton.push.Push3ExtensionDefinition".to_string()],
        )
        .expect("matches Ableton Push");
        assert_eq!(profile.id, "ableton-push");
        assert_eq!(profile.pad_grid, Some((8, 8)));
        assert_eq!(profile.default_cc_base, 71);
        assert!(BUILTIN_VENDOR_PROFILES.len() >= 17);
    }

    #[test]
    fn bex02_builds_surface_mapping_and_bridges_remote_cc_bank() {
        let zip = build_synthetic_bwextension_zip(
            "Arturia KeyLab MkII",
            "Arturia",
            "1.4",
            &["com.bitwig.extensions.controllers.arturia.keylab.KeyLabMkIIExtensionDefinition"],
        );
        let report = inspect_bwextension_bytes(&zip, "KeyLabMkII").expect("inspect");
        assert_eq!(report.surface_mapping.cc_bank_base, 74);
        assert!(report
            .surface_mapping
            .bindings
            .iter()
            .any(|b| b.kind == SurfaceControlKind::EncoderCc && b.cc_or_note == 74));

        let mut remotes = RemoteControlsState::default();
        let bound = apply_bwextension_to_remotes(&mut remotes, &report);
        assert_eq!(bound, 8);
        assert_eq!(remotes.active_page_mut().slots[0].midi_cc, Some(74));
        assert_eq!(remotes.active_page_mut().slots[7].midi_cc, Some(81));
    }
}
