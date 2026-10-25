# Implementation Plan: UX/UI Shell Improvements

## Overview

This plan refines SolidCraft's existing egui sketch shell (L5) and adds an optional coaching
layer, following the design's layering: engine-side pure pieces and `CommandSpec`s first
(`CoachingPreference` + reducer, `ui.coaching.*`, `sketch.dimension.commit`), then the L5
tool-step model, then each UI surface (feedback chip + inline prompt, snap/inference legibility,
dimension entry routing, empty-state cue, coaching surface), each landing with its tests.

Tasks 1–14 (above) cover the original sketch-focused + coaching outcomes (Requirements 1–6) and
are preserved unchanged. Tasks 15 onward add the whole-shell outcomes (Requirements 7–13):
engine `view.*` commands first (L4), then the L5 shell surfaces — command palette + ribbon,
shared dialog frame, viewport nav bar, keymap page, timeline, browser, and the app-level
coaching/onboarding extension — plus the new i18n keys and conditional theming, each landing with
its property/UI tests, finishing with the full `cargo xtask ci` gate.

Every task is Rust, honours the clean-room / thin-UI / command-driven (SolidCraft's own dotted
ids, UI acts only through `app.run`/`app.start`) / icons-in-code / asset→ATTRIBUTION.md / i18n
(`en`/`ja`/`de`/`ko`) / `Tokens`-theming / no-panic (`Result` + `?`) / wasm32 / layering (egui only
in L5) constraints, and references the requirements and design properties it satisfies.

Language: Rust (the only language in the repo; the design uses Rust, not pseudocode).

## Tasks

- [x] 1. Engine `CoachingPreference` data model and pure reducer (below L5, no egui)
  - [x] 1.1 Add `CoachingPreference` struct and reducer functions
    - Add a `coaching` settings module in `crates/engine/src` (e.g. `crates/engine/src/coaching.rs`, wired into `crates/engine/src/lib.rs`) holding `CoachingPreference { enabled, index, dismissed, completed }` with the design's `Default` (first-run: enabled, index 0, not dismissed/completed), deriving `Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize` with `#[serde(default)]`.
    - Add the pure reducers `coaching_set(&mut, enabled)`, `coaching_advance(&mut, steps)`, `coaching_dismiss(&mut)` exactly as in the design, using saturating/`min` arithmetic so an out-of-range `index` can never go out of bounds. No egui, no I/O.
    - Hold the preference as serializable application settings on the engine `Session` (not the per-design `Document`, not the config table), with a `Default`/fallback load path for a missing or malformed persisted value.
    - Keep this file buildable for `wasm32-unknown-unknown`; no `unwrap`/`expect`/`panic!`/etc.; fallible paths return `Result` + `?`.
    - _Requirements: 6.5, 6.6, 6.7, 6.8, NFR 6, NFR 11, NFR 12, NFR 14, NFR 15_

  - [x]* 1.2 Write property test for coaching advance/complete
    - **Property 12: Coaching advances monotonically and completes at the end**
    - Over arbitrary preferences and step counts, assert `advance` maps `index i`→`i+1` until the last step, then sets `completed`, and `index` never exceeds `steps-1`.
    - **Validates: Requirements 6.2, 6.8**

  - [x]* 1.3 Write property test for the dismiss invariant
    - **Property 13: Dismissing a step keeps the layer enabled**
    - Assert `coaching_dismiss` sets `dismissed = true` while `enabled` stays `true`.
    - **Validates: Requirements 6.3**

  - [x]* 1.4 Write property test for preference persistence round trip
    - **Property 14: Coaching preference persists across sessions**
    - For arbitrary sequences of set/advance/dismiss, assert serialize → deserialize yields identical `enabled/index/dismissed/completed`, and a disabled preference stays disabled on reload.
    - **Validates: Requirements 6.5, 6.6, 6.7, 6.8**

- [x] 2. `ui.coaching.*` CommandSpecs (L4 engine, no egui)
  - [x] 2.1 Add a `ui` command module with the coaching commands
    - Create `crates/engine/src/cmd/ui.rs` and register it in `crates/engine/src/cmd/mod.rs::command_specs`.
    - Define `ui.coaching.set` (params `enabled: bool`), `ui.coaching.advance`, `ui.coaching.dismiss`, and `ui.coaching.state` (read-only) with SolidCraft dotted lower-case ids, area `ui`, not on the toolbar; the mutating ones are `.noundo()`, `state` is `.noundo()` read-only. Each `run` calls the task-1 reducer on the session's `CoachingPreference` (passing the step count from params) and returns `{enabled, index, completed, dismissed, ...}` as JSON.
    - Validate params as hostile input: `enabled` must be a bool, clamp `index`/`steps` into range, return `Err` (never panic) on malformed control input, and never touch the design document.
    - _Requirements: 6.2, 6.3, 6.5, 6.6, 6.8, NFR 3, NFR 5, NFR 11, NFR 12, NFR 13_

  - [x]* 2.2 Write unit tests for `ui.coaching.*` command behaviour
    - Assert each command returns the expected JSON state, `set`/`advance`/`dismiss` mutate the persisted preference, malformed params return `Err`, and no command alters the design document or undo depth.
    - _Requirements: 6.2, 6.3, 6.5, 6.6, 6.8, NFR 13_

- [x] 3. `sketch.dimension.commit` CommandSpec with finite-number validation (L4 engine)
  - [x] 3.1 Implement the commit command
    - Add `sketch.dimension.commit` to `crates/engine/src/cmd/sketch.rs` (or a `#[path]`-included sibling), area `sketch`, `undoable`, `in_sketch`-enabled. Params per design: `entities?`, `param?`, `value` (bare number in design units or expression / "50 mm"), `type?`, `text_at?`.
    - Reuse the existing dimension/parameter engine logic (`cmd/sketch.rs::dimension`, `cmd/parameters.rs`) rather than duplicating it, so for a new dimension it creates on picked entities and for an edit it changes the named parameter.
    - Make this the single validation gate (R4.4): treat `value` as hostile — `get()`-style access, bound expression length (> 4096 chars rejected), evaluate and reject non-finite / NaN / ±inf / out-of-range / empty / non-numeric, returning a descriptive `Err` and leaving the design byte-for-byte unchanged. No panics; `Result` + `?`.
    - _Requirements: 4.2, 4.4, NFR 3, NFR 5, NFR 11, NFR 12, NFR 13_

  - [x]* 3.2 Write property test for the typed-dimension round trip
    - **Property 6: Confirming a typed dimension applies that value**
    - For finite valid expressions, assert the command runs with that value and the resulting dimension measures the evaluated value within tolerance.
    - **Validates: Requirements 4.2**

  - [x]* 3.3 Write property test for hostile dimension input
    - **Property 8: Hostile dimension input is rejected and the design is unchanged**
    - Feed NaN/±inf/overflow/empty/overlong/non-numeric expressions; assert `sketch.dimension.commit` returns `Err`, does not panic, and leaves the design document and undo history unchanged.
    - **Validates: Requirements 4.4**

