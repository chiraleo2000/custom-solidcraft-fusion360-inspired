# Requirements Document

## Introduction

SolidCraft already has a mature egui desktop shell: a command palette and ribbon, a 3D
viewport with view navigation, a feature timeline, a model/browser tree, a keyboard map,
feature dialogs, interactive sketch tools, a sketch palette, an inference/snap engine,
dimension entry, and sketch display. This feature refines that existing shell so that an
ordinary person — not only an expert — can understand it quickly and work in it fast,
across the whole application rather than inside the sketch surface alone.

The goal is to make the whole app easy for a non-expert to understand and fast to use, as a
*refinement of what already ships* — not a rewrite, and not new workspaces. The work touches
the sketch UX surface (`sketch_tools.rs`, `sketch_palette.rs`, `inference.rs`, `drag_snap.rs`,
`sketch_dims.rs`, `sketch_display.rs`), the whole-app shell surfaces (`palette.rs`,
`toolbar.rs`, `workspace.rs`, `params_dialog.rs`, the `dialogs*.rs` family, `viewport.rs`,
`viewport_layout.rs`, `keymap.rs`, `timeline.rs`, `browser.rs`), and the onboarding/coaching
surface (`coaching.rs`), supported by shell files (`theme.rs`, `help.rs`, `stand_up.rs`,
`home.rs`, `i18n.rs`, `icons.rs`).

Scope note: features that `ROADMAP.md` defers (Surface/FORM, Render, Animation, Simulation,
CAM, PCB, 2D Drawing) are **out of scope** here. **3D file import/export** (STEP, glTF, STL,
proprietary refusals, …) is owned by the separate `fusion360-feature-parity` spec
(Requirement 15) and is likewise out of scope here — this shell may only surface the
`file.export` command and list formats the engine already supports. This spec improves the UX
of what already ships; it does not add new workspaces, modelling capability, or I/O formats.

The original sketch-focused outcomes still define success and are preserved below as
Requirements 1–6:

1. A person using a sketch tool can always tell which tool is active, what it expects next,
   how a snap or inference was chosen, and how to confirm or exit.
2. A new or returning person can opt into a non-blocking, dismissible, step-through coaching
   layer, and can disable or re-enable that coaching at any time.

This feature adds whole-shell outcomes as Requirements 7–13:

3. A person can discover and reach any existing command quickly through the command palette
   and the ribbon, by name or synonym, with its shortcut shown.
4. Every feature dialog behaves consistently: a predictable layout, confirm/cancel actions
   kept reachable, units shown, a live preview, and legible errors.
5. A person can move the 3D view with confidence through standard views, orbit/pan/zoom,
   zoom-to-fit, and a return-to-home control, all driven by SolidCraft commands.
6. A person can work keyboard-first: every command carries a shortcut sourced from its
   `CommandSpec`, shortcuts are searchable, rebindable, clearable, and conflict-aware, and
   reserved keys are protected.
7. A person can read the feature timeline: feature order, re-open a feature to edit it,
   hover highlighting, and clear error/suppressed state.
8. A person can read the browser/model tree: component/body/sketch hierarchy, visibility
   toggles, sketch constraint state, and the active component.
9. A first-time or returning person is greeted with app-level onboarding that covers the main
   workflow — the empty state of a new document and the workspace tabs — not sketch alone.

All behaviour is delivered through the project's command-driven, thin-UI, clean-room
conventions. Those conventions are captured as explicit constraints in the Non-Functional
Requirements and Constraints section and apply to every requirement in this document.

## Glossary

- **SolidCraft**: The parametric 3D CAD application that owns the UX and shell.
- **Shell_UX**: The whole egui UI crate (L5) surface a person interacts with, comprising the
  command palette, ribbon/toolbar, feature dialogs, 3D viewport and view navigation, keyboard
  map, feature timeline, browser/model tree, sketch surface, and onboarding/coaching layer.
- **Sketch_UX**: The interactive sketch surface in the egui UI crate (L5), comprising the
  sketch tools, sketch palette, inference/snap rendering, dimension entry, and sketch display.
- **Active_Tool**: The sketch command currently started interactively (via `app.start(id)`),
  identified by its own dotted lower-case command id (for example `sketch.rectangle.two_point`).
- **Tool_Feedback**: The visible indication of the active tool, its name, the next input it
  expects, and the keys or clicks that confirm or exit it.
- **Inline_Prompt**: A short, in-context status cue shown during tool use (for example via the
  existing status line), describing the current step of the Active_Tool.
