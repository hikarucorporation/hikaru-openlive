# Audit: `.opsf` (Hikaru OpenStudio File) vs `.oplf` (Hikaru OpenLive File)

**Headline finding:** `.opsf` is already substantially implemented — it is not a stub. It has a real schema crate, real UI wiring, real round-trip tests, and its docs. The gaps are specific and enumerable: **no atomic write, no `$schema`, no schema file, no CLI, thin crate-level test coverage (1 test), and several fields that the docs describe but the struct does not have.**

---

## 1. `crates/hikaru_project_file/src/lib.rs` (236 lines) — shared crate

Dependencies: `serde` + `serde_json` only (`:26-28`). This is deliberate — the header comment `:5-24` explains the crate is LGPLv3 precisely so it must not depend on GPUI/sequencer, or the "format is a library" boundary collapses.

**`FORMAT_VERSION`** (`:38`) — `u32 = 1`. Doc comment `:30-37` states the rule: bump only when the JSON *shape* changes (new required field, changed type). A file with a **higher** version is rejected, not partially interpreted. It is explicitly distinguished from the app version, which is informational in `created_by`.

**`ProjectError`** (`:42-51`), `Display` at `:53-70`, `From<io::Error>` `:72`, `From<serde_json::Error>` `:78`, `std::error::Error` `:84`. Four variants:
- `Io(std::io::Error)`
- `Parse(serde_json::Error)`
- `UnsupportedVersion { found, expected }`
- `NotAProject(PathBuf)`

**`write_json`** (`:163-172`) — **`NOT atomic.`** It does `to_string_pretty` (`:164`), `create_dir_all` on the parent (`:165-169`), then a plain `std::fs::write(path, json)` (`:170`). I grepped the three format crates for `rename|tempfile|.tmp|persist|NamedTemp`: **zero hits.** A crash/disk-full mid-write truncates a user's only copy of the project. This is the single most important defect for making `.opsf` "first-class," and it affects `.oplf` identically.

**`parse_with_probe`** (`:179-200`) — reads whole file to string (`:180`), deserializes a private `VersionProbe { format_version: Option<u32> }` (`:182-190`), returns `NotAProject` if the JSON is unparseable *or* the header key is absent (`:187`, `:190`), then rejects `version > FORMAT_VERSION` with `UnsupportedVersion` (`:191-196`), and only then deserializes the full body (`:198`). Rationale at `:174-178`: version must be checked before the body so the error says "version" rather than a meaningless serde `missing field`.

Validation on parse is **header-only**. There is no structural/semantic validation (no grid rectangularity check, no `ppqn != 0`, no clip→track referential integrity, no bounds/range checks) at this layer.

**`Track`** (`:98-115`) — `id, name, volume, pan, pan_mode, mute, solo, arm, is_master, route_destination_id, sends: Vec<Send>, effects: Vec<DspSlot>, matrix_idx: Option<usize>`. Shared verbatim by `live_tracks` (oplf) and `studio_tracks` (opsf); comment `:90-96` explains it is one type on purpose to avoid two diverging converters. `matrix_idx` is `.oplf`-only (`:112-114`). **`Send`** `:119-122`, **`DspSlot`** `:131-136` (stores identity/bypass/`wavetable_path` only — `:126-129` notes synth panel params do not yet enter the format).

**`Color`** (`:148-153`) — flat HSLA `{h,s,l,a}`. Deliberately *not* GPUI's `Hsla` (`:143-146`), again to preserve the LGPL boundary.

**JSON schema / `$schema`: none.** No `$schema` field is emitted on write or read; there is no schema file anywhere on disk (see §6). The only version marker is the `format_version` field.

Tests: 2 (`:206-218` a WAV is rejected as `NotAProject`; `:220-235` a future version is rejected before body parse).

---

## 2. `crates/hikaru_openlive_file/src/lib.rs` (365 lines) — `.oplf` crate

**Types:** `EXTENSION="oplf"` `:51`, `FILE_FILTER="Hikaru OpenLive File (*.oplf)"` `:54` (with an in-place author note "Ahí corregí el 'Project' por el 'File'"), `EngineMode::OpenLive` `:62-64`, `OpenLiveProject` `:72-90`, `Metadata` `:99-112`, `Stats` `:117-122`, `Transport` `:129-149`, `DspGlobal` `:175-179`, `UiState` `:185-189`, `Matrix` `:193-201`, `TrackMeta` `:204-210`, `SceneMeta` `:213-215`, `Clip` `:223-239`, `Event` `:247-257`, `MidiNote` `:261-266`.