- [x] 4. Checkpoint - engine pieces build and pass
  - Ensure all tests pass, ask the user if questions arise.

- [x] 5. L5 tool-step model in `tools.rs`
  - [x] 5.1 Add `ToolStep` and `Tool::step()`
    - In `crates/ui-egui/src/tools.rs`, add `ToolStep { cmd, index, prompt_key, confirm_key, exit_key }` and `Tool::step()` that derives the current step by `(cmd, progress)`: step index `pts.len()` for `Kind::Draw(n)` / first-vs-next for chain tools, `picks.len()` for `Kind::Pick(n)`/`HorizontalVertical`/`Dimension`, and the per-command descriptor for `Kind::Ext`. Keep the `(cmd, n)` shape consistent with `sketch_dims::boxes_for`.
    - Move the hardcoded English `tools::hint(id)` strings behind localized catalog keys referenced by `prompt_key` (shared `confirm_key`/`exit_key` for click/Enter and Esc). No behaviour change to tool progression itself.
    - Keep `tools.rs` wasm-safe and panic-free.
    - _Requirements: 1.1, 1.2, 1.3, 1.4, 2.1, 2.2, NFR 10, NFR 12, NFR 14_

  - [x] 5.2 Add the Ext tool step descriptors in `sketch_tools.rs`
    - Convert the `hint(id)` table in `crates/ui-egui/src/sketch_tools.rs` into the step source for those commands, each behind a localized `sketch.prompt.<cmd>.<step>` key, so `Kind::Ext` tools report a step through `Tool::step()`.
    - _Requirements: 1.2, 2.1, NFR 10_

  - [x] 5.3 Add the `tool_feedback` / `ToolFeedback` derivation
    - Add `tool_feedback(app) -> Option<ToolFeedback>` (name via `tr_label(spec.label)`, expects/confirm/exit via `tr(key)`) as in the design; `None` when no tool is active so feedback disappears the next frame. Place it where the viewport can read it (e.g. in `tools.rs` or `viewport.rs`).
    - _Requirements: 1.1, 1.2, 1.3, 1.5, 1.6, 2.1, NFR 10_

  - [x]* 5.4 Write property test for complete tool feedback
    - **Property 1: Active-tool feedback is complete for every tool and step**
    - Over the sketch tool-command registry and reachable steps, assert every tool/step yields a non-empty localized name, expected-input, confirm, and exit.
    - **Validates: Requirements 1.1, 1.2, 1.3, 2.1**

  - [x]* 5.5 Write property test for step-change text
    - **Property 2: Advancing a step changes the expected input**
    - For tools with more than one distinct step, assert advancing changes the expected-input/inline-prompt text whenever the underlying steps differ.
    - **Validates: Requirements 1.4, 2.2**

- [x] 6. i18n catalog keys for the new strings
  - [x] 6.1 Add and translate catalog keys
    - Add to `crates/ui-egui/src/locales/en.json` (and translate in `ja.json`, `de.json`, `ko.json`) the keys the models reference: `sketch.prompt.<cmd>.<step>`, `sketch.confirm.click`, `sketch.confirm.enter`, `sketch.exit.esc`; snap labels `sketch.snap.point|mid|on`; inference labels `sketch.infer.parallel|perpendicular|extension|tangent|horizontal|vertical`; `sketch.empty_state_cue`; coaching `coaching.step_*`, `coaching.next`, `coaching.dismiss`, `coaching.enable`, `coaching.disable`. Route every string through `app.tr(id)` / `app.tr_label(label)` in `i18n.rs`; no hardcoded literals in draw paths.
    - _Requirements: 2.3, 3.2, 5.4, 6.9, NFR 10_

  - [x]* 6.2 Extend the i18n catalog test to the new keys
    - **Property 3: Every prompt and label string resolves through the catalog**
    - In `i18n_ui_tests.rs` / the `i18n.rs` catalog tests, assert every new key resolves (not raw id) in `en` and is present/translatable in `ja`/`de`/`ko`.
    - **Validates: Requirements 2.3, 3.2, 5.4, 6.9**

- [x] 7. Active-tool feedback chip and inline prompt in the viewport
  - [x] 7.1 Draw the feedback chip and inline prompt
    - In `crates/ui-egui/src/viewport.rs`, draw the `ToolFeedback` chip beside the existing status overlay using only `theme::Tokens` colours (`tk.overlay`, `tk.text`/`tk.text_dim`, `tk.accent`). Rebuild it from `app.tool` every frame so exiting the tool (`tools::finish` sets `app.tool = None`) removes it on the next frame.
    - Surface the inline prompt (`step.prompt_key`) through the status line on tool start and each new step; clear it on successful completion; route engine errors through the existing `app.run` error path so `set_status(msg, true)` marks the status as an error.
    - _Requirements: 1.1, 1.2, 1.3, 1.4, 1.5, 1.6, 2.1, 2.2, 2.4, 2.5, NFR 9, NFR 10_

  - [x]* 7.2 Write UI example tests for the chip lifecycle and error status
    - Using the `scenario::Harness`, assert the chip appears while a tool runs, is gone one frame after exit (R1.5/R1.6), the expected-input text updates on step change, and an engine error sets the error status flag (R2.5). Add to a new feedback test file or the existing sketch UI tests.
    - _Requirements: 1.5, 1.6, 2.2, 2.5_