- **Inference_Engine**: The existing logic (`inference.rs`) that proposes geometric
  relationships (horizontal, vertical, coincident, tangent, parallel, perpendicular, and
  similar) while a sketch tool is active.
- **Snap**: A cursor adjustment produced by `drag_snap.rs` that attaches the cursor to a
  specific geometric target (endpoint, midpoint, grid, intersection, and similar).
- **Snap_Indicator**: The visible marker and label that show which Snap or inference the cursor
  is currently attached to.
- **Dimension_Entry**: The sketch dimension input surface (`sketch_dims.rs`) through which a
  person types or edits a numeric dimension value.
- **Empty_Sketch**: A sketch that contains no sketch geometry.
- **Empty_State_Cue**: A discovery hint shown on an Empty_Sketch that points to how to begin.
- **Coaching_Layer**: The onboarding/coaching UI surface (`coaching.rs`) that presents
  step-through guidance, callouts, and empty-state cues over the Shell_UX.
- **Coaching_Step**: A single unit of guidance in the Coaching_Layer, with text and an optional
  callout anchored to a UI target.
- **Coaching_Preference**: The persisted setting that records whether the Coaching_Layer is
  enabled, and the progress a person has made through it.
- **Command**: A `CommandSpec` registered in `crates/engine/src/cmd/*` with a SolidCraft dotted
  lower-case id of the form `<area>.<thing>[.<variant>]` (for example `solid.extrude`,
  `view.save`, `ui.coaching.set`), reached through `app.run(id, params)` or `app.start(id)`.
- **Command_Palette**: The whole-app command surface (`palette.rs`): the palette bar and the
  search popup opened with the `S` key, which match Commands by id or label.
- **Ribbon**: The tabbed toolbar/panel surface (`toolbar.rs`, `workspace.rs`) that presents
  Commands grouped by workspace tab and panel.
- **Command_Entry**: A single Command's representation inside the Command_Palette or Ribbon,
  carrying its label, icon, and — where bound — its shortcut.
- **Feature_Dialog**: A parametric feature input surface (the `dialogs*.rs` family and
  `params_dialog.rs`) through which a person sets a Command's parameters before it runs.
- **Confirm_Cancel_Controls**: The actions in a Feature_Dialog that commit the Command or
  abandon it.
- **Live_Preview**: The provisional model result a Feature_Dialog shows for the current
  parameters before the Command is committed.
- **Viewport**: The 3D view surface (`viewport.rs`, `viewport_layout.rs`) including its
  navigation bar and axis view (ViewCube/axis-view corner).
- **Standard_View**: A named camera orientation reachable from the Shell_UX — top, front,
  right, home, or isometric — each applied through a SolidCraft Command.
- **View_Navigation**: Orbit, pan, zoom, zoom-to-fit, and return-to-home actions on the
  Viewport camera, each applied through a SolidCraft Command.
- **Keymap**: The keyboard-shortcut map (`keymap.rs`) that binds a key to a Command, sourced
  from each `CommandSpec`.
- **Reserved_Key**: A key the Keymap protects from rebinding because the Shell_UX relies on it.
- **Shortcut_Conflict**: A state in which one key would be bound to more than one Command.
- **Timeline**: The feature-history surface (`timeline.rs`) that shows Features in order.
- **Feature**: A single parametric step in the design's history, shown in the Timeline and the
  Browser.
- **Feature_State**: A Feature's status — ordinary, error, or suppressed.
- **Browser**: The model-tree surface (`browser.rs`) that shows the component/body/sketch
  hierarchy.
- **Browser_Node**: A single row in the Browser representing a component, body, sketch, or
  other model item.
- **Active_Component**: The component into which newly created geometry is placed, indicated in
  the Browser.
- **Sketch_Constraint_State**: Whether a sketch is under-defined, or fully-defined/locked.
- **New_Document_Empty_State**: The state of a freshly created design that has no Features.
- **Workspace_Tab**: A ribbon tab that groups Commands for a kind of work (for example SOLID,
  SKETCH, SHEET METAL, PLASTIC).
- **Theme_Tokens**: The colour and style values exposed by `theme::Tokens`.
- **Locale_Catalog**: The i18n string catalog accessed through `app.tr_label(...)` /
  `i18n::tr_label`, with entries for `en`, `ja`, `de`, and `ko`.

## Requirements

### Requirement 1: Active sketch-tool feedback

