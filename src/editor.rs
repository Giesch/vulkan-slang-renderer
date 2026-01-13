//! Editor widget wrapper types for reflection-based UI generation.
//!
//! These types encode both value and metadata (like ranges) so that
//! the facet_egui reflection system can render appropriate widgets.

use facet::Facet;

/// A value edited via egui::Slider with a defined range.
#[derive(Clone, Debug, Facet)]
pub struct Slider<T> {
    pub value: T,
    pub min: T,
    pub max: T,
}

impl<T: Copy> Slider<T> {
    pub fn new(value: T, min: T, max: T) -> Self {
        Self { value, min, max }
    }
}