- [x] 8. Snap and inference legibility + localized labels
  - [x] 8.1 Add localized type labels and token-only markers
    - In `crates/ui-egui/src/drag_snap.rs`, add a localized type label per `Target`/`Snap` variant alongside the existing glyph, draw the `Snap_Indicator` at `snap.at` using only `Tokens` colours (`tk.accent`, `tk.accent_soft`, `tk.text`), and keep publishing guide counts for the multi-inference assertion.
    - In `crates/ui-egui/src/inference.rs`, add a localized relationship name per `Kind` alongside `Inference::label()`, draw the marker at `inference.at` with `Tokens` colours (`tk.rubber_band`), render one marker per applicable guide, and clear indicators when `find`/`current` becomes `None` (via the existing `keep(None)` path) so they vanish within one frame. No literal `Color32` in these draw paths; keep the clean-room finding logic unchanged.
    - _Requirements: 3.1, 3.2, 3.3, 3.4, 3.5, 3.6, NFR 1, NFR 2, NFR 9, NFR 10_

  - [x]* 8.2 Write property test for marker position and type
    - **Property 4: Snap and inference markers appear at the snapped position**
    - Over generated sketches/cursor positions where the engine reports a target, assert a marker is produced at `snap.at` / `inference.at` carrying the named type.
    - **Validates: Requirements 3.1, 3.3**

  - [x]* 8.3 Write property test for one marker per inference
    - **Property 5: Every applicable inference gets its own marker**
    - Assert the rendered inference-marker count equals `guides.len()` when multiple inferences apply.
    - **Validates: Requirements 3.4**

  - [x]* 8.4 Write unit tests for clearing and token colours
    - Assert moving away clears the indicator within one frame (R3.5) and that the draw colours equal the chosen `Tokens` fields (R3.6).
    - _Requirements: 3.5, 3.6_

