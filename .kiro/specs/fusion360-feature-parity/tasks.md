# Implementation Plan: Fusion 360 Feature Parity

## Overview

This plan deepens SolidCraft's existing Fusion-workspace seeds toward functional parity. It is
**seed-first and additive**: every task preserves existing command ids (`surface.*`, `form.*`,
`mesh.*`, `drawing.*`, `cam.contour`) and keeps `solidcraft-container/1` designs loading, by
adding `#[serde(default)]` fields to existing `FeatureKind` variants or adding sibling commands
with new dotted ids — never a greenfield rewrite.

Stage 1 (Surface depth, Form/T-splines, Mesh reverse-engineering, UI-shell parity) is broken into
granular, immediately-actionable tasks because it is the near-term shippable alpha. Stages 2–6 are
present but coarse-grained and marked **later-phase**.

Every task names the crate(s)/files it touches, references the requirement/acceptance-criteria and
design section it implements, and carries its test obligation. Work is Rust. Each task must land a
CI-green increment (`cargo xtask ci`: fmt, clippy -D warnings, tests, assets, layers, wasm). Every
kernel call routes through `solidcraft_kernel::guard`; input-derived numbers are treated as
hostile (finite checks, `get()`, explicit caps); no panics in non-test code.

**Required vs optional:** tasks with no `*` are **required for the Stage 1 alpha**. Sub-tasks
marked `*` are **optional** (extra tests, or later-phase Stages 2–6 scaffolding) and may be
skipped for a faster alpha. All property/unit/oracle/fuzz test sub-tasks are marked `*` per the
test-optional convention; the hostile-param fuzz and oracle checks are strongly recommended before
shipping since the never-crash rule outranks feature work.

Property tags: each property test references its design property as
`Feature: fusion360-feature-parity, Property {n}: {text}` and uses ≥100 generated inputs.

---

## Reconciliation (2026-10-22, fork `dev`)

The command surface for Stages 1–6, mold, and DfAM is **confirmed registered** against the
current `docs/features.md` (479 commands) and engine `cmd/*` — verified, not assumed. The key
id families this plan references all already exist on this branch: `surface.offset` /
`surface.rule` (plus `surface.flatten`, `surface.reverse`); the full `form.*` edit set
(`form.edit_point`, `form.insert_edge`, `form.insert_point`, `form.subdivide`, `form.bridge`,
`form.weld`, `form.crease`, `form.uncrease`, `form.symmetry`, `form.extrude_face`, `form.finish`,
`form.thicken`, `form.match`); `mesh.remesh` / `mesh.deviation` (plus `mesh.smooth`, `.separate`,
`.combine`, `.merge_groups`, `.shell`, `.reverse`); the `ui.*` / `view.*` shell commands
(`ui.quick_toolbox`, `ui.repeat_last`, `ui.context_menu`, `ui.select_depth`, `ui.select_filter`,
`view.orient`, `view.visual_style`, `view.component_colors`, `view.select_mode`); the full
`mold.*` and `dfam.*` sets; and the later-stage surfaces `drawing.*` (19), `cam.*` (30), `sim.*`
(12), `generative.*` (3), `render.*` (3), `anim.*` (4), and `ecad.*` (8).

Because every referenced command already exists, the unchecked boxes below do **not** track
"does the command exist." They track the remaining **deepening** and **test/evidence**
obligations for each capability: seed-to-parity depth, property / oracle / hostile-param-fuzz
tests, control-channel screenshot evidence, and the never-crash discipline. An unchecked box must
never be read as "command missing" or as a signal to re-register or rebuild an existing command.

Do not reimplement `surface.*`, `form.*`, `mesh.*`, `drawing.*`, `cam.*`, `sim.*`, `render.*`,
`ecad.*`, or `mold.*`; the `mesh.*` and the `view.*` / `ui.*` shell families are likewise already
registered and must not be rebuilt. These are a hard do-not-rebuild list, not a queue.
Still deferred: PCB autorouting and a cloud renderer.

## Tasks

### Stage 1A — Surface workspace depth (Requirement 3 · design §1A)

- [ ] 1. Deepen surface create-as-surface fidelity
  - [ ] 1.1 Add `open_sweep` kernel helper for true open surface bodies
    - Add `pub fn open_sweep(profile, ...) -> Result<Body>` in `crates/kernel/src/surfaces.rs`
      next to `surface_body`, producing a zero-thickness open body (open profiles allowed, no
      implicit cap); route the `truck` work through `solidcraft_kernel::guard("surface.open_sweep", …)`
    - _Requirements: 3.1, 3.7 · design §1A.1_
  - [ ] 1.2 Drive `surface.extrude/revolve/sweep/loft` through the open-body path
    - In `crates/engine/src/cmd/surface.rs`, make surface-create produce a first-class open body
      via the existing `operation`/`extent` fields instead of running the solid command and
      discarding caps via `SheetWalls`; keep the command ids unchanged
    - _Requirements: 3.1 · design §1A.1_
  - [ ]* 1.3 Property test: surface-create yields a surface + one timeline feature
    - **Property 3: Surface-create yields a zero-thickness surface and one timeline feature**
    - **Validates: Requirements 3.1** (≥100 inputs; generate valid closed profiles)
  - [ ]* 1.4 Oracle case: thickened-surface volume = area × thickness (analytic)
    - Add `tests/oracle/surface_thicken_volume/` with `recipe.json` + analytic `expected.json`;
      wire into `cargo xtask oracle`
    - _Requirements: 3.5 · design testing strategy_

