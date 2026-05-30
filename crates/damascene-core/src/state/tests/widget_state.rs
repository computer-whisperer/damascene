use super::support::*;

#[derive(Default, Debug)]
struct TestCaret {
    position: usize,
    blink_phase: f32,
}
impl WidgetState for TestCaret {
    fn debug_summary(&self) -> String {
        format!("pos={} blink={:.2}", self.position, self.blink_phase)
    }
}

#[test]
fn widget_state_lazy_inserts_default_and_persists_mutations() {
    let mut state = UiState::new();
    // First call inserts the default.
    let caret = state.widget_state_mut::<TestCaret>("input.0");
    assert_eq!(caret.position, 0);
    caret.position = 7;
    caret.blink_phase = 0.5;
    // Second call returns the same instance.
    let caret = state.widget_state::<TestCaret>("input.0").expect("present");
    assert_eq!(caret.position, 7);
    assert!((caret.blink_phase - 0.5).abs() < f32::EPSILON);
    // Different id → independent storage.
    assert!(state.widget_state::<TestCaret>("input.1").is_none());
}

#[test]
fn widget_state_summary_surfaces_debug_for_tree_dump() {
    let mut state = UiState::new();
    let caret = state.widget_state_mut::<TestCaret>("input.0");
    caret.position = 12;
    caret.blink_phase = 0.25;
    let summary = state.widget_state_summary("input.0");
    assert_eq!(summary.len(), 1);
    let (type_name, debug) = &summary[0];
    assert!(type_name.ends_with("TestCaret"));
    assert_eq!(debug, "pos=12 blink=0.25");
}

#[test]
fn widget_state_gc_when_node_leaves_tree() {
    let (mut tree_a, mut state) = lay_out_counter();
    let inc_id = find_id(&tree_a, "inc").expect("inc id");
    // Seed widget_state on the inc button.
    state.widget_state_mut::<TestCaret>(&inc_id).position = 99;
    state.tick_visual_animations(&mut tree_a, Instant::now(), &Palette::default());
    assert!(state.widget_state::<TestCaret>(&inc_id).is_some());

    // Rebuild without inc. The GC sweep on the next tick should drop it.
    let mut tree_b = column([crate::text("0"), row([button("-").key("dec")])]).padding(20.0);
    layout(&mut tree_b, &mut state, Rect::new(0.0, 0.0, 400.0, 200.0));
    state.tick_visual_animations(&mut tree_b, Instant::now(), &Palette::default());
    assert!(
        state.widget_state::<TestCaret>(&inc_id).is_none(),
        "stale widget_state for inc was not GC'd"
    );
}
