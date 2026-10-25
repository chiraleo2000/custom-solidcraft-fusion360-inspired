# Requirements Document

**Fusion 360 Feature Parity**

## Introduction

SolidCraft is a clean-room, open-source, pure-Rust parametric 3D CAD application. This spec
defines a **staged, prioritized plan** to bring SolidCraft's feature set and workflows toward
functional parity with Autodesk Fusion 360, organized by Fusion's workspaces and standard
workflows so coverage is traceable to the reference teardown.

"Parity" here means **equivalent user-facing capability and workflow**, not imitation.
SolidCraft reaches each capability through its own independent design derived from public
documentation, general CAD conventions, analytic reference values, and open-source tools. The
plan is a direction of travel, not a commitment to replicate Fusion exactly.

### Current baseline (as of ROADMAP 2026-10-22)

- **At or near parity today:** sketching with a constraint solver and the full sketch
  create/modify set and 13-plus constraints; solid modelling (extrude with 5 operations,
  revolve, sweep, loft including loft-to-point, hole with cosmetic and modelled threads,
  press/pull, fillet/chamfer, shell, draft, mirror, pattern, split); parameters and
  configurations; components and assemblies with 7 joint types, motion studies, exploded views,
  and interference; sheet metal; a plastic enclosure subset; file I/O already shipping
  STEP/STP, IGES/IGS, 3MF, STL (binary + ASCII), OBJ, DXF, SVG/PDF (drawings), Gerber (ECAD),
  and the native `.solidcraft` container; a thin contextual UI with a parametric timeline;
  desktop + browser (wasm) + CLI + MCP. Mesh/scene formats beyond that set (glTF/GLB, PLY,
  USD, COLLADA, …) and any proprietary vendor formats are covered by Requirement 15.
- **Already-covered conveniences (no new work).** The teardown highlights two interaction
  techniques that SolidCraft's existing solid set already provides, tracked here only for
  traceability: Press/Pull as the unified extrude / offset-face / fillet tool (one gesture that
  dispatches to the appropriate existing solid command), and the revolve-cut technique used to
  produce circular chamfers. Both are already part of the solid set; neither adds a new
  requirement.
- **Seeded but shallow (partial command sets exist and need deepening):** Surface
  (`surface.*` — patch/stitch/thicken/trim/extrude/revolve), Form/T-splines (`form.*` — a 4×4
  grid seed), Mesh reverse-engineering (`mesh.*` — convert/section/plane-cut/repair/reduce/face
  groups), 2D Drawings (`drawing.*` — base/projected/section view, dimension, SVG export), and a
  single-contour CAM seed (`cam.contour`).
- **Absent / out of near-term scope:** Generative Design, Render (PBR), Animation (storyboard +
  published video), Simulation (FEA), full Manufacture/CAM, and Electronics/PCB.

### Non-negotiable constraints (apply to every requirement below)

These are repository rules from `AGENTS.md` and bound every requirement in this spec. They are
restated once here and are treated as implicit acceptance criteria on all features:

1. **Independent / clean-room.** No code, icons, artwork, help text, tooltips, templates,
   materials, command ids, or reference data copied from any commercial CAD product. Behaviour
   is designed from public documentation and general CAD conventions. File formats come from
   public specifications only.
2. **No commercial CAD products** may be driven, scripted, automated, or used to produce test
   data, measurements, command lists, ids, or files for this project.
3. **Never read, copy, or link GPL/LGPL/AGPL sources** (OpenCASCADE, FreeCAD incl. planegcs,
   SolveSpace, CadQuery/OCP, LibreCAD, QCAD). The FreeCAD oracle runs only as a separate program
   through its public scripting API for reference values; nothing is linked or vendored.
4. **Never crash.** No panics in non-test code (no `unwrap`/`expect`/`panic!`/`unreachable!`/
   `todo!`/`unimplemented!`, no `unsafe`); errors flow through the crate error type and `?`; an
   unfinished feature returns `"not supported yet: …"` rather than panicking. Input-derived
   numbers are treated as hostile. `truck` calls go through `solidcraft_kernel::guard`.
5. **Everything is a command.** Every user-visible behaviour is a `CommandSpec` in
   `crates/engine/src/cmd/*` with SolidCraft's own dotted lower-case id (`<area>.<thing>`),
   label, toolbar placement, icon, shortcut, params doc, `enabled`, JSON `run`, plus tests.
   Programmatic `engine.execute` never opens dialogs; only `ui.start` starts interactive tools.
