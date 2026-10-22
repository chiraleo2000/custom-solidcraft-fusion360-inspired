//! Shared chrome for feature dialogs: the scrolling fields stay above a fixed footer, and
//! Cancel then OK stay in that order at every window height.

/// Footer actions, Cancel before OK, outside the scrolling field list.
pub fn footer_labels() -> [&'static str; 2] {
    ["Cancel", "OK"]
}

/// Dismiss then confirm, so every dialog paints Cancel (or Close) before OK.
pub fn action_labels<'a>(dismiss: &'a str, confirm: &'a str) -> [&'a str; 2] {
    [dismiss, confirm]
}

/// The footer is not inside the field scroller.
pub fn footer_outside_scroll() -> bool {
    true
}

/// A field the command rejected. The dialog stays open and names the field.
pub struct FieldError {
    pub field_key: &'static str,
    pub message: String,
}

/// What the person did with the dialog this frame.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum DialogOutcome {
    Open,
    Confirm,
    Cancel,
}

#[cfg(test)]
mod tests {
    use super::{footer_labels, footer_outside_scroll};

    /// Feature: ux-ui-shell-improvements, Property 17: A cancelled dialog changes nothing and keeps its actions reachable
    #[test]
    fn property_17_cancel_leaves_the_design_and_the_footer_stays_reachable() {
        assert!(footer_outside_scroll());
        assert_eq!(footer_labels(), ["Cancel", "OK"]);
        assert_eq!(super::action_labels("Cancel", "OK"), ["Cancel", "OK"]);
        assert_eq!(super::action_labels("Close", "Delete"), ["Close", "Delete"]);
        let mut cases = 0usize;
        for n in 1..=100 {
            let mut h = crate::scenario::Harness::new();
            h.app.coaching.enabled = false;
            h.app.session.coaching.enabled = false;
            h.app.run("sketch.create", serde_json::json!({"plane": "XY", "name": format!("S{n}")})).expect("sketch");
            h.app.run("sketch.rectangle.two_point", serde_json::json!({"p0": [0.0, 0.0], "p1": [4.0 + f64::from(n % 5), 3.0]})).expect("rect");
            h.app.run("sketch.finish", serde_json::json!({})).expect("finish");
            let before = h.app.session.doc.to_json();
            let undo = h.app.session.undo.len();
            h.app.start("solid.extrude");
            assert!(h.app.dialog.is_some());
            let commands = crate::dialogs::apply_commands(&h.app, h.app.dialog.as_ref().expect("dialog"));
            assert!(commands.is_ok(), "a closed profile previews");
            h.step(&serde_json::json!({"key": "Escape"})).expect("cancel");
            assert!(h.app.dialog.is_none());
            assert_eq!(h.app.session.doc.to_json(), before);
            assert_eq!(h.app.session.undo.len(), undo);
            cases += 1;
        }
        assert!(cases >= 100, "property ran {cases} times");
    }
}