- [ ] 2. Deepen patch, trim/untrim, stitch, thicken
  - [ ] 2.1 Deepen `surface.patch` boundary handling
    - In `crates/engine/src/cmd/surface.rs` + `crates/kernel/src/surfaces.rs`, accept mixed
      edge+sketch boundaries and guide curves for one smooth B-spline face; no new id
    - _Requirements: 3.2 · design §1A.2_
  - [ ] 2.2 Deepen `surface.trim` to accept surface/curve tools
    - Extend `surface.trim` in `crates/engine/src/cmd/surface.rs` to accept a surface/curve tool in
      addition to a plane, preserving `keep` semantics; keep `surface.untrim` restoring the latest
      `SurfaceTrim`
    - _Requirements: 3.3 · design §1A.3_
  - [ ] 2.3 Deepen tolerant stitch with gap reporting
    - Add/deepen `stitch_tolerant(bodies, tol) -> Result<Stitched>` in `crates/kernel/src/sew.rs`
      (solid when faces close, else joined surface) and report the gap edges that prevented closure;
      surface by it in `surface.stitch`
    - _Requirements: 3.4 · design §1A.4_
  - [ ] 2.4 Deepen `surface.thicken` to multi-body / non-planar
    - Extend thicken in `crates/engine/src/cmd/surface.rs` + `crates/kernel/src/offset.rs` to
      multi-body and non-planar surfaces (symmetric/signed, new/join/cut/intersect)
    - _Requirements: 3.5 · design §1A.5_
  - [ ]* 2.5 Property tests for patch/trim/stitch/thicken
    - **Property 4: Planar patch area equals the enclosed area** — **Validates: 3.2**
    - **Property 5: Trim then untrim restores the surface** — **Validates: 3.3**
    - **Property 6: Stitch closes to a solid exactly when faces bound a volume** — **Validates: 3.4**
    - **Property 7: Thicken volume equals area times thickness** — **Validates: 3.5** (each ≥100 inputs)
  - [ ]* 2.6 Oracle cases: patch area (shoelace), stitched-solid volume, extend existing unit tests
    - Add analytic oracle cases; extend co-located `patch_trim_thicken_and_stitch` /
      `untrim_drops_the_latest_trim` unit tests in `crates/engine/src/cmd/surface.rs`
    - _Requirements: 3.2, 3.3, 3.4_

- [x] 3. Add `surface.offset` and `surface.rule` sibling commands
  - [x] 3.1 Add `surface_offset` kernel helper + `FeatureKind::SurfaceOffset`
    - Add `pub fn surface_offset(body, distance) -> Result<Body>` in `crates/kernel/src/offset.rs`
      (guarded); add `FeatureKind::SurfaceOffset` variant in `crates/doc/src/document.rs` with all
      new fields `#[serde(default)]`
    - _Requirements: 3.1, 3.6 · design §1A.6, Components table_
  - [x] 3.2 Register `surface.offset` and `surface.rule` commands
    - Register both as new dotted ids in `crates/engine/src/cmd/surface.rs` via `CommandSpec`
      (`.at("SURFACE", "CREATE")`, code-drawn icon in `crates/ui-egui/src/icons.rs`); add
      `FeatureKind::SurfaceRule`; unsupported configs return `bad(cmd, "not supported yet: …")`
    - _Requirements: 3.1, 3.6 · design §1A.6_
  - [ ]* 3.3 Unit + hostile-param fuzz for new surface commands
    - Co-located unit tests; extend `hostile_params_never_panic` discipline for `surface.offset`/`.rule`
    - **Property 1: Hostile input never crashes and leaves the design unchanged** — **Validates: 3.7**
    - **Property 2: Unsupported options are reported, not performed** — **Validates: 3.6** (≥100 inputs)

- [ ] 4. Checkpoint — surface depth green
  - Ensure all tests pass (`cargo xtask ci`); capture before/after control-channel screenshots of a
    surface create + thicken (`ui.screenshot`); ask the user if questions arise.

### Stage 1B — Freeform / T-spline modelling (Requirement 4 · design §1B)

- [ ] 5. Deepen the Form feature into an editable cage model (additive format change)
  - [ ] 5.1 Extend `FeatureKind::Form` with cage fields + ref structs
    - In `crates/doc/src/document.rs`, add `edits: Vec<FormEdit>`, `symmetry: Option<PlaneRef>`,
      `creases: Vec<CageEdgeRef>` to `Form`, all `#[serde(default)]`; keep legacy `pulls` (also
      `#[serde(default)]`); add `FormEdit` tagged enum and `CageVertRef`/`CageFaceRef`/`CageEdgeRef`
    - _Requirements: 4.1, 4.2 · design §1B, Document & format_
  - [ ]* 5.2 Corpus round-trip test: old 4×4-pull design loads and re-evaluates equivalently
    - Add a seed-`form.*` design to the CLI design corpus; assert load → re-eval → equivalent model
    - **Property 21: Documents round-trip across the deepened model** — **Validates: 1.4** (≥100 corpus/generated)

