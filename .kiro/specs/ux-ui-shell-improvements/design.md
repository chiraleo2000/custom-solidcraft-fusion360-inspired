# Design Document

## Overview

This feature refines SolidCraft's existing egui shell so an ordinary person — not only an
expert — can understand it quickly and work in it fast, **across the whole application** rather
than inside the sketch surface alone. It is a refinement, not greenfield, and it adds no new
workspace or modelling capability: the sketch tools, command palette, ribbon, feature dialogs,
3D viewport and view navigation, keyboard map, feature timeline, browser/model tree, inference/
snap engine, dimension entry, sketch display, and coaching surface already exist in
`crates/ui-egui` (L5). The deferred `ROADMAP.md` workspaces (Surface/FORM, Render, Animation,
Simulation, CAM, PCB, 2D Drawing) stay out of scope. 3D interchange export
(STEP/glTF/… and proprietary refusals) is owned by `fusion360-feature-parity`
Requirement 15 — this shell only starts `file.export` and must not grow format writers.

The original sketch-focused outcomes (Requirements 1–6) remain exactly as designed below:
active sketch-tool feedback, inline prompts, snap/inference legibility, dimension-entry
ergonomics, the empty-state cue, and the guided coaching layer. This revision **adds** whole-
shell outcomes (Requirements 7–13) and widens the scope of the coaching layer to cover the
app, not just the sketch:

- **R7 — Command discoverability.** The command palette and ribbon find and run any existing
  command by name or synonym, show its shortcut, and order matches predictably.
- **R8 — Dialog ergonomics.** Every feature dialog shares one layout: confirm/cancel kept
  reachable, units shown, a live preview, and legible per-field errors.
- **R9 — View navigation.** Standard views, orbit/pan/zoom, zoom-to-fit, and return-to-home are
  all applied through SolidCraft commands.
- **R10 — Keyboard-first.** Every command's shortcut comes from its `CommandSpec`; shortcuts are
  searchable, rebindable, clearable, conflict-aware, reserved-key-safe, and persisted.
- **R11 — Timeline.** The feature timeline reads in creation order, re-opens a feature to edit,
  highlights on hover, and shows error/suppressed state distinctly.
- **R12 — Browser.** The model tree shows the component/body/sketch hierarchy, visibility
  toggles, sketch constraint state, and the active component.
- **R13 — App-level onboarding.** First-run and returning onboarding covers the new-document
  empty state and the workspace tabs, not sketch alone.

Much of this surface already works; the design makes it **legible, localized, token-themed, and
command-driven** to the standard the requirements set, and fills the specific gaps (synonym
matching and shortcut display in the palette, engine `view.standard.*`/`view.fit` commands, a
shared dialog frame, hover-highlight timing in the timeline, and app-level coaching steps).

How it fits the existing shell: the UI crate is thin and swappable. Panels and overlays read
engine/session state and act only through `app.run(id, params)` (JSON form, no dialog) or
`app.start(id)` (interactive tools and dialogs, started like a toolbar click). This feature keeps
that contract. The new user-visible behaviours — committing a typed dimension, applying a
standard/fit view, and toggling or advancing the coaching layer — are registered `CommandSpec`s
in `crates/engine/src/cmd/*` with SolidCraft's own dotted lower-case ids. The L5 UI holds no
authoritative preference state of its own for coaching; it reads it from the engine and mutates
it through commands. The keymap's user bindings persist through the host's settings layer, and
the camera continues to live as UI view state that is *changed only through commands* (R9).

The design respects every non-functional constraint in the requirements: clean-room provenance
(behaviour from public CAD convention only; nothing read, copied, or derived from FreeCAD,
OpenCASCADE, planegcs, SolveSpace, LibreCAD, QCAD, Fusion, SolidWorks, Onshape, or any local copy
of them); every colour from `theme::Tokens`; every string through the `tr_label` / `Locale_Catalog`
i18n path; icons drawn in code (`icons.rs`); `Result<T, E>` with `?` and no panics; the whole stack
below the apps builds for `wasm32-unknown-unknown`; and layering is preserved — nothing below L5
gains an egui/eframe/winit/wgpu/rfd dependency, and only `crates/kernel` uses `truck`.

Note (awareness, not in scope): `sketch_tools.rs`, `sketch_palette.rs`, and `prefs.rs` carry
existing comments and preset ids that mention "Fusion". This feature does not depend on changing
them; a clean-room pass over those comments is tracked separately and is out of scope here. No new
use of another product's name is added by this feature.

Language for code examples: Rust (the only language in this repo).

## Architecture

### Layering summary

```
L0 geom
L1 sketch | kernel | render   ← render owns Camera + StandardView (orientation + fit math),
                                clean-room and egui-free; the new view commands reuse it
L2 doc              ← Coaching_Preference lives in the engine session's app settings (below L5, no egui)
L3 io
L4 engine           ← new CommandSpecs: sketch.dimension.commit, ui.coaching.*,
                      view.standard.{top,front,right,home,isometric}, view.fit
                      (existing: component.activate, browser.visibility, timeline.*, view.save…)
L5 ui-egui          ← all egui drawing: feedback, prompts, snap/inference legibility,
                      dimension entry, empty-state cue, coaching surface, the palette's
                      synonym/shortcut/ordering, the shared dialog frame, the viewport nav
                      bar + axis view, the keymap page, the timeline and the browser
apps
```

Everything that touches egui stays in L5. The engine commands and the preference store below L5
are plain data and functions over `Session`/`Value`; they do not reference egui. The camera type
(`solidcraft_render::Camera`) and its orientation/fit helpers (`StandardView::parse`,
`Camera::set_view`) already live in L1 `render` and are already used by the engine's `view`
module (`view::bounds`); the new `view.*` commands build on them so no camera logic moves into or
below egui. egui/eframe/winit/wgpu/rfd stay confined to L5.

### Where each piece lives

Changed existing files (all in `crates/ui-egui/src`):

- `tools.rs` — add a `ToolStep` model to `Tool` (current step index + a step descriptor), so the
  active tool can report its step. `Tool::for_command`, `place`, and `finish` update the step as
  points are placed and when the tool ends. The existing `hint(id)` strings move behind localized
  keys (see i18n).
- `sketch_tools.rs` — the extra tools' step descriptors (the `hint(id)` table here becomes the step
  source for those commands). The `SNAP` type-label mapping gains localized labels.
- `inference.rs` — expose the current inference's localized label and `at` point for the marker
  model (already has `Inference::label()`; it gains a localized key alongside the glyph string).
- `drag_snap.rs` — the `Snap::label()` / `Target` mapping gains localized type labels and keeps
  publishing guide counts for the multi-inference assertion.
- `sketch_display.rs` — hosts the empty-state cue drawing (a viewport overlay for the active sketch)
  and continues to own display-only sampling.
- `sketch_dims.rs` and `dim_view.rs` — route confirm/cancel of a dimension entry through the new
  `sketch.dimension.commit` command and tighten finite-number validation + status messaging.
- `viewport.rs` — draws the active-tool feedback chip and inline prompt in the existing status
  overlay region; calls into the new coaching surface and the empty-state cue.
- `theme.rs` — new `Tokens` fields only if an existing token does not already fit (see Theming).
- `i18n.rs` + `locales/*.json` — new catalog keys (see i18n).
- `icons.rs` — any coaching-layer glyph (next/dismiss/close) drawn in code; no file assets.
- `lib.rs` — `SolveApp` gains a thin, non-authoritative mirror of the coaching state read from the
  engine on load and refreshed after each coaching command; `start`/`run` set the tool step.

Whole-shell files refined by Requirements 7–13 (all in `crates/ui-egui/src`):

- `palette.rs` — `matches()` gains synonym matching from the `Locale_Catalog` and a deterministic
  order (starts-with before contains, alphabetical tie-break); each entry shows its *effective*
  shortcut (`keymap::effective`); the `S`-key popup opens focused and stays open on a no-match
  submit while showing an error status (R7).
- `toolbar.rs` + `workspace.rs` — ribbon buttons and panel drop-downs already show a shortcut from
  the `CommandSpec` default; this feature switches that to the *effective* keymap binding and
  localizes labels through `tr_label` (R7.5, R7.8).
- `params_dialog.rs` and the `dialogs*.rs` family (`dialogs.rs`, `dialogs_appearance.rs`,
  `dialogs_assembly.rs`, `dialogs_move.rs`, `dialogs_parts.rs`, `dialogs_plastic.rs`,
  `dialogs_sheet.rs`, `dialogs_surface.rs`, `dialogs_motion.rs`) — all adopt one shared
  `dialog_frame` helper (see the dialog-ergonomics section) that fixes confirm/cancel outside the
  scroll region, keeps a stable control order, surfaces units, drives a live preview, and shows
  per-field errors without closing (R8).
- `viewport.rs` — `nav_bar` and `axis_view` call the new `view.*` commands through `app.run`
  instead of mutating `app.cam` directly; labels/tooltips come from `tr`, colours from `Tokens`
  (R9).
- `viewport_layout.rs` — keeps the `Layout` (nav bar + axis-view rectangles) and the drawable-rect
  `fit_bounds` refinement used to present the result of `view.fit` (R9.3).
- `keymap.rs` — already sources defaults from `CommandSpec`, supports bind/clear/reset, conflict
  report-and-confirm, reserved keys, a search filter, and persistence; this feature localizes its
  page strings and keeps its persistence through the settings layer (R10).
- `timeline.rs` — localizes feature labels/status text through `tr`/`tr_label`, draws error and
  suppressed state from `Tokens`, and adds a short hover dwell before highlighting (R11).