6. **Layering** (`cargo xtask layers`): L0 geom → L1 sketch/kernel/render → L2 doc → L3 io →
   L4 engine → L5 ui-egui → apps. Only `crates/kernel` uses `truck`. The UI is thin and
   swappable; nothing below L5 depends on egui/eframe/winit/wgpu/rfd.
7. **wasm.** Everything below the apps builds for `wasm32-unknown-unknown` (`cargo xtask wasm`).
8. **Quality gates and attribution.** `cargo xtask ci` (fmt, clippy -D warnings, tests, assets,
   layers, wasm) stays green; every file asset is original or openly licensed with a row in
   `ATTRIBUTION.md`; every icon is drawn in code.
9. **Evidence.** Observable CAD/UI/model behaviour is demonstrated with screenshots or a short
   recording from a small public-safe reproduction, captured by driving the app through the
   control channel.
10. **Units.** Millimetres and radians internally; Z is up.

### Staging overview

| Stage | Theme | Scope |
|---|---|---|
| **Stage 1 — Close the design-workspace gap** | Make the Design workspace feel complete | Surface workspace depth, Form/T-splines, Mesh reverse-engineering, UI-shell parity (S-toolbox, marking menu, ViewCube semantics, selection modes/filters, visual styles) |
| **Stage 2 — Documentation & data out** | Communicate and hand off designs | 2D Drawings depth (views, dimensioning, annotation, parts list/BOM, title block, standards); 3D interchange export of public-specification formats (Requirement 15) |
| **Stage 3 — Downstream manufacturing** | From model to machine | Manufacture/CAM depth (setups/WCS/stock, 2D+3D toolpaths, hole recognition, G-code); mold design & design-for-additive-manufacturing (DfAM) as a Stage 3-adjacent manufacturing track (Requirement 14) |
| **Stage 4 — Analysis & generation** | Validate and explore designs | Simulation (FEA) and Generative Design foundations |
| **Stage 5 — Presentation** | Show the design | Render (PBR) and Animation (storyboard + published video) |
| **Stage 6 — Electronics** | ECAD | Schematic / PCB / 3D PCB, ERC/DRC |

Stages are ordered by priority and dependency. Each stage is independently shippable; later
stages may be re-prioritized by the owner. Every requirement names the Fusion workspace or
workflow it traces to.

Mold design and DfAM (Requirement 14) sit in a Stage 3-adjacent manufacturing track: they build
on existing solid/boolean capabilities rather than on CAM toolpaths, so Requirement 14 is
independently shippable and does not block or depend on the rest of Stage 3.

3D interchange export (Requirement 15) sits in Stage 2 ("data out"): it deepens `crates/io`
and `file.export` using the existing tessellation / B-rep export path. Near-term mesh/scene
writers are independently shippable from drawings depth; the USD family may land later inside
the same requirement. Proprietary vendor formats are **explicitly excluded** with recorded
reasons (Requirement 1.5) — SolidCraft never reverse-engineers or copies commercial CAD
product formats.

---

## Glossary

- **Parity**: Equivalent user-facing capability and workflow to the reference, not imitation of its code, ids, or assets.
- **Clean-room / Independent**: Behaviour designed solely from public documentation and general CAD conventions, with nothing copied from a commercial CAD product.
- **Seed**: A shallow, partial command set that already exists and is to be deepened rather than built greenfield.
- **BRep**: Boundary-representation solid or surface geometry defined by faces, edges, and vertices.
- **T-spline / Form cage**: The editable control-cage geometry of the Form environment used to sculpt smooth freeform bodies.
- **Guarded kernel call**: A call into the third-party geometry kernel routed through `solidcraft_kernel::guard`, which turns a kernel panic into a recoverable error.
- **CommandSpec**: The registry entry in `crates/engine/src/cmd/*` defining a user-visible command (id, label, placement, icon, shortcut, params, `enabled`, JSON `run`).
- **Timeline feature**: A parametric operation recorded on the document timeline so it can be re-evaluated and edited.
- **FEA**: Finite element analysis, the structural solve underlying the Simulation workspace.
- **ERC**: Electrical-rule check that reports schematic connectivity issues without modifying the schematic.
- **DRC**: Design-rule check that reports PCB clearance and width violations.
- **DfAM**: Design for additive manufacturing, covering print-oriented clearances and part layout.
- **WCS**: Work coordinate system that orients a machining setup.
- **Public-specification format**: A file format defined by a published open standard or
  openly documented specification that SolidCraft may implement clean-room (ISO STEP/IGES,
  Khronos glTF, ASTM AMF, Pixar OpenUSD, Wavefront OBJ, …). Proprietary vendor scene/CAD
  formats without a usable public write specification are out of scope.