- [ ] 6. Deepen the Form kernel evaluation path
  - [ ] 6.1 Add `form_sheet(cage, creases, symmetry) -> Result<Body>` in kernel
    - Add/deepen in `crates/kernel/src/formbody.rs` / `freeform.rs` building on
      `crates/kernel/src/tspline.rs` (`tspline_sheet`); mirror edits across the symmetry plane and
      sharpen creased edges before the smooth sheet; guarded
    - _Requirements: 4.2, 4.3 · design §1B (cage model, symmetry & crease)_
  - [ ] 6.2 Replay edits deterministically on `Model::refresh`
    - In `crates/doc` eval path, replay `pulls` then `edits` in order so the form stays editable
      from the timeline and re-evaluation is deterministic
    - _Requirements: 4.2 · design §1B (re-evaluability)_
  - [ ]* 6.3 Property tests for cage evaluation and symmetry
    - **Property 8: Form-create appends one evaluable feature for either start shape** — **Validates: 4.1**
    - **Property 9: Cage edits re-evaluate deterministically and change the body** — **Validates: 4.2**
    - **Property 10: Symmetric sculpting yields a symmetric body** — **Validates: 4.3** (each ≥100 inputs)

- [ ] 7. Add the Form command set (sibling ids; keep `form.create`/`form.pull`)
  - [ ] 7.1 Enforce cage caps before any kernel call
    - In `crates/engine/src/cmd/form.rs`, add `MAX_CAGE_VERTS=20_000`, `MAX_SUBDIV_LEVEL=6`,
      `MAX_EDITS=10_000`; validate finite + bounded before allocating, returning an error past a cap
    - _Requirements: 4.5 · design §1B (hostile-input caps)_
  - [ ] 7.2 Register `form.edit_point`, `form.insert_edge`, `form.subdivide`, `form.bridge`
    - Each appends a `FormEdit` and re-evaluates via `form_sheet`; new dotted ids, code-drawn icons
    - _Requirements: 4.2 · design §1B (new commands)_
  - [ ] 7.3 Register `form.crease`, `form.symmetry`, `form.finish`
    - `form.crease`/`form.symmetry` set `creases`/`symmetry`; `form.finish` converts the cage to a
      solid or surface body usable downstream; unsupported ops return `"not supported yet: …"`
    - _Requirements: 4.3, 4.4, 4.6 · design §1B_
  - [ ]* 7.4 Property + fuzz for finish and hostile cage input
    - **Property 11: A finished form is a usable solid or surface** — **Validates: 4.4**
    - **Property 1 / Property 2** over all `form.*` commands — **Validates: 4.5, 4.6** (≥100 inputs each)

- [ ] 8. Checkpoint — form depth green
  - Ensure all tests pass; capture a control-channel screenshot sequence of create → edit_point →
    symmetry → finish; ask the user if questions arise.

### Stage 1C — Mesh reverse-engineering (Requirement 5 · design §1C)

- [ ] 9. Deepen mesh section and face groups
  - [ ] 9.1 Deepen `mesh.section` to multiple ordered closed loops
    - In `crates/engine/src/cmd/mesh.rs` + `crates/kernel/src/meshbody.rs`, build multiple
      de-duplicated ordered closed loops (not one chain); a non-intersecting plane returns an error;
      cap loop/triangle counts
    - _Requirements: 5.2 · design §1C.1_
  - [ ] 9.2 Deepen `mesh.face_groups` to dihedral-angle region growing (partition)
    - Group by region growing on dihedral angle so groups partition all triangles; keep `.noundo()`
    - _Requirements: 5.3 · design §1C.2_
  - [ ]* 9.3 Property tests for section and face groups
    - **Property 12: Mesh-section points lie on the cut plane** — **Validates: 5.2**
    - **Property 13: Face groups partition the mesh** — **Validates: 5.3** (each ≥100 inputs)

- [ ] 10. Deepen mesh convert and modify ops with caps
  - [ ] 10.1 Deepen prismatic `mesh.convert` with cylindrical-region recognition
    - In `crates/engine/src/cmd/mesh.rs` + `crates/kernel/src/mesh_convert.rs`, recognize
      cylindrical regions via plane/surface-kind fitting (reuse `crates/kernel/src/surface_kind.rs`);
      faceted stays `face_count == triangle_count`; guarded
    - _Requirements: 5.4 · design §1C.3_
  - [ ] 10.2 Add `mesh.remesh` command + `FeatureKind::MeshRemesh`
    - Add `mesh_remesh(mesh, target) -> Result<Body>` in `crates/kernel/src/meshbody.rs`; add
      `FeatureKind::MeshRemesh` (`#[serde(default)]` fields) in `crates/doc/src/document.rs`; register
      `mesh.remesh` with `MAX_MESH_TRIS` cap in `crates/engine/src/cmd/mesh.rs`
    - _Requirements: 5.5 · design §1C.4_
  - [ ]* 10.3 Property + oracle for convert/reduce/cut
    - **Property 14: Mesh conversion face counts are correct per mode** — **Validates: 5.4**
    - **Property 15: Reduction does not increase triangle count and cut keeps one side** — **Validates: 5.5**
    - Oracle: faceted-conversion face count, section points on plane — **Validates: 5.2, 5.4** (≥100 inputs)