- `browser.rs` — localizes node labels and state text, draws every colour from `Tokens`, routes
  visibility and component activation through commands (already `browser.visibility` /
  `component.activate`), and shows a three-way sketch constraint-state indicator (R12).
- `coaching.rs` — the onboarding/coaching surface gains app-level steps (workspace tabs and a
  non-sketch modelling step) and a new-document empty-state cue (R13), building on the layer it
  already provides for Requirement 6.
- `i18n.rs` + `locales/*.json` — new catalog keys and per-command synonym keys (see i18n).
- `theme.rs` — reuse existing `Tokens` fields; add one only if none fits (see Theming).

New module in `crates/ui-egui/src`:

- `coaching.rs` — the onboarding/coaching surface (L5), already present for Requirement 6 and
  extended here for Requirement 13. Non-blocking overlay, dismissible, step-through, callouts
  anchored to UI targets published through the existing `scenario` handle/rect registry. Reads
  step progress and enabled/completed flags from the engine; drives every change through
  `ui.coaching.*` commands via `app.run`.

New/changed in `crates/engine/src/cmd`:

- `sketch.rs` (or a small `sketch_dimension_commit` sibling it `#[path]`-includes) — a
  `sketch.dimension.commit` command that validates and applies a typed dimension value.
- `ui.rs` (already registered in `cmd/mod.rs::command_specs`) — the `ui.coaching.*` commands that
  read and mutate `Coaching_Preference`.
- `view.rs` *(new small module, registered in `cmd/mod.rs::command_specs`)* — the
  `view.standard.{top,front,right,home,isometric}` and `view.fit` commands (see the view-navigation
  section). These sit alongside the existing named-view commands in `cmd/browser.rs`
  (`view.save`/`rename`/`delete`/`list`); nothing there is duplicated.

New/changed below L5 (no egui):

- `crates/engine/src/coaching.rs` (`CoachingPreference`, held on `Session` as `session.coaching`)
  — already holds the coaching preference as serializable application settings that persist across
  sessions. This is *not* the per-design `Document` (coaching progress is a program preference, not
  design data) and *not* view-only state: it is engine state the host persists and reloads.
- `crates/render/src/camera.rs` — `Camera`, `StandardView`, `set_view`, and the fit helpers are
  reused unchanged by the new `view.*` commands; this feature adds no camera logic below L1.

### Control-channel vs commands

`ui.*` methods on the control channel (e.g. `ui.start`, `ui.click`, `ui.screenshot`) are transport
verbs, not `CommandSpec`s. This feature's user-visible behaviour is implemented as real engine
`CommandSpec`s (`sketch.dimension.commit`, `ui.coaching.*`). The coaching surface and dimension
entry invoke them through `app.run(id, params)`. Programmatic `app.run` never opens a dialog.

## The command / data model

### New and clarified CommandSpec ids

All ids are SolidCraft's own dotted lower-case `<area>.<thing>[.<variant>]` ids, never another
product's command id.

1. `sketch.dimension.commit` — "Commit Dimension" (area `sketch`). Validates a typed value and
   applies it, either creating a new dimension on picked entities or changing an existing
   dimension's parameter. It wraps the existing `sketch.dimension` / `parameters.change` paths so
   that the UI stays thin and so the finite-number validation (R4.4) lives in one engine place.

   Params (one-line doc, EARS-friendly):
   `entities?: [refs] (new dimension) | param?: existing parameter name/constraint id (edit);
    value: number or expression (bare number in the design's units; "50 mm" to override);
    type?: auto|distance|horizontal|vertical|length|radius|diameter|angle; text_at?: [x,y].
    Rejects a non-finite or out-of-range value with an error and leaves the design unchanged.`

   The command is `undoable` and `in_sketch`-enabled; its `run` reuses the existing dimension
   engine logic (`cmd/sketch.rs::dimension`, `cmd/parameters.rs`) rather than duplicating it.

2. `ui.coaching.set` — "Set Coaching" (area `ui`, not on the toolbar). Enables or disables the
   coaching layer and persists the flag. Params:
   `enabled: bool. Stores the Coaching_Preference enabled flag. Returns {enabled, index, completed}.`

3. `ui.coaching.advance` — "Advance Coaching". Moves to the next step; past the last step marks the
   sequence complete. Params: `(none). Returns {enabled, index, completed}.`

4. `ui.coaching.dismiss` — "Dismiss Coaching Step". Hides the current step's callout without
   disabling the layer. Params: `(none). Returns {enabled, index, dismissed, completed}.`

5. `ui.coaching.state` — "Coaching State" (`.noundo()`, read-only). Returns the current
   `Coaching_Preference` for the UI to render. Params:
   `(none). Returns {enabled, index, completed, dismissed, steps: n}.`

