//! The command line / palette: type a command id with JSON parameters
//! (`Extrude {"distance": 20}`), or part of a command name to search (press S anywhere).

use egui::{RichText, vec2};
use serde_json::Value;
use solidcraft_engine::command_specs;

use crate::SolveApp;
use crate::theme::Tokens;

#[derive(Default)]
pub struct Palette {
    pub text: String,
    pub history: Vec<String>,
    focus: bool,
}

/// One palette hit, ordered by how directly it matched.
pub struct Match {
    pub id: &'static str,
    pub label: String,
    pub icon: &'static str,
    pub shortcut: Option<String>,
    pub rank: u8,
}

/// 0 = id or label starts with the query, 1 = contains it, 2 = a synonym contains it.
pub fn rank(q: &str, id: &str, label: &str, synonyms: &[String]) -> Option<u8> {
    let q = q.trim().to_ascii_lowercase();
    if q.is_empty() {
        return Some(0);
    }
    let id_l = id.to_ascii_lowercase();
    let label_l = label.to_ascii_lowercase();
    if id_l.starts_with(&q) || label_l.starts_with(&q) {
        return Some(0);
    }
    if id_l.contains(&q) || label_l.contains(&q) {
        return Some(1);
    }
    if synonyms.iter().any(|term| term.to_ascii_lowercase().contains(&q)) {
        return Some(2);
    }
    None
}

/// Locale synonyms for a command (`palette.synonyms.<id>`), used by the palette and the S-box.
pub(crate) fn synonyms_of(id: &str) -> Vec<String> {
    let key = format!("palette.synonyms.{id}");
    let text = crate::i18n::tr(&key).to_string();
    if text == key {
        return Vec::new();
    }
    text.split(',').map(str::trim).filter(|term| !term.is_empty()).map(str::to_string).collect()
}

/// Commands whose id, localized label, or synonym matches `q`, best match first.
pub fn ranked(app: &SolveApp, q: &str) -> Vec<Match> {
    let mut hits: Vec<Match> = command_specs()
        .into_iter()
        .filter_map(|spec| {
            let label = app.tr_label(spec.label).to_string();
            let rank = rank(q, spec.id, &label, &synonyms_of(spec.id))?;
            Some(Match { id: spec.id, label, icon: spec.icon, shortcut: crate::keymap::effective(app, spec.id), rank })
        })
        .collect();
    hits.sort_by(|a, b| a.rank.cmp(&b.rank).then_with(|| a.label.cmp(&b.label)).then_with(|| a.id.cmp(b.id)));
    hits
}

/// Commands whose id or English label matches the query.
pub fn matches(q: &str) -> Vec<(&'static str, &'static str, &'static str)> {
    let q = q.trim().to_ascii_lowercase();
    let mut v: Vec<(&'static str, &'static str, &'static str)> = command_specs()
        .into_iter()
        .filter(|c| q.is_empty() || c.label.to_ascii_lowercase().contains(&q) || c.id.to_ascii_lowercase().contains(&q))
        .map(|c| (c.id, c.label, c.icon))
        .collect();
    v.sort_by_key(|(id, label, _)| (!label.to_ascii_lowercase().starts_with(&q), !id.to_ascii_lowercase().starts_with(&q), *label));
    v
}

/// Execute a line of palette input.
pub fn submit(app: &mut SolveApp, line: &str) {
    let line = line.trim();
    if line.is_empty() {
        return;
    }
    app.palette.history.push(line.to_string());
    let (id, rest) = match line.find(|c: char| c.is_whitespace() || c == '{') {
        Some(i) => (&line[..i], line[i..].trim()),
        None => (line, ""),
    };
    if solidcraft_engine::find_command(id).is_some() {
        if rest.is_empty() {
            app.start(id);
        } else {
            match serde_json::from_str::<Value>(rest) {
                Ok(p) => {
                    let _ = app.run(id, p);
                }
                Err(e) => app.set_status(format!("parameters must be JSON: {e}"), true),
            }
        }
        return;
    }
    if let Some((id, _, _)) = matches(line).first() {
        app.start(id);
    } else {
        app.set_status(format!("{} `{line}`", app.tr("palette.no_match")), true);
    }
}

pub fn palette_bar(app: &mut SolveApp, ui: &mut egui::Ui) {
    let t = Tokens::get();
    egui::Panel::bottom("sc_palette").exact_size(28.0).frame(egui::Frame::NONE.fill(t.panel).inner_margin(egui::Margin::symmetric(8, 3))).show(
        ui,
        |ui| {
            ui.horizontal_centered(|ui| {
                ui.label(RichText::new("›").strong().color(t.accent));
                let r = ui.add(
                    egui::TextEdit::singleline(&mut app.palette.text)
                        .hint_text("Type a command, e.g. Extrude {\"distance\": 20}  — or press S to search")
                        .desired_width(520.0),
                );
                if app.palette.focus {
                    r.request_focus();
                    app.palette.focus = false;
                }
                if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                    let line = std::mem::take(&mut app.palette.text);
                    submit(app, &line);
                }
                if let Some(last) = app.session.log.last() {
                    ui.label(RichText::new(last).size(11.5).color(t.text_dim));
                }
            });
        },
    );
}