- [ ] 11. Add `mesh.deviation` compare command
  - [ ] 11.1 Add `mesh_deviation(brep, mesh) -> Result<Deviation>` kernel helper
    - Add to `crates/kernel/src/meshbody.rs` (or measure-adjacent module); add `Deviation { max, mean }`
      result type in `crates/doc`/`crates/kernel`; guarded; cap triangle counts
    - _Requirements: 5.6 · design §1C.5, Kernel interfaces_
  - [ ] 11.2 Register read-only `mesh.deviation` command
    - Register in `crates/engine/src/cmd/mesh.rs` with `.noundo()`; report max/mean distance as a
      number; new dotted id, code-drawn icon
    - _Requirements: 5.6 · design §1C.5_
  - [ ]* 11.3 Property + fuzz for deviation and all mesh commands
    - **Property 16: Deviation is finite, non-negative, and ~0 for an exact rebuild** — **Validates: 5.6**
    - **Property 1** over all `mesh.*` commands — **Validates: 5.7** (≥100 inputs each)

- [ ] 12. Checkpoint — mesh RE green
  - Ensure all tests pass; capture a control-channel screenshot of import → section → convert →
    deviation report; ask the user if questions arise.

### Stage 1D — UI-shell parity (Requirement 2 · design §1D)

- [ ] 13. Pure, testable shell math (render/engine) — keeps ui-egui thin & swappable
  - [ ] 13.1 Quick-toolbox + context-menu pure builders
    - Add a pure registry search over `command_specs()` + query, and
      `context_menu(selection, &registry) -> Vec<CommandInfo>` returning only commands whose
      `enabled(session)` holds, plus a synthetic Repeat-last entry; place in `crates/engine`
    - _Requirements: 2.1, 2.2, 2.7 · design §1D (quick-toolbox, context menu)_
  - [x] 13.2 View-orientation camera math (Z-up preserved)
    - Add a `crates/render` function mapping each of the 26 standard/diagonal directions to a camera
      orientation with world Z up, consistent with existing axis navigation
    - _Requirements: 2.3 · design §1D (view orientation)_
  - [ ] 13.3 Selection modes/filters pure picking
    - Add `SelectMode`, `SelectFilter`, and `pick(ray, candidates, mode, filter) -> Option<Pickable>`
      to `crates/render` (deterministic, occluded resolved by sorted depth, returns only permitted kinds)
    - _Requirements: 2.4, 2.5 · design §1D (selection modes & filters)_
  - [x] 13.4 Visual-style render parameter (no geometry change)
    - Add `VisualStyle { Shaded, ShadedEdges, HiddenEdges, Wireframe }` to `crates/render` driving
      the rasterizer/viewport; it is a view setting — not undoable, not serialized into the design
    - _Requirements: 2.6 · design §1D (visual styles)_
  - [x] 13.5 Drag selection modes (window / freeform / paint) pure builders
    - Add `select_window`, `select_freeform`, `select_paint` to `crates/render`, all pure,
      deterministic and filter-aware (reusing `SelectFilter` from 13.3): `select_window` returns
      enclosed vs crossing hits from the drag direction (left-to-right encloses, right-to-left
      crosses), `select_freeform` resolves hits by point-in-polygon against the lasso path, and
      `select_paint` accumulates hits along a swept brush path; each returns only permitted kinds
      and the active `SelectMode` (replace/add/subtract) resolves the result set deterministically
    - _Requirements: 2.9, 2.10, 2.11 · design §1D (render fns `select_window`/`select_freeform`/`select_paint`)_
  - [x] 13.6 Depth-stack chooser pure builder
    - Add `depth_stack(ray, candidates, filter) -> Vec<Pickable>` to `crates/render` returning the
      overlapping candidates under the cursor ordered by sorted ray depth, keeping only permitted
      kinds; it never hides or mutates any body — it is a read-only ordering over pick candidates
    - _Requirements: 2.12 · design §1D (render fn `depth_stack`)_
  - [ ]* 13.7 Property tests for shell math
    - **Property 17: Visual-style switches never change geometry or timeline** — **Validates: 2.6**
    - **Property 18: View orientation preserves Z-up** — **Validates: 2.3**
    - **Property 19: Selection respects mode and filter, deterministically** — **Validates: 2.4, 2.5**
    - **Property 20: Quick-toolbox and context-menu entries resolve to valid commands** — **Validates: 2.1, 2.2** (each ≥100 inputs)
  - [ ]* 13.8 Property tests for drag selection and depth chooser
    - **Feature: fusion360-feature-parity, Property 25: Drag selection is deterministic, respects the
      filter, and the mode (replace/add/subtract) composes the result set** — **Validates: 2.9, 2.10, 2.11**
    - **Feature: fusion360-feature-parity, Property 26: The depth chooser lists candidates ordered by
      ray depth and hides no body** — **Validates: 2.12** (each ≥100 inputs)

