//! Typed widget-state bucket accessors.

use std::any::TypeId;

use super::{UiState, WidgetState};

impl UiState {
    /// Look up the widget state of type `T` for `id`. Returns `None` if
    /// no entry exists or the entry was inserted as a different type.
    pub fn widget_state<T: WidgetState>(&self, id: &str) -> Option<&T> {
        self.widget_states
            .entries
            .get(&(id.to_string(), TypeId::of::<T>()))
            .and_then(|b| b.as_any().downcast_ref::<T>())
    }

    /// Get a mutable reference to the widget state of type `T` for
    /// `id`, inserting `T::default()` if no entry exists. Use this in
    /// the build closure of a stateful widget so the first call after
    /// the node enters the tree produces a fresh state, and every
    /// subsequent call returns the live one.
    pub fn widget_state_mut<T: WidgetState + Default>(&mut self, id: &str) -> &mut T {
        let key = (id.to_string(), TypeId::of::<T>());
        let entry = self
            .widget_states
            .entries
            .entry(key)
            .or_insert_with(|| Box::new(T::default()));
        entry
            .as_any_mut()
            .downcast_mut::<T>()
            .expect("widget_state TypeId match guarantees downcast succeeds")
    }

    /// Drop the widget state of type `T` for `id`, if any.
    pub fn clear_widget_state<T: WidgetState>(&mut self, id: &str) {
        self.widget_states
            .entries
            .remove(&(id.to_string(), TypeId::of::<T>()));
    }

    /// Iterate `(id, type_name, debug_summary)` for every live widget
    /// state. Used by the tree dump to surface per-widget state in the
    /// agent loop's view.
    pub fn widget_state_summary(&self, id: &str) -> Vec<(&'static str, String)> {
        self.widget_states
            .entries
            .iter()
            .filter(|((node_id, _), _)| node_id == id)
            .map(|(_, b)| (b.type_name(), b.debug_summary()))
            .collect()
    }
}