Validation is richer than opsf in three places:
- `Transport::quantization_denominator()` `:151-166` — parses musical notation `"1/4"`, tolerates bare `"4"`, clamps to ≥1 (`:156-165`) so a zero division cannot produce NaN.
- `Matrix` `:194-195` documents the invariant `tracks.len() == grid.len()` and `scenes.len() == grid[0].len()`, **but does not enforce it** — enforcement is in the GUI at `openlive.rs:370-373`.
- `DspGlobal` `:170-173` is documented as a *view* of the master track that must not diverge — again enforced only in the GUI (`openlive.rs:131-140`, `:265-268`).

Forward-compat/migration: **none beyond the shared probe.** `parse` `:278-280` is a bare `parse_with_probe`. There is no migration table, no version dispatch, no unknown-field tolerance (`serde` defaults to ignoring unknown fields), no `deny_unknown_fields`. `#[serde(default)]` is used only on optional metadata `:101-108` and `tap_tempo_enabled` `:142-143`.

`save` `:273-275` / `parse` `:278-280` are one-line delegations.

**Tests (3, `:282-365`):** quantization parsing incl. malformed (`"1/x"`) `:298-314`; zero-quantization clamp `:316-324`; save/parse round-trip asserting `engine_mode`, `created_by`, `format_version` at the JSON level then re-read `:326-365`.

---

## 3. `openlive.rs` (655) vs `openstudio.rs` (369) — conceptual diff

| Concern | `.oplf` (`openlive.rs`) | `.opsf` (`openstudio.rs`) |
|---|---|---|
| Relative audio paths | **Yes** — `common::resolve_path(base_dir, ...)` for clip path `:472` *and* per-event `source_path` `:513`; base dir threaded from `mod.rs:198` | **Yes, but only one** — `resolve_path` at `:271` for `ClipKind::Audio.sample_path`. Applied identically for all three clip kinds; no per-event concept exists. |
| Per-clip sample offsets | **N/A** — `Event` has `start_secs, trim_left_frames, visible_frames, gain, fade_in_secs, fade_out_secs` `:247-257` | **Yes** — `ClipKind::Audio { sample_offset_ticks, total_sample_ticks }` `:161-165`, round-tripped `openstudio.rs:138-142` / `:284-289` |
| Clip duration / peaks | Peaks reloaded, duration from WAV `:569-573` | `load_sample_info(&path, pl.ppqn, bpm)` `:275`; saved duration preferred if the file has one, else file duration `:276-280` |
| Orphan/sanity validation | Grid truncated to `tracks.len()` / first row to `scenes.len()` `:370-373`; `selected_slot` filtered `:374-376`; empty file grid → default grid `:347` | Clips whose `track_idx >= state.tracks().len()` are **skipped** `:240-242` |
| Range clamping | BPM clamp `:255`; `beats_per_bar.max(1)` `:257`; quantize denom from parser | BPM clamp `:177`; `beats_per_bar.max(1)` `:179`; `beat_division.max(1)` `:180`; `ppqn.max(1)` `:229`; `next_clip_id.max(1)` `:230`; `duration_ticks.max(1)` `:309` |
| Engine registration | **The big one.** `:395-465` replays `LoadClip` → `sync_audio_events_to_engine` → `UpdateMidiClipNotes` → `SetClipLoop` for every pad; `:399-419` is a long comment explaining that GUI-only reconstruction yields *silent pads*, and that ordering is non-negotiable | **Delegated.** `apply_playlist:250` sets `needs_full_sync = !next.clips.is_empty()` and comment `:247-249` explains the render cycle issues `LoadClip` — avoids double-loading |
| Master/DSP reconciliation | `dsp_global` overrides master `:265-268`; `sync_matrix_mixer_bidirectional` `:282-288` | None — opsf has no `DspGlobal` field at all |
| Isolation comments | `:19-28` | `:57-59`, `:170-174` |
| Mode forcing | `AppMode::OpenLive` `:295` | `AppMode::OpenStudio` `:200` |
| Future-proof view decode | `openlive_view_from_u8` `:317-322`, "Desconocido = el default, para que un archivo de una build futura no rompa la apertura" | `openstudio_view_from_u8` `:220-225` — same fallback pattern |