- [ ] 14. Register shell commands and wire thin egui widgets
  - [ ] 14.1 Register `ui.*` / `view.*` shell commands
    - In `crates/engine/src/cmd/ui.rs` + `view.rs`, register `ui.quick_toolbox`, `ui.context_menu`,
      `ui.repeat_last`, `ui.select_mode`, `ui.select_filter`, `view.orient`, `view.visual_style`;
      extend `ui.select_mode` to cover the new drag modes (window/freeform/paint) backed by 13.5 and
      register `ui.select_depth` driven by `depth_stack` (13.6), plus a view-only
      `view.component_colors` toggle that cycles per-component colours through `view.component_colors`
      — it is not undoable and not serialized into the design; engine holds current
      mode/filter/style/depth as session state; invalid/stale targets return an `Err` (never panic),
      consuming `ux-ui-shell-improvements` widgets where that spec already provides them
    - _Requirements: 2.1–2.6, 2.7, 2.8, 2.9, 2.10, 2.11, 2.12, 2.13, 2.14, 2.15 · design §1D (`view.component_colors`)_
  - [ ] 14.2 Wire thin egui widgets reading engine state
    - In `crates/ui-egui`, add S-toolbox, context/marking menu, ViewCube, selection filter, and
      visual-style switch that read engine state and act via `app.run(id, params)` / `app.start(id)`;
      add the depth-stack chooser (reading the `depth_stack` ordering) and the component-color toggle
      (reading `view.component_colors` state), both thin — they present engine state and act through
      `CommandSpec` only; colours from `theme::Tokens`; no behaviour bypasses `CommandSpec`
    - _Requirements: 2.7, 2.12, 2.15 · design §1D (thinness, view-only visual style)_
  - [ ]* 14.3 Fuzz invalid shell targets + control-channel screenshot evidence
    - **Property 1** over `ui.*`/`view.*` with vanished/invalid targets — **Validates: 2.8**
    - Capture `ui.start` + `ui.click` + `ui.screenshot` evidence for toolbox, ViewCube orient,
      drag-selection + depth chooser, component-color cycling, and visual-style switch (before/after)
  - [ ]* 14.4 Property test for component color cycling
    - **Feature: fusion360-feature-parity, Property 27: Cycling component colours leaves geometry, the
      timeline, and appearances unchanged** — **Validates: 2.13, 2.14, 2.15** (≥100 inputs)

- [ ] 15. Stage 1 integration + registry/docs wiring
  - [ ] 15.1 Register all new commands and regenerate feature docs
    - Ensure every new id is wired in `crates/engine/src/cmd/mod.rs`; run `cargo xtask features` to
      regenerate `docs/features.md`; update `ROADMAP.md` for the landed Stage-1 tasks
    - _Requirements: 1.6 · design (Document & format, Roadmap)_
  - [ ]* 15.2 Standard-workflow validation recipes (mesh RE + reference-canvas spline-revolve)
    - Script a control-channel/CLI recipe: import mesh → `mesh.section` → clean sketch →
      surface/solid rebuild → `mesh.deviation`; assert every step runs through a `CommandSpec`, no
      panic, unsupported steps return `"not supported yet: …"`
    - Script a reference-canvas spline-revolve recipe: insert + calibrate a reference canvas 1:1 →
      draw a centerline + a fit-point spline → confirm the sketch is fully-defined → `solid.revolve`
      360° → `solid.shell` → apply a transparent appearance; assert each step runs through a
      `CommandSpec`, the sketch is fully-defined before the revolve, and no step panics
    - _Requirements: 13.2, 13.6, 13.7 · design (standard-workflow validation)_

- [ ] 16. Stage 1 alpha checkpoint
  - Ensure `cargo xtask ci` is green (fmt, clippy -D warnings, tests, assets, layers, wasm); confirm
    all seed ids remain registered and the design corpus round-trips; capture a short control-channel
    recording of the four sub-areas; ask the user if questions arise.

### Stage 2 — 2D Drawings depth (Requirement 6 · design Stage 2) · later-phase

- [ ]* 17. Deepen `DrawingSheet` with associative views and annotations
  - [ ]* 17.1 Associative projected/section/detail views + hidden-line/section extraction
    - Deepen `crates/doc` `DrawingSheet`/`DrawingView`; add view-plane projection + hidden-line/section
      curve extraction in `crates/geom`/`crates/kernel` (truck only in kernel); deepen `crates/engine/src/cmd/drawing.rs`
    - _Requirements: 6.1, 6.2, 6.7 · design Stage 2_
  - [ ]* 17.2 Associative dimensions/center-marks/annotations, title block, BOM + balloons
    - Populate parts list from component structure; SolidCraft-original templates/symbols only
    - _Requirements: 6.3, 6.4, 6.5 · design Stage 2_
  - [ ]* 17.3 DXF/PDF export from public specs (SVG seeded)
    - Add exporters in `crates/io`; keep `drawing.export_svg`
    - _Requirements: 6.6 · design Stage 2_
  - [ ]* 17.4 Property + evidence
    - **Property 22 (later-phase): Drawing views and annotations are associative** — **Validates: 6.1–6.4**
    - **Property 23 (later-phase): exports round-trip through public formats** — **Validates: 6.6**

### Stage 3 — Manufacture / CAM depth (Requirement 7 · design Stage 3) · later-phase