These `ui.coaching.*` commands are `.noundo()` where they only change the preference (coaching
progress is not part of the design's undo history). They never open a dialog.

6. `view.standard.top`, `view.standard.front`, `view.standard.right`, `view.standard.home`,
   `view.standard.isometric` — "Top / Front / Right / Home / Isometric View" (area `view`,
   `.noundo()`, `.doc_only()`). Each applies the matching `StandardView` orientation to a camera.
   Params:
   `camera: {target, yaw, pitch, roll, distance, fov?, up?} (the current view; the design's own
    Camera JSON). Returns {camera} with the standard orientation applied; the target/distance are
    kept so the model stays framed.`
   The command resolves the name through `StandardView::parse` (L1 render) and calls
   `Camera::set_view`; `home` maps to the isometric orientation (R9.1/R9.4). Because the five ids
   are *distinct per orientation*, there is no free-text view-name parameter to mis-type; a request
   for an orientation SolidCraft does not define simply has no command id. The internal name parse
   still rejects anything unknown and returns the camera unchanged (R9.6).

7. `view.fit` — "Fit View" (area `view`, `.noundo()`, `.doc_only()`). Frames the visible model (or
   the current selection's bounds, when a selection is given) within the viewport. Params:
   `camera: {…} (the current view); aspect: number > 0 (viewport width ÷ height); selection?: bool
    (frame the selection instead of the whole model). Returns {camera, changed: bool}. With no
    visible geometry the bounds are empty, so it returns the camera unchanged and changed:false
    (R9.3 edge).`
   The command computes the model/selection bounds through the engine's existing `view::bounds`
   and fits the camera distance/target to those bounds for the given aspect. The L5 nav bar then
   applies the returned camera through `animate_to`, and the drawable-rect refinement in
   `viewport_layout::fit_bounds` nudges the framing so the model lands inside the free rectangle
   left by the floating overlays — a pure presentation adjustment that does not re-run the command.

   > Why a `camera` parameter rather than engine-owned camera state: the camera is view state the
   > UI owns (`app.cam`), not design data saved into `.solidcraft`. Passing it in and returning the
   > result keeps the command pure, keeps `app.run` dialog-free, and keeps the orientation/fit math
   > in one clean-room place (L1 render + the engine's `view::bounds`) without turning the camera
   > into document state. Orbit/pan/zoom remain continuous pointer gestures the UI applies to the
   > same camera (R9.5); only the discrete standard-view/fit/home actions go through a command,
   > exactly as R9.2 requires ("rather than by mutating camera state outside a Command").

All `view.*` and `ui.*` commands are `.noundo()` and `.doc_only()`: they touch neither the design
document nor the undo history, and they never open a dialog (programmatic `app.run` stays
dialog-free). None of these ids is, or reproduces, another product's command id.

### Coaching_Preference data model (engine, below L5)

```rust
/// Program-wide onboarding/coaching state. Persisted by the host across sessions; not part of
/// any design document and not held only in the view.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct CoachingPreference {
    /// The coaching layer is on.
    pub enabled: bool,
    /// Index of the current step (0-based); clamped to the step count.
    pub index: usize,
    /// The current step's callout is dismissed (hidden) but the layer stays enabled.
    pub dismissed: bool,
    /// The whole sequence has been completed at least once.
    pub completed: bool,
}

impl Default for CoachingPreference {
    fn default() -> Self {
        // First-run: enabled, at the first step, nothing dismissed or completed yet.
        CoachingPreference { enabled: true, index: 0, dismissed: false, completed: false }
    }
}
```

A pure reducer owns every transition (no egui, no I/O), so it is directly unit- and property-
testable:

```rust
/// How many steps the coaching sequence has (the UI's step table length, passed in so the
/// engine does not depend on L5 strings).
pub fn coaching_set(pref: &mut CoachingPreference, enabled: bool) {
    pref.enabled = enabled;
    pref.dismissed = false;
}

/// Advance to the next step; past the last step completes the sequence. `steps` is the table size.
pub fn coaching_advance(pref: &mut CoachingPreference, steps: usize) {
    pref.dismissed = false;
    let last = steps.saturating_sub(1);
    if pref.index >= last {
        pref.completed = true;
    } else {
        pref.index += 1;
    }
}

/// Hide the current step's callout without disabling the layer.
pub fn coaching_dismiss(pref: &mut CoachingPreference) {
    pref.dismissed = true;
}
```

The step *count* is passed from L5 (the step table lives with its strings in `coaching.rs`), so the
engine holds progress without owning UI copy. The `ui.coaching.advance`/`set`/`dismiss` command
`run`s call these reducers on the session's `CoachingPreference`, then return the new state as
JSON. Persistence: the engine exposes the preference as serializable settings; the host saves and
restores it the same way it already saves other settings. The UI mirror in `SolveApp` is refreshed
from the command's returned JSON — the engine remains the source of truth.

### Why not the Document or the design-configuration table

`crates/doc/src/config.rs` is the per-design *configuration table* (rows of parameter/suppression
variants) and `cmd/config.rs` drives it; neither is a settings store. The `Document` holds design
data that is saved into the `.solidcraft` file. Coaching progress is a program preference, not
design data, so it must not travel inside a design. It therefore lives as engine application
settings (below L5) persisted by the host, satisfying "through the engine, document, or
configuration layer rather than state held only in the view" without polluting designs.

## Tool-step model

The active sketch tool already lives in `app.tool: Option<tools::Tool>`. Today `Tool` tracks
`pts`, `picks`, `hover`, and inline-dimension boxes, and `tools::hint(id)` returns a hardcoded
English hint. The design derives a small, explicit step model from that existing state so feedback
(R1), the inline prompt (R2), and coaching anchors (R6) all read one source.

```rust
/// What the active tool is doing right now, derived from its command id and progress.
pub struct ToolStep {
    /// The tool's command id (its name is `tr_label(spec.label)`).
    pub cmd: &'static str,
    /// 0-based index of the current input step.
    pub index: usize,
    /// Localized catalog key describing the input this step expects (R1.2 / R2).
    pub prompt_key: &'static str,
    /// Localized key for how to confirm the current step (click / Enter) (R1.3).
    pub confirm_key: &'static str,
    /// Localized key for how to exit the tool (Esc / right-click) (R1.3).
    pub exit_key: &'static str,
}

impl Tool {
    /// The current step, derived from `kind`, `cmd`, and how many points/picks are down.
    pub fn step(&self) -> ToolStep { /* table lookup by (cmd, progress) */ }
}
```

Deriving the step:

- For `Kind::Draw(n)` tools the step index is `pts.len()` (and for the free-form chain tools it is
  "first point" vs "next point / finish"). `sketch_dims::boxes_for` already keys off `(cmd, n)`, so
  the step table reuses the same `(cmd, n)` shape and stays consistent with the dimension boxes.
- For `Kind::Pick(n)` / `Kind::HorizontalVertical` / `Kind::Dimension` the index is `picks.len()`.
- For `Kind::Ext` (the `sketch_tools.rs` tools) the step maps to that module's existing per-command
  hint, now behind a localized key.

The feedback model the UI draws:

```rust
/// Nothing to show when no tool is active (R1.5/R1.6: `None` ⇒ no feedback that frame).
pub fn tool_feedback(app: &SolveApp) -> Option<ToolFeedback> {
    let step = app.tool.as_ref()?.step();
    Some(ToolFeedback {
        name: app.tr_label(find_command(step.cmd)?.label).to_string(),
        expects: app.tr(step.prompt_key).to_string(),
        confirm: app.tr(step.confirm_key).to_string(),
        exit: app.tr(step.exit_key).to_string(),
    })
}
```

`viewport.rs` draws `ToolFeedback` as a compact chip beside the existing status overlay, using
`Tokens` colours. Because it is rebuilt from `app.tool` every frame, exiting the tool (which sets
`app.tool = None` in `tools::finish`) removes the feedback on the very next frame (R1.6). Advancing
a step changes `index` and hence `prompt_key`, so the expected-input text updates (R1.4 / R2.2).

The inline prompt (R2) is the same `step.prompt_key` surfaced through the status line on tool start
and on each new step; a successful completion clears it (the tool resets or ends), and an engine
error routes through the existing `app.run` error path, which calls `set_status(msg, true)` so the
status is marked as an error (R2.5).

## Snap and inference legibility

The snap/inference logic already produces the data; this feature makes it legible and localized.

- `drag_snap::Snap` carries `at`, `target`, and `guides`. `Snap::label()` returns a short glyph
  ("Pt", "Mid", "On", "" for Align). This feature adds a localized *type label* alongside the glyph
  (e.g. a catalog key per `Target` variant) so R3.2's label names the snap type in the user's
  language. The indicator is drawn at `snap.at` using `Tokens` colours (ring `tk.accent`, label box
  `tk.accent_soft`, text `tk.text`) — exactly the tokens already used in `drag_snap::show`.
- `inference::Inference` carries `kind`, `at`, and `guide`, with `Inference::label()` glyphs ("//",
  "⊥", "Ext", "T"). This feature adds a localized relationship name per `Kind` for R3.3's marker and
  draws the marker at `inference.at` with `Tokens` colours (dashed guide `tk.rubber_band`).
- Multiple applicable inferences (R3.4): the drag-snap `Align` case already builds up to two guides
  (nearest horizontal + nearest vertical) and publishes `drag_guides` count; each guide renders its
  own marker, so the marker count equals `guides.len()`.
- Leaving a target (R3.5): `drag_snap::find` / `inference::lines`/`tangent` call `keep(None)` when
  nothing is near, clearing `CURRENT`/`PENDING`; the next frame draws no indicator.
- All colours come from `Tokens` (R3.6) — no literal `Color32` in the snap/inference draw paths.

The snap/inference *finding* stays unchanged and clean-room (SolidCraft's own `inference.rs` and
`drag_snap.rs`); only the labelling and localization are added.

## Dimension entry ergonomics

Today `sketch_dims.rs` drives inline value boxes while drawing, and `dim_view.rs` drives the
dimension-as-dimension display plus an inline value editor for existing dimensions. This feature
keeps both surfaces and routes their confirm/cancel through the engine.

- Focus (R4.1): when an entry becomes active (`dims_focus` for drawing boxes; the `fresh` frames in
  `dim_view::edit_box`), the egui `TextEdit` requests focus and selects its whole value so typed
  characters replace it. This behaviour already exists; the design preserves it and asserts it.
- Confirm → command (R4.2): confirming runs `sketch.dimension.commit` through `app.run` with the
  typed `value`. For a new dimension the `entities`/`text_at` come from the dimension tool's picks;
  for an edit the `param` names the existing dimension. The resulting dimension measures the typed
  value, closing the round trip (type `v` ⇒ measured ≈ `v`).
- Edit prefill (R4.3): opening the editor on an existing dimension prepopulates `expr` from the
  parameter's stored expression (as `dim_view::double_click` / `edit_param` already do), so the box
  shows the current value.
- Finite validation (R4.4): the single validation gate lives in `sketch.dimension.commit` (engine).
  It evaluates the expression and rejects any value that is not finite, is `NaN`/`±inf`, exceeds the
  model's size limits, overly long (> 4096 chars), or otherwise invalid — returning
  `Result::Err(message)`; the UI shows that message via `set_status(msg, true)` and leaves the
  design unchanged. This mirrors the existing `sketch_dims::validate` rules but makes the engine the
  authority so hostile input (R4.4, "input-derived numbers are hostile") can never panic and is
  always a `Result`.
- Cancel (R4.5): Esc or a click away discards the draft without executing any command. The draft is
  evaluated only in a scratch session (`dim_view` already uses `session.scratch()`), so cancelling
  leaves `session.doc` byte-for-byte unchanged and the undo depth untouched.

## Empty-state and discovery cue

A new overlay, drawn from `sketch_display.rs` (or a small helper it calls), shows a discovery cue on
an empty sketch:

```rust
/// The cue is visible exactly when the active sketch has no geometry and no tool is running.
pub fn empty_state_visible(app: &SolveApp) -> bool {
    let Some(sid) = app.session.active_sketch else { return false };
    if app.tool.is_some() { return false; }                 // R5.3
    app.session.world_state().sketch(sid)
        .is_some_and(|ss| ss.sketch.curves.is_empty()
            && ss.sketch.points.iter().enumerate().all(|(i, _)| i == 0)) // only the origin point
}
```

- R5.1: visible while empty and no tool is running.
- R5.2: adding the first curve makes `curves` non-empty ⇒ hidden.
- R5.3: starting a tool sets `app.tool = Some(..)` ⇒ hidden for the tool's duration.
- R5.4: the cue text comes from a `Locale_Catalog` key (`sketch.empty_state_cue`), drawn with
  `Tokens` colours (`tk.text_dim` on `tk.overlay`).

The cue is a passive overlay with no interactive widgets, so it does not block the viewport.

## Command discoverability (palette and ribbon) — R7

`palette.rs` already matches commands by id/label (`matches`), opens a search popup on `S`
(`palette::popup`, gated by `app.ui.palette_open`), orders starts-with before contains with an
alphabetical tie-break, starts a no-param command through `app.start`, and reports no match on
submit. This feature closes the three remaining gaps — synonyms, effective-shortcut display, and a
localized label source — and tightens the ordering guarantee into a tested, deterministic function.

### Deterministic matching and ordering

The core becomes a pure, testable function so the order is a property, not an accident:

```rust
/// A palette match: the command plus why it matched, for a stable order.
pub struct Match {
    pub id: &'static str,
    pub label_key: &'static str,     // the CommandSpec label, shown via tr_label
    pub icon: &'static str,
    pub shortcut: Option<String>,    // the EFFECTIVE binding, not the CommandSpec default (R7.5)
}

/// Rank for ordering: 0 = id/label starts with the query, 1 = id/label contains it,
/// 2 = only a synonym matches. Lower sorts first; ties break alphabetically by localized label.
fn rank(q: &str, id: &str, label: &str, synonyms: &[&str]) -> Option<u8> { /* … */ }

/// Commands whose id, localized label, or a Locale_Catalog synonym contains `q`
/// (case-insensitive), ordered deterministically.
pub fn matches(app: &SolveApp, q: &str) -> Vec<Match> { /* command_specs → rank → sort */ }
```

- **Query matching (R7.2):** a command matches when its `id`, its *localized* label
  (`tr_label(spec.label)`), or any synonym contains the lower-cased query. Matching the localized
  label — not the English literal — means a German user who types a German word still finds the
  command.
- **Ordering (R7.3):** sort by `(rank, localized_label)`. Starts-with (rank 0) sorts before
  contains (rank 1) before synonym-only (rank 2); within a rank the localized label breaks ties
  alphabetically. This is total and deterministic for any input, which Property 15 asserts.
- **Synonyms (R7.4, R7.8):** a command's synonyms come from the `Locale_Catalog` under a key
  derived from the command id, `palette.synonyms.<id>` — a single catalog string of
  comma-separated terms per locale (e.g. `en: "push, pull, boss"` for `solid.extrude`). Only
  commands that benefit get a synonym key; absent keys mean no synonyms. Because synonyms live in
  the catalog, they are localized for `en`/`ja`/`de`/`ko` like every other string, and no synonym
  is ever an exact copy of another product's command label (clean-room; the terms are ordinary
  CAD/English words chosen independently).
- **Shortcut on the entry (R7.5):** each `Match.shortcut` is `keymap::effective(app, id)` — the
  binding actually in force (default from `CommandSpec` or the user's override), so the palette and
  the ribbon show the *same* key the user would press. The ribbon (`toolbar.rs`,
  `workspace.rs`) switches its `shortcut_text`/drop-down key column from `spec.shortcut` to
  `keymap::effective` for the same reason.

### Popup focus, activation, and no-match

- **Focus on open (R7.1):** `popup` already calls `r.request_focus()` on its `TextEdit` every
  frame while `palette_open`, so the input is focused for typing the moment `S` opens it.
- **Activation (R7.6):** picking an entry (click or Enter on the top match) calls `app.start(id)`;
  `app.start` runs a no-param command directly and opens the interactive tool/dialog for one that
  needs input — matching the toolbar-click path. Programmatic `app.run` is never used here, so the
  palette never bypasses the dialog for a command that needs one.
- **No match (R7.7):** `submit` already sets an error status (`set_status(…, true)`) and runs
  nothing when the text matches no command. For the popup, a no-match submit additionally **leaves
  the popup open** (it only closes on a pick or Escape) so the person can correct the query; no
  command runs, so the design is unchanged.
- **S-key guard:** the global `S` handler in `toolbar::shortcuts` only opens the palette when no
  text field has focus (it already early-returns while a dimension entry, a dialog field, or the
  keymap capture is focused), so typing `s` into a field never pops the search open.

Labels and synonyms are drawn through `tr_label`/`tr`; icons through `icons.rs`; the popup and bar
colours through `Tokens`. Nothing in this surface mutates the design except by starting a command.

## Dialog ergonomics (shared frame) — R8

The `dialogs*.rs` family and `params_dialog.rs` each build their own window today. To make every
feature dialog behave the same way, this feature introduces one shared helper the whole family
calls, so the layout and the confirm/cancel contract live in one place rather than being
re-implemented per dialog.

```rust
/// The common chrome every feature dialog wears. The body scrolls; the footer does not.
pub struct DialogFrame<'a> {
    pub title_key: &'a str,          // localized title (R8.7)
    pub preview: Option<Value>,      // (command id, params) to preview while valid (R8.3)
    pub error: Option<FieldError>,   // { field_key, message } to show, keeping the dialog open (R8.6)
}

pub struct FieldError { pub field_key: &'static str, pub message: String }

/// Outcome of one frame of a dialog.
pub enum DialogOutcome { Open, Confirm, Cancel }

impl DialogFrame<'_> {
    /// Draw title, a scrolling body (via `body`), then a fixed footer with Cancel and OK
    /// in a stable order outside the scroll area. Returns what the user did this frame.
    pub fn show(self, app: &mut SolveApp, ctx: &egui::Context,
                body: impl FnMut(&mut SolveApp, &mut egui::Ui)) -> DialogOutcome { /* … */ }
}
```

- **Reachable confirm/cancel (R8.1):** `show` lays the window out as `TopBottomPanel`-style
  regions — a bottom footer holding Cancel then OK, allocated *before* the central
  `ScrollArea::vertical` that holds the fields. The footer therefore never scrolls off, at any
  window height, and the two actions keep the same position and order in every dialog.
- **Units (R8.2):** dimensioned fields render through a shared `dimension_field(label_key, unit)`
  that always draws the unit (`mm`, `rad`, …) beside the value, reusing the `UNITS`/`unit_label`
  vocabulary already in `params_dialog.rs`. A unit-less field shows none.
- **Live preview (R8.3):** while the current parameters are valid, `show` runs the command's
  preview the same way `params_dialog` already does — through the engine on a scratch session and
  the existing `preview` pipeline — so the viewport shows the provisional result before commit. A
  frame budget keeps the preview responsive (well under the 200 ms the requirement implies); when
  any field is invalid, `preview` is `None` and no preview is shown (so an invalid state never
  flashes a stale or wrong result).
- **Confirm / cancel (R8.4/R8.5):** `Confirm` makes the caller run the real command through
  `app.run(id, params)` (one undo step); `Cancel` drops the dialog's draft. Because the preview and
  every validation run on a scratch session, cancelling leaves `session.doc` and the undo depth
  byte-for-byte unchanged.
- **Per-field errors (R8.6):** when `app.run` (or a live validation) rejects a value, the dialog
  stores a `FieldError` naming the offending field and keeps itself open; `show` draws the message
  next to that field in `Tokens` error colour. The engine command remains the single validation
  authority (the UI does not re-derive the rule), so the message is the engine's own.
- **Theme + i18n (R8.7):** every colour from `Tokens`, every title/label/message/unit through
  `tr`/`tr_label`.

Migration is incremental: `dialog_frame` is added first with `params_dialog.rs` as the reference
adopter (it already has units, a live preview, and per-field errors), then each `dialogs_*.rs`
dialog is moved onto it. The helper's contract — footer outside the scroll, stable order, preview
only while valid, open-on-error — is what the correctness properties below assert, independent of
any one dialog.

## View navigation (standard views, orbit/pan/zoom, fit, home) — R9

Today `viewport::nav_bar` and `axis_view` apply the camera directly: Fit/Home call
`app.animate_view("fit"|"home")`, an axis tip calls `app.animate_to(cam.looking_from(dir))`, and
`animate_view` resolves names through `StandardView::parse` and `Camera::set_view`. The behaviour
is right; the gap is that the discrete view changes mutate the camera *outside a command* (R9.2).
This feature routes those discrete actions through the new `view.standard.*` / `view.fit` commands
while leaving continuous pointer navigation (orbit/pan/zoom) as the direct gestures they must be.

### Applying a view through a command

```rust
/// Apply a standard view by command, then animate the camera to the result (R9.2/R9.4).
fn apply_standard(app: &mut SolveApp, which: &str) {
    let cam = serde_json::to_value(app.cam).unwrap_or_default();
    if let Ok(out) = app.run(&format!("view.standard.{which}"), json!({ "camera": cam }))
        && let Some(to) = out.get("camera").and_then(|c| serde_json::from_value(c.clone()).ok())
    {
        app.animate_to(to);     // existing easing / immediate-vs-animated preference
    }
}

/// Fit the visible model (or selection) by command (R9.3).
fn apply_fit(app: &mut SolveApp, selection: bool) {
    let aspect = app.viewport.rect.map(|r| f64::from(r.width()) / f64::from(r.height().max(1.0))).unwrap_or(1.5);
    let cam = serde_json::to_value(app.cam).unwrap_or_default();
    if let Ok(out) = app.run("view.fit", json!({ "camera": cam, "aspect": aspect, "selection": selection }))
        && out.get("changed").and_then(Value::as_bool) == Some(true)
        && let Some(to) = out.get("camera").and_then(|c| serde_json::from_value(c.clone()).ok())
    {
        app.pending_fit = /* bounds for the drawable-rect refinement */ None;
        app.animate_to(to);     // viewport_layout::fit_bounds then nudges into the free rect
    }
}
```

- **Standard-view controls (R9.1):** the nav bar's `home` button and the axis-view tips/home
  corner map to `view.standard.{top,front,right,home,isometric}`; the axis gizmo's reversible
  "click a tip to look along it" still orbits to a direction, but each *named* standard view is one
  command.
- **Fit (R9.3):** the nav bar's `fit` button (F6) calls `view.fit`. With no visible geometry the
  command returns `changed:false` and the UI shows a brief localized "nothing to fit" status and
  does not move the camera.
- **Home (R9.4):** the return-to-home control calls `view.standard.home`.
- **Orbit / pan / zoom (R9.5):** these remain continuous drags/wheel the UI applies to `app.cam`
  each frame (`Camera::orbit`/`pan`/zoom). They are gestures, not discrete named states, so R9.2's
  "rather than by mutating camera state outside a Command" does not force a per-frame command — the
  requirement scopes commands to the *named* standard views and fit/home, which is exactly what the
  five `view.standard.*` ids plus `view.fit` cover.
- **Unknown view name (R9.6):** there is no free-text view-name entry point in the UI — the five
  orientations are five fixed command ids — so a user cannot ask for an undefined standard view. If
  a programmatic caller passes an unknown name to the parse inside a `view.*` command, the parse
  returns `None` and the command returns the camera unchanged, so the camera never moves on a bad
  name.
- **Theme + i18n (R9.7):** the nav bar and axis view draw every colour from `Tokens` (axis colours
  from the engine's `colors`, highlights/overlays from `tk.accent`/`tk.overlay`) and source every
  tooltip/label through `tr`.

The camera stays UI view state; the commands make the *decision* of where it should go and keep
that decision clean-room and testable in L1/L4, while the UI owns only the easing animation.

## Components and Interfaces

This section indexes the components and interfaces this feature touches; the full descriptions live
in the sections referenced below, which this heading groups rather than restates.

- **Where each piece lives** (Architecture › *Where each piece lives*): the changed and new modules
  across `crates/ui-egui/src` (L5), `crates/engine/src/cmd` (L4), and below-L5 engine/render code,
  and which Requirement (R1–R13) each refines.
- **Control-channel vs commands** (Architecture › *Control-channel vs commands*): the boundary
  between transport verbs (`ui.start`/`ui.click`/`ui.screenshot`) and the real `CommandSpec`s this
  feature adds.
- **CommandSpec interfaces** (The command / data model › *New and clarified CommandSpec ids*): the
  SolidCraft-own dotted ids, their params docs, and `enabled`/undo semantics for
  `sketch.dimension.commit`, `ui.coaching.*`, `view.standard.{top,front,right,home,isometric}`, and
  `view.fit`.
- **Per-surface UI components (R1–R13):** the active-tool feedback and inline prompt (*Tool-step
  model*), snap/inference legibility (*Snap and inference legibility*), dimension entry (*Dimension
  entry ergonomics*), the empty-state cue (*Empty-state and discovery cue*), the palette and ribbon
  (*Command discoverability — R7*), the shared dialog frame interface (*Dialog ergonomics — R8*),
  the viewport nav bar / axis view (*View navigation — R9*), the keymap page (*Keyboard-first command
  flow — R10*), the timeline (*Timeline legibility — R11*), the browser (*Browser legibility — R12*),
  and the coaching surface and app-level onboarding (*Coaching layer design*, *App-level onboarding
  (R13)*).
- **Cross-cutting interfaces:** localization keys (*i18n*) and `Tokens` fields (*Theming*) each UI
  component draws through, plus the error-handling contract (*Error handling*) every added fallible
  operation obeys.

## Data Models

This section indexes the data models (data structures and their shapes) this feature defines or
consumes; each full definition lives in the referenced section and is not restated here.

- **`CoachingPreference`** (The command / data model › *Coaching_Preference data model (engine,
  below L5)*): the persisted `{enabled, index, dismissed, completed}` program-preference struct and
  its pure reducers (`coaching_set`/`coaching_advance`/`coaching_dismiss`).
- **`ToolStep` and `ToolFeedback`** (*Tool-step model*): the active sketch tool's derived step model
  (`cmd`, `index`, `prompt_key`, `confirm_key`, `exit_key`) and the per-frame feedback model
  (`name`, `expects`, `confirm`, `exit`) the viewport draws.
- **`Match`** (Command discoverability — R7 › *Deterministic matching and ordering*): a palette
  match row (`id`, `label_key`, `icon`, effective `shortcut`) and its `rank` ordering.
- **`DialogFrame` / `FieldError` / `DialogOutcome`** (Dialog ergonomics — R8): the shared dialog
  chrome model (title, optional preview, optional per-field error) and its per-frame outcome.
- **`CoachingStep` and the `STEPS` table** (Coaching layer design › *Step definition data*): the
  localized coaching step (`text_key`, optional `anchor`) and the ordered app-level step sequence,
  plus the `CoachingView` render model.
- **Camera parameter shape** (View navigation — R9): the `{target, yaw, pitch, roll, distance,
  fov?, up?}` camera JSON passed to and returned from `view.standard.*` / `view.fit`, reusing
  `solidcraft_render::Camera` / `StandardView` unchanged.

## Timeline legibility — R11

`timeline.rs` already lays features out left-to-right, re-opens a feature on double-click, lights
the hovered feature in the viewport, and distinguishes error and suppressed features. This feature
makes it read for an ordinary person: localized text, token-sourced state colours, and a short
hover dwell so the highlight does not flicker.

- **Creation order (R11.1):** the strip iterates `session.doc.features` in order; features are
  stored in creation order, so the drawn order is the creation order. (Reordering is a separate
  explicit `timeline.reorder` command the user drives; the display always follows the stored
  order.)
- **Re-open to edit (R11.2):** `resp.double_clicked()` already sets `edit = Some(id)` and
  `app.edit_feature(id)` re-opens the feature through its own editing command/dialog. This feature
  keeps that and routes it through the command path (no camera/doc mutation outside the command).
- **Hover highlight with dwell (R11.3):** today `app.viewport.hover_feature = hovered` highlights
  immediately. This feature adds a ~150 ms dwell: the hovered id is only promoted to
  `hover_feature` once the pointer has rested on the same feature button for the dwell, and
  `hover_feature` is cleared the frame the pointer leaves the strip. A small
  `HoverDwell { id, since }` on the viewport state (pure, testable: given a hover-id stream and
  timestamps, it returns the promoted id) owns the timing, so a drive-by sweep across features does
  not strobe the viewport.
- **Error state (R11.4):** a feature whose `model.result(id).error` is `Some` draws with
  `tk.timeline_item_error` fill and `tk.error` edge — both `Tokens` fields.
- **Suppressed state (R11.5):** a suppressed feature keeps the ordinary fill but draws a strike
  line in `tk.text_dim` and dims its icon to `tk.border`, which is visibly distinct from both the
  ordinary and the error presentation. (Error ≠ suppressed ≠ ordinary are three distinguishable
  looks.)
- **Localized text (R11.6):** the feature's display name goes through `tr_dyn` (user names pass
  through, known keys translate) and the status words in the tooltip ("Error", "Warning",
  "Double-click to edit…") move behind `tr` catalog keys so the whole tooltip is localized.

## Browser legibility — R12

`browser.rs` already draws the component → body → sketch hierarchy, toggles visibility through
`browser.visibility`, shows and sets the active component through a radio that runs
`component.activate`, and localizes its control tooltips through `tr`. This feature localizes the
remaining node text, draws every colour from `Tokens`, and adds the three-way sketch
constraint-state indicator.

- **Hierarchy (R12.1):** unchanged — components hold their bodies, sketches, and construction
  geometry as `Browser_Node` rows.
- **Visibility toggle (R12.2):** the eye control already runs `browser.visibility` (view state, not
  an undo step). The row's eye glyph reflects the item's current visibility, so toggling and the
  indicator stay in sync; if the command returns an error the row keeps its previous indicator
  state (the UI reads visibility from the session, which the failed command did not change).
- **Sketch constraint state (R12.3):** a sketch row shows one of three indicators driven by the
  sketch's solver report (`report.curve_determined` / degrees-of-freedom already computed in the
  engine), coloured from `Tokens`:
  - *under-defined* → `tk.sketch` (the "still movable" colour), e.g. a small open-ring glyph;
  - *fully-defined* → `tk.sketch_fixed`, a filled glyph;
  - *locked/fixed* → `tk.sketch_locked`, a padlock badge (the existing `badge("lock", …)`).
  A pure `sketch_constraint_state(report) -> {UnderDefined, FullyDefined, Locked}` classifier makes
  the mapping testable and keeps the three looks distinct.
- **Active component (R12.4/R12.5):** exactly one component row shows the filled activation radio
  (`row.radio == Some(active)`), where `active == (session.active_component == id)`; clicking an
  inactive radio runs `component.activate`. If activation fails the indicator stays on the
  previously active component (again, the UI reads `active_component` from the session, which the
  failed command left unchanged).
- **Theme + i18n (R12.6):** node labels go through `tr_dyn`/`tr_label`, state/indicator tooltips
  through `tr` (the existing `ui.show`/`ui.hide`/`ui.activate`/`ui.active_component` keys plus new
  `browser.sketch.*_defined` keys), and every colour comes from `Tokens`.

## Keyboard-first command flow — R10

`keymap.rs` already implements the whole of Requirement 10; this feature keeps its behaviour and
only localizes its page strings. The design records the contract so the properties can assert it:

- **Defaults from `CommandSpec` (R10.1):** `effective(app, id)` returns the user's override if any,
  else `spec.shortcut`. No binding is invented in the UI.
- **Press runs the bound command (R10.2):** `command_for(app, key)` returns the enabled command a
  key runs; `toolbar::shortcuts` dispatches it with `app.start(id)`.
- **Rebind / clear / reset (R10.3/R10.4/R10.7):** `bind(app, id, key, replace)` stores an override
  (and removes it when the key equals the `CommandSpec` default, so "reset to default" is just
  dropping the override); `bind(…, "")` clears; `reset(app, None)` restores every default.
- **Conflict (R10.5):** binding a key another command uses returns an "already used by …" error
  unless `replace`; the page surfaces the conflict and a "Use … here anyway" confirm, and only on
  confirm does `bind(…, replace=true)` take the key (clearing it from the others).
- **Reserved keys (R10.6):** `RESERVED` (Escape, S, V, Delete, Ctrl+Z, …) are refused by `bind`
  and stay with the shell.
- **Persistence (R10, NFR 8):** `Keymap::prefs()`/`load()` serialize the override map; the host
  saves and reloads it through the settings layer, so bindings survive a restart — not view-only
  state. Changing it is not a design/undo action.
- **Search (R10.8):** the page's filter matches a command's label, id, or current key, so a
  shortcut is found without scrolling.
- **i18n:** the page's own strings ("Filter commands or keys", "Reset All", "press a key…", the
  conflict buttons) move behind `tr` catalog keys for `en`/`ja`/`de`/`ko`.

## Coaching layer design (new surface: `coaching.rs`)

### Step definition data

Steps live in L5 with their strings; only progress lives in the engine.

```rust
/// One coaching step: localized text and an optional callout anchored to a published UI handle.
struct CoachingStep {
    /// Catalog key for the step's body text (R6.9).
    text_key: &'static str,
    /// A `scenario`/handle target id to anchor the callout to, e.g. "toolbar:sketch.line"
    /// or "sketch_palette:finish". `None` ⇒ a centered, un-anchored step.
    anchor: Option<&'static str>,
}

const STEPS: &[CoachingStep] = &[
    // App-level onboarding first (R13): where you are, how to move between workspaces.
    CoachingStep { text_key: "coaching.step_workspace_tabs", anchor: Some("toolbar:tab.SOLID") },  // R13.2
    CoachingStep { text_key: "coaching.step_create_sketch",  anchor: Some("toolbar:sketch.create") },
    CoachingStep { text_key: "coaching.step_draw_line",      anchor: Some("toolbar:sketch.line") },
    CoachingStep { text_key: "coaching.step_close_profile",  anchor: None },
    CoachingStep { text_key: "coaching.step_dimension",      anchor: Some("toolbar:sketch.dimension") },
    CoachingStep { text_key: "coaching.step_finish",         anchor: Some("sketch_palette:finish") },
    CoachingStep { text_key: "coaching.step_extrude",        anchor: Some("toolbar:solid.extrude") },  // R13.3 (non-sketch)
    // ... a short, ordered introduction to the whole modelling workflow.
];
```

The step table now covers the **whole app**, not the sketch alone (R13): at least one step
introduces the ribbon's `Workspace_Tab` navigation (`coaching.step_workspace_tabs`, anchored to a
workspace tab handle) and at least one step covers the modelling workflow beyond the sketch
surface (`coaching.step_extrude`, a non-sketch `solid.*` command). The sketch-workflow steps from
Requirement 6 remain in the sequence; Requirement 13 widens the introduction around them rather
than replacing it.