**What `openlive.rs` has that `openstudio.rs` does NOT:** engine clip registration, `dsp_global` master override, bidirectional matrix↔mixer sync, per-event (multi-sample) audio reload, grid dimension truncation, `next_event_id` reconstruction `:485`, MIDI-vs-audio content sniffing via `is_midi_file` `:474`, and `clip.refresh_preview()` `:569`.

**What `openstudio.rs` has that `openlive.rs` does NOT:** handling of `sample_offset_ticks`/`total_sample_ticks`, `ppqn` persistence, `next_clip_id` restoration, automation clips with `CurvePoint` curves and `target_param` (`:167-170`, `:292-302`), and per-clip `Color`.

**A real bug in `openstudio.rs` worth flagging** — `:276-280`:
```rust
let duration_ticks = if file_duration_ticks > 0 {
    saved.duration_ticks      // saved duration wins
} else {
    file_duration_ticks.max(1)
};
```
If the WAV *cannot* be read (`file_duration_ticks == 0`, e.g. a missing/moved sample) this evaluates to `0.max(1) == 1`, i.e. a **1-tick clip**, silently losing `saved.duration_ticks`. The comment at `:272-274` justifies preferring file duration, but the branches are inverted relative to that intent: the sample-missing case is exactly where the saved value is most valuable. Compare `:285-289`, where the same `if *total_sample_ticks > 0` guard *does* correctly fall back to `duration_ticks`. Also note `build_playlist_clip` receives `pl` and reads `pl.ppqn` (`:275`) but the caller sets `next.ppqn` *before* iterating (`:229`), so that one is correct.

**Comments/TODO markers:** no `TODO`, `FIXME`, `unimplemented!`, or `todo!` anywhere in either file. All commentary is prose explaining design rationale — unusually high quality and intent-dense. The only `panic!` in the whole format path is a test assertion at `hikaru_project_file/src/lib.rs:232`.

---

## 4. `crates/hikaru_gui/src/project/common.rs` (271 lines) — helpers

| Helper | Line | Used by openlive | Used by openstudio |
|---|---|---|---|
| `capture_track(&mixer::Track) -> Track` | `:59` | `:62` | `:63` |
| `build_track(&Track) -> mixer::Track` | `:99` | via `tracks_or_default` `:262` | via `tracks_or_default` `:185` |
| `default_live_tracks()` (MASTER only) | `:130` | `:262`, `:593` | — |
| `default_studio_tracks()` (MASTER + TRACK 01) | `:148` | — | `:185`, `:326` |
| `color_to_gpui` / `color_from_gpui` | `:196`, `:201` | — | `:161`, `:261` |
| `resolve_path(base_dir, raw)` | `:213` | `:472`, `:513` | `:271` |
| `push_track_mix_to_engine` | `:230` | `:290` | `:195` |
| `title_from_path` | `:260` | `:648` | `:362` |
| `tracks_or_default` | `:265` | `:262` | `:185` |
| re-exports `ProjectError, Track, FORMAT_VERSION` | `:56` | `:44` | `:48` |

Note `default_live_tracks` has **no `total_scenes`/`scene` counterpart** and `default_studio_tracks` hardcodes exactly one non-master track — `blank()` in openlive derives one track per matrix row `:594-610`, but openstudio's `blank()` does not derive tracks from anything `:324-349`. Asymmetry worth noting if you want `.opsf` to be first-class.

Also `build_track:114-122` reconstructs `DspSlot` via `mixer::DspSlot::new(id, name)` and sets only `active` — it **drops `wavetable_path`**, which `capture_track:86-91` does serialize. This is a pre-existing round-trip data loss affecting *both* formats (the wavetable reference written to disk is discarded on load).

---

## 5. UI wiring — exact locations

All save/load entry points are in **`crates/hikaru_gui/src/views/menu_bar.rs`**:

- `menu_bar.rs:35-37` — reads `(app.open_menu, app.project_dirty, app.mode)` at render time
- `menu_bar.rs:103-107` — File-menu item disabled/dimmed when `idx == FILE_MENU && dirty`
- `menu_bar.rs:255-268` — `new_project()`: reads dirty `:256`, `confirm_discard()` `:258`, `project::reset(state)` `:263`, clears `project_path`/`project_dirty` `:264-265`
- `menu_bar.rs:274-298` — `open_project()`: `project_path.clone()` `:275`, `project::ask_open_path(...)` `:276`, `project::load(...)` `:282`, on success sets `project_path`/`project_dirty=false` `:291-292`
- `menu_bar.rs:309-341` — `save_project(cx, force_dialog)`: reads `project_path` + `mode` `:310-311`, reuses path or `project::ask_save_path(mode, ...)` `:315`, `project::save(...)` `:323`, sets `project_path` + clears dirty `:331-332`
- `menu_bar.rs:346-349` — `confirm_discard()` via `rfd::MessageDialog`