- [ ]* 18. Promote `CamJob` into setups with 2D/3D toolpaths and G-code
  - [ ]* 18.1 Setups (WCS + stock + targets) and 2D ops (contour/pocket/drill)
    - Deepen `CamJob` in `crates/doc`; SolidCraft-authored 2D offset math in `crates/geom`/`crates/kernel`;
      extend `crates/engine/src/cmd/cam.rs` (keep `cam.contour`)
    - _Requirements: 7.1, 7.2 · design Stage 3_
  - [ ]* 18.2 3D surface toolpaths, hole recognition, G-code post, simulation report
    - Scallop/stepover math (own); cylindrical-face detection reusing kernel surface-kind analysis;
      configurable SolidCraft-authored G-code post in `crates/io`/`crates/engine`; removed/remaining stock report
    - _Requirements: 7.3, 7.4, 7.5, 7.6, 7.7 · design Stage 3_
  - [ ]* 18.3 Property + round-trip
    - **Property 23 (later-phase): toolpaths and exports round-trip** — **Validates: 7.2, 7.5**

### Stage 4 — Simulation (FEA) & Generative Design (Requirements 8, 9 · design Stage 4) · later-phase

- [ ]* 19. Simulation and generative study entities with non-GPL solver seam
  - [ ]* 19.1 `sim` study entity + own/openly-licensed non-GPL solver, method recorded
    - Add study entity in `crates/doc` (target body, material, constraints, loads); result field via
      SolidCraft-own or non-GPL dependency; colour-map overlay in `crates/render`; min/max reported;
      under-constrained/over-cap refuses misleading results
    - _Requirements: 8.1, 8.2, 8.3, 8.4, 8.5 · design Stage 4_
  - [ ]* 19.2 `generative` study entity + candidate generation and promotion
    - Preserve/obstacle/loads/objective in `crates/doc`; own/non-GPL method, method recorded; promote
      candidate into editable body
    - _Requirements: 9.1, 9.2, 9.3, 9.4 · design Stage 4_
  - [ ]* 19.3 Property: geometry unchanged, checks sound
    - **Property 24 (later-phase)** — **Validates: 8.3, 9.2**

### Stage 5 — Render (PBR) & Animation (Requirements 10, 11 · design Stage 5) · later-phase

- [ ]* 20. PBR render path and animation storyboard
  - [ ]* 20.1 PBR appearances (reuse appearance targeting) + image output
    - PBR path in `crates/render`; reuse `crates/doc/src/appearance.rs`; image file via `crates/io`;
      SolidCraft-original/openly-licensed materials with `ATTRIBUTION.md` rows; missing asset falls back
    - _Requirements: 10.1, 10.2, 10.3, 10.4 · design Stage 5_
  - [ ]* 20.2 Storyboard reusing motion-study/exploded-view + video/image-sequence export
    - Keyed poses + auto-explode in `crates/doc` (`motion`); publish via `crates/io`; never alter real placements
    - _Requirements: 11.1, 11.2, 11.3, 11.4, 11.5 · design Stage 5_
  - [ ]* 20.3 Property: geometry/placements unchanged, interpolation reproduces keyed poses
    - **Property 24 (later-phase)** — **Validates: 10.3, 11.2, 11.3**

### Stage 6 — Electronics / PCB (Requirement 12 · design Stage 6) · later-phase

- [ ]* 21. Schematic/PCB entities, ERC/DRC, push-to-3D
  - [ ]* 21.1 Schematic (components+nets) and PCB layout entities; ERC/DRC read-only checks
    - Add entities in `crates/doc`; ECAD import/export via public-spec formats in `crates/io`;
      openly-licensed/SolidCraft-original part data; checks report without modifying the design
    - _Requirements: 12.1, 12.2, 12.3, 12.5, 12.6 · design Stage 6_
  - [ ]* 21.2 Push board to 3D as a mechanical component
    - Produce a 3D PCB body placeable in the assembly
    - _Requirements: 12.4 · design Stage 6_
  - [ ]* 21.3 Property: geometry unchanged, DRC/ERC sound
    - **Property 24 (later-phase)** — **Validates: 12.2, 12.3**

- [ ]* 22. Remaining standard-workflow validation (Requirement 13) · later-phase
  - [ ]* 22.1 Parametric-mechanical-part, design-for-manufacturing, assembly-and-motion recipes
    - Scripted control-channel/CLI recipes running only registered commands end-to-end; timeline edit
      re-evaluates dependents; motion respects limits/contact; interference reported; evidence captured
    - _Requirements: 13.1, 13.3, 13.4, 13.5, 13.6 · design (standard-workflow validation)_
  - [ ]* 22.2 Mold-design workflow recipe
    - Scripted control-channel/CLI recipe: build a prototype body → `mold.box` → `combine` Cut with
      `keep_tools` (Combine Cut Keep Tools) → `mold.split` into two parts with `mold.alignment_pins`
      and `mold.pour_spout`; assert every step runs through a `CommandSpec`, no step panics, and any
      unsupported configuration returns `"not supported yet: …"`; capture before/after evidence
    - _Requirements: 13.8 · design (standard-workflow validation, Stage-3-adjacent — Mold design & DfAM)_

