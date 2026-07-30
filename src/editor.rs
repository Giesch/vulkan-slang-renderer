//! Editor widget wrapper types for reflection-based UI generation.
//!
//! These types encode both value and metadata (like ranges) so that
//! the facet_egui reflection system can render appropriate widgets.

use egui::Ui;
use facet::Facet;
use glam::Vec3;

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

/// An integer value edited via egui::Slider with an inclusive range.
/// Use this instead of [`Slider`] for values that index something, so the
/// widget can't hand back a fraction of an index.
#[derive(Clone, Debug, Facet)]
pub struct IntSlider {
    pub value: i64,
    pub min: i64,
    pub max: i64,
}

impl IntSlider {
    pub fn new(value: i64, min: i64, max: i64) -> Self {
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

/// An RGB color edited via egui's color picker.
///
/// Stored as bytes, the way a GX color register holds it, so the widget reads
/// the same as the `rgb8(...)` constants and the decomp they were copied from.
/// egui treats these as sRGB, which is the right space: the shader's final
/// `srgbDecode` says the byte/255 values reaching it are gamma-encoded.
///
/// No alpha, deliberately — the GX color overrides this drives are RGB-only.
#[derive(Clone, Debug, Facet)]
pub struct ColorPicker {
    pub rgb: [u8; 3],
}

impl ColorPicker {
    pub fn new(rgb: [u8; 3]) -> Self {
        Self { rgb }
    }

    /// Build from the 0..1 form uniforms take, so a caller can seed the widget
    /// from the constant it replaces instead of restating the bytes.
    pub fn from_vec3(rgb: Vec3) -> Self {
        let byte = |x: f32| (x * 255.0).round().clamp(0.0, 255.0) as u8;
        Self::new([byte(rgb.x), byte(rgb.y), byte(rgb.z)])
    }

    /// The 0..1 form uniforms take.
    pub fn to_vec3(&self) -> Vec3 {
        let [r, g, b] = self.rgb;
        Vec3::new(r as f32, g as f32, b as f32) / 255.0
    }

    /// Render this color picker in egui, returning true if the color changed.
    pub fn render_ui(&mut self, ui: &mut Ui) -> bool {
        ui.color_edit_button_srgb(&mut self.rgb).changed()
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
        let Self { selected, labels } = self;
        render_radio_group(ui, selected, labels)
    }
}

/// Radio groups this large or larger split into two vertical columns; a single
/// row of that many buttons makes the debug window uncomfortably wide.
const MIN_TWO_COLUMN_RADIO_OPTIONS: usize = 4;

/// Render `labels` as radio buttons in the ambient layout direction.
/// `offset` is the index of `labels[0]` within the full option list.
fn radio_buttons(ui: &mut Ui, selected: &mut usize, offset: usize, labels: &[String]) -> bool {
    let mut changed = false;
    for (i, label) in labels.iter().enumerate() {
        if ui.radio_value(selected, offset + i, label).changed() {
            changed = true;
        }
    }
    changed
}

/// Render a radio button group, returning true if the selection changed.
/// Splits into two vertical columns once there are enough options that a
/// single row would stretch the debug window.
pub fn render_radio_group(ui: &mut Ui, selected: &mut usize, labels: &[String]) -> bool {
    if labels.len() < MIN_TWO_COLUMN_RADIO_OPTIONS {
        return radio_buttons(ui, selected, 0, labels);
    }

    // Extra option goes in the left column, so an odd count reads top-to-bottom.
    let split = labels.len().div_ceil(2);
    let (left, right) = labels.split_at(split);

    let mut changed = ui.vertical(|ui| radio_buttons(ui, selected, 0, left)).inner;
    changed |= ui
        .vertical(|ui| radio_buttons(ui, selected, split, right))
        .inner;
    changed
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