Dispatch layer, `crates/hikaru_gui/src/project/mod.rs`:
- `mod.rs:168-174` `save` (mode → format), `mod.rs:180-186` `save_as`
- `mod.rs:197-211` `load` — `base_dir` from `path.parent()` `:198`, extension → mode `:199`, `openlive::parse`+`apply` `:201-202` / `openstudio::parse`+`apply` `:205-206`, `NotAProject` for unknown extension `:208`
- `mod.rs:145-151` `retarget_extension` — retargets a saved `.oplf` to `.opsf` if the mode changed (`:135-144` explains why: would otherwise write an opsf payload inside a file named `.oplf`, silently losing the arrangement on reopen)
- `mod.rs:223-235` `reset`, `mod.rs:246-251` `filter_for`, `mod.rs:257-273` `ask_save_path`, `mod.rs:279-288` `ask_open_path`

Dirty-flag / autosave / recent files — **all absent**:
- `AppState` fields: `app.rs:355` `project_path: Option<PathBuf>`, `app.rs:360` `project_dirty: bool`; initialized `app.rs:671-672`
- `project_dirty = true` appears **nowhere in production code**. The only occurrence in the entire workspace is `crates/hikaru_gui/tests/menu_bar_open.rs:328` (a test forcing the flag). Every production site only ever sets it to `false` (`menu_bar.rs:265, 292, 332`). **The unsaved-changes prompt is therefore dead code — it can never trigger in the running app.** This is a genuine, high-impact bug and likely your best quick win.
- **No autosave** — no timer, no `.hikaru_autosave`, no backup file anywhere.
- **No recent-files menu** — `grep` for `recent` across `hikaru_gui/src` returns nothing.
- No on-disk session recovery: closing the app with an unsaved `.opsf` loses it.

---

## 6. Schema files, CLI, migration tooling

**Schema file: does not exist.** I searched the whole tree (excluding `target/`) for `*.json`, `*schema*`, `*.opsf`, `*.oplf`. The only JSON hit is `vst3-host-patched/.cargo_vcs_info.json`. The identifier `"hikaru-openstudio-file/opsf-v1.json"` appears **only** in `docs/hikaru-project-files/hikaru-openstudio-file/OPSF-README.md:119`, as a literal inside an example JSON block. Nothing generates it, nothing resolves it, nothing validates against it, and `write_json` emits no `$schema` key — so a written `.opsf` does **not** match its own documented example. Same story for the `.oplf` equivalent.

The documented example (OPSF-README `:118-183`) also **diverges from the implemented struct**: it shows `metadata.stats.total_scenes` (`:129`, an `.oplf` field not present in `hikaru_openstudio_file::Stats`), clips shaped as `{"idx":1,"clip":{"type":"audio","source":...,"muted":...,"solo":...}}` (`:145-181`) rather than the real `Vec<(usize, PlaylistClipSaved)>` with a serde-tagged `kind`, and no `studio_tracks` / `ui` blocks at all. Its `ppqn: 96` (`:143`) also conflicts with the crate's test fixture value of `960` (`hikaru_openstudio_file/src/lib.rs:229`). **The `.opsf` spec document is stale relative to the code.** By contrast `OPLF-README.md` is a normative rules document (§3.2 forbidden hybrid fields, §3.3 exact rfd filter string) that *matches* the implementation.

**CLI binary: does not exist.** The only `[[bin]]` in the workspace is `crates/hikaru_gui/Cargo.toml:41-43` → `hikaru_openlive`, a GUI binary. `docs/hikaru-openlive-cli/hikaru-openlive-cli-README.md` opens with an explicit disclaimer on line 1: *"Este es un borrador, no está completo"* — it is a design draft, and its `hikaru_openlive new my_project.opsf` invocation (quoted at OPSF-README `:12`) is fictional. `grep` for `clap`/`--cli`/`$schema` across the format crates and `main.rs` returns nothing.

