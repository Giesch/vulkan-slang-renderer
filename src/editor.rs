//! Editor widget wrapper types for reflection-based UI generation.
//!
//! These types encode both value and metadata (like ranges) so that
//! the facet_egui reflection system can render appropriate widgets.

use egui::Ui;
use facet::Facet;

/// A value edited via egui::Slider with a defined range.
#[derive(Clone, Debug, Facet)]
pub struct Slider {
    pub value: f32,
    pub min: f32,
    pub max: f32,
}

impl Slider {
    pub fn new(value: f32, min: f32, max: f32) -> Self {
        Self { value, min, max }
    }

    /// Render this slider in egui, returning true if the value changed.
    pub fn render_ui(&mut self, ui: &mut Ui) -> bool {
        let response = ui.add(egui::Slider::new(&mut self.value, self.min..=self.max));
        response.changed()
    }
}

/// A boolean toggle edited via egui::Checkbox.
#[derive(Clone, Debug, Facet)]
pub struct Checkbox {
    pub checked: bool,
}

impl Checkbox {
    pub fn new(checked: bool) -> Self {
        Self { checked }
    }

    /// Render this checkbox in egui, returning true if the value changed.
    pub fn render_ui(&mut self, ui: &mut Ui) -> bool {
        ui.checkbox(&mut self.checked, "").changed()
    }
}

/// A radio button group for selecting one of several options.
#[derive(Clone, Debug, Facet)]
pub struct RadioButton {
    pub selected: usize,
    pub labels: Vec<String>,
}

impl RadioButton {
    pub fn new(labels: &[&str]) -> Self {
        Self {
            selected: 0,
            labels: labels.iter().map(|s| s.to_string()).collect(),
        }
    }

    /// Render this radio button group in egui, returning true if the selection changed.
    pub fn render_ui(&mut self, ui: &mut Ui) -> bool {
        let mut changed = false;
        for (i, label) in self.labels.iter().enumerate() {
            if ui.radio_value(&mut self.selected, i, label).changed() {
                changed = true;
            }
        }
        changed
    }
}

/// Convert a PascalCase name to a display string with spaces.
/// e.g. `WetAreaMask` → `"Wet Area Mask"`
pub fn pascal_to_display(name: &str) -> String {
    let mut result = String::with_capacity(name.len() + 4);
    for (i, ch) in name.chars().enumerate() {
        if i > 0 && ch.is_uppercase() {
            result.push(' ');
        }
        result.push(ch);
    }
    result
}

/// A read-only text label for displaying values in the editor UI.
#[derive(Clone, Debug, Facet)]
pub struct Label {
    pub text: String,
}

impl Label {
    pub fn new(text: impl Into<String>) -> Self {
        Self { text: text.into() }
    }

    pub fn set(&mut self, text: impl Into<String>) {
        self.text = text.into();
    }

    /// Render this label in egui. Always returns false (labels are read-only).
    pub fn render_ui(&self, ui: &mut Ui) {
        ui.label(&self.text);
    }
}
