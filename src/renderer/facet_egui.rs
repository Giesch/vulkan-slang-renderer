//! Auto-generated egui UI from facet reflection

use egui::Ui;
use facet::{Facet, StructType, Type, UserType};

/// Classification of a field's type for UI rendering.
enum FieldKind<'a> {
    Slider {
        inner_type: PrimitiveKind,
        struct_type: &'a StructType,
    },
    Glam(GlamKind),
    Primitive(PrimitiveKind),
    Struct(&'a StructType),
    Unsupported,
}

#[derive(Clone, Copy)]
enum GlamKind {
    Vec2,
    Vec3,
    Vec4,
    Quat,
    Mat4,
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
fn classify_field<'a>(type_identifier: &str, ty: &'a Type) -> FieldKind<'a> {
    // Check for Slider wrapper type
    if let Some(slider) = parse_slider(type_identifier, ty) {
        return slider;
    }

    // Check for glam types
    if let Some(glam) = parse_glam(type_identifier) {
        return FieldKind::Glam(glam);
    }

    // Check for primitives
    if let Some(prim) = parse_primitive(type_identifier) {
        return FieldKind::Primitive(prim);
    }

    // Check for nested structs
    if let Type::User(UserType::Struct(struct_type)) = ty {
        return FieldKind::Struct(struct_type);
    }

    FieldKind::Unsupported
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