**Migration/conversion tool: none.** `mod.rs:180` mentions `save_as` exists *"para los tests y para las herramientas de conversión"*, but no such tool exists, and `save_as` has **zero non-test callers** (grep showed only `project_roundtrip.rs` hits, which use `project::save`). No `.hikaru` (legacy format) migration code exists despite `mod.rs:9-22` and the test `la_extension_legacy_no_es_un_proyecto` (`project_roundtrip.rs:1034`) both referencing it — there is no path from an old `.hikaru` file to either new format.

---

## 7. Test run — verbatim summary

```
cargo test -p hikaru_openstudio_file -p hikaru_openlive_file

   Compiling hikaru_openlive_file v2.22.2
 Finished `test` profile [unoptimized + debuginfo] target(s) in 4.74s
     Running unittests src/lib.rs (target/debug/deps/hikaru_openlive_file-d8c81dc52d6297ce)
running 3 tests
test tests::una_cuantizacion_en_cero_no_deja_la_division_en_cero ... ok
test tests::la_cuantizacion_se_lee_en_notacion_musical ... ok
test tests::un_oplf_que_se_guarda_trae_la_version_que_dijo ... ok
test result: ok. 3 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

     Running unittests src/lib.rs (target/debug/deps/hikaru_openstudio_file-cc7786f7411023ed)
running 1 test
test tests::un_opsf_conserva_el_ppqn_y_los_ticks ... ok
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

   Doc-tests hikaru_openlive_file
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
   Doc-tests hikaru_openstudio_file
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
```

**All 4 tests pass.** Note the imbalance: `.oplf` has 3 crate tests vs `.opsf`'s 1. `hikaru_project_file` wasn't in your command but has 2 more. The real coverage lives in `crates/hikaru_gui/tests/project_roundtrip.rs` (1,100+ lines, ~25 tests), which does exercise `.opsf` thoroughly: purity/isolation `:221,:301`, header versioning `:342`, mode-driven extension `:382,:420`, studio tracks `:523`, playlist ticks + peak recalculation `:844`, ppqn round-trip `:911`, mode-on-reopen `:942`, bad-file rejection `:990,:1008,:1034`, and `New Project` `:1061`.

---

## 8. TODO / FIXME / unimplemented! / panic!

**None.** Exhaustive grep across `hikaru_openstudio_file`, `hikaru_project_file`, `hikaru_openlive_file`, `hikaru_gui/src/project/`:

- `hikaru_project_file/src/lib.rs:232` — `panic!` inside the `una_version_futura_se_rechaza_antes_de_parsear` test only (test assertion, fine)
- `hikaru_openlive_file/src/lib.rs:243`, `hikaru_openstudio_file/src/lib.rs:11`, `hikaru_gui/src/project/openstudio.rs:11` — the Spanish word "TODO" meaning **"all"**, not a task marker

No `unimplemented!()`, no `todo!()`, no `FIXME`, `XXX`, or `HACK`. The absence of stubs means the gaps in §6 are **omissions, not placeholders** — which is good news: adding `$schema`, atomic write, and a schema file is pure addition with no stubs to unwind.

---

## Prioritized recommendations**Correctness / data loss (do first):**
1. `common.rs:230` `push_track_mix_to_engine` + **`project_dirty` never set true** — the unsaved-changes guard is non-functional. Every mutating operation should set it.
2. `project_file/src/lib.rs:170` — make `write_json` atomic (temp file in the same dir + `fsync` + `rename`). Affects both formats; a corrupt `.oplf`/`.opsf` write is currently unrecoverable.
3. `openstudio.rs:276-280` — inverted duration fallback silently produces 1-tick clips for missing samples; should fall back to `saved.duration_ticks`.
4. `common.rs:114-122` — `build_track` drops `wavetable_path`, so it is written but never restored (both formats).

**First-class `.opsf` parity with `.oplf`:**
5. Add `$schema` emission + author the two schema files, so the documented `"hikaru-openstudio-file/opsf-v1.json"` becomes real.
6. **Rewrite `OPSF-README.md`** to match the implemented struct (it currently documents a shape the code does not produce).
7. Add structural validation in `parse_with_probe` or an opsf `validate()`: `ppqn != 0`, `loop_start <= loop_end`, clip track indices in range, grid/track count invariants — oplf has some, opsf has none.
8. Add a real migration/dispatch layer for future `format_version` bumps (currently reject-only).
9. Backfill `.opsf` crate tests toward oplf parity (bad JSON, truncated file, missing optional fields, automation-clip round-trip, sample-offset round-trip).
10. Add autosave + recent-files menu, and either build the CLI or mark its README clearly non-existent.

I made no modifications to any files.