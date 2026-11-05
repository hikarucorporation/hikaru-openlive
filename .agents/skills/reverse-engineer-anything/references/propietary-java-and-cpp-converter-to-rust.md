# Proprietary Java/C++ to Rust & GPUI Conversion Spec

## 1. Context & Objective
This specification guides the extraction, decompilation analysis, and translation of proprietary DAW components (specifically Java-based UI controllers/events and C++ DSP core modules, such as Bitwig's Detail Editor Panel and Audio Clip Inspector) into clean, open-source, thread-safe Rust models designed for **Hikaru OpenLive** using the **GPUI** framework.

The goal is **NOT** a 1:1 line-by-line code translation. The goal is to **deconstruct UI state, structural layouts, and clip data models**, then re-implement them idiomatically in Rust.

---

## 2. Java UI Deconstruction (Swing/Custom Framework -> GPUI)

### A. Layout Hierarchy Extraction
When inspecting Java UI components (e.g., Detail Editor, Clip Canvas, Note Grid):
* **Ignore Boilerplate:** Strip away legacy listeners (`ActionListener`, `ItemListener`), binding frameworks, and custom paint methods (`paintComponent`).
* **Extract Component Tree:** Identify the pure layout tree:
  * Panes / Splitters -> `gpui::div()`, `gpui::flex()`, or flexbox configurations.
  * Timeline Canvas / Waveform Display -> Custom canvas elements / SVG vector paths.
  * Inspector Controls -> State-bound reactive elements.

### B. State Machine Mapping
* Convert mutable Java instance variables (`double gain`, `boolean isLooping`) into single-source-of-truth Rust application state.
* UI updates **must** follow GPUI's entity-component-reactive model using `Model<T>` or `ViewContext`.
* Map UI interactions into explicit Rust event enums:
  ```rust
  pub enum DetailEditorEvent {
      SetClipGain { clip_id: ClipId, gain_db: f32 },
      AdjustWarpMarker { clip_id: ClipId, sample_index: usize, new_time: f64 },
      ToggleLoop { clip_id: ClipId, active: bool },
  }

3. C++ Audio Clip & DSP Data Model Extraction
A. Data Layout & Parsing
 * Locate binary structures or native memory layouts representing audio clip metadata (offsets, pitch shifts, loop boundaries, transient markers).
 * Re-map native C++ pointers and dynamic arrays to fixed-width, memory-aligned Rust struct representations:
   #[repr(C)]
#[derive(Debug, Clone, PartialEq)]
pub struct AudioClipMetadata {
    pub sample_start: u64,
    pub sample_end: u64,
    pub loop_start: u64,
    pub loop_end: u64,
    pub transpose_semitones: i8,
    pub gain_linear: f32,
}

B. Real-Time Audio Thread Safety
 * No Heap Allocations: Never convert C++ DSP loops into code using Vec::push, Box::new, or dynamic allocations inside the audio thread.
 * Lock-Free Messaging: Communication between the GPUI Detail Editor Panel and the audio engine must utilize lock-free SPSC / MPSC ringbuffers or atomic primitives (AtomicBool, AtomicF32).
4. Reverse Engineering Extraction Pipeline (REA Step-by-Step)
When analyzing target Java .class / .jar or C++ .so / .dll binaries for UI panels:
 * Step 1: Structural Audit
   * Map all visual properties of the panel (e.g., Audio Clip Editor: Waveform bounds, Warp handles, Track color, Time ruler).
 * Step 2: Model Isolation
   * Isolate data attributes from rendering logic. Output a clean .rs data representation.
 * Step 3: GPUI View Construction
   * Generate idiomatic GPUI view functions matching the layout:
     impl Render for DetailEditorPanel {
    fn render(&mut self, _cx: &mut ViewContext<Self>) -> impl IntoElement {
        div()
            .flex()
            .flex_col()
            .w_full()
            .h_64()
            .bg(rgb(0x1e1e1e))
            .child(/* Waveform canvas & controls */)
    }
}

5. Strict Constraints
 * ❌ DO NOT output direct Java wrappers, JNI bindings, or C++ FFI unless strictly necessary for binary analysis.
 * ❌ DO NOT preserve blocking synchronous UI event loops.
 * ✅ ALWAYS enforce thread safety, zero-copy audio buffer references, and clean GPUI layout declarations.