---

### Stage 3-adjacent — Mold design & DfAM (Requirement 14 · design "Stage 3-adjacent — Mold design & DfAM") · later-phase

- [x]* 23. Mold & DfAM command track (additive; builds on existing solid/boolean capabilities)
  - [x]* 23.1 Extend Combine with Keep Tools
    - In `crates/doc/src/document.rs`, add `#[serde(default)] keep_tools: bool` to the existing
      Combine `FeatureKind` variant so `solidcraft-container/1` keeps round-tripping; honor it in the
      combine command in `crates/engine/src/cmd/` so the tool body is preserved on a cut instead of
      being consumed; finite/bounded validation on all inputs
    - _Requirements: 14.2 · design Stage-3-adjacent_
  - [x]* 23.2 Mold box + cavity
    - Add a guarded `mold_box(...)` helper in `crates/kernel` (every `truck` call through
      `solidcraft_kernel::guard`); add `FeatureKind::MoldBox` (`#[serde(default)]` fields) in
      `crates/doc/src/document.rs`; register `mold.box`; register `mold.cavity` as the thin
      keep-tools combine wrapper (Combine Cut with `keep_tools = true`); code-drawn icons; cap body
      counts before any kernel call
    - _Requirements: 14.1, 14.2 · design Stage-3-adjacent_
  - [x]* 23.3 Split, alignment pins, pour spout
    - Register `mold.split` (split into upper/lower, `FeatureKind::MoldSplit`), `mold.alignment_pins`
      (parametric boss + hole at the parting corners, `FeatureKind::AlignmentPins`, capped pin
      count), and `mold.pour_spout` (swept/extruded cut, `FeatureKind::PourSpout`); all kernel work
      guarded; new variants use `#[serde(default)]`; unsupported configs return `"not supported yet: …"`
    - _Requirements: 14.3, 14.4 · design Stage-3-adjacent_
  - [x]* 23.4 DfAM clearance + nesting
    - Register `dfam.clearance` (parametric gap via offset-face / user parameter) and `dfam.nest`
      (`pack_bodies` — a SolidCraft-own, capped 2D packer in `crates/geom`, `FeatureKind::Nesting`,
      **no GPL packer**); guard every kernel call; cap packing/body counts before allocating;
      hostile/finite checks on all parameters
    - _Requirements: 14.5, 14.6 · design Stage-3-adjacent_
  - [x]* 23.5 Property + hostile-param fuzz for the mold/DfAM track
    - **Feature: fusion360-feature-parity, Property 28 (later-phase): mold cavity volume equals the
      prototype volume with Keep Tools preserving the tool; a split union equals the pre-split mold;
      nested bodies are non-overlapping and within bounds** — **Validates: 14.7**
    - **Feature: fusion360-feature-parity, Property 1: Hostile input never crashes and leaves the
      design unchanged** over all `mold.*` / `dfam.*` commands — **Validates: 14.8** (each ≥100 inputs)
  - Note: all new `FeatureKind` variants (`MoldBox`, `MoldSplit`, `AlignmentPins`, `PourSpout`,
    `Nesting`) and the Combine `keep_tools` field use `#[serde(default)]` so `solidcraft-container/1`
    round-trips; `mold_box`/`pack_bodies` and every other kernel call route through
    `solidcraft_kernel::guard`; body, pin, and packing counts are capped before any kernel call.

---

### Stage 2 — 3D interchange export (Requirement 15 · design "Stage 2 — 3D interchange export") · later-phase

> Do **not** rebuild STEP/IGES/3MF/STL/OBJ/`file.export` — they already ship and are covered by
> `crates/engine/src/export_formats_tests.rs`. These tasks add public-spec writers beside them and
> **refuse** proprietary vendor formats (Requirement 15.7 / Requirement 1.5).

- [x]* 24. Public-specification 3D export writers in `crates/io`
  - [x]* 24.1 OBJ companion MTL + `Format` extensions for tessellated public formats
    - Extend `crates/io/src/lib.rs` `Format` / `Format::from_name` / `export` for `.mtl` (written
      beside OBJ with Wavefront `mtllib`/`usemtl`, colours from appearances), `.gltf`, `.glb`,
      `.ply`, `.amf`, `.dae`, `.x3d`, `.wrl`/`.vrml`, and `.vrm` (glTF-based); reuse `export_mesh`;
      cap triangle counts; pure Rust so `cargo xtask wasm` stays green; wire through existing
      `file.export` / CLI / MCP `export` (no new modelling command id unless params demand it)
    - _Requirements: 15.1, 15.2, 15.3, 15.5, 15.6 · design Stage 2 interchange_
  - [x]* 24.2 Refuse proprietary vendor formats with clear errors
    - Ensure `Format::from_name` / `file.export` reject `.fbx`, `.blend`, `.max`, `.c4d`, `.ma`/
      `.mb`, `.dwg`, `.skp`, `.sldprt`/`.sldasm`, `.f3d`, `.ipt`/`.iam`, `.sat`, `.x_t`/`.x_b` with
      an unsupported-format error that names the public equivalent (glTF or STEP); never link a
      proprietary SDK or read GPL sources; export UI lists only public formats
    - _Requirements: 15.7, 1.5 · design Stage 2 interchange (explicit refusals)_
  - [x]* 24.3 OpenUSD export (ASCII first, then binary/package as bindings allow)
    - Add `.usda` (then `.usd`/`.usdc`/`.usdz`) via OpenUSD public spec or an openly licensed
      non-GPL path; on wasm or missing native deps return `"not supported yet: …"` rather than
      breaking the wasm build
    - _Requirements: 15.4, 15.5, 15.6 · design Stage 2 interchange (OpenUSD)_
  - [x]* 24.4 Round-trip / structural tests + hostile export fuzz
    - Extend `crates/engine/src/export_formats_tests.rs` and `crates/io` unit tests for each new
      writer; assert proprietary extensions error and write no file; empty selection → `Err`
    - **Property 29 (later-phase): Public interchange exports are structurally sound; proprietary
      formats are refused** — **Validates: 15.1–15.4, 15.6–15.8**
    - **Property 1** over `file.export` with hostile paths/formats/body lists — **Validates: 15.6**
      (≥100 inputs)