Anchoring uses the existing `scenario::publish_handle` / `publish_rect` registry (the same handles
the control channel and tests already read via `ui.handles`). The workspace-tab step anchors to a
`toolbar:tab.<TAB>` handle that `toolbar.rs` publishes for each ribbon tab; when a target is not on
screen this frame, the step falls back to a centered callout.

### Non-blocking and reachable (R6.4)

The callout is drawn as a foreground `egui::Area` sized to its own content, positioned beside its
anchor, and it does *not* cover the viewport or the palette's interactive regions. It installs no
full-screen input-capturing layer. Sketch clicks, palette toggles, and tool interactions reach
their widgets exactly as before; only the small callout rectangle (with its Next/Dismiss controls)
consumes input, and it is placed clear of the anchored control so the control stays clickable. A UI
harness test clicks a sketch point while coaching is shown and asserts the tool still received it.

### Dismiss vs disable semantics

- Dismiss (R6.3): `ui.coaching.dismiss` sets `dismissed = true`. The current step's callout hides
  but `enabled` stays `true`; the next step (on the next `advance`) is presentable again.
- Disable (R6.5): `ui.coaching.set { enabled: false }` turns the whole layer off and persists it.
- Re-enable (R6.6): `ui.coaching.set { enabled: true }`.
- Startup (R6.7): the layer's visibility derives from the persisted `enabled` flag; a persisted
  `disabled` keeps it hidden until re-enabled.