- **Tessellated export**: An export path that writes triangle (or polygon) mesh data produced
  by the existing guarded kernel tessellation, as opposed to exact B-rep exchange (STEP/IGES).

---

## Requirements

### Requirement 1: Staged, traceable parity plan (meta)

**User Story:** As a SolidCraft maintainer, I want the parity effort organized into prioritized
stages traceable to Fusion's workspaces and standard workflows, so that progress is reviewable,
each increment ships value, and no capability is silently dropped.

#### Acceptance Criteria

1. WHEN the parity plan is recorded THEN the system SHALL organize capabilities into the six
   stages above, each tagged with the Fusion workspace or workflow it addresses.
2. WHEN a parity capability is specified THEN the system SHALL carry the non-negotiable
   constraints (independence, no-GPL, no-panic, command-architecture, layering, wasm,
   attribution) as implicit acceptance criteria.
3. WHEN a stage is planned THEN the system SHALL mark it independently shippable, so later
   stages can be re-prioritized or deferred without blocking earlier ones.
4. WHEN a capability already exists as a shallow seed (Surface, Form, Mesh, Drawing, CAM) THEN
   the plan SHALL treat it as a seed to deepen rather than as greenfield, and SHALL preserve
   existing commands, ids, and saved designs.
5. IF a Fusion capability conflicts with a non-negotiable constraint THEN the system SHALL adopt
   an independent equivalent or explicitly exclude the capability with a recorded reason, rather
   than copying the Fusion behaviour.
6. WHEN roadmap state changes for a parity capability THEN `ROADMAP.md` and (where commands
   change) `docs/features.md` via `cargo xtask features` SHALL be updated to reflect it.

---

### Requirement 2: UI-shell parity (Design workspace shell) · Stage 1

**User Story:** As a user coming from Fusion, I want SolidCraft's interactive shell to offer the
same fast interaction model — an on-canvas quick-access toolbox, a right-click action menu, a
view orientation widget, selection modes and filters, and switchable visual styles — so I can
work without hunting through menus.

> Note: This overlaps the separate `ux-ui-shell-improvements` spec. These criteria stand on
> their own; where the other spec already delivers a capability, this spec consumes it rather
> than duplicating the implementation.

#### Acceptance Criteria

1. WHEN the user presses the quick-toolbox shortcut on the canvas THEN the system SHALL open a
   searchable, customizable list of commands and run the chosen command through `app.run`/`app.start`.
2. WHEN the user right-clicks on the canvas or on an entity THEN the system SHALL present a
   context action menu whose entries map to registered commands valid for the current selection,
   including a Repeat-last-command entry.
3. WHEN the user interacts with the view-orientation widget THEN the system SHALL reorient the
   camera to the corresponding standard or diagonal view with Z-up preserved, consistent with
   the existing axis-navigation behaviour.
4. WHEN the user selects a selection mode (entity, feature/body, or depth/occluded) THEN the
   system SHALL restrict picking accordingly and resolve occluded picks deterministically.
5. WHEN the user enables a selection filter (e.g. faces, edges, vertices, sketch curves, bodies,
   components) THEN the system SHALL limit pickable entity kinds to the filtered set.
6. WHEN the user chooses a visual style (shaded, shaded with visible edges, shaded with hidden
   edges, wireframe) THEN the render output SHALL switch to that style without altering CAD
   geometry or timeline history.
7. WHEN any shell interaction runs a command THEN it SHALL go through the command layer (no
   behaviour bypasses `CommandSpec`), and the shell SHALL remain thin (reads engine state,
   colours from `theme::Tokens`), keeping `crates/ui-egui` swappable.
8. IF a shell action targets an entity that no longer exists or is invalid THEN the system SHALL
   report an error and leave the model and any pending input unchanged, never panicking.
9. WHEN the user picks a window selection mode and drags a box left-to-right THEN the system
   SHALL select only entities fully enclosed by the box, and WHEN the user drags the box
   right-to-left THEN the system SHALL select entities that the box encloses or crosses, resolved
   deterministically from the drag direction.
10. WHEN the user picks the freeform/lasso selection mode and drags a closed freeform region
    THEN the system SHALL select entities inside that region deterministically.