fn parse_glam(type_identifier: &str) -> Option<GlamKind> {
    match type_identifier {
        "glam::Vec2" | "glam::f32::Vec2" => Some(GlamKind::Vec2),
        "glam::Vec3" | "glam::f32::Vec3" | "glam::Vec3A" | "glam::f32::Vec3A" => {
            Some(GlamKind::Vec3)
        }
        "glam::Vec4" | "glam::f32::Vec4" => Some(GlamKind::Vec4),
        "glam::Quat" | "glam::f32::Quat" => Some(GlamKind::Quat),
        "glam::Mat4" | "glam::f32::Mat4" => Some(GlamKind::Mat4),
        _ => None,
    }
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

/// Render a primitive value.
fn render_primitive(ui: &mut Ui, ptr: *mut u8, kind: PrimitiveKind) -> bool {
    match kind {
        PrimitiveKind::F32 => render_drag_value::<f32>(ui, ptr, 0.1),
        PrimitiveKind::F64 => render_drag_value::<f64>(ui, ptr, 0.1),
        PrimitiveKind::I32 => render_drag_value::<i32>(ui, ptr, 1.0),
        PrimitiveKind::I64 => render_drag_value::<i64>(ui, ptr, 1.0),
        PrimitiveKind::U32 => render_drag_value::<u32>(ui, ptr, 1.0),
        PrimitiveKind::U64 => render_drag_value::<u64>(ui, ptr, 1.0),
        PrimitiveKind::Bool => {
            let value_ptr = ptr as *mut bool;
            let mut v = unsafe { *value_ptr };
            let response = ui.checkbox(&mut v, "");
            if response.changed() {
                unsafe { *value_ptr = v };
                return true;
            }
            false
        }
    }
}

fn render_drag_value<T: egui::emath::Numeric>(ui: &mut Ui, ptr: *mut u8, speed: f64) -> bool {
    let value_ptr = ptr as *mut T;
    let mut v = unsafe { *value_ptr };
    let response = ui.add(egui::DragValue::new(&mut v).speed(speed));
    if response.changed() {
        unsafe { *value_ptr = v };
        return true;
    }
    false
}

/// Render a glam type.
fn render_glam(ui: &mut Ui, ptr: *mut u8, kind: GlamKind) -> bool {
    match kind {
        GlamKind::Vec2 => render_vec2(ui, ptr),
        GlamKind::Vec3 => render_vec3(ui, ptr),
        GlamKind::Vec4 => render_vec4(ui, ptr),
        GlamKind::Quat => render_quat(ui, ptr),
        GlamKind::Mat4 => render_mat4(ui, ptr),
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
    let kind = classify_field(shape.type_identifier, &shape.ty);

    match kind {
        FieldKind::Slider {
            inner_type,
            struct_type,
        } => render_slider(ui, ptr, inner_type, struct_type),

        FieldKind::Glam(glam_kind) => render_glam(ui, ptr, glam_kind),

        FieldKind::Primitive(prim_kind) => render_primitive(ui, ptr, prim_kind),

        FieldKind::Struct(struct_type) => render_struct(ui, ptr, struct_type),

        FieldKind::Unsupported => {
            ui.label(format!("Unsupported type: {}", shape.type_identifier));
            false
        }
    }
}

fn render_struct(ui: &mut Ui, base_ptr: *mut u8, struct_type: &StructType) -> bool {
    let mut modified = false;

    for field in struct_type.fields {
        let field_ptr = unsafe { base_ptr.add(field.offset) };
        let field_shape = field.shape.get();
        let field_type_name = field_shape.type_identifier;
        let kind = classify_field(field_type_name, &field_shape.ty);

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
                FieldKind::Glam(glam_kind) => {
                    if render_glam(ui, field_ptr, glam_kind) {
                        modified = true;
                    }
                }
                FieldKind::Primitive(prim_kind) => {
                    if render_primitive(ui, field_ptr, prim_kind) {
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
                FieldKind::Unsupported => {
                    ui.label(format!("({})", field_type_name));
                }
            }
        });
    }

    modified
}

fn render_vec2(ui: &mut Ui, ptr: *mut u8) -> bool {
    let v = unsafe { &mut *(ptr as *mut glam::Vec2) };
    let mut modified = false;
    ui.horizontal(|ui| {
        modified |= ui
            .add(egui::DragValue::new(&mut v.x).prefix("x: ").speed(0.1))
            .changed();
        modified |= ui
            .add(egui::DragValue::new(&mut v.y).prefix("y: ").speed(0.1))
            .changed();
    });

    modified
}

fn render_vec3(ui: &mut Ui, ptr: *mut u8) -> bool {
    let v = unsafe { &mut *(ptr as *mut glam::Vec3) };
    let mut modified = false;
    ui.horizontal(|ui| {
        modified |= ui
            .add(egui::DragValue::new(&mut v.x).prefix("x: ").speed(0.1))
            .changed();
        modified |= ui
            .add(egui::DragValue::new(&mut v.y).prefix("y: ").speed(0.1))
            .changed();
        modified |= ui
            .add(egui::DragValue::new(&mut v.z).prefix("z: ").speed(0.1))
            .changed();
    });

    modified
}

fn render_vec4(ui: &mut Ui, ptr: *mut u8) -> bool {
    let v = unsafe { &mut *(ptr as *mut glam::Vec4) };
    let mut modified = false;
    ui.horizontal(|ui| {
        modified |= ui
            .add(egui::DragValue::new(&mut v.x).prefix("x: ").speed(0.1))
            .changed();
        modified |= ui
            .add(egui::DragValue::new(&mut v.y).prefix("y: ").speed(0.1))
            .changed();
        modified |= ui
            .add(egui::DragValue::new(&mut v.z).prefix("z: ").speed(0.1))
            .changed();
        modified |= ui
            .add(egui::DragValue::new(&mut v.w).prefix("w: ").speed(0.1))
            .changed();
    });

    modified
}

fn render_quat(ui: &mut Ui, ptr: *mut u8) -> bool {
    let q = unsafe { &mut *(ptr as *mut glam::Quat) };
    // Display as euler angles for easier editing
    let (mut x, mut y, mut z) = q.to_euler(glam::EulerRot::XYZ);
    let mut modified = false;

    ui.horizontal(|ui| {
        ui.label("euler:");
        modified |= ui
            .add(egui::DragValue::new(&mut x).prefix("x: ").speed(0.01))
            .changed();
        modified |= ui
            .add(egui::DragValue::new(&mut y).prefix("y: ").speed(0.01))
            .changed();
        modified |= ui
            .add(egui::DragValue::new(&mut z).prefix("z: ").speed(0.01))
            .changed();
    });

    if modified {
        *q = glam::Quat::from_euler(glam::EulerRot::XYZ, x, y, z);
    }

    modified
}

fn render_mat4(ui: &mut Ui, ptr: *mut u8) -> bool {
    let m = unsafe { &mut *(ptr as *mut glam::Mat4) };
    let mut modified = false;

    ui.vertical(|ui| {
        for row in 0..4 {
            ui.horizontal(|ui| {
                for col in 0..4 {
                    let mut v = m.col(col)[row];
                    if ui.add(egui::DragValue::new(&mut v).speed(0.01)).changed() {
                        m.col_mut(col)[row] = v;
                        modified = true;
                    }
                }
            });
        }
    });

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