- Completion (R6.8): advancing past the last step (`ui.coaching.advance` at `index == len-1`) sets
  `completed = true` and persists it.

### Rendering model

```rust
/// What to draw this frame; `None` ⇒ nothing (disabled, dismissed, or completed-and-hidden).
pub fn coaching_view(app: &SolveApp) -> Option<CoachingView> {
    let p = &app.coaching;              // UI mirror, refreshed from the engine
    if !p.enabled || p.dismissed { return None; }   // R6.3 / R6.7
    let step = STEPS.get(p.index.min(STEPS.len().saturating_sub(1)))?;
    Some(CoachingView {
        text: app.tr(step.text_key).to_string(),     // R6.9
        anchor: step.anchor.and_then(crate::scenario::handle_at), // R6.1
        index: p.index,
        last: p.index + 1 >= STEPS.len(),
    })
}
```

The surface draws the callout with `Tokens` colours (R6.10): panel `tk.dialog_bg`, border
`tk.dialog_border`, text `tk.text`, accent/Next button `tk.accent`, a subtle connector to the
anchor in `tk.accent_soft`. The Next/Dismiss/Close glyphs are drawn in code via `icons.rs`
(R7 of the NFRs). Clicking Next calls `app.run("ui.coaching.advance", json!({}))`; Dismiss calls
`ui.coaching.dismiss`; the enable/disable toggle (surfaced in Help or Preferences) calls
`ui.coaching.set`. After each call the UI refreshes its mirror from the returned JSON.

