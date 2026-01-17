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