**User Story:** As a person sketching, I want to always see which tool is active and what it
expects next, so that I never lose track of the current action.

#### Acceptance Criteria

1. WHILE an Active_Tool is running, THE Sketch_UX SHALL display the name of the Active_Tool.
2. WHILE an Active_Tool is running, THE Sketch_UX SHALL display the next input the Active_Tool
   expects.
3. WHILE an Active_Tool is running, THE Sketch_UX SHALL display the action that confirms the
   current step and the action that exits the Active_Tool.
4. WHEN the Active_Tool advances from one input step to the next, THE Sketch_UX SHALL update the
   displayed expected input to describe the new step.
5. WHEN no Active_Tool is running, THE Sketch_UX SHALL hide the active-tool feedback.
6. WHEN the Active_Tool is exited, THE Sketch_UX SHALL remove the active-tool feedback within one
   rendered frame.

### Requirement 2: Inline prompts during sketch tool use

**User Story:** As a person sketching, I want inline status prompts while a tool runs, so that I
know what to do at each step without consulting a manual.

#### Acceptance Criteria

1. WHEN an Active_Tool starts, THE Sketch_UX SHALL display an Inline_Prompt describing the first
   input step of the Active_Tool.
2. WHEN the Active_Tool reaches a new input step, THE Sketch_UX SHALL replace the Inline_Prompt
   with text describing the new input step.
3. WHERE a sketch Command exposes a params documentation string, THE Sketch_UX SHALL source the
   Inline_Prompt text from a Locale_Catalog entry rather than from a hardcoded literal.
4. IF the Active_Tool completes successfully, THEN THE Sketch_UX SHALL clear the Inline_Prompt.
5. IF the Active_Tool reports an error, THEN THE Sketch_UX SHALL display the error text through
   the status line and SHALL mark the status as an error.

### Requirement 3: Snap and inference legibility

**User Story:** As a person sketching, I want to see and understand each snap and inference, so
that I can trust why the cursor moved and which relationship will be created.

#### Acceptance Criteria

1. WHILE the cursor is attached to a Snap, THE Sketch_UX SHALL render a Snap_Indicator at the
   snapped point.
2. WHILE the cursor is attached to a Snap, THE Sketch_UX SHALL display a label naming the type
   of the Snap.
3. WHILE the Inference_Engine proposes a geometric relationship, THE Sketch_UX SHALL render a
   marker naming the proposed relationship.
4. WHEN more than one inference applies to the current cursor position, THE Sketch_UX SHALL
   render a marker for each applicable inference.
5. WHEN the cursor leaves a Snap or inference target, THE Sketch_UX SHALL remove that target's
   Snap_Indicator within one rendered frame.
6. THE Sketch_UX SHALL draw every Snap_Indicator and inference marker using colours obtained
   from Theme_Tokens.

### Requirement 4: Dimension entry ergonomics

**User Story:** As a person sketching, I want to type and edit dimension values smoothly, so
that I can set exact sizes without fighting the input.

#### Acceptance Criteria

1. WHEN a Dimension_Entry becomes active, THE Sketch_UX SHALL focus the numeric input so that
   typed characters enter the dimension value.
2. WHEN a person confirms a typed value in a Dimension_Entry, THE Sketch_UX SHALL run the
   corresponding sketch dimension Command with the typed value as a parameter.
3. WHEN a person selects an existing dimension to edit, THE Sketch_UX SHALL populate the
   Dimension_Entry with the current value of that dimension.
4. IF a person confirms a Dimension_Entry whose value is not a finite number, THEN THE
   Sketch_UX SHALL reject the entry and SHALL display an explanatory status message.
5. WHEN a person cancels a Dimension_Entry, THE Sketch_UX SHALL leave the existing dimension
   value unchanged.

### Requirement 5: Empty-state and discovery cues for a new sketch

**User Story:** As a person who just created a sketch, I want a cue showing how to begin, so
that an empty canvas does not leave me guessing.

#### Acceptance Criteria

1. WHILE the current sketch is an Empty_Sketch and no Active_Tool is running, THE Sketch_UX
   SHALL display an Empty_State_Cue describing how to start drawing.
2. WHEN the first sketch geometry is added to an Empty_Sketch, THE Sketch_UX SHALL hide the
   Empty_State_Cue.
3. WHEN a person starts an Active_Tool on an Empty_Sketch, THE Sketch_UX SHALL hide the
   Empty_State_Cue for the duration of the Active_Tool.