### App-level onboarding (R13)

Requirement 13 extends the same coaching surface to the whole app. Three additions build on the
existing layer without new persisted state:

- **New-document empty-state cue (R13.1/R13.4):** when the current design is in the
  `New_Document_Empty_State` — no features — the layer shows a short, centered empty-state cue
  (`coaching.empty_state_cue`) describing how to begin a design. The cue's visibility is a pure
  predicate over session state, mirroring the sketch empty-state cue:

  ```rust
  /// The new-document cue shows while the design has no features (R13.1) and hides once the
  /// first feature exists (R13.4). Independent of the enabled/dismissed coaching flags: it is a
  /// discovery hint, not a coaching step.
  pub fn new_document_cue_visible(app: &SolveApp) -> bool {
      app.session.doc.features.is_empty()
  }
  ```

  Adding the first feature makes `features` non-empty, so the cue hides on the next frame (R13.4).

- **First-run vs returning (reuses the `completed` flag):** `CoachingPreference` already records
  `completed`. A first-time person (default: `enabled`, `completed == false`) sees the full step
  sequence; a returning person who has finished it once (`completed == true`) still gets the
  empty-state cue on a brand-new document but is not re-walked through every step unless they
  re-enable/restart coaching. No new field is needed — first-run vs returning is derived from the
  persisted `completed` flag, which keeps the engine the source of truth and the view a mirror.

- **Non-blocking and reachable (R13.5):** the app-level callouts and the empty-state cue use the
  same non-full-screen `egui::Area`/overlay approach as Requirement 6: only the small callout
  rectangle consumes input; the ribbon tabs, viewport, timeline, browser, and palette stay fully
  reachable. A harness test clicks a ribbon tab and a toolbar command while onboarding is shown and
  asserts both still fire.

- **Disable + persist (R13.6):** disabling app-level onboarding is the same `ui.coaching.set
  { enabled: false }` command, persisted through the engine's settings layer; the empty-state cue
  is suppressed while coaching is disabled.

- **Theme + i18n (R13.7):** every app-level string (`coaching.step_workspace_tabs`,
  `coaching.step_extrude`, `coaching.empty_state_cue`, and the control labels) is a
  `Locale_Catalog` key translated for `en`/`ja`/`de`/`ko`; every colour comes from `Tokens`.

## i18n

New `Locale_Catalog` keys follow the existing dotted-id pattern in `i18n.rs` / `locales/*.json`
(stable id → English → id fallback). Keys are added to `en.json` and translated in `ja.json`,
`de.json`, `ko.json`:

- Tool steps / prompts: one `sketch.prompt.<cmd>.<step>` key per `(command, step)` the step table
  references (these replace the hardcoded English hints in `tools.rs` / `sketch_tools.rs`), plus
  shared `sketch.confirm.click`, `sketch.confirm.enter`, `sketch.exit.esc` for the confirm/exit
  affordances.
- Snap/inference type labels: `sketch.snap.point|mid|on`, `sketch.infer.parallel|perpendicular|
  extension|tangent|horizontal|vertical`.
- Empty-state cue: `sketch.empty_state_cue`.
- Coaching: `coaching.step_*` per step (including the app-level `coaching.step_workspace_tabs` and
  `coaching.step_extrude`), the new-document `coaching.empty_state_cue`, plus control labels
  `coaching.next`, `coaching.close`, `coaching.dismiss`, `coaching.enable`, `coaching.disable`.

New whole-shell keys (R7–R12):

- Palette synonyms: `palette.synonyms.<command id>` — one comma-separated string per locale, added
  only for commands that benefit (e.g. `palette.synonyms.solid.extrude`). The matcher splits on
  commas and trims. Clean-room: the terms are ordinary CAD/English words, never another product's
  command label.
- Palette status: `palette.no_match` (the explanatory no-match message).
- Dialog frame: `dialog.ok`, `dialog.cancel`, and per-field label/error keys owned by each dialog
  (the shared frame only provides the chrome strings).
- View navigation: `view.standard.top|front|right|home|isometric` labels, `view.fit` label,
  `view.nothing_to_fit` status, and nav/axis tooltips `view.nav.orbit|pan|zoom|fit|home|lookat`.
- Timeline: `timeline.status.error`, `timeline.status.warning`, `timeline.status.suppressed`, and
  the `timeline.tip_*` tooltip fragments ("Double-click to edit · drag to reorder · …").
- Browser: `browser.sketch.under_defined`, `browser.sketch.fully_defined`, `browser.sketch.locked`
  (the three constraint-state indicator tooltips), reusing the existing `ui.show`/`ui.hide`/
  `ui.activate`/`ui.active_component` keys for the controls.
