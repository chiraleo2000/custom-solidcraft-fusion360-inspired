//! Optional sketch coaching. Progress lives on the engine session; this module only draws it.

use egui::{RichText, Stroke, vec2};
use serde_json::json;

use crate::SolveApp;
use crate::theme::Tokens;

struct Step {
    text_key: &'static str,
    anchor: Option<&'static str>,
}

const STEPS: &[Step] = &[
    Step { text_key: "coaching.step_workspace_tabs", anchor: Some("toolbar:tab.SOLID") },
    Step { text_key: "coaching.step_toolbox", anchor: None },
    Step { text_key: "coaching.step_create_sketch", anchor: Some("toolbar:sketch.create") },
    Step { text_key: "coaching.step_draw_line", anchor: Some("toolbar:sketch.line") },
    Step { text_key: "coaching.step_close_profile", anchor: None },
    Step { text_key: "coaching.step_dimension", anchor: Some("toolbar:sketch.dimension") },
    Step { text_key: "coaching.step_fully_defined", anchor: Some("sketch_palette:auto_constrain") },
    Step { text_key: "coaching.step_finish", anchor: Some("sketch_palette:finish") },
    Step { text_key: "coaching.step_extrude", anchor: Some("toolbar:solid.extrude") },
    Step { text_key: "coaching.step_new_component", anchor: Some("toolbar:component.create") },
    Step { text_key: "coaching.step_activate", anchor: None },
];

/// What to draw this frame.
pub struct CoachingView {
    pub text: String,
    pub anchor: Option<&'static str>,
    pub index: usize,
    pub last: bool,
}

pub fn refresh(app: &mut SolveApp) {
    app.coaching = app.session.coaching.clone();
}

/// Enable or disable coaching and keep the mirror in step with the engine.
pub fn toggle(app: &mut SolveApp) {
    let enabled = !app.session.coaching.enabled;
    let _ = app.run("ui.coaching.set", json!({"enabled": enabled, "steps": STEPS.len()}));
    refresh(app);
}

/// A new design with no features shows how to begin, while coaching is enabled.
pub fn new_document_cue_visible(app: &SolveApp) -> bool {
    app.coaching.enabled && app.session.doc.features.is_empty()
}

pub fn coaching_view(app: &SolveApp) -> Option<CoachingView> {
    let pref = &app.coaching;
    if !pref.enabled || pref.dismissed || pref.completed || STEPS.is_empty() {
        return None;
    }
    let index = pref.index.min(STEPS.len() - 1);
    let step = STEPS.get(index)?;
    Some(CoachingView { text: app.tr(step.text_key).to_string(), anchor: step.anchor, index, last: index + 1 >= STEPS.len() })
}