11. WHEN the user picks the paint selection mode and drags the cursor across faces or edges THEN
    the system SHALL add each face or edge the cursor passes over to the selection
    deterministically.
12. WHEN the user presses and holds over overlapping geometry THEN the system SHALL present a
    chooser of the stacked profile, face, edge, and body candidates ordered by sorted depth,
    without first hiding any bodies.
13. WHEN the user chooses a visual style THEN the system SHALL offer exactly four named styles —
    Shaded, Shaded with hidden edges, Shaded with visible edges only, and Wireframe — with the
    three wireframe sub-variants treated as one Wireframe style family.
14. WHEN the user switches the visual style THEN the system SHALL treat the change as a view-only
    setting that is not recorded on the undo stack and is not serialized with the design.
15. WHEN the user toggles component color cycling THEN the system SHALL display each component in
    a distinct color in the viewport and in the browser/timeline, without changing the model
    geometry or the assigned appearances.

> Clean-room note: none of these interactions copy Fusion's icons, command ids, tooltips, or
> keyboard shortcuts. SolidCraft uses its own dotted lower-case command ids and code-drawn icons
> throughout.

---

### Requirement 3: Surface workspace depth (Surface) · Stage 1

**User Story:** As a user modelling complex shapes, I want zero-thickness surface tools —
extrude/revolve/sweep/loft as surfaces plus patch, trim, untrim, stitch, and thicken — so I can
build and repair geometry that pure solid tools can't express, then convert it to a solid.

#### Acceptance Criteria

1. WHEN the user runs a surface creation command (`surface.extrude`, `surface.revolve`,
   `surface.sweep`, `surface.loft`) THEN the system SHALL produce a zero-thickness surface body
   recorded as a parametric timeline feature.
2. WHEN the user runs `surface.patch` on a closed boundary of edges or a planar sketch region
   THEN the system SHALL create a face spanning that boundary (planar when planar, otherwise a
   smooth surface).
3. WHEN the user runs `surface.trim` with a cutting tool and a keep-side point THEN the system
   SHALL remove the portion on the other side, and an untrim operation SHALL restore the
   removed region from its underlying surface.
4. WHEN the user runs `surface.stitch` on surfaces that together bound a closed volume THEN the
   system SHALL produce a solid body; otherwise it SHALL produce a joined surface body.
5. WHEN the user runs `surface.thicken` on surface bodies THEN the system SHALL offset them into
   a solid of the given thickness, supporting symmetric and signed thickness and the
   new/join/cut/intersect operations.
6. IF a surface operation is given a configuration not yet supported THEN the system SHALL
   return `"not supported yet: …"` with context and leave the design unchanged, never panicking.
7. WHEN any surface operation calls the kernel THEN it SHALL route through
   `solidcraft_kernel::guard` so a kernel panic becomes a recoverable error.

---

### Requirement 4: Freeform / T-spline modelling (Form) · Stage 1

**User Story:** As an industrial designer, I want a Form environment to sculpt organic shapes
from editable control geometry — create a form primitive, pull/edit control points, insert edges,
subdivide, bridge, crease, and mirror with symmetry — then finish it into a solid or surface body.

#### Acceptance Criteria

1. WHEN the user runs a form-create command THEN the system SHALL create an editable freeform
   body (control cage) recorded as a timeline feature, with box and cylinder starting shapes.
2. WHEN the user edits the control cage (pull/move control points or faces, insert edge,
   subdivide, bridge) THEN the system SHALL update the smooth body and keep edits re-evaluable
   from the timeline.
3. WHEN the user applies a crease to an edge or sets a symmetry plane THEN the resulting body
   SHALL reflect the sharpened edge or mirrored edits.
4. WHEN the user finishes the form THEN the system SHALL produce a solid or surface body usable
   by downstream features.
5. WHILE a form is being edited the system SHALL bound control counts and subdivision levels so
   hostile or extreme inputs cannot exhaust memory or hang, returning an error past the cap.
6. IF a form operation is not yet supported THEN the system SHALL return
   `"not supported yet: …"` and leave the design unchanged.

---

### Requirement 5: Mesh reverse-engineering (Mesh) · Stage 1

**User Story:** As a user with a scanned or downloaded mesh, I want to import it, inspect it,
section it into sketches, clean it up, and convert it to editable BRep geometry, so I can
rebuild a parametric model and compare deviation against the original.

#### Acceptance Criteria

1. WHEN the user imports a mesh (STL/OBJ/3MF) THEN the system SHALL create a mesh body in the
   browser and the document (already supported via file import).