/// The search popup (S).
pub fn popup(app: &mut SolveApp, ctx: &egui::Context) {
    if !app.ui.palette_open {
        return;
    }
    let mut open = true;
    let mut picked: Option<&'static str> = None;
    let t = Tokens::get();
    let center = ctx.content_rect().center();
    crate::frame::window(ctx, "Search commands", crate::frame::Width::Normal)
        .id(egui::Id::new("sc_search"))
        .pivot(egui::Align2::LEFT_TOP)
        .fixed_pos(egui::pos2(center.x - 200.0, 150.0))
        .fixed_size(vec2(400.0, 340.0))
        .collapsible(false)
        .open(&mut open)
        .show(ctx, |ui| {
            let r = ui.add(egui::TextEdit::singleline(&mut app.palette.text).hint_text("command name").desired_width(380.0));
            // Read Enter before taking the focus back (lost_focus reads the focus as it is now).
            let entered = r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            r.request_focus();
            let list = ranked(app, &app.palette.text);
            if entered {
                if let Some(hit) = list.first() {
                    picked = Some(hit.id);
                } else if !app.palette.text.trim().is_empty() {
                    app.set_status(app.tr("palette.no_match"), true);
                }
            }
            egui::ScrollArea::vertical().max_height(280.0).show(ui, |ui| {
                for hit in list.iter().take(60) {
                    let enabled = solidcraft_engine::find_command(hit.id).is_some_and(|c| c.info(&app.session).enabled);
                    ui.horizontal(|ui| {
                        let (ir, _) = ui.allocate_exact_size(vec2(18.0, 18.0), egui::Sense::hover());
                        crate::icons::paint(ui.painter(), ir, hit.icon, t.icon, t.icon_fill, t.accent);
                        if ui.add_enabled(enabled, egui::Button::new(hit.label.as_str()).frame(false)).clicked() {
                            picked = Some(hit.id);
                        }
                        if let Some(key) = &hit.shortcut {
                            ui.label(RichText::new(key).size(10.5).color(t.text_dim));
                        }
                        ui.label(RichText::new(hit.id).size(10.5).color(t.text_dim));
                    });
                }
            });
        });
    if let Some(id) = picked {
        app.palette.text.clear();
        app.ui.palette_open = false;
        app.start(id);
    } else if !open || ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
        app.ui.palette_open = false;
        app.palette.text.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::{rank, ranked, synonyms_of};

    /// Feature: ux-ui-shell-improvements, Property 15: Palette matches are ordered deterministically
    #[test]
    fn property_15_palette_matches_are_ordered() {
        let h = crate::scenario::Harness::new();
        let app = &h.app;
        let queries = ["e", "ex", "line", "sketch", "body", "z", "view", "fit", "trim", "aa"];
        let mut cases = 0usize;
        for n in 0..120 {
            let q = queries[n % queries.len()];
            let a = ranked(app, q);
            let b = ranked(app, q);
            assert_eq!(a.len(), b.len());
            for (left, right) in a.iter().zip(&b) {
                assert_eq!(left.id, right.id);
            }
            for pair in a.windows(2) {
                let order = pair[0].rank < pair[1].rank || (pair[0].rank == pair[1].rank && pair[0].label <= pair[1].label);
                assert!(order, "{q}");
            }
            cases += 1;
        }
        assert!(cases >= 100, "property ran {cases} times");
    }

    /// Feature: ux-ui-shell-improvements, Property 16: A synonym finds its command
    #[test]
    fn property_16_a_synonym_finds_its_command() {
        let h = crate::scenario::Harness::new();
        let app = &h.app;
        let mut cases = 0usize;
        for spec in solidcraft_engine::command_specs() {
            for term in synonyms_of(spec.id) {
                let hit = ranked(app, &term);
                assert!(hit.iter().any(|m| m.id == spec.id), "{} via {term}", spec.id);
                cases += 1;
            }
        }
        assert!(cases >= 1, "a command needs a synonym");
        let _ = rank("thicken", "solid.extrude", "Extrude", &["lengthen, thicken".to_string()]);
    }
}
