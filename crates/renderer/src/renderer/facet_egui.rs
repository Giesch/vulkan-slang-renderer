//! Auto-generated egui UI from facet reflection

use crate::editor::{Checkbox, Label, RadioButton, Slider, pascal_to_display};
use egui::Ui;
use facet::{EnumRepr, Facet, Poke, PokeStruct, Shape, Type, UserType};

/// Classification of a field's type for UI rendering.
enum FieldKind {
    Slider,
    Checkbox,
    RadioButton,
    Label,
    Collapsing,
    UnitEnum,
}

/// Classify a field's type for rendering.
/// Returns None for an unsupported editor type.
fn classify_field(shape: &Shape) -> Option<FieldKind> {
    if shape.is_type::<Slider>() {
        return Some(FieldKind::Slider);
    }

    if shape.is_type::<Checkbox>() {
        return Some(FieldKind::Checkbox);
    }

    if shape.is_type::<RadioButton>() {
        return Some(FieldKind::RadioButton);
    }

    if shape.is_type::<Label>() {
        return Some(FieldKind::Label);
    }

    if let Type::User(UserType::Enum(enum_type)) = &shape.ty
        && enum_type.variants.iter().all(|v| v.data.fields.is_empty())
    {
        return Some(FieldKind::UnitEnum);
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

/// Render a Checkbox wrapper type.
fn render_checkbox(ui: &mut Ui, mut poke: Poke<'_, '_>) -> bool {
    let checkbox = poke
        .get_mut::<Checkbox>()
        .expect("type mismatch: expected Checkbox");
    checkbox.render_ui(ui)
}

/// Render a RadioButton wrapper type.
fn render_radio_button(ui: &mut Ui, mut poke: Poke<'_, '_>) -> bool {
    let radio = poke
        .get_mut::<RadioButton>()
        .expect("type mismatch: expected RadioButton");
    radio.render_ui(ui)
}

/// Render a Label wrapper type.
fn render_label(ui: &mut Ui, poke: Poke<'_, '_>) {
    let label = poke.get::<Label>().expect("type mismatch: expected Label");
    label.render_ui(ui);
}

/// Render a unit enum as radio buttons using facet reflection.
fn render_unit_enum(ui: &mut Ui, poke: Poke<'_, '_>) -> bool {
    let poke_enum = poke.into_enum().expect("expected enum");
    let current = poke_enum.variant_index().expect("variant index");
    let variants = poke_enum.variants();
    let enum_repr = poke_enum.enum_repr();

    let labels: Vec<String> = variants.iter().map(|v| pascal_to_display(v.name)).collect();

    let mut selected = current;
    for (i, label) in labels.iter().enumerate() {
        if ui.radio_value(&mut selected, i, label).changed() {
            // selection changed handled below
        }
    }

    if selected != current {
        let new_disc = variants[selected].discriminant.expect("discriminant");
        let mut inner = poke_enum.into_inner();
        let ptr = inner.data_mut().as_mut_byte_ptr();
        unsafe {
            match enum_repr {
                EnumRepr::U8 => ptr.cast::<u8>().write(new_disc as u8),
                EnumRepr::U16 => ptr.cast::<u16>().write(new_disc as u16),
                EnumRepr::U32 => ptr.cast::<u32>().write(new_disc as u32),
                EnumRepr::U64 => ptr.cast::<u64>().write(new_disc as u64),
                EnumRepr::USize => ptr.cast::<usize>().write(new_disc as usize),
                EnumRepr::I8 => ptr.cast::<i8>().write(new_disc as i8),
                EnumRepr::I16 => ptr.cast::<i16>().write(new_disc as i16),
                EnumRepr::I32 => ptr.cast::<i32>().write(new_disc as i32),
                EnumRepr::I64 => ptr.cast::<i64>().write(new_disc),
                EnumRepr::ISize => ptr.cast::<isize>().write(new_disc as isize),
                _ => panic!("unsupported enum repr for unit enum radio buttons"),
            }
        }
        true
    } else {
        false
    }
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
        FieldKind::Checkbox => render_checkbox(ui, poke),
        FieldKind::RadioButton => render_radio_button(ui, poke),
        FieldKind::Label => {
            render_label(ui, poke);
            false
        }
        FieldKind::UnitEnum => render_unit_enum(ui, poke),
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
                FieldKind::Checkbox => {
                    if render_checkbox(ui, field_poke) {
                        modified = true;
                    }
                }
                FieldKind::RadioButton => {
                    if render_radio_button(ui, field_poke) {
                        modified = true;
                    }
                }
                FieldKind::Label => {
                    render_label(ui, field_poke);
                }
                FieldKind::UnitEnum => {
                    if render_unit_enum(ui, field_poke) {
                        modified = true;
                    }
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