2. WHEN the user runs `mesh.section` on a mesh body with an origin and normal THEN the system
   SHALL produce sketch curves along the cut usable as a reconstruction reference.
3. WHEN the user runs `mesh.face_groups` THEN the system SHALL group facets into regions usable
   for selection and conversion.
4. WHEN the user runs `mesh.convert` in prismatic or faceted mode THEN the system SHALL produce
   a BRep body (prismatic merges coplanar facets within tolerance; faceted keeps a face per
   triangle).
5. WHEN the user runs `mesh.plane_cut`, `mesh.remesh`, `mesh.reduce`, or `mesh.repair` THEN the
   system SHALL modify the mesh body accordingly, bounding triangle and loop counts against
   hostile sizes.
6. WHEN the reconstruction is complete THEN the system SHALL provide a deviation comparison
   between the rebuilt BRep body and the source mesh, reported as a measured distance.
7. IF a mesh operation exceeds configured size/complexity caps or is unsupported THEN the system
   SHALL return an error and leave the design unchanged, never panicking.

---

### Requirement 6: 2D Drawings depth (Drawing) · Stage 2

**User Story:** As an engineer preparing manufacturing documentation, I want to produce 2D
drawing sheets with base/projected/section/detail views, dimensions and annotations, a title
block, center marks, balloons, and a parts list/BOM, and export them to a standard format, so I
can hand off a complete drawing.

#### Acceptance Criteria

1. WHEN the user creates a drawing and a base view from a model or component THEN the system
   SHALL place an orthographic view on the sheet referencing the model (seeded by `drawing.base_view`).
2. WHEN the user adds a projected, section, or detail view from an existing view THEN the system
   SHALL derive it associatively (projected direction, section cut plane, or detail region).
3. WHEN the user adds a dimension, center mark, or annotation to a view THEN the system SHALL
   place it associatively so it updates when the underlying geometry changes.
4. WHEN the user adds a parts list / BOM to an assembly drawing and balloons to component
   instances THEN the system SHALL populate the list from the component structure and link
   balloons to rows.
5. WHEN the user applies a drafting standard (e.g. ISO or ASME-style first/third-angle, arrow
   styles) and a title block THEN the sheet SHALL reflect that standard and metadata, using only
   SolidCraft's own original templates and symbols.
6. WHEN the user exports a sheet THEN the system SHALL write a standard vector format (SVG, and
   DXF/PDF as available) derived from public specifications (seeded by `drawing.export_svg`).
7. IF a view references geometry that changed or vanished THEN the system SHALL update what it
   can and report the lost reference, never panicking.

---

### Requirement 7: Manufacture / CAM depth (Manufacture) · Stage 3

**User Story:** As a machinist, I want to define a setup (WCS, stock), generate 2D and 3D
toolpaths with hole recognition, simulate material removal, and post-process to G-code, so I can
machine the modelled part.

#### Acceptance Criteria

1. WHEN the user creates a machining setup THEN the system SHALL capture a work coordinate
   system, stock definition, and the target body/bodies as a document entity.
2. WHEN the user defines a 2D toolpath (contour, pocket, drill) on selected geometry THEN the
   system SHALL compute a tool-radius-compensated path with depth stepping (extending the
   `cam.contour` seed).
3. WHEN the user defines a 3D toolpath on a surface/body THEN the system SHALL compute a path
   that respects the tool geometry and a stepover.
4. WHEN hole recognition is run on a body THEN the system SHALL identify cylindrical holes and
   propose drill operations.
5. WHEN the user post-processes a toolpath THEN the system SHALL emit G-code text through a
   configurable, SolidCraft-authored post, with no vendor post-processor files copied.
6. WHEN a toolpath is simulated THEN the system SHALL report removed material / remaining stock
   for review.
7. IF toolpath inputs are invalid or geometry is unsupported THEN the system SHALL return an
   error and leave the document unchanged, never panicking.

---

### Requirement 8: Simulation foundations (Simulation) · Stage 4

**User Story:** As an engineer, I want to set up a structural study with constraints and loads,
solve it, and view stress/displacement results, so I can check a design before manufacturing.

#### Acceptance Criteria

1. WHEN the user creates a simulation study THEN the system SHALL capture the target body,
   material, constraints, and loads as a document entity.
2. WHEN the study is solved THEN the system SHALL compute a result field (e.g. static stress /
   displacement) using SolidCraft's own solver or an openly licensed, non-GPL dependency, with
   the method recorded.
