//! Auto-generated egui UI from facet reflection

use egui::Ui;
use facet::{Facet, StructType, Type, UserType};

/// Classification of a field's type for UI rendering.
enum FieldKind<'a> {
    Slider {
        inner_type: PrimitiveKind,
        struct_type: &'a StructType,
    },
    Struct(&'a StructType),
}

#[derive(Clone, Copy)]
enum PrimitiveKind {
    F32,
    F64,
    I32,
    I64,
    U32,
    U64,
    Bool,
}

/// Classify a field's type for rendering.
/// Returns None for an unsupported editor type.
fn classify_field<'a>(type_identifier: &str, ty: &'a Type) -> Option<FieldKind<'a>> {
    // Check for Slider wrapper type
    if let Some(slider) = parse_slider(type_identifier, ty) {
        return Some(slider);
    }

    // Check for nested structs
    if let Type::User(UserType::Struct(struct_type)) = ty {
        return Some(FieldKind::Struct(struct_type));
    }

    None
}

fn parse_slider<'a>(type_identifier: &str, ty: &'a Type) -> Option<FieldKind<'a>> {
    if type_identifier != "Slider" {
        return None;
    }

    let Type::User(UserType::Struct(struct_type)) = ty else {
        return None;
    };

    let value_field = struct_type.fields.iter().find(|f| f.name == "value")?;

    let inner_type = value_field.shape.get().type_identifier;
    let prim = parse_primitive(inner_type)?;

    Some(FieldKind::Slider {
        inner_type: prim,
        struct_type,
    })
}

fn parse_primitive(type_identifier: &str) -> Option<PrimitiveKind> {
    match type_identifier {
        "f32" => Some(PrimitiveKind::F32),
        "f64" => Some(PrimitiveKind::F64),
        "i32" => Some(PrimitiveKind::I32),
        "i64" => Some(PrimitiveKind::I64),
        "u32" => Some(PrimitiveKind::U32),
        "u64" => Some(PrimitiveKind::U64),
        "bool" => Some(PrimitiveKind::Bool),
        _ => None,
    }
}

/// Render a Slider wrapper type.
fn render_slider(
    ui: &mut Ui,
    ptr: *mut u8,
    inner_type: PrimitiveKind,
    struct_type: &StructType,
) -> bool {
    match inner_type {
        PrimitiveKind::F32 => render_slider_typed::<f32>(ui, ptr, struct_type),
        PrimitiveKind::F64 => render_slider_typed::<f64>(ui, ptr, struct_type),
        _ => false, // Slider only supports f32/f64 currently
    }
}

/// Render editable UI for any Facet type.
/// Returns true if any value was modified.
pub fn render_facet_ui<'a, T: Facet<'a>>(ui: &mut Ui, value: &mut T) -> bool {
    let shape = T::SHAPE;
    let ptr = value as *mut T as *mut u8;
    let Some(kind) = classify_field(shape.type_identifier, &shape.ty) else {
        return false;
    };

    match kind {
        FieldKind::Slider {
            inner_type,
            struct_type,
        } => render_slider(ui, ptr, inner_type, struct_type),

        FieldKind::Struct(struct_type) => render_struct(ui, ptr, struct_type),
    }
}

// TODO rename or remove this
fn render_struct(ui: &mut Ui, base_ptr: *mut u8, struct_type: &StructType) -> bool {
    let mut modified = false;

    for field in struct_type.fields {
        let field_ptr = unsafe { base_ptr.add(field.offset) };
        let field_shape = field.shape.get();
        let field_type_name = field_shape.type_identifier;
        let Some(kind) = classify_field(field_type_name, &field_shape.ty) else {
            return false;
        };

        ui.horizontal(|ui| {
            ui.label(field.name);

            match kind {
                FieldKind::Slider {
                    inner_type,
                    struct_type,
                } => {
                    if render_slider(ui, field_ptr, inner_type, struct_type) {
                        modified = true;
                    }
                }
                FieldKind::Struct(nested_struct) => {
                    ui.collapsing(field.name, |ui| {
                        if render_struct(ui, field_ptr, nested_struct) {
                            modified = true;
                        }
                    });
                }
            }
        });
    }

    modified
}

fn render_slider_typed<T>(ui: &mut Ui, ptr: *mut u8, struct_type: &StructType) -> bool
where
    T: egui::emath::Numeric + Default,
{
    let mut value: T = Default::default();
    let mut min: T = Default::default();
    let mut max: T = T::from_f64(1.0);
    let mut value_offset: usize = 0;

    for field in struct_type.fields {
        let field_ptr = unsafe { ptr.add(field.offset) };
        match field.name {
            "value" => {
                value = unsafe { *(field_ptr as *const T) };
                value_offset = field.offset;
            }
            "min" => min = unsafe { *(field_ptr as *const T) },
            "max" => max = unsafe { *(field_ptr as *const T) },
            _ => {}
        }
    }

    let mut v = value;
    let response = ui.add(egui::Slider::new(&mut v, min..=max));
    if response.changed() {
        let value_ptr = unsafe { ptr.add(value_offset) as *mut T };
        unsafe { *value_ptr = v };
        return true;
    }

    false
}