4. THE Sketch_UX SHALL source the Empty_State_Cue text from a Locale_Catalog entry.

### Requirement 6: Guided coaching layer

**User Story:** As a new or returning person, I want an optional step-through coaching layer, so
that I can learn the sketch workflow without it blocking my work.

#### Acceptance Criteria

1. WHILE the Coaching_Layer is enabled, THE Coaching_Layer SHALL present the current
   Coaching_Step with its text and, where defined, a callout anchored to its UI target.
2. WHEN a person advances the Coaching_Layer, THE Coaching_Layer SHALL present the next
   Coaching_Step.
3. WHEN a person dismisses the current Coaching_Step, THE Coaching_Layer SHALL hide that
   Coaching_Step without disabling the Coaching_Layer.
4. WHILE the Coaching_Layer is displayed, THE Coaching_Layer SHALL leave every Sketch_UX
   interaction reachable so that the person can continue working.
5. WHEN a person disables the Coaching_Layer, THE SolidCraft SHALL store the disabled state in
   the Coaching_Preference through the engine, document, or configuration layer.
6. WHEN a person re-enables the Coaching_Layer, THE SolidCraft SHALL store the enabled state in
   the Coaching_Preference through the engine, document, or configuration layer.
7. WHEN SolidCraft starts with the Coaching_Preference set to disabled, THE Coaching_Layer SHALL
   remain hidden until a person re-enables it.
8. WHEN the final Coaching_Step is dismissed or advanced past, THE Coaching_Layer SHALL mark the
   coaching sequence complete in the Coaching_Preference.
9. THE Coaching_Layer SHALL source every displayed string from a Locale_Catalog entry.
10. THE Coaching_Layer SHALL draw its callouts and surfaces using colours obtained from
    Theme_Tokens.

### Requirement 7: Command discoverability across the app

**User Story:** As a person who does not remember exact command names, I want to find and run
any existing command by searching for a word or a synonym, so that I can act quickly without
hunting through menus.

#### Acceptance Criteria

1. WHEN a person opens the Command_Palette search popup with the `S` key, THE Command_Palette
   SHALL present an input focused for typing.
2. WHEN a person types a query into the Command_Palette, THE Command_Palette SHALL list every
   Command whose id or label contains the query, matched case-insensitively.
3. WHEN the Command_Palette lists matches for a query, THE Command_Palette SHALL order a
   Command whose label or id starts with the query before a Command that only contains the
   query.
4. WHERE a Command carries a synonym term in the Locale_Catalog, THE Command_Palette SHALL
   include that Command in the matches for a query that matches the synonym.
5. WHERE a Command has a Keymap binding, THE Command_Palette and THE Ribbon SHALL display that
   Command's shortcut on its Command_Entry.
6. WHEN a person activates a Command_Entry that takes no parameters, THE Shell_UX SHALL start
   that Command through `app.start(id)`.
7. IF a person submits palette text that matches no Command, THEN THE Command_Palette SHALL
   display an explanatory status message and SHALL leave the design unchanged.
8. THE Command_Palette and THE Ribbon SHALL source every Command label and synonym from a
   Locale_Catalog entry.

### Requirement 8: Consistent feature-dialog ergonomics

**User Story:** As a person configuring a feature, I want every dialog to look and behave the
same way, so that I can set values confidently without relearning each dialog.

#### Acceptance Criteria

1. WHILE a Feature_Dialog is open, THE Feature_Dialog SHALL keep its Confirm_Cancel_Controls
   outside the scrolling region so that both actions stay reachable at every window height.
2. WHERE a Feature_Dialog field holds a dimensioned value, THE Feature_Dialog SHALL display the
   unit of that field.
3. WHILE a Feature_Dialog holds parameters that produce a valid result, THE Feature_Dialog
   SHALL show a Live_Preview of that result before the Command is committed.
4. WHEN a person confirms a Feature_Dialog, THE Shell_UX SHALL run the corresponding Command
   with the dialog's parameters through `app.run(id, params)`.
5. WHEN a person cancels a Feature_Dialog, THE Shell_UX SHALL discard the dialog's parameters
   and SHALL leave the design unchanged.
6. IF a person enters a field value that the Command rejects, THEN THE Feature_Dialog SHALL
   display an explanatory message identifying the field and SHALL keep the Feature_Dialog open.
7. THE Feature_Dialog SHALL obtain every colour it draws from Theme_Tokens and SHALL source
   every label and message from a Locale_Catalog entry.

### Requirement 9: Understandable view navigation