3. WHEN results are displayed THEN the system SHALL render a result overlay (colour map) and
   report min/max values without altering the model geometry.
4. WHEN inputs change THEN the study SHALL mark results stale and allow re-solving.
5. IF the study is under-constrained, unsupported, or exceeds size caps THEN the system SHALL
   report the issue and refuse to produce misleading results, never panicking.

---

### Requirement 9: Generative Design foundations (Generative Design) · Stage 4

**User Story:** As a designer, I want to define preserve/obstacle regions, loads, and goals and
explore generated geometry options, so I can discover lightweight shapes.

#### Acceptance Criteria

1. WHEN the user sets up a generative study (preserve regions, obstacle regions, loads,
   constraints, objective) THEN the system SHALL capture it as a document entity.
2. WHEN a study is generated THEN the system SHALL produce one or more candidate bodies using
   SolidCraft's own or an openly licensed non-GPL method, with the method recorded.
3. WHEN candidates are produced THEN the user SHALL be able to review and promote a candidate
   into an editable body.
4. IF the setup is invalid or exceeds caps THEN the system SHALL report the issue and refuse,
   never panicking.

---

### Requirement 10: Render (Render) · Stage 5

**User Story:** As a user presenting a design, I want to apply physically based appearances and
scene lighting and produce a rendered image, so I can communicate the look of the product.

#### Acceptance Criteria

1. WHEN the user assigns a PBR appearance (base colour, metalness, roughness) to a body, face,
   or component THEN the render SHALL reflect it, reusing the existing appearance targeting
   model and only SolidCraft-original or openly licensed materials.
2. WHEN the user sets scene lighting/environment and renders THEN the system SHALL produce an
   image written to a file.
3. WHEN appearances or scene change THEN the preview SHALL update without altering CAD geometry.
4. IF a requested appearance or environment asset is unavailable THEN the system SHALL fall back
   to a default and report it, never panicking, and every shipped asset SHALL have an
   `ATTRIBUTION.md` row.

---

### Requirement 11: Animation (Animation) · Stage 5

**User Story:** As a user documenting assembly, I want a storyboard timeline with automatic and
manual exploded steps and component motion, and the ability to publish a video, so I can show how
a product goes together.

#### Acceptance Criteria

1. WHEN the user creates an animation storyboard THEN the system SHALL capture keyed component
   positions along a timeline, reusing the existing motion-study and exploded-view machinery.
2. WHEN the user requests an auto-explode THEN the system SHALL generate explode steps from the
   assembly without changing the model's real placements.
3. WHEN the user plays the storyboard THEN the system SHALL interpolate component transforms
   over time.
4. WHEN the user publishes THEN the system SHALL export the animation as a video or image
   sequence to a file.
5. IF the assembly has no components or a step is invalid THEN the system SHALL report it and
   leave placements unchanged, never panicking.

---

### Requirement 12: Electronics / PCB (Electronics) · Stage 6

**User Story:** As a user designing an enclosure for electronics, I want schematic capture, PCB
layout with design-rule checks, and a 3D PCB usable in the mechanical assembly, so I can
co-design board and enclosure.

#### Acceptance Criteria

1. WHEN the user creates a schematic THEN the system SHALL capture components and nets as a
   document entity, using only openly licensed or SolidCraft-original part data.
2. WHEN the user runs an electrical-rule check (ERC) THEN the system SHALL report connectivity
   issues without modifying the schematic.
3. WHEN the user lays out a PCB and runs a design-rule check (DRC) THEN the system SHALL report
   clearance/width violations.
4. WHEN the user pushes the board to 3D THEN the system SHALL produce a 3D PCB body placeable in
   the mechanical assembly as a component.
5. WHEN importing/exporting ECAD data THEN the system SHALL use only public-specification formats.
6. IF a rule check finds violations or inputs are invalid THEN the system SHALL report them and
   leave the design unchanged, never panicking.

---

### Requirement 13: Standard-workflow validation (cross-workspace) · all stages

**User Story:** As a maintainer, I want each of Fusion's standard end-to-end workflows to be
exercisable in SolidCraft through the command/control layer, so parity is validated by complete
workflows rather than isolated commands.

#### Acceptance Criteria

1. WHEN the parametric-mechanical-part workflow is run (sketch → revolve/extrude → revolve-cut →
   tapped hole → mirror → timeline edit) THEN each step SHALL complete through registered
   commands and the timeline edit SHALL re-evaluate dependents.