/// Draw the current callout. It covers only its own rectangle.
pub fn show(app: &mut SolveApp, ui: &egui::Ui) {
    refresh(app);
    if new_document_cue_visible(app) {
        let tk = Tokens::get();
        let viewport = ui.max_rect();
        let pos = egui::pos2(viewport.center().x - 140.0, viewport.center().y - 24.0);
        crate::frame::area(egui::Id::new("sc_new_document_cue")).fixed_pos(pos).order(egui::Order::Background).show(ui.ctx(), |ui| {
            ui.label(RichText::new(app.tr("coaching.empty_state_cue")).color(tk.text_dim).size(14.0));
        });
    }
    let Some(view) = coaching_view(app) else { return };
    let tk = Tokens::get();
    let anchor = view.anchor.and_then(crate::scenario::handle_at);
    let viewport = ui.max_rect();
    let pos = anchor
        .map(|p| egui::pos2((p.x + 12.0).min(viewport.right() - 300.0), (p.y + 20.0).min(viewport.bottom() - 110.0)))
        .unwrap_or(egui::pos2((viewport.right() - 300.0).max(viewport.left()), (viewport.bottom() - 130.0).max(viewport.top())));
    let mut advance = false;
    let mut dismiss = false;
    crate::frame::area(egui::Id::new("sc_coaching")).fixed_pos(pos).constrain(true).order(egui::Order::Foreground).show(ui.ctx(), |ui| {
        egui::Frame::new()
            .fill(tk.dialog_bg)
            .stroke(Stroke::new(1.0, tk.dialog_border))
            .inner_margin(egui::Margin::symmetric(10, 8))
            .corner_radius(6.0)
            .show(ui, |ui| {
                ui.set_max_width(280.0);
                ui.label(RichText::new(view.text).color(tk.text).size(13.0));
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    let next = if view.last { app.tr("coaching.close") } else { app.tr("coaching.next") };
                    if ui.button(RichText::new(next).color(tk.text)).clicked() {
                        advance = true;
                    }
                    if ui.button(RichText::new(app.tr("coaching.dismiss")).color(tk.text_dim)).clicked() {
                        dismiss = true;
                    }
                    let icon = if view.last { "coaching_close" } else { "coaching_next" };
                    let (rect, _) = ui.allocate_exact_size(vec2(16.0, 16.0), egui::Sense::hover());
                    crate::icons::paint(ui.painter(), rect, icon, tk.text, tk.dialog_bg, tk.accent);
                    if dismiss {
                        let (rect, _) = ui.allocate_exact_size(vec2(16.0, 16.0), egui::Sense::hover());
                        crate::icons::paint(ui.painter(), rect, "coaching_dismiss", tk.text_dim, tk.dialog_bg, tk.accent_soft);
                    }
                });
            });
        crate::scenario::publish_handle("coaching_callout", ui.min_rect().center());
    });
    if let Some(anchor) = anchor
        && let Some(callout) = crate::scenario::handle_at("coaching_callout")
    {
        ui.painter().line_segment([anchor, callout], Stroke::new(1.0, tk.accent_soft));
    }
    let steps = json!({"steps": STEPS.len()});
    if advance {
        let _ = app.run("ui.coaching.advance", steps);
        refresh(app);
    } else if dismiss {
        let _ = app.run("ui.coaching.dismiss", steps);
        refresh(app);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scenario::Harness;

    /// Feature: ux-ui-shell-improvements, Property 11: The coaching layer presents the step at its current index
    #[test]
    fn property_11_coaching_presents_the_step_at_its_index() {
        let mut h = Harness::new();
        let mut cases = 0usize;
        for n in 0..120 {
            let index = n % STEPS.len();
            h.app.coaching.enabled = true;
            h.app.coaching.dismissed = false;
            h.app.coaching.index = index;
            let view = coaching_view(&h.app).expect("visible");
            assert_eq!(view.index, index);
            assert_eq!(view.text, h.app.tr(STEPS[index].text_key));
            assert_eq!(view.anchor, STEPS[index].anchor);
            assert_ne!(view.text, STEPS[index].text_key, "the step text is translated");
            cases += 1;
        }
        h.app.coaching.dismissed = true;
        assert!(coaching_view(&h.app).is_none());
        h.app.coaching.dismissed = false;
        h.app.coaching.enabled = false;
        assert!(coaching_view(&h.app).is_none());
        assert!(cases >= 100, "property ran {cases} times");
        assert!(STEPS.iter().any(|step| step.text_key == "coaching.step_workspace_tabs"));
        assert!(STEPS.iter().any(|step| step.text_key == "coaching.step_toolbox"));
        assert!(STEPS.iter().any(|step| step.text_key == "coaching.step_extrude"));
        assert!(STEPS.iter().any(|step| step.text_key == "coaching.step_fully_defined"));
        assert!(STEPS.iter().any(|step| step.text_key == "coaching.step_new_component"));
        assert!(STEPS.iter().any(|step| step.text_key == "coaching.step_activate"));
    }

    /// Feature: ux-ui-shell-improvements, Property 25: The new-document cue is shown exactly when the design has no features
    #[test]
    fn property_25_new_document_cue_follows_an_empty_feature_list() {
        let mut empty = Harness::new();
        let mut full = Harness::new();
        full.app.run("solid.box", serde_json::json!({"length": 1, "width": 1, "height": 1})).expect("box");
        let mut cases = 0usize;
        for n in 0..120 {
            let enabled = n % 2 == 0;
            empty.app.coaching.enabled = enabled;
            full.app.coaching.enabled = enabled;
            assert_eq!(new_document_cue_visible(&empty.app), enabled && empty.app.session.doc.features.is_empty());
            assert!(!new_document_cue_visible(&full.app));
            cases += 1;
        }
        assert!(cases >= 100, "property ran {cases} times");
    }
}
