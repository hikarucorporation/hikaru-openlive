// Hikaru OpenLive / OpenStudio - Bitwig Pop-up Browser (egui)
// GNU AGPLv3
// crates/hikaru_gui/src/views/popup_browser.rs

use std::collections::{BTreeMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;

use egui::{
    pos2, vec2, Align, Align2, Color32, Context, FontId, Id, Key, Layout, Order, Rect, RichText,
    ScrollArea, Sense, Stroke, TextEdit, UiBuilder,
};
use hikaru_dsp::synth::open_poly::{eval_osc_preview, OpenPolyOscType, OpenPolyWavetableData};
use hikaru_plugin_host::{PluginDescriptor, PluginFormat, PluginKind};

use crate::app::AppState;
use crate::audio_proxy::GuiCommand;
use crate::project::bwdevice_shim::{
    discover_bitwig_presets, inspect_bwpreset_preview, AvailableBitwigPreset, BwPresetPreview,
};
use crate::theme;
use crate::ui::fonts;
use crate::ui::icons;
use crate::views::dsp_rack::PLUGIN_CATALOG;
use crate::views::hikaru_native_plugins::hikaru_openmodulation::{
    self, ModulatorKind, MODULATOR_CATALOG, MOD_ARROW_CYAN,
};
use crate::views::hikaru_native_plugins::hikaru_openpoly::{
    self, AvailableWavetable, WavetableSource,
};
use crate::views::hikaru_native_plugins::hikaru_openwavetable::wavetable_io::FRAME_SAMPLES;
use crate::views::mixer::{DspSlot, Track};

// Bitwig Pop-up Browser palette (matched to Bitwig Studio 5.x)
const FRAME_BG: Color32 = Color32::from_rgb(0x36, 0x37, 0x3B);
const FRAME_BORDER_OUTER: Color32 = Color32::from_rgb(0x14, 0x15, 0x18);
const FRAME_BORDER_INNER: Color32 = Color32::from_rgb(0x52, 0x54, 0x5A);
const RAIL_BG: Color32 = Color32::from_rgb(0x27, 0x28, 0x2C);
const SIDEBAR_BG: Color32 = Color32::from_rgb(0x2F, 0x30, 0x34);
const WELL_BG: Color32 = Color32::from_rgb(0x12, 0x13, 0x15);
const SEARCH_BG: Color32 = Color32::from_rgb(0x1B, 0x1C, 0x1F);
const ROW_HOVER_BG: Color32 = Color32::from_rgb(0x21, 0x24, 0x2A);
const ROW_SEL_BG: Color32 = Color32::from_rgb(0x2B, 0x3B, 0x4A);
const FILTER_SEL_BG: Color32 = Color32::from_rgb(0x30, 0x40, 0x4E);
const BTN_BG: Color32 = Color32::from_rgb(0x4E, 0x50, 0x56);
const BTN_HOVER_BG: Color32 = Color32::from_rgb(0x5C, 0x5E, 0x65);
const BTN_BORDER: Color32 = Color32::from_rgb(0x26, 0x27, 0x2A);

const INST_YELLOW: Color32 = Color32::from_rgb(0xEA, 0xBA, 0x0A);
const FX_ORANGE: Color32 = Color32::from_rgb(0xE8, 0x6A, 0x17);
const NOTE_BLUE: Color32 = Color32::from_rgb(0x3A, 0x9E, 0xD8);
const PLUGIN_PURPLE: Color32 = Color32::from_rgb(0x9A, 0x7A, 0xD8);
const QUICK_BTN_BG: Color32 = Color32::from_rgb(0x47, 0x43, 0x34);
const QUICK_BTN_ACTIVE_BG: Color32 = Color32::from_rgb(0x69, 0x5E, 0x33);
const ACTIVE_BLUE_TEXT: Color32 = Color32::from_rgb(0x4C, 0xA8, 0xE8);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BrowserTargetMode {
    NewInstrumentTrack,
    InsertDevice { track_idx: usize },
    InsertModulator { track_idx: usize, slot_idx: usize },
    InsertWetFx { track_idx: usize, parent_slot_idx: usize },
    SelectWavetable { track_idx: usize, slot_idx: usize },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BrowserRailTab {
    Everything,
    #[default]
    Instruments,
    AudioFx,
    NoteFx,
    Plugins,
}

impl BrowserRailTab {
    fn title(self) -> &'static str {
        match self {
            Self::Everything => "Everything",
            Self::Instruments => "All Instruments",
            Self::AudioFx => "Audio FX",
            Self::NoteFx => "Note & Mod",
            Self::Plugins => "VST3 / CLAP",
        }
    }

    fn accent(self) -> Color32 {
        match self {
            Self::Everything => Color32::from_rgb(0xB8, 0xBC, 0xC4),
            Self::Instruments => INST_YELLOW,
            Self::AudioFx => FX_ORANGE,
            Self::NoteFx => NOTE_BLUE,
            Self::Plugins => PLUGIN_PURPLE,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QuickInstrumentFilter {
    Drums,
    Bass,
    Synth,
    Sampler,
    Modulation,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MetadataFilterTab {
    Location,
    FileKind,
    #[default]
    Category,
    Creator,
    Device,
    Tags,
}

impl MetadataFilterTab {
    const ALL: [Self; 6] = [
        Self::Location,
        Self::FileKind,
        Self::Category,
        Self::Creator,
        Self::Device,
        Self::Tags,
    ];

    fn label(self) -> &'static str {
        match self {
            Self::Location => "Location",
            Self::FileKind => "File Kind",
            Self::Category => "Category",
            Self::Creator => "Creator",
            Self::Device => "Device",
            Self::Tags => "Tags",
        }
    }

    fn any_label(self) -> &'static str {
        match self {
            Self::Location => "Any Location",
            Self::FileKind => "Any File Kind",
            Self::Category => "Any Category",
            Self::Creator => "Any Creator",
            Self::Device => "Any Device",
            Self::Tags => "Any Tag",
        }
    }
}

#[derive(Debug, Clone)]
pub enum BrowserPayload {
    Native { name: String },
    External(PluginDescriptor),
    Modulator { kind: ModulatorKind },
    Wavetable(WavetableSource),
    BitwigPreset(PathBuf),
}

#[derive(Debug, Clone)]
struct BrowserItem {
    id: String,
    name: String,
    category: String,
    creator: String,
    location: String,
    file_kind: String,
    device_type: String,
    tags: Vec<String>,
    description: String,
    rail: BrowserRailTab,
    quick: Option<QuickInstrumentFilter>,
    badge_color: Color32,
    payload: BrowserPayload,
}

#[derive(Debug, Clone)]
pub struct PopupBrowserState {
    pub open: bool,
    pub mode: BrowserTargetMode,
    pub active_rail: BrowserRailTab,
    pub favorites_only: bool,
    pub quick_filter: Option<QuickInstrumentFilter>,
    pub metadata_tab: MetadataFilterTab,
    pub selected_filter_value: Option<String>,
    pub search_query: String,
    pub selected_item_idx: usize,
    pub favorites: HashSet<String>,
    pub focus_search: bool,
    pub wavetable_catalog: Vec<AvailableWavetable>,
    pub wavetable_preview_cache: Option<(String, Arc<OpenPolyWavetableData>)>,
    pub bitwig_preset_catalog: Vec<AvailableBitwigPreset>,
    pub bitwig_preset_preview_cache: Option<(String, BwPresetPreview)>,
}

impl Default for PopupBrowserState {
    fn default() -> Self {
        Self {
            open: false,
            mode: BrowserTargetMode::NewInstrumentTrack,
            active_rail: BrowserRailTab::Instruments,
            favorites_only: false,
            quick_filter: None,
            metadata_tab: MetadataFilterTab::Category,
            selected_filter_value: None,
            search_query: String::new(),
            selected_item_idx: 0,
            favorites: HashSet::new(),
            focus_search: false,
            wavetable_catalog: Vec::new(),
            wavetable_preview_cache: None,
            bitwig_preset_catalog: Vec::new(),
            bitwig_preset_preview_cache: None,
        }
    }
}

impl PopupBrowserState {
    fn refresh_bitwig_presets(&mut self) {
        self.bitwig_preset_catalog = discover_bitwig_presets(None);
        self.bitwig_preset_preview_cache = None;
    }

    pub fn open_new_instrument(&mut self) {
        self.open = true;
        self.mode = BrowserTargetMode::NewInstrumentTrack;
        self.active_rail = BrowserRailTab::Instruments;
        self.quick_filter = None;
        self.selected_filter_value = None;
        self.search_query.clear();
        self.selected_item_idx = 0;
        self.focus_search = true;
        self.refresh_bitwig_presets();
    }

    pub fn open_insert_device(&mut self, track_idx: usize) {
        self.open = true;
        self.mode = BrowserTargetMode::InsertDevice { track_idx };
        self.active_rail = BrowserRailTab::Everything;
        self.quick_filter = None;
        self.selected_filter_value = None;
        self.search_query.clear();
        self.selected_item_idx = 0;
        self.focus_search = true;
        self.refresh_bitwig_presets();
    }

    pub fn open_insert_wet_fx(&mut self, track_idx: usize, parent_slot_idx: usize) {
        self.open = true;
        self.mode = BrowserTargetMode::InsertWetFx {
            track_idx,
            parent_slot_idx,
        };
        self.active_rail = BrowserRailTab::AudioFx;
        self.quick_filter = None;
        self.selected_filter_value = None;
        self.search_query.clear();
        self.selected_item_idx = 0;
        self.focus_search = true;
        self.refresh_bitwig_presets();
    }

    pub fn open_insert_modulator(&mut self, track_idx: usize, slot_idx: usize) {
        self.open = true;
        self.mode = BrowserTargetMode::InsertModulator {
            track_idx,
            slot_idx,
        };
        self.active_rail = BrowserRailTab::Everything;
        self.metadata_tab = MetadataFilterTab::Category;
        self.quick_filter = None;
        self.selected_filter_value = None;
        self.search_query.clear();
        self.selected_item_idx = 0;
        self.focus_search = true;
        self.refresh_bitwig_presets();
    }

    pub fn open_select_wavetable(
        &mut self,
        track_idx: usize,
        slot_idx: usize,
        current_table_name: Option<&str>,
        extra_dir: Option<PathBuf>,
    ) {
        self.open = true;
        self.mode = BrowserTargetMode::SelectWavetable {
            track_idx,
            slot_idx,
        };
        self.active_rail = BrowserRailTab::Everything;
        self.metadata_tab = MetadataFilterTab::Category;
        self.quick_filter = None;
        self.selected_filter_value = None;
        self.search_query.clear();
        self.wavetable_catalog =
            hikaru_openpoly::discover_available_wavetables(extra_dir.as_deref());
        self.wavetable_preview_cache = None;
        self.selected_item_idx = current_table_name
            .and_then(|name| {
                self.wavetable_catalog
                    .iter()
                    .position(|w| w.name.eq_ignore_ascii_case(name))
            })
            .unwrap_or(0);
        self.focus_search = true;
    }
}

fn build_wavetable_items(catalog: &[AvailableWavetable]) -> Vec<BrowserItem> {
    catalog
        .iter()
        .map(|wt| {
            let file_kind = match &wt.source {
                WavetableSource::Builtin(_) => "Built-in Wavetable".to_string(),
                WavetableSource::File(_) => "WAV Wavetable".to_string(),
            };
            BrowserItem {
                id: wt.id.clone(),
                name: wt.name.clone(),
                category: wt.category.clone(),
                creator: if matches!(wt.source, WavetableSource::Builtin(_)) {
                    "Hikaru".to_string()
                } else {
                    "User".to_string()
                },
                location: wt.location.clone(),
                file_kind,
                device_type: "OpenPoly".to_string(),
                tags: vec![wt.category.clone(), "Wavetable".to_string()],
                description: wt.description.clone(),
                rail: BrowserRailTab::Instruments,
                quick: Some(QuickInstrumentFilter::Synth),
                badge_color: INST_YELLOW,
                payload: BrowserPayload::Wavetable(wt.source.clone()),
            }
        })
        .collect()
}

fn build_bitwig_preset_items(
    catalog: &[AvailableBitwigPreset],
    modulators_only: bool,
) -> Vec<BrowserItem> {
    catalog
        .iter()
        .filter(|p| if modulators_only { p.is_modulator } else { !p.is_modulator })
        .map(|p| {
            let (rail, badge_color, quick) = if p.file_kind == ".bwclip" {
                (
                    if p.category.to_ascii_lowercase().contains("audio") {
                        BrowserRailTab::AudioFx
                    } else {
                        BrowserRailTab::NoteFx
                    },
                    NOTE_BLUE,
                    None,
                )
            } else if p.file_kind == ".bwremotecontrols" {
                (
                    BrowserRailTab::AudioFx,
                    MOD_ARROW_CYAN,
                    Some(QuickInstrumentFilter::Modulation),
                )
            } else if p.file_kind == ".bwextension" {
                (
                    BrowserRailTab::NoteFx,
                    MOD_ARROW_CYAN,
                    Some(QuickInstrumentFilter::Modulation),
                )
            } else if p.is_modulator {
                (
                    BrowserRailTab::NoteFx,
                    MOD_ARROW_CYAN,
                    Some(QuickInstrumentFilter::Modulation),
                )
            } else if p.is_fx {
                (BrowserRailTab::AudioFx, FX_ORANGE, None)
            } else {
                (
                    BrowserRailTab::Instruments,
                    INST_YELLOW,
                    Some(QuickInstrumentFilter::Synth),
                )
            };
            BrowserItem {
                id: p.id.clone(),
                name: p.name.clone(),
                category: p.category.clone(),
                creator: p.creator.clone(),
                location: p.location.clone(),
                file_kind: p.file_kind.clone(),
                device_type: p.device_name.clone(),
                tags: p.tags.clone(),
                description: p.description.clone(),
                rail,
                quick,
                badge_color,
                payload: BrowserPayload::BitwigPreset(p.path.clone()),
            }
        })
        .collect()
}

fn build_modulator_items() -> Vec<BrowserItem> {
    MODULATOR_CATALOG
        .iter()
        .map(|meta| BrowserItem {
            id: format!("modulator:{}", meta.name),
            name: meta.name.to_string(),
            category: meta.category.label().to_string(),
            creator: "Hikaru".to_string(),
            location: "Modulators".to_string(),
            file_kind: "Modulator".to_string(),
            device_type: "Modulator".to_string(),
            tags: vec![meta.category.label().to_string(), "Modulator".to_string()],
            description: meta.description.to_string(),
            rail: BrowserRailTab::NoteFx,
            quick: Some(QuickInstrumentFilter::Modulation),
            badge_color: MOD_ARROW_CYAN,
            payload: BrowserPayload::Modulator { kind: meta.kind },
        })
        .collect()
}

fn build_catalog_items(discovered_plugins: &[PluginDescriptor]) -> Vec<BrowserItem> {
    let mut items = Vec::new();

    for entry in &PLUGIN_CATALOG {
        let (rail, quick, badge_color, desc) = match entry.name {
            "OpenPoly" => (
                BrowserRailTab::Instruments,
                Some(QuickInstrumentFilter::Synth),
                INST_YELLOW,
                "Hybrid modular polyphonic synthesizer (AGPL-v3): swappable oscillators, analog filters, FEG, ADSR, Sub/Noise & LFO.",
            ),
            "OpenGrid" => (
                BrowserRailTab::Instruments,
                Some(QuickInstrumentFilter::Synth),
                INST_YELLOW,
                "Polyphonic modular patch synthesizer (AGPL-v3): 97 grid modules across 16 categories, Bezier cables, live minimap & presets.",
            ),
            "Sampler" => (
                BrowserRailTab::Instruments,
                Some(QuickInstrumentFilter::Sampler),
                INST_YELLOW,
                "Multi-mode audio sample player with loop points, ADSR envelope, and pitch tracking.",
            ),
            "Hikaru OpenDMS" => (
                BrowserRailTab::Instruments,
                Some(QuickInstrumentFilter::Drums),
                INST_YELLOW,
                "16-pad drum machine & synthesizer with per-pad sampler and insert FX chain.",
            ),
            "OpenLFO MOD" | "OpenModulation" => (
                BrowserRailTab::NoteFx,
                Some(QuickInstrumentFilter::Modulation),
                NOTE_BLUE,
                "Bitwig-style dual LFO modulator (LFO1 + LFO2) with phase, depth, retrigger, sync, FX slot, and mix.",
            ),
            "OpenEQ3" => (
                BrowserRailTab::AudioFx,
                None,
                FX_ORANGE,
                "3-band parametric equalizer for track and master tone shaping.",
            ),
            "OpenSpectral Phaser" => (
                BrowserRailTab::AudioFx,
                None,
                FX_ORANGE,
                "Multi-stage spectral phaser with stereo spread and feedback control.",
            ),
            "Hikaru OpenComb" | "Hikaru Ultracomb" => (
                BrowserRailTab::AudioFx,
                None,
                FX_ORANGE,
                "Clean stereo comb filter with bipolar feedback, XY frequency pad, and mix.",
            ),
            "Tool" => (
                BrowserRailTab::AudioFx,
                None,
                FX_ORANGE,
                "Utility stereo processor: gain, pan, width, phase invert and L/R swap.",
            ),
            "DC Offset" => (
                BrowserRailTab::AudioFx,
                None,
                FX_ORANGE,
                "Adds constant DC offset or removes DC with a 5 Hz high-pass filter.",
            ),
            "Dual Pan" => (
                BrowserRailTab::AudioFx,
                None,
                FX_ORANGE,
                "Independent stereo panning for Left and Right input channels.",
            ),
            "Test Tone" => (
                BrowserRailTab::AudioFx,
                None,
                FX_ORANGE,
                "Diagnostic sine, white noise and pink noise generator.",
            ),
            "Time Shift" => (
                BrowserRailTab::AudioFx,
                None,
                FX_ORANGE,
                "Precise millisecond and sub-sample delay utility.",
            ),
            "Tremolo" => (
                BrowserRailTab::AudioFx,
                None,
                FX_ORANGE,
                "Stereo LFO amplitude modulator with stereo phase offset.",
            ),
            "Ring-Mod" => (
                BrowserRailTab::AudioFx,
                None,
                FX_ORANGE,
                "Carrier ring modulator with multiple waveforms and dry/wet mix.",
            ),
            "Hikaru Open8-Bit" | "Bit-8" => (
                BrowserRailTab::AudioFx,
                None,
                FX_ORANGE,
                "ADAA1 digital degrader: Clock/Jitter/Chance, Gate, Shape (Rectify + Clip/Fold/Wrap), Quantize (Off/On-zero, Diffusion, Dither, x²), Anti-alias & Wet FX.",
            ),
            "Hikaru OpenFreqShifter+" | "Freq Shifter+" | "Freq Shifter" => (
                BrowserRailTab::AudioFx,
                None,
                FX_ORANGE,
                "SSB quadrature Hilbert frequency shifter (6P/12P/18P/24P), 4x oversampling, Stereo Twist, Feedback Delay, FB Cuts, FB Shift & nested Wet FX.",
            ),
            "Hikaru OpenPhaser+" | "OpenPhaser+" | "Phaser+" => (
                BrowserRailTab::AudioFx,
                None,
                FX_ORANGE,
                "Multi-model analog phaser (GS 6-stage optical, EHx 4-stage OTA, MX 4-stage JFET, MF 12-stage synth) with Color, FB, Speed, Depth, Polarity Invert (∅) & Stereo (⚭).",
            ),
            "Hikaru OpenFlanger+" | "OpenFlanger+" | "Flanger+" => (
                BrowserRailTab::AudioFx,
                None,
                FX_ORANGE,
                "Multi-model flanger (DP digital gritty, MX classic pedal, TFX smooth sparkly, WA wide studio) with Color, FB, Speed, Depth, Polarity Invert (∅) & Stereo (⚭).",
            ),
            _ => (
                BrowserRailTab::AudioFx,
                None,
                FX_ORANGE,
                "Hikaru built-in DSP device.",
            ),
        };

        items.push(BrowserItem {
            id: format!("native:{}", entry.name),
            name: entry.label.to_string(),
            category: entry.category.to_string(),
            creator: "Hikaru".to_string(),
            location: "Built-in".to_string(),
            file_kind: "Native Device".to_string(),
            device_type: entry.category.to_string(),
            tags: vec![entry.category.to_string(), "Native".to_string()],
            description: desc.to_string(),
            rail,
            quick,
            badge_color,
            payload: BrowserPayload::Native {
                name: entry.name.to_string(),
            },
        });
    }

    for plug in discovered_plugins {
        let fmt_label = match plug.format {
            PluginFormat::VST3 => "VST3",
            PluginFormat::CLAP => "CLAP",
        };
        let (rail, cat, badge_color) = match plug.kind {
            PluginKind::Instrument => (
                BrowserRailTab::Instruments,
                format!("{fmt_label} Instrument"),
                INST_YELLOW,
            ),
            PluginKind::Effect => (
                BrowserRailTab::AudioFx,
                format!("{fmt_label} Effect"),
                FX_ORANGE,
            ),
        };
        let vendor = if plug.vendor.trim().is_empty() {
            "Unknown".to_string()
        } else {
            plug.vendor.clone()
        };
        items.push(BrowserItem {
            id: plug.id.clone(),
            name: plug.name.clone(),
            category: cat.clone(),
            creator: vendor.clone(),
            location: "External Plugins".to_string(),
            file_kind: fmt_label.to_string(),
            device_type: match plug.kind {
                PluginKind::Instrument => "Instrument".to_string(),
                PluginKind::Effect => "Audio FX".to_string(),
            },
            tags: vec![fmt_label.to_string(), vendor],
            description: format!(
                "{fmt_label} plugin by {}. Path: {}",
                plug.vendor,
                plug.path.display()
            ),
            rail,
            quick: None,
            badge_color,
            payload: BrowserPayload::External(plug.clone()),
        });
    }

    items.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    items
}

fn matches_rail(item: &BrowserItem, rail: BrowserRailTab) -> bool {
    match rail {
        BrowserRailTab::Everything => true,
        BrowserRailTab::Instruments => item.rail == BrowserRailTab::Instruments,
        BrowserRailTab::AudioFx => item.rail == BrowserRailTab::AudioFx,
        BrowserRailTab::NoteFx => item.rail == BrowserRailTab::NoteFx,
        BrowserRailTab::Plugins => matches!(item.payload, BrowserPayload::External(_)),
    }
}

fn filter_values_for_tab(
    items: &[BrowserItem],
    tab: MetadataFilterTab,
) -> Vec<(String, usize)> {
    let mut counts: BTreeMap<String, usize> = BTreeMap::new();
    for item in items {
        match tab {
            MetadataFilterTab::Location => {
                *counts.entry(item.location.clone()).or_insert(0) += 1;
            }
            MetadataFilterTab::FileKind => {
                *counts.entry(item.file_kind.clone()).or_insert(0) += 1;
            }
            MetadataFilterTab::Category => {
                *counts.entry(item.category.clone()).or_insert(0) += 1;
            }
            MetadataFilterTab::Creator => {
                *counts.entry(item.creator.clone()).or_insert(0) += 1;
            }
            MetadataFilterTab::Device => {
                *counts.entry(item.device_type.clone()).or_insert(0) += 1;
            }
            MetadataFilterTab::Tags => {
                for tag in &item.tags {
                    *counts.entry(tag.clone()).or_insert(0) += 1;
                }
            }
        }
    }
    counts.into_iter().collect()
}

fn item_matches_metadata(
    item: &BrowserItem,
    tab: MetadataFilterTab,
    value: &str,
) -> bool {
    match tab {
        MetadataFilterTab::Location => item.location == value,
        MetadataFilterTab::FileKind => item.file_kind == value,
        MetadataFilterTab::Category => item.category == value,
        MetadataFilterTab::Creator => item.creator == value,
        MetadataFilterTab::Device => item.device_type == value,
        MetadataFilterTab::Tags => item.tags.iter().any(|t| t == value),
    }
}

fn paint_piano_badge(painter: &egui::Painter, rect: Rect, bg: Color32) {
    painter.rect_filled(rect, 4.0, bg);
    painter.rect_stroke(
        rect,
        4.0,
        Stroke::new(1.0_f32, Color32::from_black_alpha(90)),
        egui::StrokeKind::Inside,
    );
    let inner = rect.shrink(rect.width() * 0.16);
    let key_col = Color32::from_rgb(0x1B, 0x1A, 0x14);
    // Outer keyboard outline
    painter.rect_stroke(
        inner,
        1.5,
        Stroke::new((rect.width() * 0.055).max(1.2), key_col),
        egui::StrokeKind::Inside,
    );
    // White key dividers
    let w = inner.width();
    let h = inner.height();
    for i in 1..4 {
        let x = inner.min.x + w * (i as f32 / 4.0);
        painter.line_segment(
            [pos2(x, inner.min.y), pos2(x, inner.max.y)],
            Stroke::new((rect.width() * 0.04).max(1.0), key_col),
        );
    }
    // Black keys on top half
    let bk_w = (w * 0.14).max(2.0);
    let bk_h = h * 0.56;
    for frac in [0.25_f32, 0.5, 0.75] {
        let cx = inner.min.x + w * frac;
        let bk = Rect::from_min_max(
            pos2(cx - bk_w * 0.5, inner.min.y),
            pos2(cx + bk_w * 0.5, inner.min.y + bk_h),
        );
        painter.rect_filled(bk, 0.5, key_col);
    }
}

fn paint_device_badge(painter: &egui::Painter, rect: Rect, item: &BrowserItem) {
    if matches!(item.payload, BrowserPayload::Wavetable(_)) {
        painter.rect_filled(rect, 3.5, Color32::from_rgb(0x28, 0x22, 0x14));
        painter.rect_stroke(
            rect,
            3.5,
            Stroke::new(1.0_f32, Color32::from_rgb(0x6E, 0x54, 0x1E)),
            egui::StrokeKind::Inside,
        );
        let inner = rect.shrink(rect.width() * 0.16);
        let pts: Vec<egui::Pos2> = (0..=10)
            .map(|i| {
                let t = i as f32 / 10.0;
                let y = (t * std::f32::consts::TAU).sin() * (inner.height() * 0.38);
                pos2(inner.min.x + t * inner.width(), inner.center().y - y)
            })
            .collect();
        painter.add(egui::Shape::line(pts, Stroke::new(1.4_f32, INST_YELLOW)));
        return;
    }
    match item.rail {
        BrowserRailTab::Instruments => paint_piano_badge(painter, rect, item.badge_color),
        BrowserRailTab::AudioFx => {
            painter.rect_filled(rect, 3.5, item.badge_color);
            painter.circle_stroke(
                rect.center(),
                rect.width() * 0.25,
                Stroke::new(1.4_f32, Color32::from_rgb(0x1E, 0x14, 0x0C)),
            );
        }
        BrowserRailTab::NoteFx => {
            painter.rect_filled(rect, 3.5, item.badge_color);
            let c = rect.center();
            painter.circle_filled(
                pos2(c.x - rect.width() * 0.1, c.y + rect.height() * 0.14),
                rect.width() * 0.14,
                Color32::from_rgb(0x10, 0x1A, 0x24),
            );
            painter.line_segment(
                [
                    pos2(c.x + rect.width() * 0.04, c.y + rect.height() * 0.14),
                    pos2(c.x + rect.width() * 0.04, c.y - rect.height() * 0.22),
                ],
                Stroke::new(1.6_f32, Color32::from_rgb(0x10, 0x1A, 0x24)),
            );
        }
        _ => {
            painter.rect_filled(rect, 3.5, item.badge_color);
            painter.rect_stroke(
                rect.shrink(rect.width() * 0.24),
                1.5,
                Stroke::new(1.4_f32, Color32::from_rgb(0x18, 0x1A, 0x20)),
                egui::StrokeKind::Inside,
            );
        }
    }
}

fn paint_quick_filter_icon(
    painter: &egui::Painter,
    rect: Rect,
    filter: QuickInstrumentFilter,
    col: Color32,
) {
    let c = rect.center();
    match filter {
        QuickInstrumentFilter::Drums => {
            // 3x3 drum pad grid
            let step = 4.2;
            for gy in -1..=1 {
                for gx in -1..=1 {
                    let p = pos2(c.x + gx as f32 * step, c.y + gy as f32 * step);
                    painter.rect_filled(Rect::from_center_size(p, vec2(2.6, 2.6)), 0.5, col);
                }
            }
        }
        QuickInstrumentFilter::Bass => {
            // Bass clef / low-end arc + 2 dots
            painter.circle_stroke(pos2(c.x - 1.5, c.y - 1.0), 4.0, Stroke::new(1.5_f32, col));
            painter.circle_filled(pos2(c.x + 4.5, c.y - 2.5), 1.3, col);
            painter.circle_filled(pos2(c.x + 4.5, c.y + 1.5), 1.3, col);
        }
        QuickInstrumentFilter::Synth => {
            // Oscilloscope / keyboard synth
            let r = Rect::from_center_size(c, vec2(12.0, 9.0));
            painter.rect_stroke(r, 1.0, Stroke::new(1.2_f32, col), egui::StrokeKind::Inside);
            painter.line_segment(
                [pos2(r.min.x + 2.0, c.y), pos2(c.x - 1.0, r.min.y + 2.0)],
                Stroke::new(1.2_f32, col),
            );
            painter.line_segment(
                [pos2(c.x - 1.0, r.min.y + 2.0), pos2(c.x + 1.0, r.max.y - 2.0)],
                Stroke::new(1.2_f32, col),
            );
            painter.line_segment(
                [pos2(c.x + 1.0, r.max.y - 2.0), pos2(r.max.x - 2.0, c.y)],
                Stroke::new(1.2_f32, col),
            );
        }
        QuickInstrumentFilter::Sampler => {
            // Mini piano keys
            let r = Rect::from_center_size(c, vec2(12.0, 9.0));
            painter.rect_stroke(r, 1.0, Stroke::new(1.2_f32, col), egui::StrokeKind::Inside);
            for i in 1..4 {
                let x = r.min.x + r.width() * (i as f32 / 4.0);
                painter.line_segment(
                    [pos2(x, r.min.y), pos2(x, r.center().y + 1.0)],
                    Stroke::new(1.3_f32, col),
                );
            }
        }
        QuickInstrumentFilter::Modulation => {
            // LFO wave
            let pts = [
                pos2(c.x - 6.0, c.y + 2.0),
                pos2(c.x - 2.0, c.y - 4.0),
                pos2(c.x + 2.0, c.y + 4.0),
                pos2(c.x + 6.0, c.y - 2.0),
            ];
            for w in pts.windows(2) {
                painter.line_segment([w[0], w[1]], Stroke::new(1.4_f32, col));
            }
        }
    }
}

fn apply_browser_selection(state: &mut AppState, item: &BrowserItem) {
    let build_slot = |item: &BrowserItem| -> DspSlot {
        match &item.payload {
            BrowserPayload::Native { name } => DspSlot::new(0, name.clone()),
            BrowserPayload::External(desc) => {
                let mut slot = DspSlot::default();
                slot.name = desc.name.clone();
                slot.has_gui = Some(desc.has_gui);
                slot.external = Some(desc.clone());
                slot
            }
            BrowserPayload::Modulator { .. }
            | BrowserPayload::Wavetable(_)
            | BrowserPayload::BitwigPreset(_) => DspSlot::default(),
        }
    };

    if let BrowserPayload::BitwigPreset(path) = &item.payload {
        if crate::project::bwclip::is_bwclip_file(path) {
            let (t_idx, s_idx) = state.matrix_state.selected_slot.unwrap_or((0, 0));
            let safe_t = t_idx.min(state.matrix_state.tracks.len().saturating_sub(1));
            let safe_s = s_idx.min(state.matrix_state.scenes.len().saturating_sub(1));
            crate::views::matrix::load_clip_into_slot(
                &mut state.matrix_state,
                &state.audio_proxy,
                safe_t,
                safe_s,
                path.clone(),
                state.transport.bpm,
            );
            state.project_dirty = true;
            state.popup_browser.open = false;
            return;
        }
        if crate::project::bwdevice_shim::is_bwremotecontrols_file(path) {
            let track_idx = match state.popup_browser.mode {
                BrowserTargetMode::InsertDevice { track_idx } => track_idx,
                BrowserTargetMode::InsertModulator { track_idx, .. } => track_idx,
                _ => state.safe_track_index(),
            };
            let _ = crate::views::remote_controls::load_bwremotecontrols_from_path(
                state,
                track_idx,
                None,
                path,
            );
            state.show_dsp_rack = true;
            state.popup_browser.open = false;
            return;
        }
        if crate::project::bwextension_shim::is_bwextension_file(path) {
            if let Ok(report) = crate::project::bwextension_shim::inspect_bwextension_file(path) {
                crate::project::bwextension_shim::apply_bwextension_to_app(state, &report);
                let _ = hikaru_mcp_port::record_bitwig_drop(path, "popup-browser");
            }
            state.popup_browser.open = false;
            return;
        }
        match state.popup_browser.mode {
            BrowserTargetMode::NewInstrumentTrack => {
                state.matrix_state.add_track();
                state.audio_proxy.send(GuiCommand::AddTrack);
                let new_idx = state.matrix_state.tracks.len();
                let mut tr = Track::new(new_idx, item.name.clone(), false);
                tr.matrix_idx = Some(new_idx - 1);
                if let Some(last) = state.matrix_state.tracks.last_mut() {
                    last.name = item.name.clone();
                }
                state.live_tracks.push(tr.clone());
                state.studio_tracks.push(tr);
                let mixer_idx = state.current_tracks().len().saturating_sub(1);
                state.selected_track_index = mixer_idx;
                state.selected_fx_index = None;
                crate::views::dsp_rack::load_device_preset_from_path(state, mixer_idx, path);
                state.show_dsp_rack = true;
            }
            BrowserTargetMode::InsertDevice { track_idx } => {
                crate::views::dsp_rack::load_device_preset_from_path(state, track_idx, path);
                state.selected_track_index = track_idx;
                state.show_dsp_rack = true;
            }
            BrowserTargetMode::InsertModulator {
                track_idx,
                slot_idx,
            } => {
                if crate::project::bwmodulator_shim::is_bwmodulator_file(path) {
                    if let Ok(translation) =
                        crate::project::bwmodulator_shim::load_bwmodulator_file(path, None)
                    {
                        crate::project::bwmodulator_shim::instantiate_bwmodulator_in_bay(
                            track_idx,
                            slot_idx,
                            &translation,
                        );
                        state.project_dirty = true;
                    }
                }
            }
            _ => {}
        }
        state.popup_browser.open = false;
        return;
    }

    match state.popup_browser.mode {
        BrowserTargetMode::NewInstrumentTrack => {
            state.matrix_state.add_track();
            state.audio_proxy.send(GuiCommand::AddTrack);
            let new_idx = state.matrix_state.tracks.len();
            let track_name = match &item.payload {
                BrowserPayload::External(desc) => desc.name.clone(),
                _ => format!("Inst {new_idx}"),
            };
            if let Some(last) = state.matrix_state.tracks.last_mut() {
                last.name = track_name.clone();
            }
            let mut tr = Track::new(new_idx, track_name, false);
            tr.matrix_idx = Some(new_idx - 1);
            let mut slot = build_slot(item);
            let slot_name = slot.name.clone();
            let ext_desc = slot.external.clone();
            if ext_desc.is_some() {
                slot.engine_loaded = true;
            }
            tr.effects.push(slot);

            state.live_tracks.push(tr.clone());
            state.studio_tracks.push(tr);

            let mixer_idx = state.current_tracks().len().saturating_sub(1);
            state.selected_track_index = mixer_idx;
            state.selected_fx_index = None;
            state.selected_slot_index = 0;
            state.show_dsp_rack = true;
            state.project_dirty = true;

            if let Some(desc) = ext_desc {
                hikaru_openmodulation::ensure_empty_bay_for_slot(mixer_idx, 0);
                if let Some(et_idx) = mixer_idx.checked_sub(1) {
                    state.audio_proxy.send(GuiCommand::LoadExternalPlugin {
                        track_idx: et_idx,
                        slot_idx: 0,
                        descriptor: desc.clone(),
                    });
                }
                state.request_external_params(&desc);
            } else if crate::views::hikaru_native_plugins::hikaru_opencomb::is_opencomb_name(&slot_name)
                || crate::views::hikaru_native_plugins::hikaru_bit8::is_bit8_name(&slot_name)
                || crate::views::hikaru_native_plugins::hikaru_freq_shifter_plus::is_freq_shifter_plus_name(&slot_name)
                || crate::views::hikaru_native_plugins::hikaru_tier_s_fx::is_tier_s_fx(&slot_name)
            {
                if let Some(et_idx) = mixer_idx.checked_sub(1) {
                    state.audio_proxy.send(GuiCommand::LoadNativeEffect {
                        track_idx: et_idx,
                        slot_idx: 0,
                        name: slot_name,
                    });
                }
            }
        }
        BrowserTargetMode::InsertDevice { track_idx } => {
            let mut slot = build_slot(item);
            let slot_name = slot.name.clone();
            let ext_desc = slot.external.clone();
            if ext_desc.is_some() {
                slot.engine_loaded = true;
            }
            let mut inserted_slot: Option<usize> = None;
            if let Some(track) = state.current_tracks_mut().get_mut(track_idx) {
                let new_slot_idx = track.effects.len();
                track.effects.push(slot);
                inserted_slot = Some(new_slot_idx);
            }
            if let Some(new_slot_idx) = inserted_slot {
                state.selected_track_index = track_idx;
                state.selected_slot_index = new_slot_idx;
                state.show_dsp_rack = true;
                state.project_dirty = true;

                if let Some(desc) = ext_desc {
                    hikaru_openmodulation::ensure_empty_bay_for_slot(track_idx, new_slot_idx);
                    if let Some(et_idx) = track_idx.checked_sub(1) {
                        state.audio_proxy.send(GuiCommand::LoadExternalPlugin {
                            track_idx: et_idx,
                            slot_idx: new_slot_idx,
                            descriptor: desc.clone(),
                        });
                    }
                    state.request_external_params(&desc);
                } else if crate::views::hikaru_native_plugins::hikaru_opencomb::is_opencomb_name(&slot_name)
                || crate::views::hikaru_native_plugins::hikaru_bit8::is_bit8_name(&slot_name)
                || crate::views::hikaru_native_plugins::hikaru_freq_shifter_plus::is_freq_shifter_plus_name(&slot_name)
                || crate::views::hikaru_native_plugins::hikaru_openphaser_plus::is_openphaser_plus_name(&slot_name)
                || crate::views::hikaru_native_plugins::hikaru_openflanger_plus::is_openflanger_plus_name(&slot_name)
                || crate::views::hikaru_native_plugins::hikaru_tier_s_fx::is_tier_s_fx(&slot_name)
            {
                    if let Some(et_idx) = track_idx.checked_sub(1) {
                        state.audio_proxy.send(GuiCommand::LoadNativeEffect {
                            track_idx: et_idx,
                            slot_idx: new_slot_idx,
                            name: slot_name,
                        });
                    }
                }
            }
        }
        BrowserTargetMode::InsertModulator {
            track_idx,
            slot_idx,
        } => {
            if let BrowserPayload::Modulator { kind } = item.payload {
                hikaru_openmodulation::add_bay_modulator(track_idx, slot_idx, kind);
                state.project_dirty = true;
            }
        }
        BrowserTargetMode::SelectWavetable {
            track_idx,
            slot_idx,
        } => {
            if let BrowserPayload::Wavetable(source) = &item.payload {
                if let Some(table) = hikaru_openpoly::load_wavetable_from_source(source) {
                    if let Some(track) = state.current_tracks_mut().get_mut(track_idx) {
                        if let Some(slot) = track.effects.get_mut(slot_idx) {
                            slot.open_poly_mut().load_wavetable(table);
                            state.project_dirty = true;
                        }
                    }
                }
            }
        }
        BrowserTargetMode::InsertWetFx {
            track_idx,
            parent_slot_idx,
        } => {
            let mut slot = build_slot(item);
            let slot_name = slot.name.clone();
            let ext_desc = slot.external.clone();
            if ext_desc.is_some() {
                slot.engine_loaded = true;
            }
            let mut inserted_wet_idx: Option<usize> = None;
            if let Some(track) = state.current_tracks_mut().get_mut(track_idx) {
                if let Some(parent) = track.effects.get_mut(parent_slot_idx) {
                    let wet_idx = parent.wet_fx_chain.len();
                    parent.wet_fx_chain.push(slot);
                    inserted_wet_idx = Some(wet_idx);
                }
            }
            if let Some(wet_idx) = inserted_wet_idx {
                state.selected_track_index = track_idx;
                state.selected_slot_index = parent_slot_idx;
                state.show_dsp_rack = true;
                state.project_dirty = true;

                if let Some(desc) = ext_desc {
                    if let Some(et_idx) = track_idx.checked_sub(1) {
                        state.audio_proxy.send(GuiCommand::LoadBit8WetExternalPlugin {
                            track_idx: et_idx,
                            slot_idx: parent_slot_idx,
                            wet_idx,
                            descriptor: desc.clone(),
                        });
                    }
                    state.request_external_params(&desc);
                } else if crate::views::hikaru_native_plugins::hikaru_opencomb::is_opencomb_name(&slot_name)
                    || crate::views::hikaru_native_plugins::hikaru_bit8::is_bit8_name(&slot_name)
                    || crate::views::hikaru_native_plugins::hikaru_freq_shifter_plus::is_freq_shifter_plus_name(&slot_name)
                    || crate::views::hikaru_native_plugins::hikaru_openphaser_plus::is_openphaser_plus_name(&slot_name)
                    || crate::views::hikaru_native_plugins::hikaru_openflanger_plus::is_openflanger_plus_name(&slot_name)
                || crate::views::hikaru_native_plugins::hikaru_openflanger_plus::is_openflanger_plus_name(&slot_name)
                    || crate::views::hikaru_native_plugins::hikaru_tier_s_fx::is_tier_s_fx(&slot_name)
                {
                    if let Some(et_idx) = track_idx.checked_sub(1) {
                        state.audio_proxy.send(GuiCommand::LoadBit8WetNativeEffect {
                            track_idx: et_idx,
                            slot_idx: parent_slot_idx,
                            wet_idx,
                            name: slot_name,
                        });
                    }
                }
            }
        }
    }
    state.popup_browser.open = false;
}

pub fn render(ctx: &Context, state: &mut AppState) {
    if !state.popup_browser.open {
        return;
    }

    // Auto-scan VST3/CLAP plugins the first time the Pop-up Browser is opened
    if !state.plugin_settings_state.has_scanned_once
        && !state.plugin_settings_state.is_scanning
        && !matches!(
            state.popup_browser.mode,
            BrowserTargetMode::InsertModulator { .. } | BrowserTargetMode::SelectWavetable { .. }
        )
    {
        state.plugin_settings_state.start_scan();
    }

    // Escape closes the browser
    if ctx.input(|i| i.key_pressed(Key::Escape)) {
        state.popup_browser.open = false;
        return;
    }

    let screen = ctx.screen_rect();
    let modal_w = (screen.width() - 40.0).clamp(580.0, 790.0);
    let modal_h = (screen.height() - 60.0).clamp(360.0, 430.0);
    let modal_pos = pos2(
        (screen.center().x - modal_w * 0.5).round(),
        (screen.center().y - modal_h * 0.5).round(),
    );

    // Subtle modal backdrop dimmer
    let dim_layer = egui::LayerId::new(Order::Foreground, Id::new("bitwig_popup_browser_dim"));
    let dim_painter = ctx.layer_painter(dim_layer);
    dim_painter.rect_filled(screen, 0.0, Color32::from_black_alpha(70));

    let all_items = match state.popup_browser.mode {
        BrowserTargetMode::InsertModulator { .. } => {
            let mut items = build_modulator_items();
            items.extend(build_bitwig_preset_items(
                &state.popup_browser.bitwig_preset_catalog,
                true,
            ));
            items
        }
        BrowserTargetMode::SelectWavetable { .. } => {
            build_wavetable_items(&state.popup_browser.wavetable_catalog)
        }
        _ => {
            let mut items = build_catalog_items(&state.plugin_settings_state.discovered_plugins);
            items.extend(build_bitwig_preset_items(
                &state.popup_browser.bitwig_preset_catalog,
                false,
            ));
            items
        }
    };

    // Filter by rail + favorites + quick filter
    let rail_items: Vec<BrowserItem> = all_items
        .iter()
        .filter(|it| matches_rail(it, state.popup_browser.active_rail))
        .filter(|it| {
            !state.popup_browser.favorites_only || state.popup_browser.favorites.contains(&it.id)
        })
        .filter(|it| {
            state
                .popup_browser
                .quick_filter
                .is_none_or(|qf| it.quick == Some(qf))
        })
        .cloned()
        .collect();

    let filter_values = filter_values_for_tab(&rail_items, state.popup_browser.metadata_tab);

    // Apply metadata filter + search query
    let query_low = state.popup_browser.search_query.trim().to_lowercase();
    let filtered_items: Vec<BrowserItem> = rail_items
        .iter()
        .filter(|it| {
            state.popup_browser.selected_filter_value.as_ref().is_none_or(
                |val| item_matches_metadata(it, state.popup_browser.metadata_tab, val),
            )
        })
        .filter(|it| {
            query_low.is_empty()
                || it.name.to_lowercase().contains(&query_low)
                || it.category.to_lowercase().contains(&query_low)
                || it.creator.to_lowercase().contains(&query_low)
                || it.file_kind.to_lowercase().contains(&query_low)
        })
        .cloned()
        .collect();

    if !filtered_items.is_empty() {
        state.popup_browser.selected_item_idx = state
            .popup_browser
            .selected_item_idx
            .min(filtered_items.len() - 1);
    } else {
        state.popup_browser.selected_item_idx = 0;
    }

    // Keyboard navigation (Up/Down/Enter)
    let mut confirm_item: Option<BrowserItem> = None;
    if !filtered_items.is_empty() {
        if ctx.input(|i| i.key_pressed(Key::ArrowDown)) {
            state.popup_browser.selected_item_idx =
                (state.popup_browser.selected_item_idx + 1).min(filtered_items.len() - 1);
        }
        if ctx.input(|i| i.key_pressed(Key::ArrowUp)) {
            state.popup_browser.selected_item_idx =
                state.popup_browser.selected_item_idx.saturating_sub(1);
        }
        if ctx.input(|i| i.key_pressed(Key::Enter)) {
            confirm_item = filtered_items
                .get(state.popup_browser.selected_item_idx)
                .cloned();
        }
    }

    let mut close_requested = false;

    egui::Area::new(Id::new("bitwig_popup_browser_area"))
        .order(Order::Foreground)
        .fixed_pos(modal_pos)
        .show(ctx, |ui| {
            let outer_rect = Rect::from_min_size(modal_pos, vec2(modal_w, modal_h));
            ui.painter().rect_filled(outer_rect, 4.0, FRAME_BG);
            ui.painter().rect_stroke(
                outer_rect,
                4.0,
                Stroke::new(1.5_f32, FRAME_BORDER_OUTER),
                egui::StrokeKind::Outside,
            );
            ui.painter().rect_stroke(
                outer_rect.shrink(0.5),
                3.5,
                Stroke::new(1.0_f32, FRAME_BORDER_INNER),
                egui::StrokeKind::Inside,
            );

            let content_rect = outer_rect.shrink(6.0);
            let rail_w = 34.0;
            let sidebar_w = 196.0;
            let right_w = 164.0;
            let gap = 5.0;
            let center_w = (content_rect.width() - rail_w - sidebar_w - right_w - gap * 3.0).max(200.0);

            let rail_rect = Rect::from_min_size(
                content_rect.min,
                vec2(rail_w, content_rect.height()),
            );
            let sidebar_rect = Rect::from_min_size(
                pos2(rail_rect.max.x + gap, content_rect.min.y),
                vec2(sidebar_w, content_rect.height()),
            );
            let center_rect = Rect::from_min_size(
                pos2(sidebar_rect.max.x + gap, content_rect.min.y),
                vec2(center_w, content_rect.height()),
            );
            let right_rect = Rect::from_min_size(
                pos2(center_rect.max.x + gap, content_rect.min.y),
                vec2(right_w, content_rect.height()),
            );

            // =========================================================
            // 1. LEFTMOST VERTICAL ICON RAIL (~34px)
            // =========================================================
            ui.painter().rect_filled(rail_rect, 2.0, RAIL_BG);
            ui.allocate_new_ui(
                UiBuilder::new()
                    .max_rect(rail_rect)
                    .layout(Layout::top_down(Align::Min)),
                |ui| {
                    let rail_tabs = [
                        (BrowserRailTab::Everything, "Everything (All Devices)"),
                        (BrowserRailTab::Instruments, "All Instruments"),
                        (BrowserRailTab::AudioFx, "Audio FX"),
                        (BrowserRailTab::NoteFx, "Note & Modulation"),
                        (BrowserRailTab::Plugins, "External Plugins (VST3 / CLAP)"),
                    ];

                    for (idx, (tab, tooltip)) in rail_tabs.iter().enumerate() {
                        let y_off = if idx == 0 { 6.0 } else { 18.0 + idx as f32 * 34.0 };
                        let btn_r = Rect::from_min_size(
                            pos2(rail_rect.min.x + 4.0, rail_rect.min.y + y_off),
                            vec2(26.0, 26.0),
                        );
                        let resp = ui.interact(
                            btn_r,
                            Id::new(("bw_rail_tab", idx)),
                            Sense::click(),
                        );
                        let active = state.popup_browser.active_rail == *tab;
                        if active {
                            ui.painter().rect_filled(
                                btn_r,
                                3.0,
                                Color32::from_rgb(0x3D, 0x3D, 0x36),
                            );
                            ui.painter().rect_stroke(
                                btn_r,
                                3.0,
                                Stroke::new(1.0_f32, tab.accent()),
                                egui::StrokeKind::Inside,
                            );
                        } else if resp.hovered() {
                            ui.painter().rect_filled(
                                btn_r,
                                3.0,
                                Color32::from_rgb(0x34, 0x36, 0x3C),
                            );
                        }

                        let icon_col = if active {
                            tab.accent()
                        } else {
                            Color32::from_rgb(0x88, 0x8C, 0x94)
                        };
                        let ir = btn_r.shrink(5.0);
                        match tab {
                            BrowserRailTab::Everything => {
                                let s = 6.0;
                                for gy in 0..2 {
                                    for gx in 0..2 {
                                        let p = pos2(
                                            ir.min.x + 2.0 + gx as f32 * 8.0,
                                            ir.min.y + 2.0 + gy as f32 * 8.0,
                                        );
                                        ui.painter().rect_filled(
                                            Rect::from_min_size(p, vec2(s, s)),
                                            1.0,
                                            icon_col,
                                        );
                                    }
                                }
                            }
                            BrowserRailTab::Instruments => {
                                paint_piano_badge(ui.painter(), ir, icon_col);
                            }
                            BrowserRailTab::AudioFx => {
                                ui.painter().rect_filled(ir, 2.5, icon_col);
                                ui.painter().circle_filled(
                                    ir.center(),
                                    3.0,
                                    Color32::from_rgb(0x20, 0x18, 0x10),
                                );
                            }
                            BrowserRailTab::NoteFx => {
                                ui.painter().rect_filled(ir, 2.5, icon_col);
                                ui.painter().circle_filled(
                                    pos2(ir.center().x - 1.5, ir.center().y + 2.0),
                                    2.2,
                                    Color32::from_rgb(0x14, 0x1C, 0x24),
                                );
                            }
                            BrowserRailTab::Plugins => {
                                ui.painter().rect_stroke(
                                    ir.shrink(1.0),
                                    2.0,
                                    Stroke::new(1.5_f32, icon_col),
                                    egui::StrokeKind::Inside,
                                );
                                ui.painter().circle_filled(ir.center(), 2.5, icon_col);
                            }
                        }

                        if resp.on_hover_text(*tooltip).clicked() {
                            state.popup_browser.active_rail = *tab;
                            state.popup_browser.selected_filter_value = None;
                            state.popup_browser.quick_filter = None;
                            state.popup_browser.selected_item_idx = 0;
                        }
                    }

                    ui.painter().line_segment(
                        [
                            pos2(rail_rect.min.x + 6.0, rail_rect.min.y + 40.0),
                            pos2(rail_rect.max.x - 6.0, rail_rect.min.y + 40.0),
                        ],
                        Stroke::new(1.0_f32, Color32::from_rgb(0x3C, 0x3E, 0x44)),
                    );
                },
            );

            // =========================================================
            // 2. LEFT FILTER SIDEBAR (~196px)
            // =========================================================
            ui.painter().rect_filled(sidebar_rect, 2.0, SIDEBAR_BG);
            ui.allocate_new_ui(
                UiBuilder::new()
                    .max_rect(sidebar_rect.shrink(6.0))
                    .layout(Layout::top_down(Align::Min)),
                |ui| {
                    ui.spacing_mut().item_spacing = vec2(0.0, 4.0);
                    let sb_w = ui.available_width();

                    // Header: "All Instruments" + Favorite Star toggle
                    let (hdr_r, _) = ui.allocate_exact_size(vec2(sb_w, 20.0), Sense::hover());
                    let header_title = if matches!(
                        state.popup_browser.mode,
                        BrowserTargetMode::SelectWavetable { .. }
                    ) {
                        "Wavetables"
                    } else {
                        state.popup_browser.active_rail.title()
                    };
                    ui.painter().text(
                        pos2(hdr_r.min.x + 2.0, hdr_r.center().y),
                        Align2::LEFT_CENTER,
                        header_title,
                        fonts::semibold(11.5),
                        Color32::from_rgb(0xDD, 0xE0, 0xE6),
                    );
                    let star_r = Rect::from_min_size(
                        pos2(hdr_r.max.x - 20.0, hdr_r.center().y - 9.0),
                        vec2(18.0, 18.0),
                    );
                    let star_resp = ui.interact(star_r, Id::new("bw_popup_fav_filter"), Sense::click());
                    let star_col = if state.popup_browser.favorites_only {
                        INST_YELLOW
                    } else if star_resp.hovered() {
                        Color32::from_rgb(0xCC, 0xCE, 0xD4)
                    } else {
                        Color32::from_rgb(0x8A, 0x8E, 0x96)
                    };
                    ui.painter().text(
                        star_r.center(),
                        Align2::CENTER_CENTER,
                        if state.popup_browser.favorites_only { "★" } else { "☆" },
                        FontId::proportional(13.0),
                        star_col,
                    );
                    if star_resp.on_hover_text("Filter Favorites Only").clicked() {
                        state.popup_browser.favorites_only = !state.popup_browser.favorites_only;
                        state.popup_browser.selected_item_idx = 0;
                    }

                    // Large centered yellow piano badge (52x52)
                    let (badge_row_r, _) = ui.allocate_exact_size(vec2(sb_w, 56.0), Sense::hover());
                    let big_r = Rect::from_center_size(badge_row_r.center(), vec2(52.0, 52.0));
                    paint_piano_badge(
                        ui.painter(),
                        big_r,
                        state.popup_browser.active_rail.accent(),
                    );

                    // 5 quick filter buttons
                    let quick_filters = [
                        (QuickInstrumentFilter::Drums, "Drums / Percussion"),
                        (QuickInstrumentFilter::Bass, "Bass"),
                        (QuickInstrumentFilter::Synth, "Synths / Polysynth"),
                        (QuickInstrumentFilter::Sampler, "Sampler / Keys"),
                        (QuickInstrumentFilter::Modulation, "Modulation / Note"),
                    ];
                    let (qf_row_r, _) = ui.allocate_exact_size(vec2(sb_w, 24.0), Sense::hover());
                    let qf_gap = 3.0;
                    let qf_btn_w = ((sb_w - qf_gap * 4.0) / 5.0).floor();
                    for (i, (qf, tip)) in quick_filters.iter().enumerate() {
                        let qr = Rect::from_min_size(
                            pos2(qf_row_r.min.x + i as f32 * (qf_btn_w + qf_gap), qf_row_r.min.y + 1.0),
                            vec2(qf_btn_w, 22.0),
                        );
                        let qresp = ui.interact(qr, Id::new(("bw_qf", i)), Sense::click());
                        let active = state.popup_browser.quick_filter == Some(*qf);
                        ui.painter().rect_filled(
                            qr,
                            2.5,
                            if active {
                                QUICK_BTN_ACTIVE_BG
                            } else if qresp.hovered() {
                                Color32::from_rgb(0x52, 0x4D, 0x3C)
                            } else {
                                QUICK_BTN_BG
                            },
                        );
                        if active {
                            ui.painter().rect_stroke(
                                qr,
                                2.5,
                                Stroke::new(1.0_f32, INST_YELLOW),
                                egui::StrokeKind::Inside,
                            );
                        }
                        paint_quick_filter_icon(
                            ui.painter(),
                            qr,
                            *qf,
                            if active {
                                INST_YELLOW
                            } else {
                                Color32::from_rgb(0xC8, 0xB8, 0x7A)
                            },
                        );
                        if qresp.on_hover_text(*tip).clicked() {
                            state.popup_browser.quick_filter = if active { None } else { Some(*qf) };
                            state.popup_browser.selected_item_idx = 0;
                        }
                    }

                    // 2-column Metadata Filter Tabs (3 rows x 2 cols)
                    let tab_col_w = ((sb_w - 4.0) * 0.5).floor();
                    for row in 0..3 {
                        let (row_r, _) = ui.allocate_exact_size(vec2(sb_w, 19.0), Sense::hover());
                        for col in 0..2 {
                            let tab = MetadataFilterTab::ALL[row * 2 + col];
                            let tr = Rect::from_min_size(
                                pos2(row_r.min.x + col as f32 * (tab_col_w + 4.0), row_r.min.y),
                                vec2(tab_col_w, 18.0),
                            );
                            let tresp = ui.interact(
                                tr,
                                Id::new(("bw_meta_tab", row, col)),
                                Sense::click(),
                            );
                            let active = state.popup_browser.metadata_tab == tab;
                            if active {
                                ui.painter().rect_filled(
                                    tr,
                                    2.5,
                                    Color32::from_rgb(0x24, 0x27, 0x2D),
                                );
                            } else if tresp.hovered() {
                                ui.painter().rect_filled(
                                    tr,
                                    2.5,
                                    Color32::from_rgb(0x37, 0x39, 0x3F),
                                );
                            }
                            let text_col = if active {
                                ACTIVE_BLUE_TEXT
                            } else {
                                Color32::from_rgb(0xC4, 0xC7, 0xCE)
                            };
                            ui.painter().text(
                                pos2(tr.min.x + 5.0, tr.center().y),
                                Align2::LEFT_CENTER,
                                format!("* {}", tab.label()),
                                fonts::regular(10.5),
                                text_col,
                            );
                            if tresp.clicked() {
                                state.popup_browser.metadata_tab = tab;
                                state.popup_browser.selected_filter_value = None;
                                state.popup_browser.selected_item_idx = 0;
                            }
                        }
                    }

                    // Thin divider
                    let (div_r, _) = ui.allocate_exact_size(vec2(sb_w, 6.0), Sense::hover());
                    ui.painter().line_segment(
                        [
                            pos2(div_r.min.x, div_r.center().y),
                            pos2(div_r.max.x, div_r.center().y),
                        ],
                        Stroke::new(1.0_f32, Color32::from_rgb(0x20, 0x21, 0x24)),
                    );

                    // Scrollable Filter Values list
                    let list_h = ui.available_height().max(40.0);
                    ScrollArea::vertical()
                        .id_salt("bw_popup_filter_values")
                        .max_height(list_h)
                        .auto_shrink([false; 2])
                        .show(ui, |ui| {
                            ui.spacing_mut().item_spacing.y = 1.0;
                            let rw = ui.available_width();

                            let any_sel = state.popup_browser.selected_filter_value.is_none();
                            let (any_r, any_resp) =
                                ui.allocate_exact_size(vec2(rw, 20.0), Sense::click());
                            if any_sel {
                                ui.painter().rect_filled(any_r, 2.0, FILTER_SEL_BG);
                            } else if any_resp.hovered() {
                                ui.painter().rect_filled(
                                    any_r,
                                    2.0,
                                    Color32::from_rgb(0x36, 0x38, 0x3E),
                                );
                            }
                            ui.painter().text(
                                pos2(any_r.min.x + 6.0, any_r.center().y),
                                Align2::LEFT_CENTER,
                                format!("* {}", state.popup_browser.metadata_tab.any_label()),
                                fonts::semibold(10.5),
                                if any_sel {
                                    Color32::WHITE
                                } else {
                                    Color32::from_rgb(0xD0, 0xD3, 0xDA)
                                },
                            );
                            ui.painter().text(
                                pos2(any_r.max.x - 6.0, any_r.center().y),
                                Align2::RIGHT_CENTER,
                                format!("{}", rail_items.len()),
                                fonts::regular(10.0),
                                theme::TEXT_MUTED,
                            );
                            if any_resp.clicked() {
                                state.popup_browser.selected_filter_value = None;
                                state.popup_browser.selected_item_idx = 0;
                            }

                            for (val, count) in &filter_values {
                                let is_sel = state
                                    .popup_browser
                                    .selected_filter_value
                                    .as_deref()
                                    == Some(val.as_str());
                                let (vr, vresp) =
                                    ui.allocate_exact_size(vec2(rw, 20.0), Sense::click());
                                if is_sel {
                                    ui.painter().rect_filled(vr, 2.0, FILTER_SEL_BG);
                                } else if vresp.hovered() {
                                    ui.painter().rect_filled(
                                        vr,
                                        2.0,
                                        Color32::from_rgb(0x36, 0x38, 0x3E),
                                    );
                                }
                                let tag_r = Rect::from_min_size(
                                    pos2(vr.min.x + 6.0, vr.center().y - 5.0),
                                    vec2(10.0, 10.0),
                                );
                                ui.painter().rect_filled(
                                    tag_r,
                                    2.0,
                                    Color32::from_rgb(0x70, 0x52, 0x52),
                                );
                                ui.painter().text(
                                    pos2(vr.min.x + 21.0, vr.center().y),
                                    Align2::LEFT_CENTER,
                                    val,
                                    fonts::regular(10.5),
                                    if is_sel {
                                        Color32::WHITE
                                    } else {
                                        Color32::from_rgb(0xC8, 0xCB, 0xD2)
                                    },
                                );
                                ui.painter().text(
                                    pos2(vr.max.x - 6.0, vr.center().y),
                                    Align2::RIGHT_CENTER,
                                    format!("{count}"),
                                    fonts::regular(10.0),
                                    theme::TEXT_MUTED,
                                );
                                if vresp.clicked() {
                                    state.popup_browser.selected_filter_value = Some(val.clone());
                                    state.popup_browser.selected_item_idx = 0;
                                }
                            }
                        });
                },
            );

            // =========================================================
            // 3. CENTER MAIN RESULTS PANE
            // =========================================================
            ui.painter().rect_filled(center_rect, 2.0, WELL_BG);
            ui.painter().rect_stroke(
                center_rect,
                2.0,
                Stroke::new(1.0_f32, Color32::from_rgb(0x0B, 0x0C, 0x0E)),
                egui::StrokeKind::Inside,
            );
            ui.allocate_new_ui(
                UiBuilder::new()
                    .max_rect(center_rect.shrink(4.0))
                    .layout(Layout::top_down(Align::Min)),
                |ui| {
                    ui.spacing_mut().item_spacing = vec2(0.0, 4.0);
                    let cw = ui.available_width();

                    // Search Bar
                    let (sb_rect, _) = ui.allocate_exact_size(vec2(cw, 24.0), Sense::hover());
                    ui.painter().rect_filled(sb_rect, 2.0, SEARCH_BG);
                    ui.painter().rect_stroke(
                        sb_rect,
                        2.0,
                        Stroke::new(1.0_f32, Color32::from_rgb(0x28, 0x2A, 0x30)),
                        egui::StrokeKind::Inside,
                    );

                    let mg_center = pos2(sb_rect.min.x + 12.0, sb_rect.center().y - 1.0);
                    ui.painter().circle_stroke(
                        mg_center,
                        4.2,
                        Stroke::new(1.4_f32, Color32::from_rgb(0xB0, 0xB4, 0xBC)),
                    );
                    ui.painter().line_segment(
                        [
                            pos2(mg_center.x + 3.0, mg_center.y + 3.0),
                            pos2(mg_center.x + 6.2, mg_center.y + 6.2),
                        ],
                        Stroke::new(1.5_f32, Color32::from_rgb(0xB0, 0xB4, 0xBC)),
                    );

                    let edit_rect = Rect::from_min_max(
                        pos2(sb_rect.min.x + 24.0, sb_rect.min.y + 3.0),
                        pos2(sb_rect.max.x - 24.0, sb_rect.max.y - 2.0),
                    );
                    let search_hint = if matches!(
                        state.popup_browser.mode,
                        BrowserTargetMode::SelectWavetable { .. }
                    ) {
                        "Search wavetables..."
                    } else {
                        "Search devices & plugins..."
                    };
                    let te_resp = ui.put(
                        edit_rect,
                        TextEdit::singleline(&mut state.popup_browser.search_query)
                            .frame(false)
                            .hint_text(search_hint)
                            .font(fonts::regular(11.5))
                            .text_color(Color32::from_rgb(0xEC, 0xEE, 0xF2)),
                    );
                    if state.popup_browser.focus_search {
                        te_resp.request_focus();
                        state.popup_browser.focus_search = false;
                    }

                    let clr_r = Rect::from_min_max(
                        pos2(sb_rect.max.x - 22.0, sb_rect.min.y + 2.0),
                        pos2(sb_rect.max.x - 4.0, sb_rect.max.y - 2.0),
                    );
                    let clr_resp =
                        ui.interact(clr_r, Id::new("bw_popup_search_clr"), Sense::click());
                    icons::paint_icon(
                        ui.painter(),
                        clr_r.shrink(4.0),
                        icons::X_CLOSE,
                        if clr_resp.hovered() {
                            Color32::WHITE
                        } else {
                            Color32::from_rgb(0x90, 0x94, 0x9C)
                        },
                        1.5,
                    );
                    if clr_resp.clicked() {
                        state.popup_browser.search_query.clear();
                        state.popup_browser.selected_item_idx = 0;
                    }

                    // Device Results List
                    let list_h = ui.available_height().max(40.0);
                    ScrollArea::vertical()
                        .id_salt("bw_popup_device_list")
                        .max_height(list_h)
                        .auto_shrink([false; 2])
                        .show(ui, |ui| {
                            ui.spacing_mut().item_spacing.y = 1.0;
                            let rw = ui.available_width();

                            if filtered_items.is_empty() {
                                ui.add_space(24.0);
                                ui.vertical_centered(|ui| {
                                    let empty_msg = if matches!(
                                        state.popup_browser.mode,
                                        BrowserTargetMode::SelectWavetable { .. }
                                    ) {
                                        "No matching wavetables"
                                    } else {
                                        "No matching instruments or devices"
                                    };
                                    ui.label(
                                        RichText::new(empty_msg)
                                            .font(fonts::regular(11.0))
                                            .color(theme::TEXT_MUTED),
                                    );
                                });
                                return;
                            }

                            for (idx, item) in filtered_items.iter().enumerate() {
                                let is_sel = idx == state.popup_browser.selected_item_idx;
                                let (row_r, row_resp) =
                                    ui.allocate_exact_size(vec2(rw, 24.0), Sense::click());

                                if is_sel {
                                    ui.painter().rect_filled(row_r, 2.0, ROW_SEL_BG);
                                } else if row_resp.hovered() {
                                    ui.painter().rect_filled(row_r, 2.0, ROW_HOVER_BG);
                                }

                                let badge_r = Rect::from_min_size(
                                    pos2(row_r.min.x + 6.0, row_r.center().y - 8.0),
                                    vec2(16.0, 16.0),
                                );
                                paint_device_badge(ui.painter(), badge_r, item);

                                ui.painter().text(
                                    pos2(row_r.min.x + 28.0, row_r.center().y),
                                    Align2::LEFT_CENTER,
                                    &item.name,
                                    fonts::regular(11.5),
                                    if is_sel {
                                        Color32::WHITE
                                    } else {
                                        Color32::from_rgb(0xDC, 0xDF, 0xE6)
                                    },
                                );

                                ui.painter().text(
                                    pos2(row_r.max.x - 24.0, row_r.center().y),
                                    Align2::RIGHT_CENTER,
                                    &item.category,
                                    fonts::regular(9.5),
                                    Color32::from_rgb(0x76, 0x7A, 0x84),
                                );

                                let is_fav = state.popup_browser.favorites.contains(&item.id);
                                if is_fav || row_resp.hovered() {
                                    let fav_r = Rect::from_min_size(
                                        pos2(row_r.max.x - 19.0, row_r.center().y - 8.0),
                                        vec2(16.0, 16.0),
                                    );
                                    let fav_resp = ui.interact(
                                        fav_r,
                                        Id::new(("bw_popup_fav", idx)),
                                        Sense::click(),
                                    );
                                    ui.painter().text(
                                        fav_r.center(),
                                        Align2::CENTER_CENTER,
                                        if is_fav { "★" } else { "☆" },
                                        FontId::proportional(11.5),
                                        if is_fav {
                                            INST_YELLOW
                                        } else {
                                            Color32::from_rgb(0x88, 0x8C, 0x94)
                                        },
                                    );
                                    if fav_resp.clicked() {
                                        if is_fav {
                                            state.popup_browser.favorites.remove(&item.id);
                                        } else {
                                            state.popup_browser.favorites.insert(item.id.clone());
                                        }
                                    }
                                }

                                if row_resp.clicked() {
                                    state.popup_browser.selected_item_idx = idx;
                                }
                                if row_resp.double_clicked() {
                                    confirm_item = Some(item.clone());
                                }
                            }
                        });
                },
            );

            // =========================================================
            // 4. RIGHT PREVIEW & ACTION PANE (~164px)
            // =========================================================
            let right_inner = right_rect.shrink(4.0);
            let buttons_h = 28.0;
            let preview_rect = Rect::from_min_max(
                right_inner.min,
                pos2(right_inner.max.x, right_inner.max.y - buttons_h - 6.0),
            );
            let buttons_rect = Rect::from_min_max(
                pos2(right_inner.min.x, right_inner.max.y - buttons_h),
                right_inner.max,
            );

            if let Some(sel_item) = filtered_items.get(state.popup_browser.selected_item_idx) {
                let mut wt_preview: Option<Arc<OpenPolyWavetableData>> = None;
                let mut bw_preview: Option<BwPresetPreview> = None;
                if let BrowserPayload::BitwigPreset(path) = &sel_item.payload {
                    let needs_load = state
                        .popup_browser
                        .bitwig_preset_preview_cache
                        .as_ref()
                        .is_none_or(|(id, _)| id != &sel_item.id);
                    if needs_load {
                        let preview = inspect_bwpreset_preview(path);
                        state.popup_browser.bitwig_preset_preview_cache =
                            Some((sel_item.id.clone(), preview));
                    }
                    bw_preview = state
                        .popup_browser
                        .bitwig_preset_preview_cache
                        .as_ref()
                        .filter(|(id, _)| id == &sel_item.id)
                        .map(|(_, p)| p.clone());
                } else if let BrowserPayload::Wavetable(source) = &sel_item.payload {
                    let needs_load = state
                        .popup_browser
                        .wavetable_preview_cache
                        .as_ref()
                        .is_none_or(|(id, _)| id != &sel_item.id);
                    if needs_load {
                        if let Some(table) = hikaru_openpoly::load_wavetable_from_source(source) {
                            let data = Arc::new(OpenPolyWavetableData::new(
                                table.samples,
                                table.frames,
                                FRAME_SAMPLES,
                            ));
                            state.popup_browser.wavetable_preview_cache =
                                Some((sel_item.id.clone(), data));
                        } else {
                            state.popup_browser.wavetable_preview_cache = None;
                        }
                    }
                    wt_preview = state
                        .popup_browser
                        .wavetable_preview_cache
                        .as_ref()
                        .filter(|(id, _)| id == &sel_item.id)
                        .map(|(_, data)| data.clone());
                }

                ui.allocate_new_ui(
                    UiBuilder::new()
                        .max_rect(preview_rect.shrink(4.0))
                        .layout(Layout::top_down(Align::Min)),
                    |ui| {
                        ui.add_space(6.0);
                        if let Some(wt_data) = wt_preview {
                            let (wt_box, _) = ui.allocate_exact_size(
                                vec2(ui.available_width(), 82.0),
                                Sense::hover(),
                            );
                            ui.painter().rect_filled(wt_box, 3.0, WELL_BG);
                            ui.painter().rect_stroke(
                                wt_box,
                                3.0,
                                Stroke::new(1.0_f32, Color32::from_rgb(0x2D, 0x30, 0x38)),
                                egui::StrokeKind::Inside,
                            );
                            let slices = 7usize;
                            let samples = 44usize;
                            let pad_x = 12.0;
                            let w = wt_box.width() - pad_x * 2.0 - 18.0;
                            let amp = wt_box.height() * 0.20;
                            for s in (0..slices).rev() {
                                let depth = s as f32 / (slices - 1) as f32;
                                let ox = wt_box.min.x + pad_x + depth * 18.0;
                                let oy = wt_box.center().y + 11.0 - depth * 22.0;
                                let is_front = s == 0;
                                let mut pts = Vec::with_capacity(samples + 1);
                                for i in 0..=samples {
                                    let t = i as f32 / samples as f32;
                                    let v = eval_osc_preview(
                                        OpenPolyOscType::Wavetable,
                                        t,
                                        depth,
                                        0.0,
                                        Some(&wt_data),
                                    );
                                    pts.push(pos2(ox + t * w, oy - v * amp));
                                }
                                let col = if is_front {
                                    INST_YELLOW
                                } else {
                                    Color32::from_rgba_unmultiplied(0xC8, 0x8E, 0x22, 115)
                                };
                                ui.painter().add(egui::Shape::line(
                                    pts,
                                    Stroke::new(if is_front { 1.6_f32 } else { 1.0_f32 }, col),
                                ));
                            }
                            ui.add_space(6.0);
                            ui.label(
                                RichText::new(&sel_item.name)
                                    .font(fonts::semibold(12.0))
                                    .color(Color32::WHITE),
                            );
                            ui.label(
                                RichText::new(format!(
                                    "{} · {} frames",
                                    sel_item.category, wt_data.frames
                                ))
                                .font(fonts::regular(10.0))
                                .color(INST_YELLOW),
                            );
                            ui.label(
                                RichText::new(format!("{} · {}", sel_item.location, sel_item.file_kind))
                                    .font(fonts::regular(9.5))
                                    .color(theme::TEXT_MUTED),
                            );
                        } else if let Some(bw) = bw_preview {
                            let (icon_r, _) =
                                ui.allocate_exact_size(vec2(34.0, 34.0), Sense::hover());
                            paint_device_badge(ui.painter(), icon_r, sel_item);
                            ui.add_space(6.0);
                            ui.label(
                                RichText::new(&bw.preset_name)
                                    .font(fonts::semibold(12.0))
                                    .color(Color32::WHITE),
                            );
                            ui.label(
                                RichText::new(format!("{} · {}", bw.device_name, sel_item.file_kind))
                                    .font(fonts::regular(10.0))
                                    .color(sel_item.badge_color),
                            );
                            ui.label(
                                RichText::new(format!("{} · {}", bw.creator, bw.category))
                                    .font(fonts::regular(9.5))
                                    .color(theme::TEXT_MUTED),
                            );
                            ui.add_space(4.0);
                            ui.label(
                                RichText::new(format!(
                                    "{} params · {} mods",
                                    bw.param_count, bw.modulator_count
                                ))
                                .font(fonts::regular(9.5))
                                .color(Color32::from_rgb(0x9E, 0xD2, 0xFF)),
                            );
                            if bw.grid_node_count > 0 {
                                ui.label(
                                    RichText::new(format!(
                                        "Grid: {} nodes · {} cables",
                                        bw.grid_node_count, bw.grid_cable_count
                                    ))
                                    .font(fonts::regular(9.5))
                                    .color(INST_YELLOW),
                                );
                            }
                            if !bw.sample_params.is_empty() {
                                ui.add_space(4.0);
                                for (pname, pval) in bw.sample_params.iter().take(4) {
                                    ui.label(
                                        RichText::new(format!("{}: {:.0}%", pname, pval * 100.0))
                                            .font(fonts::regular(9.0))
                                            .color(Color32::from_rgb(0xB4, 0xB8, 0xC0)),
                                    );
                                }
                            }
                        } else {
                            let (icon_r, _) =
                                ui.allocate_exact_size(vec2(34.0, 34.0), Sense::hover());
                            paint_device_badge(ui.painter(), icon_r, sel_item);
                            ui.add_space(8.0);
                            ui.label(
                                RichText::new(&sel_item.name)
                                    .font(fonts::semibold(12.0))
                                    .color(Color32::WHITE),
                            );
                            ui.label(
                                RichText::new(format!(
                                    "{} · {}",
                                    sel_item.creator, sel_item.file_kind
                                ))
                                .font(fonts::regular(10.0))
                                .color(theme::TEXT_MUTED),
                            );
                        }
                        ui.add_space(6.0);
                        ui.label(
                            RichText::new(&sel_item.description)
                                .font(fonts::regular(10.0))
                                .color(Color32::from_rgb(0xB4, 0xB8, 0xC0)),
                        );
                    },
                );
            }

            // Bottom pinned Cancel / OK buttons
            let bw = ((buttons_rect.width() - 6.0) * 0.5).floor();
            let cancel_r = Rect::from_min_size(buttons_rect.min, vec2(bw, buttons_h));
            let cancel_resp = ui.interact(cancel_r, Id::new("bw_popup_cancel"), Sense::click());
            ui.painter().rect_filled(
                cancel_r,
                2.5,
                if cancel_resp.hovered() { BTN_HOVER_BG } else { BTN_BG },
            );
            ui.painter().rect_stroke(
                cancel_r,
                2.5,
                Stroke::new(1.0_f32, BTN_BORDER),
                egui::StrokeKind::Inside,
            );
            ui.painter().text(
                cancel_r.center(),
                Align2::CENTER_CENTER,
                "Cancel",
                fonts::semibold(11.0),
                Color32::from_rgb(0xE8, 0xEA, 0xEE),
            );
            if cancel_resp.clicked() {
                close_requested = true;
            }

            let has_sel = filtered_items
                .get(state.popup_browser.selected_item_idx)
                .is_some();
            let ok_r = Rect::from_min_size(
                pos2(cancel_r.max.x + 6.0, buttons_rect.min.y),
                vec2(bw, buttons_h),
            );
            let ok_resp = ui.interact(ok_r, Id::new("bw_popup_ok"), Sense::click());
            ui.painter().rect_filled(
                ok_r,
                2.5,
                if !has_sel {
                    Color32::from_rgb(0x3C, 0x3E, 0x43)
                } else if ok_resp.hovered() {
                    BTN_HOVER_BG
                } else {
                    BTN_BG
                },
            );
            ui.painter().rect_stroke(
                ok_r,
                2.5,
                Stroke::new(1.0_f32, BTN_BORDER),
                egui::StrokeKind::Inside,
            );
            ui.painter().text(
                ok_r.center(),
                Align2::CENTER_CENTER,
                "OK",
                fonts::semibold(11.0),
                if has_sel {
                    Color32::from_rgb(0xEC, 0xEE, 0xF2)
                } else {
                    theme::TEXT_DISABLED
                },
            );
            if has_sel && ok_resp.clicked() {
                confirm_item = filtered_items
                    .get(state.popup_browser.selected_item_idx)
                    .cloned();
            }
        });

    if close_requested {
        state.popup_browser.open = false;
    } else if let Some(item) = confirm_item {
        apply_browser_selection(state, &item);
    }
}
