//! Auto-generated egui UI from facet reflection

use crate::editor::{Label, Slider};
use egui::Ui;
use facet::{Facet, Poke, PokeStruct, Shape, Type, UserType};

/// Classification of a field's type for UI rendering.
enum FieldKind {
    Slider,
    Label,
    Collapsing,
}

/// Classify a field's type for rendering.
/// Returns None for an unsupported editor type.
fn classify_field(shape: &Shape) -> Option<FieldKind> {
    if shape.is_type::<Slider>() {
        return Some(FieldKind::Slider);
    }

    if shape.is_type::<Label>() {
        return Some(FieldKind::Label);
    }

    // Check for nested structs
    if let Type::User(UserType::Struct(_)) = &shape.ty {
        return Some(FieldKind::Collapsing);
    }

    None
}

/// Render a Slider wrapper type.
fn render_slider(ui: &mut Ui, mut poke: Poke<'_, '_>) -> bool {
    let slider = poke
        .get_mut::<Slider>()
        .expect("type mismatch: expected Slider");
    slider.render_ui(ui)
}

/// Render a Label wrapper type.
fn render_label(ui: &mut Ui, poke: Poke<'_, '_>) {
    let label = poke.get::<Label>().expect("type mismatch: expected Label");
    label.render_ui(ui);
}

/// Render editable UI for any Facet type.
/// Returns true if any value was modified.
pub fn render_facet_ui<'a, T: Facet<'a>>(ui: &mut Ui, value: &mut T) -> bool {
    let poke = Poke::new(value);
    let shape = poke.shape();

    let Some(kind) = classify_field(shape) else {
        return false;
    };

    match kind {
        FieldKind::Slider => render_slider(ui, poke),
        FieldKind::Label => {
            render_label(ui, poke);
            false
        }
        FieldKind::Collapsing => {
            let poke_struct = poke.into_struct().expect("expected struct");
            render_collapsing(ui, poke_struct)
        }
    }
}

fn render_collapsing(ui: &mut Ui, mut poke_struct: PokeStruct<'_, '_>) -> bool {
    let mut modified = false;
    let field_count = poke_struct.field_count();

    for i in 0..field_count {
        let field_name = poke_struct.ty().fields[i].name;
        let field_poke = poke_struct.field(i).expect("field index out of bounds");
        let field_shape = field_poke.shape();

        let Some(kind) = classify_field(field_shape) else {
            continue;
        };

        ui.horizontal(|ui| {
            ui.label(field_name);

            match kind {
                FieldKind::Slider => {
                    if render_slider(ui, field_poke) {
                        modified = true;
                    }
                }
                FieldKind::Label => {
                    render_label(ui, field_poke);
                }
                FieldKind::Collapsing => {
                    ui.collapsing(field_name, |ui| {
                        let nested_struct = field_poke.into_struct().expect("expected struct");
                        if render_collapsing(ui, nested_struct) {
                            modified = true;
                        }
                    });
                }
            }
        });
    }

    modified
}