**User Story:** As a person inspecting a model, I want standard views and clear orbit, pan,
zoom, fit, and home controls, so that I can look at my design from any angle without getting
lost.

#### Acceptance Criteria

1. THE Viewport SHALL present controls to apply each Standard_View — top, front, right, home,
   and isometric.
2. WHEN a person activates a Standard_View control, THE Shell_UX SHALL apply that Standard_View
   by running a SolidCraft Command rather than by mutating camera state outside a Command.
3. WHEN a person requests zoom-to-fit, THE Shell_UX SHALL run a SolidCraft Command that frames
   the visible model within the Viewport.
4. WHEN a person activates the return-to-home control, THE Shell_UX SHALL apply the home
   Standard_View.
5. THE Viewport SHALL present orbit, pan, and zoom controls for View_Navigation.
6. IF a View_Navigation Command receives a view name that is not a Standard_View, THEN THE
   Shell_UX SHALL reject the request and SHALL leave the camera unchanged.
7. THE Viewport navigation controls and axis view SHALL draw every colour from Theme_Tokens and
   SHALL source every label from a Locale_Catalog entry.

### Requirement 10: Keyboard-first command flow

**User Story:** As a person who prefers the keyboard, I want every command to have a learnable,
adjustable shortcut, so that I can work fast and resolve any conflicts myself.

#### Acceptance Criteria

1. THE Keymap SHALL source each Command's default shortcut from that Command's `CommandSpec`.
2. WHEN a person presses a bound key, THE Shell_UX SHALL run the Command that the Keymap binds
   to that key.
3. WHEN a person rebinds a Command to a key, THE Keymap SHALL store the new binding and SHALL
   apply it to subsequent key presses.
4. WHEN a person clears a Command's binding, THE Keymap SHALL remove that binding so that the
   key triggers no Command.
5. IF a person rebinds a Command to a key already bound to a different Command, THEN THE Keymap
   SHALL report the Shortcut_Conflict and SHALL require the person to confirm replacing the
   existing binding before storing the new binding.
6. IF a person attempts to rebind a Reserved_Key, THEN THE Keymap SHALL reject the rebinding and
   SHALL keep the Reserved_Key available to the Shell_UX.
7. WHEN a person resets the Keymap, THE Keymap SHALL restore every Command's shortcut to its
   `CommandSpec` default.
8. THE Keymap page SHALL let a person search shortcuts by Command label or id so that a
   shortcut can be found without scrolling the whole list.

### Requirement 11: Readable feature timeline

**User Story:** As a person editing a parametric model, I want to read and revisit my feature
history, so that I can understand how the model was built and change earlier steps.

#### Acceptance Criteria

1. THE Timeline SHALL display every Feature of the current design in the order the Features
   were created.
2. WHEN a person double-clicks a Feature in the Timeline, THE Shell_UX SHALL re-open that
   Feature for editing through its Command.
3. WHEN a person hovers a Feature in the Timeline, THE Shell_UX SHALL highlight the geometry
   that the hovered Feature produces in the Viewport.
4. WHILE a Feature is in the error Feature_State, THE Timeline SHALL mark that Feature as an
   error using a colour obtained from Theme_Tokens.
5. WHILE a Feature is in the suppressed Feature_State, THE Timeline SHALL mark that Feature as
   suppressed distinctly from an ordinary Feature.
6. THE Timeline SHALL source every Feature label and status text from a Locale_Catalog entry.

### Requirement 12: Browser that communicates model state

**User Story:** As a person managing a model, I want the browser tree to show my components,
bodies, and sketches with their state, so that I can control visibility and know where new
work goes.

#### Acceptance Criteria

1. THE Browser SHALL display the component, body, and sketch hierarchy of the current design as
   Browser_Nodes.
2. WHEN a person toggles the visibility control on a Browser_Node, THE Shell_UX SHALL run a
   SolidCraft Command that shows or hides the item the Browser_Node represents.
3. WHILE a sketch is in the under-defined Sketch_Constraint_State, THE Browser SHALL mark that
   sketch distinctly from a sketch in the fully-defined or locked Sketch_Constraint_State.
4. THE Browser SHALL indicate which component is the Active_Component.
5. WHEN a person activates a different component in the Browser, THE Shell_UX SHALL run a
   SolidCraft Command that makes that component the Active_Component.
6. THE Browser SHALL source every Browser_Node label and state indicator text from a
   Locale_Catalog entry and SHALL obtain every colour it draws from Theme_Tokens.