- [x] 9. Dimension entry routing through `sketch.dimension.commit`
  - [x] 9.1 Route confirm/cancel and prefill through the engine
    - In `crates/ui-egui/src/sketch_dims.rs` and `crates/ui-egui/src/dim_view.rs`, keep both entry surfaces but route confirm through `app.run("sketch.dimension.commit", ...)` with the typed `value` (entities/`text_at` from the dimension tool's picks for a new dimension; `param` for an edit). Preserve focus-and-select-all so typed characters replace the value (R4.1), prefill the editor from the parameter's stored expression (R4.3), surface commit errors via `set_status(msg, true)`, and keep cancel evaluating only in a scratch session so the design and undo depth stay unchanged (R4.5).
    - _Requirements: 4.1, 4.2, 4.3, 4.4, 4.5, NFR 3, NFR 4, NFR 5_

  - [x]* 9.2 Write property test for the edit prefill round trip
    - **Property 7: Editing an existing dimension prepopulates its current value**
    - Assert opening an existing dimension for editing populates the entry with an expression evaluating to the current value.
    - **Validates: Requirements 4.3**

  - [x]* 9.3 Write property test for cancel leaving nothing changed
    - **Property 9: Cancelling a dimension entry changes nothing**
    - Assert cancelling a typed-but-cancelled draft leaves `session.doc` and undo history identical to before the entry opened.
    - **Validates: Requirements 4.5**

  - [x]* 9.4 Write UI example test for focus behaviour
    - Using the harness, assert an activated entry focuses the numeric input and typed characters enter the value (R4.1).
    - _Requirements: 4.1_

- [x] 10. Checkpoint - interactive sketch legibility surfaces build and pass
  - Ensure all tests pass, ask the user if questions arise.

- [x] 11. Empty-state and discovery cue
  - [x] 11.1 Draw the empty-state cue
    - Add `empty_state_visible(app) -> bool` and the cue overlay drawing in `crates/ui-egui/src/sketch_display.rs` (or a small helper it calls): visible iff the active sketch has no geometry and no tool is running; hidden once the first curve is added and for the duration of an active tool. Draw text from the `sketch.empty_state_cue` key with `Tokens` colours (`tk.text_dim` on `tk.overlay`), as a passive overlay with no interactive widgets so it never blocks the viewport.
    - _Requirements: 5.1, 5.2, 5.3, 5.4, NFR 9, NFR 10_

  - [x]* 11.2 Write property test for cue visibility
    - **Property 10: The empty-state cue is shown exactly when the sketch is empty and idle**
    - Over generated active-sketch states, assert `empty_state_visible == (is_empty && no_tool)`.
    - **Validates: Requirements 5.1, 5.2, 5.3**

- [x] 12. Coaching surface (`coaching.rs`) and UI mirror
  - [x] 12.1 Add the coaching step table, view model, and mirror
    - Create `crates/ui-egui/src/coaching.rs` with the `CoachingStep { text_key, anchor }` table (`STEPS`) and `coaching_view(app) -> Option<CoachingView>` returning `None` when disabled/dismissed and otherwise the step text (via `app.tr`), anchor resolved through `scenario::handle_at`, index, and `last` flag.
    - In `crates/ui-egui/src/lib.rs`, add a thin non-authoritative `app.coaching` mirror loaded from the engine on start and refreshed from each `ui.coaching.*` command's returned JSON; have `start`/`run` set the tool step. Pass `STEPS.len()` as the step count to the advance command so the engine stays free of L5 strings.
    - _Requirements: 6.1, 6.5, 6.6, 6.7, 6.8, NFR 6, NFR 10, NFR 15_

  - [x] 12.2 Draw the non-blocking callout and wire controls
    - Draw the callout as a self-sized foreground `egui::Area` positioned beside its published anchor (centered fallback when off-screen), with no full-screen input-capturing layer so sketch clicks, palette toggles, and tool interactions still reach their widgets (R6.4). Use only `Tokens` colours (`tk.dialog_bg`, `tk.dialog_border`, `tk.text`, `tk.accent`, `tk.accent_soft`); draw Next/Dismiss/Close glyphs in code via `crates/ui-egui/src/icons.rs` (no file assets). Wire Next → `app.run("ui.coaching.advance", ...)`, Dismiss → `ui.coaching.dismiss`, and an enable/disable toggle (in Help/Preferences) → `ui.coaching.set`, refreshing the mirror after each call. Call the surface from `viewport.rs`.
    - _Requirements: 6.1, 6.2, 6.3, 6.4, 6.5, 6.6, 6.7, 6.8, NFR 7, NFR 9, NFR 10_

  - [x]* 12.3 Write property test for the step-at-index view
    - **Property 11: The coaching layer presents the step at its current index**
    - For an enabled, non-dismissed preference with an in-range index, assert `coaching_view` presents `STEPS[index]` text and the same anchor target.
    - **Validates: Requirements 6.1**

  - [x]* 12.4 Write UI harness test for non-blocking behaviour and colours
    - With coaching shown, assert a sketch click still reaches the active tool (R6.4), and assert the callout draws with the chosen `Tokens` fields (R6.10).
    - _Requirements: 6.4, 6.10_

- [x] 13. Theming additions (only if an existing token does not fit)
  - [x]* 13.1 Add coaching tokens if review finds existing fields insufficient
    - If `dialog_*` + `accent_soft` prove visually insufficient against the viewport, add a `coaching_callout`/`coaching_connector` pair to `crates/ui-egui/src/theme.rs` in both `Tokens::light()` and `Tokens::dark()`, covered by the existing theme-contrast test style. No literal colours in the feature's draw paths.
    - _Requirements: 6.10, NFR 9_

- [x] 14. Final checkpoint - full quality gates
  - [x] 14.1 Run `cargo xtask ci`
    - Run `cargo xtask ci` (fmt, clippy -D warnings, tests, assets, layers, wasm) with `CARGO_TARGET_DIR` set to the repo `target`, and fix any failures (formatting, lint, layering, wasm build, asset attribution) until green.
    - _Requirements: NFR 1, NFR 2, NFR 7, NFR 8, NFR 9, NFR 10, NFR 11, NFR 12, NFR 14, NFR 15_

- [x] 15. `view.*` CommandSpecs — standard views and fit (L4 engine, no egui)
  - [x] 15.1 Add the `view` command module with standard-view and fit commands
    - Create `crates/engine/src/cmd/view.rs` and register it in `crates/engine/src/cmd/mod.rs::command_specs`, alongside (not duplicating) the existing named-view commands in `cmd/browser.rs` (`view.save`/`rename`/`delete`/`list`).
    - Define `view.standard.top`, `view.standard.front`, `view.standard.right`, `view.standard.home`, `view.standard.isometric` — area `view`, `.noundo()`, `.doc_only()`, not undoable, five distinct ids (no free-text view-name param). Each `run` takes `camera: {target, yaw, pitch, roll, distance, fov?, up?}` (the design's own `solidcraft_render::Camera` JSON), resolves the orientation through `StandardView::parse` (L1 render), applies it via `Camera::set_view` keeping target/distance so the model stays framed, and returns `{camera}`. `home` maps to the isometric orientation (R9.1/R9.4). Reuse the render `Camera`/`StandardView` unchanged — add no camera logic below L1.
    - Define `view.fit` — area `view`, `.noundo()`, `.doc_only()`. Params `camera: {…}`, `aspect: number > 0`, `selection?: bool`. Compute visible-model/selection bounds via the engine's existing `view::bounds`, fit the camera distance/target to those bounds for the given aspect, and return `{camera, changed: bool}`. With no visible geometry (empty bounds) return the camera unchanged and `changed:false` (R9.3 edge).
    - Treat `camera`/`aspect` as hostile input: reject a malformed or non-finite camera, a non-positive/`NaN` aspect, or a camera that fails `is_valid` with a descriptive `Err` (never panic); an unknown orientation name from `StandardView::parse` returns the camera unchanged (R9.6). `get()`-style access, `Result` + `?`, no `unwrap`/`expect`/`panic!`/`unsafe`. Keep buildable for `wasm32-unknown-unknown`.
    - _Requirements: 9.1, 9.2, 9.3, 9.4, 9.6, NFR 3, NFR 4, NFR 5, NFR 6, NFR 11, NFR 13, NFR 14, NFR 16, NFR 17_

  - [x]* 15.2 Write property test for standard-view orientation and unknown-name rejection
    - **Property 18: A standard view applies its orientation and rejects unknown names**
    - Over generated valid cameras, assert each `view.standard.*` id returns a valid camera carrying that orientation; assert an unknown orientation name returns the camera unchanged.
    - **Validates: Requirements 9.2, 9.6**

  - [x]* 15.3 Write property test for fit framing
    - **Property 19: Fit frames a non-empty model and leaves an empty one unchanged**
    - Over generated designs and aspects, assert a non-empty model yields a valid camera framing the bounds with `changed:true`, and an empty model returns the camera unchanged with `changed:false`.
    - **Validates: Requirements 9.3**

- [x] 16. Checkpoint - engine `view.*` commands build and pass
  - Ensure all tests pass, ask the user if questions arise.

- [x] 17. Command palette and ribbon discoverability — R7 (L5)
  - [x] 17.1 Make `matches` a deterministic pure function with synonyms and effective shortcuts
    - In `crates/ui-egui/src/palette.rs`, define `Match { id, label_key, icon, shortcut: Option<String> }` and a pure `rank(q, id, label, synonyms) -> Option<u8>` (0 = id/localized-label starts-with, 1 = contains, 2 = synonym-only). Rewrite `matches(app, q) -> Vec<Match>` to match on `id`, the localized label (`tr_label(spec.label)`), or a `Locale_Catalog` synonym (`palette.synonyms.<id>`, comma-separated, split-and-trimmed) case-insensitively, sorted by `(rank, localized_label)` — total and deterministic for any input.
    - Set each `Match.shortcut` to `keymap::effective(app, id)` (the binding actually in force, not the `CommandSpec` default) so the palette shows the key the user would press (R7.5).
    - In the `S`-key search popup (`palette::popup`), keep `request_focus()` so the input is focused on open (R7.1); confirm the `S` global handler only opens when no text field is focused. Activating an entry calls `app.start(id)` (R7.6, never `app.run`). A no-match submit sets an error status (`palette.no_match` via `set_status(_, true)`) and leaves the popup open and the design unchanged (R7.7). Read catalog entries with `get()`-style access and bounded lengths; no panics.
    - _Requirements: 7.1, 7.2, 7.3, 7.4, 7.5, 7.6, 7.7, 7.8, NFR 5, NFR 9, NFR 10, NFR 11, NFR 13_

  - [x] 17.2 Switch ribbon labels/shortcuts to localized + effective
    - In `crates/ui-egui/src/toolbar.rs` and `crates/ui-egui/src/workspace.rs`, source every ribbon button and panel drop-down label through `tr_label` and switch the displayed shortcut column from `spec.shortcut` to `keymap::effective(app, id)` (R7.5/R7.8). Publish a `toolbar:tab.<TAB>` handle per ribbon tab for coaching anchoring. Colours from `Tokens`, icons via `icons.rs`.
    - _Requirements: 7.5, 7.8, NFR 9, NFR 10, NFR 11_

  - [x]* 17.3 Write property test for deterministic palette ordering
    - **Property 15: Palette matches are ordered deterministically**
    - Over generated queries and command registries, assert `matches` is total and deterministic and that every starts-with match precedes every contains-only match, which precedes every synonym-only match; ties ascend by localized label.
    - **Validates: Requirements 7.2, 7.3**

  - [x]* 17.4 Write property test for synonym matching
    - **Property 16: A synonym finds its command**
    - For any command carrying a `palette.synonyms.<id>` term, assert a query contained in that synonym returns a match list including that command.
    - **Validates: Requirements 7.4**

  - [x]* 17.5 Write UI harness test for popup focus, shortcut, activation, no-match
    - Using `scenario::Harness`, open the popup with `S` and assert the input is focused (R7.1), an entry shows its effective shortcut (R7.5), activating a no-param entry calls `app.start` (R7.6), and a no-match submit leaves the popup open, shows the error status, and leaves the design unchanged (R7.7).
    - _Requirements: 7.1, 7.5, 7.6, 7.7_

- [x] 18. Shared dialog frame and dialog migration — R8 (L5)
  - [x] 18.1 Add the shared `dialog_frame` helper
    - Add a `dialog_frame` module (e.g. `crates/ui-egui/src/dialog_frame.rs`, wired into `lib.rs`) with `DialogFrame { title_key, preview, error }`, `FieldError { field_key, message }`, `DialogOutcome { Open, Confirm, Cancel }`, and `DialogFrame::show(app, ctx, body)` per design: a fixed bottom footer holding Cancel then OK in a stable order allocated *before* the central `ScrollArea::vertical` (so confirm/cancel never scroll off at any window height, R8.1); a shared `dimension_field(label_key, unit)` that draws the unit (`mm`/`rad`/…) beside dimensioned values reusing the existing `UNITS`/`unit_label` vocabulary (R8.2); a live preview run on a scratch session via the existing preview pipeline when parameters are valid and `None`/suppressed when any field is invalid, within a frame budget well under 200 ms (R8.3); `Confirm` lets the caller run the command via `app.run` and `Cancel` drops the draft leaving `session.doc`/undo depth unchanged (R8.4/R8.5); a per-field `FieldError` drawn next to the offending field in `Tokens` error colour while keeping the dialog open (R8.6). Colours from `Tokens`, strings via `tr`/`tr_label` (R8.7). No panics; `Result` + `?`.
    - _Requirements: 8.1, 8.2, 8.3, 8.4, 8.5, 8.6, 8.7, NFR 5, NFR 9, NFR 10, NFR 11, NFR 13_

  - [x] 18.2 Adopt the frame in `params_dialog.rs` (reference adopter) and the `dialogs*.rs` family
    - Move `crates/ui-egui/src/params_dialog.rs` onto `dialog_frame` first (it already has units, a live preview, and per-field errors), then migrate `crates/ui-egui/src/dialogs.rs`, `dialogs_appearance.rs`, `dialogs_assembly.rs`, `dialogs_move.rs`, `dialogs_parts.rs`, `dialogs_plastic.rs`, `dialogs_sheet.rs`, `dialogs_surface.rs`, and `dialogs_motion.rs` to the shared frame so every dialog keeps confirm/cancel outside the scroll in a stable order, shows units, previews while valid, and shows per-field errors without closing. Confirm runs the command through `app.run(id, params)`; the engine command stays the single validation authority. Colours from `Tokens`, labels/messages via `tr`/`tr_label`.
    - _Requirements: 8.1, 8.2, 8.3, 8.4, 8.5, 8.6, 8.7, NFR 5, NFR 9, NFR 10, NFR 11_

  - [x]* 18.3 Write property test for cancel invariant and reachable actions
    - **Property 17: A cancelled dialog changes nothing and keeps its actions reachable**
    - Over generated dialogs/parameters, assert Cancel/OK sit outside the scroll in a fixed order, the preview is present exactly when parameters are valid (suppressed when invalid), and a `Cancel` leaves `session.doc` and undo depth unchanged.
    - **Validates: Requirements 8.1, 8.3, 8.5**

  - [x]* 18.4 Write UI example tests for error-keeps-open and confirm-runs-once
    - Drive one `params`-style edit and one `dialogs_*` feature dialog with the harness: assert a rejected field keeps the dialog open with a localized per-field message (R8.6) and a confirm runs the command exactly once through `app.run` (R8.4).
    - _Requirements: 8.4, 8.6_

- [x] 19. Viewport nav bar + axis view via `view.*` commands — R9 (L5)
  - [x] 19.1 Route discrete view changes through commands; keep orbit/pan/zoom as gestures
    - In `crates/ui-egui/src/viewport.rs`, change `nav_bar` and `axis_view` so the standard-view controls, Fit, and Home apply the camera *only* through `app.run("view.standard.{top,front,right,home,isometric}", {camera})` / `app.run("view.fit", {camera, aspect, selection})` and then `animate_to` the returned camera — never mutating `app.cam` directly for a named/fit/home action (R9.1/R9.2/R9.3/R9.4). On `view.fit` returning `changed:false`, show a brief localized `view.nothing_to_fit` status and do not move the camera (R9.3 edge). Keep orbit/pan/zoom as continuous pointer gestures applied to `app.cam` each frame (R9.5). Draw every colour from `Tokens` (axis tips from the engine `colors::AXIS_*`, highlights `tk.accent`/`tk.overlay`) and every tooltip/label via `tr` (`view.nav.orbit|pan|zoom|fit|home|lookat`) (R9.7). No literal `Color32`; no panics.
    - _Requirements: 9.1, 9.2, 9.3, 9.4, 9.5, 9.7, NFR 5, NFR 9, NFR 10, NFR 11_

  - [x] 19.2 Keep the drawable-rect fit refinement in `viewport_layout.rs`
    - In `crates/ui-egui/src/viewport_layout.rs`, keep the `Layout` (nav-bar + axis-view rectangles) and the `fit_bounds` drawable-rect refinement that nudges the framed model into the free rectangle left by the floating overlays — a pure presentation adjustment that does not re-run the `view.fit` command.
    - _Requirements: 9.3, 9.7, NFR 9, NFR 10_

  - [x]* 19.3 Write UI harness test for command-path navigation
    - Using the harness, click the nav bar's Home/Fit and an axis tip and assert the camera moved only via the command path (the returned camera), that labels/tooltips come from `tr`, and that colours come from `Tokens` (R9.2/R9.7).
    - _Requirements: 9.2, 9.7_

- [x] 20. Keymap page localization and R10 completeness — R10 (L5)
  - [x] 20.1 Localize the keymap page and fill any R10 gaps
    - In `crates/ui-egui/src/keymap.rs`, move the page's own chrome strings behind `tr` catalog keys (`keymap.filter_hint`, `keymap.reset_all`, `keymap.press_a_key`, `keymap.use_anyway`, `keymap.cancel`, `keymap.conflict`). Confirm and, where any gap exists, complete the R10 contract already present: `effective` defaults from `CommandSpec` (R10.1), pressing a bound key runs the command via `app.start` (R10.2), `bind`/clear/`reset` store/remove overrides and restore defaults (R10.3/R10.4/R10.7), a conflicting bind reports and requires a confirming `replace` before taking the key (R10.5), reserved keys are refused (R10.6), the filter searches label/id/key (R10.8), and the override map persists through the settings layer (NFR 8). `load`/`bind`/`reset` read persisted data with bounded iteration, skip malformed entries, and never panic. Colours from `Tokens`.
    - _Requirements: 10.1, 10.2, 10.3, 10.4, 10.5, 10.6, 10.7, 10.8, NFR 8, NFR 9, NFR 10, NFR 11, NFR 13_

  - [x]* 20.2 Write property test for bindings round-trip, reset, reserved, and conflict
    - **Property 20: Keymap bindings round-trip, reset to defaults, and respect reserved keys and conflicts**
    - Over generated bind/clear/reset/conflict sequences, assert the override map round-trips serialize→deserialize to the same effective bindings, `reset` restores every `CommandSpec` default, a reserved key is never bound, and a conflicting bind takes effect only after a confirming `replace` (after which the key runs exactly one command). Keep the existing `rebinding_detects_conflicts_and_resets` test.
    - **Validates: Requirements 10.1, 10.3, 10.4, 10.5, 10.6, 10.7**

  - [x]* 20.3 Write catalog test for localized keymap page strings
    - Assert each `keymap.*` page key resolves in `en` and is present/translatable in `ja`/`de`/`ko`.
    - _Requirements: 10.8, NFR 12_

- [x] 21. Timeline legibility — R11 (L5)
  - [x] 21.1 Localize labels/status, token state colours, and add a hover dwell
    - In `crates/ui-egui/src/timeline.rs`, iterate `session.doc.features` in stored (creation) order so the drawn order is the creation order (R11.1). Keep `double_clicked()` re-opening a feature through its own editing command (`app.edit_feature(id)`), with no camera/doc mutation outside the command (R11.2). Add a pure, testable `HoverDwell { id, since }` on the viewport state: promote a hovered feature to `hover_feature` only after a ~150 ms dwell and clear `hover_feature` the frame the pointer leaves the strip (R11.3). Draw error state with `tk.timeline_item_error` fill + `tk.error` edge (R11.4) and suppressed state as the ordinary fill with a `tk.text_dim` strike + `tk.border`-dimmed icon — distinct from both ordinary and error (R11.5). Route the feature display name through `tr_dyn` and the status words (`timeline.status.error|warning|suppressed`, `timeline.tip_*`) through `tr` (R11.6). Colours only from `Tokens`; no panics.
    - _Requirements: 11.1, 11.2, 11.3, 11.4, 11.5, 11.6, NFR 5, NFR 9, NFR 10, NFR 11_

  - [x]* 21.2 Write property test for creation order and distinct states
    - **Property 21: The timeline reads in creation order with distinct feature states**
    - Over generated feature lists, assert the drawn order equals the creation order and that error / suppressed / ordinary map to three distinguishable `Tokens`-sourced presentations.
    - **Validates: Requirements 11.1, 11.4, 11.5**

  - [x]* 21.3 Write property test for the hover dwell classifier
    - **Property 22: Hover highlighting follows a dwell and clears on leave**
    - Over a generated hovered-id/timestamp stream, assert the dwell classifier promotes a feature only after the ~150 ms dwell and clears on leave.
    - **Validates: Requirements 11.3**

  - [x]* 21.4 Write UI harness test for double-click-to-edit and status i18n
    - Using the harness, assert double-clicking a feature re-opens it through its command and that the status text is localized.
    - _Requirements: 11.2, 11.6_

- [x] 22. Browser legibility — R12 (L5)
  - [x] 22.1 Localize node text, token colours, three-way constraint state, visibility/active sync
    - In `crates/ui-egui/src/browser.rs`, keep the component → body → sketch hierarchy as `Browser_Node` rows (R12.1). Route the eye control through `browser.visibility` and read each row's visibility from the session so the toggle and indicator stay in sync; on command error the row keeps its previous indicator (R12.2). Add a pure, testable `sketch_constraint_state(report) -> {UnderDefined, FullyDefined, Locked}` classifier driven by the sketch solver report, mapped to three visually distinct indicators coloured from `Tokens` — `tk.sketch` (under-defined, open ring), `tk.sketch_fixed` (fully-defined, filled), `tk.sketch_locked` (locked, padlock badge) (R12.3). Mark exactly one component row active where `active == (session.active_component == id)` with the `tk.accent` radio; clicking an inactive radio runs `component.activate`, and on failure the indicator stays on the previously active component (R12.4/R12.5). Node labels via `tr_dyn`/`tr_label`, state tooltips via `tr` (`browser.sketch.under_defined|fully_defined|locked`, reusing `ui.show`/`ui.hide`/`ui.activate`/`ui.active_component`); every colour from `Tokens` (R12.6). No panics.
    - _Requirements: 12.1, 12.2, 12.3, 12.4, 12.5, 12.6, NFR 5, NFR 9, NFR 10, NFR 11_

  - [x]* 22.2 Write property test for the constraint-state classifier
    - **Property 23: The browser classifies every sketch into one of three constraint states**
    - Over generated sketch solver reports, assert the classifier is total and returns exactly one of under-defined / fully-defined / locked, mapped to three distinct indicators.
    - **Validates: Requirements 12.3**

  - [x]* 22.3 Write property test for single active component and visibility sync
    - **Property 24: Exactly one component is marked active and visibility stays in sync**
    - Over generated designs, assert exactly one component row is active and equals `session.active_component`, and that the row's visibility indicator always reflects the session's visibility (a rejected toggle leaves it unchanged).
    - **Validates: Requirements 12.2, 12.4**

  - [x]* 22.4 Write UI harness test for activation and i18n/colour sourcing
    - Using the harness, assert clicking an inactive component radio runs `component.activate` and that labels/colours come from `tr`/`Tokens`.
    - _Requirements: 12.5, 12.6_

- [x] 23. App-level onboarding extension — R13 (L5)
  - [x] 23.1 Extend the coaching step table, add the new-document cue and first-run/returning logic
    - In `crates/ui-egui/src/coaching.rs`, extend the `STEPS` table so it covers the whole app: add at least one step introducing the ribbon `Workspace_Tab` navigation (`coaching.step_workspace_tabs`, anchored to a `toolbar:tab.<TAB>` handle) (R13.2) and at least one step covering the modelling workflow beyond the sketch surface (`coaching.step_extrude`, a non-sketch `solid.*` command) (R13.3), keeping the Requirement-6 sketch steps in sequence. Add a pure `new_document_cue_visible(app) -> bool` returning `app.session.doc.features.is_empty()` and draw a short centered empty-state cue (`coaching.empty_state_cue`) while true, hidden once the first feature exists (R13.1/R13.4). Derive first-run vs returning from the persisted `completed` flag (no new field): a first-timer sees the full sequence, a returning person still gets the empty-state cue on a new document. Pass `STEPS.len()` as the step count to `ui.coaching.advance`. Disabling onboarding is the same `ui.coaching.set { enabled: false }` persisted through the engine (R13.6); the cue is suppressed while coaching is disabled.
    - _Requirements: 13.1, 13.2, 13.3, 13.4, 13.6, NFR 4, NFR 5, NFR 8, NFR 10, NFR 15_

  - [x] 23.2 Draw the non-blocking app-level callouts and anchor to ribbon tabs
    - In `crates/ui-egui/src/coaching.rs` (called from `viewport.rs`/`toolbar.rs`), draw the app-level callouts and the empty-state cue as self-sized foreground `egui::Area`s positioned beside their published anchors (centered fallback off-screen), with no full-screen input-capturing layer, so the ribbon tabs, viewport, timeline, browser, and palette stay fully reachable (R13.5). Anchor the workspace-tab step to the `toolbar:tab.<TAB>` handle published in task 17.2. Colours from `Tokens`; Next/Dismiss/Close glyphs drawn in code via `icons.rs`; every string via `tr` (R13.7). No panics.
    - _Requirements: 13.2, 13.5, 13.7, NFR 7, NFR 9, NFR 10, NFR 11_

  - [x]* 23.3 Write property test for the new-document cue predicate
    - **Property 25: The new-document cue is shown exactly when the design has no features**
    - Over generated session states, assert `new_document_cue_visible == features.is_empty()`.
    - **Validates: Requirements 13.1, 13.4**

  - [x]* 23.4 Write reducer/catalog test for the app-level step model
    - Assert the `STEPS` table includes a workspace-tab step and a non-sketch step, and that every app-level key (`coaching.step_workspace_tabs`, `coaching.step_extrude`, `coaching.empty_state_cue`) translates in all four locales.
    - _Requirements: 13.2, 13.3, 13.7_

  - [x]* 23.5 Write UI harness test for non-blocking onboarding
    - With onboarding shown, assert a ribbon-tab switch and a toolbar command both still fire (R13.5).
    - _Requirements: 13.5_

- [x] 24. i18n catalog keys for the whole-shell strings — R7–R13 (L5)
  - [x] 24.1 Add and translate the new whole-shell catalog keys
    - Add to `crates/ui-egui/src/locales/en.json` (and translate in `ja.json`, `de.json`, `ko.json`) all new keys the shell models reference: palette synonyms `palette.synonyms.<command id>` (comma-separated per locale, only for commands that benefit; ordinary CAD/English words — never another product's command label) and `palette.no_match`; dialog chrome `dialog.ok`, `dialog.cancel`; view `view.standard.top|front|right|home|isometric`, `view.fit`, `view.nothing_to_fit`, `view.nav.orbit|pan|zoom|fit|home|lookat`; timeline `timeline.status.error|warning|suppressed`, `timeline.tip_*`; browser `browser.sketch.under_defined|fully_defined|locked`; keymap `keymap.filter_hint|reset_all|press_a_key|use_anyway|cancel|conflict`; coaching app-level `coaching.step_workspace_tabs`, `coaching.step_extrude`, `coaching.empty_state_cue`. Route every string through `app.tr(id)`/`app.tr_label(label)`; no hardcoded literals in draw paths.
    - _Requirements: 7.8, 8.7, 9.7, 10.8, 11.6, 12.6, 13.7, NFR 10, NFR 12_

  - [x]* 24.2 Extend the i18n catalog test to the new whole-shell keys
    - **Property 3: Every prompt and label string resolves through the catalog**
    - In `i18n_ui_tests.rs` / the `i18n.rs` catalog tests, assert every new whole-shell key (palette/dialog/view/timeline/browser/keymap/coaching app-level) resolves in `en` (not raw id) and is present/translatable in `ja`/`de`/`ko`, including the per-command synonym keys.
    - **Validates: Requirements 7.8, 8.7, 9.7, 10.8, 11.6, 12.6, 13.7**

- [x] 25. Whole-shell theming additions (only if an existing token does not fit)
  - [x]* 25.1 Add any missing tokens if review finds existing fields insufficient
    - If `timeline_item*`, `sketch*`, `dialog_*`, `accent_soft`, or the axis/overlay tokens prove visually insufficient for the timeline state, browser constraint state, dialog frame, or app-level coaching callout, add the needed field(s) to `crates/ui-egui/src/theme.rs` in both `Tokens::light()` and `Tokens::dark()`, covered by the existing theme-contrast test style. No literal colours in the feature's draw paths.
    - _Requirements: 8.7, 9.7, 11.4, 11.5, 12.6, 13.7, NFR 11_

- [ ] 26. Final checkpoint - full quality gates (whole feature)
  - [ ] 26.1 Run `cargo xtask ci`
    - Run `cargo xtask ci` (fmt, clippy -D warnings, tests, assets, layers, wasm) with `CARGO_TARGET_DIR` set to the repo `target`, and fix any failures (formatting, lint, layering, wasm build, asset attribution) until green across the engine `view.*` commands and every L5 shell surface added by tasks 15–25.
    - _Requirements: NFR 1, NFR 2, NFR 7, NFR 9, NFR 10, NFR 11, NFR 13, NFR 14, NFR 16, NFR 17_

## Notes

- Tasks marked with `*` are optional (tests and the conditional theming field) and can be skipped
  for a faster MVP; the core implementation path is never optional.
- Each task references specific requirement IDs — R1..R6 acceptance criteria for tasks 1–14,
  R7..R13 (and the NFR constraints) for tasks 15–26 — for traceability.
- Property-test sub-tasks each reference one design Correctness Property (Properties 1–25) and run
  at least 100 generated iterations, tagged
  `Feature: ux-ui-shell-improvements, Property {n}: {property text}`.
- Checkpoints (tasks 4, 10, 14, 16, 26) ensure incremental validation; task 14 closes the sketch/
  coaching arc and task 26 runs the full `cargo xtask ci` gate (fmt, clippy -D, tests, assets,
  layers, wasm) across the whole feature after the whole-shell work in tasks 15–25.
- Layering: engine `view.*` commands (task 15, L4) and the render `Camera`/`StandardView` they
  reuse (L1) carry no egui; every shell surface in tasks 17–25 stays in the L5 UI crate; the UI
  acts only through `app.run`/`app.start` and the camera remains UI view state changed only through
  commands.
- Clean-room: all engine logic and UI legibility are SolidCraft's own; palette synonyms are
  ordinary CAD/English words, never another product's command label; no code, ids, strings, icons,
  or data are sourced from FreeCAD/OpenCASCADE/planegcs/SolveSpace/LibreCAD/QCAD/Fusion/SolidWorks/
  Onshape or the local FreeCAD install.
- Same-file-writer separation (reflected in the waves): `viewport.rs` (tasks 19.1 then 23.2),
  `coaching.rs` (23.1 then 23.2), `toolbar.rs` (17.2 then 23.2), and the `locales/*.json` /
  `theme.rs` writers never share a wave.

### Status audit (checkbox sync with the repo)

- The engine arc (tasks 1–4) is implemented and its tests pass: `crates/engine/src/coaching.rs`
  (`CoachingPreference` + reducers, Properties 12/13/14), `crates/engine/src/cmd/ui.rs`
  (`ui.coaching.*` + unit tests), `sketch.dimension.commit` in `crates/engine/src/cmd/sketch.rs`
  with `crates/engine/src/cmd/sketch_commit_tests.rs` (Properties 6/8). Verified via
  `cargo test -p solidcraft-engine --lib coaching` and `commit_tests` (all green).
- The sketch/coaching L5 arc (tasks 5–13) is implemented and its property tests pass
  (`cargo test -p solidcraft-ui-egui --lib -- property_` → Properties 1, 2, 3, 4&5, 10, 11 green):
  `tools.rs` (`ToolStep`/`Tool::step()`/`tool_feedback`), `sketch_tools.rs` step descriptors,
  i18n keys + `shell_ui_tests.rs::property_3`, the viewport feedback chip + inline prompt,
  `drag_snap.rs`/`inference.rs` localized markers, dimension routing through
  `sketch.dimension.commit` in `dim_view.rs`/`sketch_dims.rs`, the empty-state cue in
  `sketch_display.rs`, and the coaching surface in `ui-egui/src/coaching.rs`.
- Tasks 9.2 / 9.3 (Properties 7 and 9) and parent task 9 are marked done: the behaviours are
  covered on fork `dev` (named properties and/or
  `shell_ui_tests.rs::editing_a_dimension_starts_from_its_value_and_cancel_keeps_the_design`).
- Task 13.1 is a conditional optional task: the coaching callout reuses the existing
  `dialog_bg`/`dialog_border`/`accent`/`accent_soft` tokens and no `coaching_callout`/
  `coaching_connector` fields were added, i.e. the review found existing tokens sufficient, so the
  conditional task is satisfied with no change.
- The whole-shell arc (tasks 15–25) **is implemented on fork `dev`** as of 2026-10-22:
  `crates/engine/src/cmd/view.rs` (`view.standard.*`, `view.fit`, `view.orient`),
  `crates/ui-egui/src/dialog_frame.rs` (Cancel-before-OK via `action_labels`),
  `palette.rs` ranking/synonyms, timeline dwell + token states + localized context menu,
  browser three-state sketch constraint indicator, and Properties 7, 9, 15–25.
  Implementation checkboxes for 15–25 are marked done so agents do **not** rebuild those
  surfaces. Task 26 (`cargo xtask ci`) stays open until a full gate is green on this fork.
- **Out of scope:** 3D interchange writers (glTF, PLY, USD, proprietary refusals, …). That work
  lives in `fusion360-feature-parity` Requirement 15 / task 24. Do not add format exporters under
  this UX shell plan.

## Task Dependency Graph

```json
{
  "waves": [
    { "id": 0, "tasks": ["1.1", "6.1"] },
    { "id": 1, "tasks": ["1.2", "1.3", "1.4", "2.1", "3.1", "6.2"] },
    { "id": 2, "tasks": ["2.2", "3.2", "3.3", "5.1", "8.1"] },
    { "id": 3, "tasks": ["5.2", "5.3", "8.2", "8.3", "8.4", "9.1"] },
    { "id": 4, "tasks": ["5.4", "5.5", "7.1", "9.2", "9.3", "9.4", "11.1", "12.1"] },
    { "id": 5, "tasks": ["7.2", "11.2", "12.2", "13.1"] },
    { "id": 6, "tasks": ["12.3", "12.4"] },
    { "id": 7, "tasks": ["14.1", "15.1"] },
    { "id": 8, "tasks": ["15.2", "15.3", "17.1", "18.1", "20.1", "21.1", "22.1", "24.1"] },
    { "id": 9, "tasks": ["17.2", "17.3", "17.4", "18.2", "18.3", "20.2", "20.3", "21.2", "21.3", "22.2", "22.3", "24.2"] },
    { "id": 10, "tasks": ["17.5", "18.4", "19.1", "21.4", "22.4", "23.1"] },
    { "id": 11, "tasks": ["19.2", "19.3", "23.2", "23.3", "23.4"] },
    { "id": 12, "tasks": ["23.5", "25.1"] },
    { "id": 13, "tasks": ["26.1"] }
  ]
}
```