2. WHEN the mesh reverse-engineering workflow is run (import mesh → section sketch → clean sketch
   → extrude/revolve/loft/sweep → deviation compare) THEN each step SHALL complete and the
   deviation SHALL be reported.
3. WHEN the design-for-manufacturing workflow is run (user parameters → clearance/combine/split →
   alignment features) THEN parameter edits SHALL propagate and boolean combine with keep-tools
   SHALL behave as specified.
4. WHEN the assembly-and-motion workflow is run (components → joints → joint limits → motion link
   → interference) THEN motion SHALL respect limits and contact, and interference SHALL be reported.
5. WHEN each workflow is validated THEN observable results SHALL be captured as screenshots or a
   recording from a small public-safe reproduction driven through the control channel.
6. WHEN any workflow runs THEN no step SHALL panic; unsupported steps SHALL return
   `"not supported yet: …"` and leave the design in a consistent state.
7. WHEN the reference-canvas spline-revolve workflow is run (insert and calibrate a reference
   canvas image on a plane at 1:1 scale → sketch a centerline and a fit-point spline profile →
   revolve 360° about the axis → shell to a wall thickness → assign a transparent appearance)
   THEN each step SHALL complete through registered commands, and the sketch SHALL reach a
   fully-defined state before the revolve step runs.
8. WHEN the mold-design workflow is run (model a prototype → create a mold box → Combine (Cut)
   with Keep Tools to form the cavity → split into a two-part mold with alignment pins and a pour
   spout) THEN each step SHALL complete through registered commands, no step SHALL panic, and an
   unsupported step SHALL return `"not supported yet: …"`.

---

### Requirement 14: Mold design & design-for-additive-manufacturing (DfAM) · Stage 3-adjacent

**User Story:** As a user preparing parts for casting or 3D printing, I want mold-cavity creation
and print-oriented clearances, so that I can produce a mold from a prototype body and lay out
printable parts with correct fit.

#### Acceptance Criteria

1. WHEN the user runs the mold-box command on a prototype body THEN the system SHALL create a
   mold box body that fully encloses the prototype body.
2. WHEN the user runs a boolean Combine (Cut) with the Keep-Tools option enabled, using the
   prototype body as the tool against the mold box THEN the system SHALL carve the prototype's
   shape as a cavity in the mold box while preserving the prototype body, generalizing the
   combine / keep-tools behaviour referenced in Requirement 13.3.
3. WHEN the user splits the mold body on a parting plane THEN the system SHALL produce an upper
   half and a lower half as separate bodies.
4. WHEN the user adds two-part mold features THEN the system SHALL create alignment-pin features
   at the corners of the parting interface and a pour-spout channel from the outer face into the
   cavity.
5. WHEN the user sets a clearance-gap parameter between mating printed parts (for example a
   print-in-place hinge) THEN the system SHALL apply the gap driven by that user parameter so
   parameter edits propagate to the fit.
6. WHEN the user runs the nesting/batch-layout command on multiple bodies THEN the system SHALL
   arrange the bodies into a single print-run layout.
7. WHEN any mold or DfAM operation runs THEN it SHALL execute through a registered `CommandSpec`,
   and WHEN a requested configuration is not yet supported THEN the system SHALL return
   `"not supported yet: …"`.
8. IF a mold or DfAM operation receives hostile or invalid input THEN the system SHALL return an
   error and leave the design unchanged, never panicking.

---

### Requirement 15: 3D interchange export (public-specification formats) · Stage 2

**User Story:** As a user handing a design to another tool, printer, or game engine, I want to
export bodies through `file.export` into the common public 3D formats, so I can interchange
geometry without depending on any commercial CAD product's proprietary archive.

> Clean-room / independence note (Requirement 1.5): every format below is either already
> shipping, planned from a **public specification**, or **explicitly excluded** with a recorded
> reason. SolidCraft SHALL NOT implement write (or read) support for proprietary vendor CAD /
> DCC scene formats by reverse-engineering, linking a proprietary SDK, or reading GPL sources
> (Blender). Where a format is excluded, the independent equivalent is the closest public
> format in the same row (for example glTF/GLB instead of FBX; STEP instead of Parasolid /
> ACIS / SolidWorks / Fusion / Inventor archives).

#### Acceptance Criteria