---

## Notes

- Tasks with no `*` are **required for the Stage 1 alpha**; `*` sub-tasks are **optional** (extra
  tests, or later-phase Stages 2–6 scaffolding). Running "required tasks" ships the Stage 1 alpha;
  running "required and optional" also lands the full test matrix and begins Stages 2–6.
- Because the never-crash rule outranks feature work, the hostile-param fuzz and oracle sub-tasks
  are strongly recommended before any Stage 1 commit even though they are formally optional.
- Every task is additive: existing command ids (`surface.*`, `form.*`, `mesh.*`, `drawing.*`,
  `cam.contour`, the existing Combine) and saved `solidcraft-container/1` designs keep working; new
  fields use `#[serde(default)]` (including the Combine `keep_tools` flag and the new `MoldBox`,
  `MoldSplit`, `AlignmentPins`, `PourSpout`, `Nesting` variants), and new capabilities use sibling
  ids (`mold.*`, `dfam.*`).
- The Mold & DfAM track (task 23) is an independently-shippable, **later-phase** addition kept
  `*`-optional to stay consistent with how this file treats post-Stage-1 work (Stages 2–6); it builds
  on the existing solid/boolean capabilities and introduces no GPL packer — the 2D nesting packer is
  SolidCraft-own in `crates/geom`.
- Two new standard-workflow recipes are seeded alongside the existing validation (`*`-optional like
  15.2/22): the reference-canvas spline-revolve workflow (15.2) and the mold-design workflow (22.2),
  each asserting every step runs through a `CommandSpec`, the sketch is fully-defined where required,
  no step panics, and unsupported steps return `"not supported yet: …"`.
- All kernel work routes through `solidcraft_kernel::guard`; only `crates/kernel` touches `truck`;
  shell math lives in `crates/render`/`crates/engine` so `crates/ui-egui` stays thin and swappable;
  everything below apps keeps building for wasm (`cargo xtask wasm`).
- Expected oracle values come only from analytic math or the FreeCAD oracle run as a separate
  program — never from a commercial CAD product.
- Icons for new commands are drawn in code (`crates/ui-egui/src/icons.rs`); any file asset needs an
  `ATTRIBUTION.md` row (`cargo xtask assets`).
- `cargo xtask ci` must be green before every commit; `cargo xtask features` + `ROADMAP.md` are
  updated whenever commands are added.
- 3D interchange (task 24) is Stage-2 / `*`-optional like drawings: it deepens `crates/io` only,
  reuses existing tessellation, and **excludes** proprietary vendor formats (FBX, BLEND, MAX, C4D,
  Maya, DWG, SKP, SolidWorks, Fusion `.f3d`, Inventor, ACIS SAT, Parasolid) per Requirement 1.5 —
  the independent equivalents are glTF/GLB or STEP.

## Task Dependency Graph

```json
{
  "waves": [
    { "id": 0, "tasks": ["1.1", "5.1", "9.2", "13.2", "13.3", "13.4", "13.5", "13.6"] },
    { "id": 1, "tasks": ["1.2", "2.3", "2.4", "3.1", "6.1", "7.1", "9.1", "10.1", "11.1"] },
    { "id": 2, "tasks": ["1.3", "1.4", "2.1", "2.2", "3.2", "5.2", "6.2", "7.2", "10.2", "11.2", "13.1"] },
    { "id": 3, "tasks": ["2.5", "2.6", "3.3", "6.3", "7.3", "9.3", "10.3", "11.3", "13.7", "13.8", "14.1"] },
    { "id": 4, "tasks": ["7.4", "14.2", "14.4", "15.1"] },
    { "id": 5, "tasks": ["14.3", "15.2", "17.1", "18.1", "19.1", "20.1", "21.1", "23.1", "24.1", "24.2"] },
    { "id": 6, "tasks": ["17.2", "17.3", "18.2", "19.2", "20.2", "21.2", "23.2", "24.3"] },
    { "id": 7, "tasks": ["17.4", "18.3", "19.3", "20.3", "21.3", "22.1", "22.2", "23.3", "23.4", "24.4"] },
    { "id": 8, "tasks": ["23.5"] }
  ]
}
```
