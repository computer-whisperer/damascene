//! Form submission — the tree half of HTML's `submit` event.
//!
//! [`RunnerCore`](crate::runtime::RunnerCore) passes the events of a
//! key press, a pointer release and an assistive-technology action
//! through [`with_submissions`] before handing them to the host. An
//! event that submits a form gets a [`UiEventKind::Submit`] routed to
//! that form right behind it. The rule itself is documented where an
//! author meets it, on [`El::form_owner`].

use crate::event::{UiEvent, UiEventKind, UiTarget};
use crate::state::query::target_in_subtree;
use crate::tree::El;

/// `events` with a `Submit` inserted after each event that submits a
/// form in `tree`.
pub(crate) fn with_submissions(tree: &El, events: Vec<UiEvent>) -> Vec<UiEvent> {
    if !events.iter().any(|e| gesture(e).is_some()) {
        return events;
    }
    let mut out = Vec::with_capacity(events.len() + 1);
    for event in events {
        let submit = submission(tree, &event);
        out.push(event);
        out.extend(submit);
    }
    out
}

/// How an event can ask for a submission, before the tree is
/// consulted.
#[derive(Clone, Copy)]
enum Gesture {
    /// A click or activation: submits when it lands on a
    /// [`El::submit`] control.
    Control,
    /// Plain Enter: submits from a single-line text field (HTML's
    /// implicit submission).
    Enter,
}

fn gesture(event: &UiEvent) -> Option<Gesture> {
    match event.kind {
        UiEventKind::Click | UiEventKind::Activate => Some(Gesture::Control),
        UiEventKind::KeyDown if event.is_plain_enter() => Some(Gesture::Enter),
        _ => None,
    }
}

fn submission(tree: &El, event: &UiEvent) -> Option<UiEvent> {
    let gesture = gesture(event)?;
    let target = event.target.as_ref()?;

    // Walk the path from the root to the event's target, tracking the
    // nearest form and whether a submit control has been passed
    // since. A keyed leaf inside a submit button (its icon, say)
    // submits as the button does.
    let mut form: Option<(&El, &String)> = None;
    let mut in_submit = false;
    let mut node = tree;
    loop {
        if let Some(key) = form_key(node) {
            form = Some((node, key));
            in_submit = false;
        }
        in_submit |= node.submit;
        if node.computed_id == target.node_id {
            break;
        }
        node = node
            .children
            .iter()
            .find(|c| target_in_subtree(&c.computed_id, &target.node_id))?;
    }

    let (form, key) = form?;
    let submits = match gesture {
        Gesture::Control => in_submit,
        Gesture::Enter => is_single_line_text(node) && !default_button_disabled(form),
    };
    submits.then(|| UiEvent {
        kind: UiEventKind::Submit,
        target: Some(UiTarget {
            key: key.clone(),
            node_id: form.computed_id.clone(),
            rect: form.computed_rect,
            tooltip: form.tooltip_text().map(str::to_string),
            scroll_offset_y: 0.0,
            content_inset: form.content_inset(),
            font_size: form.font_size,
            line_height: form.line_height,
        }),
        modifiers: event.modifiers,
        ..UiEvent::synthetic_submit(key.clone())
    })
}

/// The key a submission of `node` is routed to: `node` is a form in
/// the sense of this module when it is a [`El::form_owner`] with a
/// key. An unkeyed form owner is layout only, and transparent here —
/// its controls belong to the nearest keyed form above it.
pub(crate) fn form_key(node: &El) -> Option<&String> {
    node.key.as_ref().filter(|_| node.form_owner)
}

/// A field that submits its form on Enter: one that declares editable
/// text and is not multiline — `<input>`, as against `<textarea>`,
/// where Enter is a line break.
fn is_single_line_text(node: &El) -> bool {
    node.a11y
        .as_deref()
        .and_then(|props| props.text_edit.as_deref())
        .is_some_and(|edit| !edit.multiline)
}

/// HTML's default button is the form's first submit control in tree
/// order, and a disabled one blocks implicit submission: the app
/// disabled the button because the form is not ready, so Enter must
/// not get around it.
fn default_button_disabled(form: &El) -> bool {
    default_button(form).is_some_and(|button| button.a11y.as_deref().is_some_and(|p| p.disabled))
}

/// The first [`El::submit`] control under `node` in tree order. A
/// nested form's controls are its own.
fn default_button(node: &El) -> Option<&El> {
    node.children.iter().find_map(|child| {
        if form_key(child).is_some() {
            None
        } else if child.submit {
            Some(child)
        } else {
            default_button(child)
        }
    })
}