- Keymap page: `keymap.filter_hint`, `keymap.reset_all`, `keymap.press_a_key`,
  `keymap.use_anyway`, `keymap.cancel`, `keymap.conflict` (the page's own chrome).

Every string the UI shows for this feature is fetched through `app.tr(id)` / `app.tr_label(label)`
(which route to `Locale`), never a hardcoded literal in the draw path. The existing i18n test that
checks placeholders and English-catalog membership is extended to the new direct-id call sites and
to the per-command synonym keys (each must resolve in `en` and be present/translatable in `ja`,
`de`, `ko`).

## Theming

All new drawing reuses existing `Tokens` fields where they fit:

- Tool feedback chip and inline prompt: `tk.overlay` background, `tk.text` / `tk.text_dim`,
  `tk.accent` for the confirm/exit affordance highlight (as the existing status chip does).
- Snap indicator / inference marker: `tk.accent`, `tk.accent_soft`, `tk.rubber_band` (already used).
- Empty-state cue (sketch and new-document): `tk.text_dim` on `tk.overlay`.
- Coaching callout: `tk.dialog_bg`, `tk.dialog_border`, `tk.text`, `tk.accent`, `tk.accent_soft`.
- Palette popup/bar: `tk.panel`, `tk.text` / `tk.text_dim`, `tk.accent`; the shortcut on an entry
  in `tk.text_dim`, icons in `tk.icon`/`tk.icon_fill`/`tk.accent`.
- Shared dialog frame: `tk.dialog_bg`/`tk.dialog_border` chrome, `tk.text` labels, `tk.accent`
  OK/preview affordance, `tk.error` for a per-field error message.
- Viewport nav bar + axis view: `tk.overlay`/`tk.border` for the bar, `tk.accent`/`tk.accent_soft`
  for active/hover, `tk.icon`/`tk.icon_fill` for glyphs; axis tip colours from the engine's
  `colors::AXIS_*` (clean-room engine palette), never literal `Color32`.
- Timeline state: `tk.timeline_item`, `tk.timeline_item_error`, `tk.error`, `tk.accent_soft`,
  `tk.text_dim` (suppressed strike/icon dim), `tk.border` — all existing fields.
- Browser constraint state: `tk.sketch` (under-defined), `tk.sketch_fixed` (fully-defined),
  `tk.sketch_locked` (locked); active radio `tk.accent`.

A new token is introduced only if review finds no existing field fits (candidate: a dedicated
`coaching_callout`/`coaching_connector` pair if `dialog_*` + `accent_soft` prove visually
insufficient against the viewport). Any new field is added to both `Tokens::light()` and
`Tokens::dark()` and covered by the existing theme contrast test style. No literal colours appear in
this feature's draw paths.

## Error handling

- Every fallible operation added by this feature returns `Result<T, E>` and propagates with `?`;
  no `unwrap`/`expect`/`panic!`/`unreachable!`/`todo!`/`unimplemented!`/`unsafe` in non-test code
  (the production crate roots already `#![deny(...)]` these).
- `sketch.dimension.commit` treats its `value` as hostile input: it reads with `get()`-style access,
  checks finiteness and size caps, bounds the expression length, and returns a descriptive `Err`
  for anything invalid (R4.4). The engine never panics on bad input; the UI surfaces the error via
  `set_status(_, true)` and leaves the design unchanged.
- `ui.coaching.*` commands validate their params (`enabled` must be a bool) and clamp `index` into
  range; they return `Err` rather than panicking on malformed control input, and they never touch
  the design document.
- The coaching reducer uses saturating/`min` arithmetic so an out-of-range persisted `index` cannot
  index out of bounds.
- Reading a missing or malformed persisted `CoachingPreference` falls back to `Default` (first-run)
  rather than failing to start.
- `view.standard.*` / `view.fit` treat the incoming `camera` and `aspect` as hostile input: a
  malformed or non-finite camera, a non-positive/`NaN` aspect, or a camera that fails `is_valid`
  returns a descriptive `Err` and the UI keeps the current camera; the orientation parse rejects an
  unknown name and returns the camera unchanged (R9.6). The fit math already guards empty/non-finite
  bounds and returns `None`/`changed:false` rather than producing a `NaN` camera.
- The palette matcher, timeline hover-dwell classifier, browser constraint-state classifier, and
  dialog-frame validation are all total pure functions over their inputs: they return a definite
  result (an ordered list, a promoted id, a state, an outcome) for every input and never panic. The
  palette splits synonym strings and reads catalog entries with `get()`-style access and bounded
  lengths.
- The shared dialog frame runs its live preview and every field validation on a scratch session, so
  a preview that errors or a rejected field never mutates `session.doc` or the undo history; the
  engine command remains the single validation authority and returns a `Result`.
- `keymap::bind`/`reset`/`load` are fallible-safe: `bind` returns `Err` for reserved keys and
  unconfirmed conflicts; `load` reads the override map with bounded iteration and skips malformed
  entries. None of the keymap paths panic on bad persisted data.

## Testing strategy

Tests follow the existing ui-egui style: pure helpers get unit/property tests; interactive flows use
the `scenario::Harness` (`*_ui_tests.rs`) that drives `ui.start` / `ui.click` / `ui.text` /
`ui.key` and inspects published handles, the accesskit tree, and session state. Everything must
build for `wasm32-unknown-unknown` and pass `cargo xtask ci` (fmt, clippy -D warnings, tests,
assets, layers, wasm).

Dual approach — property tests for universal behaviour, example/UI tests for specific flows and
layout:

- Tool feedback & steps: property tests over the sketch tool-command registry assert every tool/step
  yields a localized name, expected-input, confirm, and exit (R1.1–R1.3), and that advancing a step
  changes the expected-input where steps differ (R1.4). UI examples assert the chip appears while a
  tool runs and is gone one frame after exit (R1.5/R1.6), using a new `coaching`/feedback test file
  plus assertions in the existing sketch UI tests.
- Inline prompt i18n: a catalog property asserts every prompt/confirm/exit/snap/inference/coaching
  key the models reference resolves in `en` and translates in `ja`/`de`/`ko` (R2.3, R3.2, R5.4,
  R6.9), extending `i18n_ui_tests.rs` / the `i18n.rs` catalog tests.
- Snap/inference legibility: property tests over generated sketches assert a snap/inference marker is
  produced at `snap.at` / `inference.at` when `find`/`current` is `Some` (R3.1/R3.3), that the
  guide-marker count equals `guides.len()` (R3.4), and example/unit tests that moving away clears the
  indicator (R3.5) and that the draw colours equal the chosen `Tokens` fields (R3.6).
- Dimension entry: property tests assert the typed-value round trip (confirm `v` ⇒ measured ≈ `v`,
  R4.2) and the prefill round trip (editor opens with the stored expression, R4.3), building on the
  engine's existing `sketch.dimension` round-trip tests. A hostile-input property feeds NaN/±inf/
  overflow/empty/overlong/non-numeric expressions and asserts `sketch.dimension.commit` returns
  `Err`, sets an error status, never panics, and leaves the design unchanged (R4.4). A property
  asserts cancel leaves `session.doc` and undo depth unchanged (R4.5). UI examples cover focus (R4.1)
  and the error-status flag (R2.5).
- Empty-state cue: a property over generated sketch states asserts
  `empty_state_visible == (is_empty && no_tool)` (R5.1/R5.2/R5.3); a catalog test covers the cue key
  (R5.4).
- Coaching: property tests over the pure reducer assert step progression (`advance` maps `i`→`i+1`
  until the last, then `completed`), the dismiss invariant (`enabled` stays true, callout hidden),
  the step model (`index i` ⇒ `STEPS[i]` text + anchor), and the preference persistence round trip
  (set enabled/disabled/advance-past-end → serialize → deserialize → same flags), covering
  R6.1/R6.2/R6.3/R6.5/R6.6/R6.7/R6.8. A UI harness example asserts the layer is non-blocking: with
  coaching shown, a sketch click still reaches the active tool (R6.4). A colour test covers R6.10.

Whole-shell surfaces (R7–R13) use the same dual approach:

- Command palette (R7): property tests over `matches(app, q)` across generated queries assert the
  ordering invariant (every starts-with match precedes every contains-only match, which precedes
  every synonym-only match; ties are alphabetical by localized label) and that it is total and
  deterministic (Property 15), and that any command reachable by a synonym term is in the results
  for a query matching that synonym (Property 16). A UI harness example opens the popup with `S`,
  asserts the input is focused, that an entry shows its effective shortcut, that activating a
  no-param entry calls `app.start`, and that a no-match submit leaves the popup open, shows an
  error status, and does not change the design (R7.1/R7.5/R7.6/R7.7). A catalog test covers the
  synonym keys and `palette.no_match` in all four locales (R7.8).
- Dialog frame (R8): a property over generated dialogs/parameters asserts the frame always places
  Cancel/OK in the same order outside the scroll region (so both are reachable at any window
  height), that the preview is present exactly when the parameters are valid (and suppressed when
  invalid), and that a `Cancel` leaves `session.doc` and the undo depth unchanged (Property 17). UI
  examples drive one representative dialog of each shape (a `params`-style edit and a `dialogs_*`
  feature) to confirm a rejected field keeps the dialog open with a localized per-field message and
  that a confirm runs the command once (R8.4/R8.6).
- View navigation (R9): property tests over `view.standard.*` across generated cameras assert each
  id applies its orientation and keeps the camera valid, and that an unknown orientation name
  returns the camera unchanged (Property 18); a property over `view.fit` across generated models
  asserts a non-empty model yields a camera that frames the bounds (`changed:true`) and an empty
  model returns the camera unchanged (`changed:false`) (Property 19). A UI harness example clicks
  the nav bar's Home/Fit and an axis tip and asserts the camera moved only via the command path and
  the labels/colours come from `tr`/`Tokens` (R9.2/R9.7).
- Keyboard map (R10): property tests over generated bind/clear/reset/conflict sequences assert the
  persistence round trip (serialize → deserialize → same effective bindings), that `reset` restores
  every `CommandSpec` default, that a reserved key is always refused, and that a conflicting bind
  only takes effect after a confirming `replace` (Property 20). The existing
  `rebinding_detects_conflicts_and_resets` unit test is kept; a catalog test covers the localized
  page strings.
- Timeline (R11): a property over generated feature lists asserts the drawn order equals the
  creation order and that error/suppressed/ordinary features map to three distinguishable
  presentations (Property 21); a property over a generated hover-id/timestamp stream asserts the
  dwell classifier promotes a feature only after the ~150 ms dwell and clears it on leave (Property
  22). UI examples cover double-click-to-edit and the i18n of the status text.
- Browser (R12): a property over generated sketch solver reports asserts the three-way
  constraint-state classifier is total and maps under-defined / fully-defined / locked to three
  distinct indicators (Property 23); a property over generated designs asserts exactly one
  component row is marked active and it equals `session.active_component`, and that toggling
  visibility runs `browser.visibility` while a failed command leaves the indicator in sync with the
  session (Property 24). UI examples cover activation via `component.activate` and the i18n/colour
  sourcing.
- App-level onboarding (R13): a property over generated session states asserts
  `new_document_cue_visible == features.is_empty()` (shown on an empty design, hidden once the first
  feature exists) (Property 25). A reducer/catalog check asserts the step table includes a
  workspace-tab step and a non-sketch step and that every app-level key translates in all four
  locales (R13.2/R13.3/R13.7). A UI harness example asserts the onboarding is non-blocking: a ribbon
  tab and a toolbar command still fire while it is shown (R13.5).

Property-test configuration: each property runs at least 100 generated iterations and is tagged
`Feature: ux-ui-shell-improvements, Property {n}: {property text}` and references its design
property below. Unit/UI tests cover the specific examples, edge cases, and layout/focus behaviour
that are not universal properties. Any temporary files created while running the app for evidence
are cleaned up; no generated screenshots or reference data are committed.

Clean-room reminder for tests: expected values are analytic or come from SolidCraft's own
open-source paths; no test data, measurements, command ids, or files are produced with or copied
from any commercial CAD product or the local FreeCAD install.

## Correctness Properties

*A property is a characteristic or behavior that should hold true across all valid executions of a
system — essentially, a formal statement about what the system should do. Properties serve as the
bridge between human-readable specifications and machine-verifiable correctness guarantees.*

### Property 1: Active-tool feedback is complete for every tool and step

For any sketch tool command and any reachable input step of that tool, the tool-feedback model
produced for a running instance of that tool exposes a non-empty localized tool name, a non-empty
localized description of the input the step expects, a non-empty localized confirm affordance, and a
non-empty localized exit affordance.

**Validates: Requirements 1.1, 1.2, 1.3, 2.1**

### Property 2: Advancing a step changes the expected input

For any sketch tool with more than one distinct input step, advancing the tool from one step to the
next changes the displayed expected-input (and inline-prompt) text whenever the underlying steps
differ.

**Validates: Requirements 1.4, 2.2**

### Property 3: Every prompt and label string resolves through the catalog

For any catalog key referenced by a tool step prompt, confirm/exit affordance, snap or inference
type label, empty-state cue, or coaching step, the key resolves to a real translation in the English
catalog (not to the raw id) and is present/translatable in the Japanese, German, and Korean
catalogs.

**Validates: Requirements 2.3, 3.2, 5.4, 6.9**

### Property 4: Snap and inference markers appear at the snapped position

For any sketch and cursor position for which the snap engine or inference engine reports a target,
the legibility model produces a marker located at that target's point (`snap.at` / `inference.at`)
and carrying the target's named type.

**Validates: Requirements 3.1, 3.3**

### Property 5: Every applicable inference gets its own marker

For any cursor position to which more than one inference applies, the number of rendered inference
markers equals the number of applicable inference guides.

**Validates: Requirements 3.4**

### Property 6: Confirming a typed dimension applies that value

For any finite, valid dimension expression confirmed in a dimension entry, the sketch dimension
command runs with that expression as its value and the resulting dimension measures the value the
expression evaluates to (within numeric tolerance).

**Validates: Requirements 4.2**

### Property 7: Editing an existing dimension prepopulates its current value

For any existing dimension, opening it for editing populates the entry with an expression that
evaluates to that dimension's current value.

**Validates: Requirements 4.3**

### Property 8: Hostile dimension input is rejected and the design is unchanged

For any dimension-entry value that is not a finite in-range number (including NaN, ±infinity,
overflowing magnitudes, empty, overlong, or non-numeric expressions), confirming it returns an
error, marks the status as an error, does not panic, and leaves the design document and undo history
unchanged.

**Validates: Requirements 4.4**

### Property 9: Cancelling a dimension entry changes nothing

For any existing dimension and any typed-but-cancelled draft, cancelling the entry leaves the design
document and undo history identical to their state before the entry was opened.

**Validates: Requirements 4.5**

### Property 10: The empty-state cue is shown exactly when the sketch is empty and idle

For any active-sketch state, the empty-state cue is visible if and only if the sketch contains no
sketch geometry and no active tool is running.

**Validates: Requirements 5.1, 5.2, 5.3**

### Property 11: The coaching layer presents the step at its current index

For any enabled, non-dismissed coaching preference whose index is within the step table, the
coaching view presents the text of the step at that index and the same anchor target the step table
defines for it.

**Validates: Requirements 6.1**

### Property 12: Coaching advances monotonically and completes at the end

For any coaching preference, advancing moves the current index to the next step until the last step,
after which advancing marks the sequence complete; the index never exceeds the step table bounds.

**Validates: Requirements 6.2, 6.8**

### Property 13: Dismissing a step keeps the layer enabled

For any enabled coaching preference, dismissing the current step hides that step's callout while
leaving the layer enabled.

**Validates: Requirements 6.3**

### Property 14: Coaching preference persists across sessions

For any sequence of enable, disable, advance, and dismiss operations, the resulting coaching
preference round-trips through serialization and deserialization unchanged, so a restart restores
the same enabled, index, dismissed, and completed state (and a disabled preference keeps the layer
hidden until re-enabled).

**Validates: Requirements 6.5, 6.6, 6.7, 6.8**

### Property 15: Palette matches are ordered deterministically

For any query string and any registry of commands, the palette match list is total and
deterministic, and it is ordered so that every command matched by a prefix of its id or localized
label precedes every command matched only by a substring, which in turn precedes every command
matched only by a synonym; within one rank the matches are in ascending order of their localized
label.

**Validates: Requirements 7.2, 7.3**

### Property 16: A synonym finds its command

For any command that carries a synonym term in the Locale_Catalog, any query that is contained in
that synonym term returns a match list that includes that command.

**Validates: Requirements 7.4**

### Property 17: A cancelled dialog changes nothing and keeps its actions reachable

For any feature dialog and any parameters typed into it, the shared dialog frame places its confirm
and cancel controls outside the scrolling region in a fixed order (so both are reachable at every
window height), shows a live preview exactly when the current parameters are valid, and — when the
dialog is cancelled — leaves the design document and the undo history identical to their state
before the dialog opened.

**Validates: Requirements 8.1, 8.3, 8.5**

### Property 18: A standard view applies its orientation and rejects unknown names

For any valid camera, each `view.standard.*` command returns a valid camera carrying that
command's standard orientation, and for any view name that is not a defined standard view the view
command returns the camera unchanged.

**Validates: Requirements 9.2, 9.6**

### Property 19: Fit frames a non-empty model and leaves an empty one unchanged

For any design and viewport aspect, `view.fit` returns a valid camera that frames the visible model
(or selection) bounds when those bounds are non-empty, and returns the camera unchanged with
`changed:false` when there is no visible geometry to frame.

**Validates: Requirements 9.3**

### Property 20: Keymap bindings round-trip, reset to defaults, and respect reserved keys and conflicts

For any sequence of bind, clear, reset, and conflict-resolution operations, the resulting keymap
round-trips through serialization and deserialization to the same effective bindings; a reset
restores every command's `CommandSpec` default; a reserved key is never bound; and a bind to a key
another command already uses takes effect only after a confirming replace, after which the key runs
exactly one command.

**Validates: Requirements 10.1, 10.3, 10.4, 10.5, 10.6, 10.7**

### Property 21: The timeline reads in creation order with distinct feature states

For any design, the timeline draws its features in the order the features were created, and an
error feature, a suppressed feature, and an ordinary feature each render with a distinguishable
presentation drawn from Theme_Tokens.

**Validates: Requirements 11.1, 11.4, 11.5**

### Property 22: Hover highlighting follows a dwell and clears on leave

For any stream of hovered timeline features and timestamps, the viewport highlight is set to a
feature only after the pointer has rested on that feature for the dwell interval, and is cleared on
the frame the pointer leaves the timeline strip.

**Validates: Requirements 11.3**

### Property 23: The browser classifies every sketch into one of three constraint states

For any sketch solver report, the browser's constraint-state classifier returns exactly one of
under-defined, fully-defined, or locked, and the three states map to three visually distinct
indicators.

**Validates: Requirements 12.3**

### Property 24: Exactly one component is marked active and visibility stays in sync

For any design, exactly one browser component row is marked as the active component and it equals
the session's active component; toggling a node's visibility runs the visibility command, and the
row's visibility indicator always reflects the session's visibility state (so a rejected toggle
leaves the indicator unchanged).

**Validates: Requirements 12.2, 12.4**

### Property 25: The new-document cue is shown exactly when the design has no features

For any design state, the new-document onboarding cue is visible if and only if the design contains
no features, so it appears on a freshly created design and hides once the first feature is added.

**Validates: Requirements 13.1, 13.4**