1. WHEN the user exports through `file.export` (or the equivalent CLI / MCP `export` tool)
   THEN the system SHALL continue to support the already-shipping body formats: STEP (`.step` /
   `.stp`), IGES (`.iges` / `.igs`), 3MF (`.3mf`), STL binary (`.stl`) and STL ASCII
   (`.stla` / `stl-ascii`), Wavefront OBJ (`.obj`), and the native `.solidcraft` design
   container, preserving existing command ids and round-trip behaviour.
2. WHEN the user exports Wavefront OBJ THEN the system SHALL optionally write a companion
   Material Template Library file (`.mtl`) that references per-body (or per-appearance) base
   colour / opacity derived from SolidCraft appearances, with the OBJ referencing that MTL by
   public Wavefront conventions — never by copying another product's material names.
3. WHEN the user exports a tessellated interchange format THEN the system SHALL write, through
   the same `file.export` path and the existing guarded tessellation, at least: Khronos glTF
   (`.gltf`) and binary glTF (`.glb`), Stanford PLY (`.ply`), ASTM AMF (`.amf`), COLLADA
   (`.dae`), X3D (`.x3d`), VRML 2.0 (`.wrl` / `.vrml`), and VRM (`.vrm`, as a glTF-based
   public extension). Units SHALL remain millimetres internally and be declared in each format
   where the specification defines a unit field; Z-up SHALL be preserved or explicitly
   transformed with a recorded axis convention in the file metadata.
4. WHEN the user exports Universal Scene Description THEN the system SHALL write at least one
   of `.usd`, `.usda`, `.usdc`, or `.usdz` using the **OpenUSD** public specification (or an
   openly licensed non-GPL binding), staged after the tessellated set in criterion 3 if needed;
   unsupported USD features return `"not supported yet: …"` without panicking.
5. WHEN any new export format is added THEN it SHALL live in `crates/io` (L3), be selectable by
   extension through `Format::from_name`, be reachable from `file.export` / CLI / MCP, build for
   `wasm32-unknown-unknown` where the writer is pure Rust (or degrade gracefully on wasm with a
   clear error when a native-only dependency is required), and treat path / size / triangle-count
   inputs as hostile (finite checks, caps, `get()` indexing).
6. IF the design has no exportable bodies, or a format / option is not yet supported, THEN the
   system SHALL return an error (including `"not supported yet: …"` where applicable) and leave
   the design unchanged, never panicking.
7. WHEN a proprietary or closed vendor format is requested THEN the system SHALL refuse it with
   a clear unsupported-format error and SHALL NOT attempt to write it. The following are
   **explicitly excluded** under Requirement 1.5, with the recorded reason and the independent
   public equivalent:

   | Requested | Reason excluded | Independent equivalent |
   |---|---|---|
   | `.fbx` (Filmbox) | Autodesk proprietary; no clean-room public write spec usable without a proprietary SDK | `.gltf` / `.glb` |
   | `.blend` (Blender) | Native Blender format; implementing it via Blender sources would violate the no-GPL rule | `.gltf` / `.glb` or `.obj` |
   | `.max` (3ds Max) | Autodesk proprietary scene | `.gltf` / `.glb` |
   | `.c4d` (Cinema 4D) | Maxon proprietary project | `.gltf` / `.glb` |
   | `.ma` / `.mb` (Maya) | Autodesk proprietary scene | `.gltf` / `.glb` |
   | `.dwg` (AutoCAD Drawing) | Autodesk proprietary binary; DXF is the public exchange form | `.dxf` (already shipping for sketches/drawings) |
   | `.skp` (SketchUp) | Trimble proprietary | `.gltf` / `.glb` or `.obj` |
   | `.sldprt` / `.sldasm` (SolidWorks) | Dassault proprietary; commercial CAD product formats are forbidden | `.step` / `.stp` |
   | `.f3d` (Fusion 360 archive) | Autodesk proprietary; commercial CAD product formats are forbidden | `.solidcraft` or `.step` |
   | `.ipt` / `.iam` (Inventor) | Autodesk proprietary; commercial CAD product formats are forbidden | `.step` / `.stp` |
   | `.sat` (ACIS) | Spatial proprietary kernel format | `.step` / `.stp` |
   | `.x_t` / `.x_b` / Parasolid | Siemens proprietary kernel format | `.step` / `.stp` |

8. WHEN an export completes THEN observable results for new formats SHALL be covered by
   round-trip or structural tests in `crates/io` / `crates/engine` (extending
   `export_formats_tests`), with expected values from analytic geometry or SolidCraft's own
   re-import — never from a commercial CAD product.