### Requirement 13: App-level first-run onboarding

**User Story:** As a first-time or returning person, I want onboarding that covers the whole
app, so that I know how to begin a design and move between workspaces, not just how to sketch.

#### Acceptance Criteria

1. WHILE the current design is in the New_Document_Empty_State, THE Coaching_Layer SHALL display
   an empty-state cue describing how to begin a design.
2. THE Coaching_Layer SHALL include at least one Coaching_Step that introduces the
   Workspace_Tab navigation of the Ribbon.
3. THE Coaching_Layer SHALL include at least one Coaching_Step that covers the main modelling
   workflow beyond the sketch surface.
4. WHEN the first Feature is added to a design in the New_Document_Empty_State, THE
   Coaching_Layer SHALL hide the New_Document_Empty_State cue.
5. WHILE the app-level onboarding is displayed, THE Coaching_Layer SHALL leave every Shell_UX
   interaction reachable so that the person can continue working.
6. WHEN a person disables the app-level onboarding, THE SolidCraft SHALL store the disabled
   state in the Coaching_Preference through the engine, document, or configuration layer.
7. THE Coaching_Layer SHALL source every app-level onboarding string from a Locale_Catalog
   entry and SHALL obtain every colour it draws from Theme_Tokens.

## Non-Functional Requirements and Constraints

These constraints are non-negotiable and apply to every requirement above.

### Clean-room provenance

1. THE SolidCraft SHALL derive the design of every feature in this document from public
   documentation and general CAD conventions only, including any "modern CAD feel", which SHALL
   be an independent design and SHALL NOT be a copy of another product.
2. THE SolidCraft SHALL NOT incorporate code, user-interface layouts, icons, artwork, tooltips,
   help text, templates, command identifiers, or keyboard shortcuts sourced from FreeCAD,
   OpenCASCADE, planegcs, SolveSpace, LibreCAD, QCAD, Fusion 360, SolidWorks, Onshape, any
   other commercial CAD product, or any local copy of those products such as a local FreeCAD
   installation.
3. THE SolidCraft SHALL NOT introduce any Command id or Command label that is the name of
   another CAD product or that reproduces another product's command identifier.

### Command-driven behaviour and thin UI

4. THE SolidCraft SHALL expose every user-visible behaviour as a Command in
   `crates/engine/src/cmd/*` identified by a SolidCraft dotted lower-case id of the form
   `<area>.<thing>[.<variant>]`.
5. THE Shell_UX SHALL invoke behaviour only through `app.run(id, params)` or `app.start(id)`.
6. WHEN behaviour runs through `app.run(id, params)`, THE SolidCraft SHALL run the Command
   without opening a dialog.
7. WHEN a person starts an interactive tool or dialog through a Ribbon or Viewport control via
   `app.start(id)`, THE Shell_UX SHALL run the resulting behaviour through the same Command the
   programmatic path uses.
8. THE SolidCraft SHALL persist Coaching_Preference and Keymap state through the engine,
   document, or configuration layer rather than through state held only in the view.

### Visual assets and theming

9. THE Shell_UX SHALL render every icon used by this feature from code in `icons.rs`.
10. WHERE this feature introduces a file-based asset, THE SolidCraft SHALL record that asset in a
    row of ATTRIBUTION.md.
11. THE Shell_UX SHALL obtain every colour used by this feature from Theme_Tokens.

### Internationalisation

12. THE SolidCraft SHALL expose every new user-facing string of this feature through the
    `app.tr_label(...)` / Locale_Catalog i18n pattern, with entries for the `en`, `ja`, `de`,
    and `ko` locales.

### Reliability

13. THE SolidCraft SHALL return a `Result<T, E>` and propagate errors with `?` for every
    fallible operation added by this feature in non-test code.
14. THE SolidCraft SHALL NOT use `unwrap`, `expect`, `panic!`, `unreachable!`, `todo!`,
    `unimplemented!`, or `unsafe` in the non-test code added by this feature.
15. IF a feature path added by this feature cannot complete an operation, THEN THE SolidCraft
    SHALL return an error rather than terminate.

### Portability and layering

16. THE SolidCraft SHALL build every crate below the applications, including code added by this
    feature below L5, for the `wasm32-unknown-unknown` target.
17. THE SolidCraft SHALL keep every egui, eframe, winit, wgpu, and rfd dependency of this feature
    within the L5 UI crate and SHALL NOT introduce any such dependency into a crate below L5.
