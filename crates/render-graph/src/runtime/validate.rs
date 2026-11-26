//! Pure validation for the plain-data graph description.

use super::desc::*;
use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TableKind {
    Texture,
    Buffer,
    Import,
    Value,
    Uniform,
    Pipeline,
    Schema,
}

impl fmt::Display for TableKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Texture => "texture",
            Self::Buffer => "buffer",
            Self::Import => "import",
            Self::Value => "value",
            Self::Uniform => "uniform",
            Self::Pipeline => "pipeline",
            Self::Schema => "schema",
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum UnsupportedFeature {
    WindowSizeClass,
    ColorAttachmentUsage,
    DepthAttachmentUsage,
    OffscreenTargets,
    MultipleRasterPasses,
    RasterInWhen,
    GroupSourceValue,
}

impl fmt::Display for UnsupportedFeature {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::WindowSizeClass => "window-relative texture sizes (phase 5, ledger S7)",
            Self::ColorAttachmentUsage => "color-attachment texture usage (phase 5, ledger S6)",
            Self::DepthAttachmentUsage => "depth-attachment texture usage (phase 5, ledger S6)",
            Self::OffscreenTargets => "offscreen raster targets (phase 5, ledger S6)",
            Self::MultipleRasterPasses => "a second raster pass (phase 5, ledger S6)",
            Self::RasterInWhen => "a raster pass inside an optional scope (phase 5, ledger S6)",
            Self::GroupSourceValue => "per-frame dispatch group counts (phase 3a, ledger S1)",
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ScopeKind {
    Repeat,
    Optional,
}

impl fmt::Display for ScopeKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Repeat => "repeat",
            Self::Optional => "optional",
        })
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum GraphError {
    IdOutOfRange {
        table: TableKind,
        id: u32,
    },
    DuplicateWrite {
        command: String,
        tex: String,
    },
    MutateAndRead {
        command: String,
        tex: String,
    },
    MutateAndWrite {
        command: String,
        tex: String,
    },
    WriteAndPrevRead {
        command: String,
        tex: String,
    },
    PrevReadWithoutWrite {
        command: String,
        tex: String,
    },
    RepeatUniformRotatesTexture {
        repeat: String,
        uniform: String,
        tex: String,
    },
    SharedUniformRotatesTexture {
        uniform: String,
        writer: String,
        tex: String,
    },
    RasterInRepeat {
        repeat: String,
    },
    PassAfterMainRaster {
        pass: String,
    },
    DrawWritesTexture {
        draw: String,
        tex: String,
    },
    TextureExtentZero {
        texture: String,
        width: u32,
        height: u32,
    },
    TextureExtentTooLarge {
        texture: String,
        width: u32,
        height: u32,
        max: u32,
    },
    PipelineKindMismatch {
        command: String,
        expected: PipelineKind,
    },
    IndirectArgsNotImmutable {
        draw: String,
    },
    DrawIndicesNotIndexBuffer {
        draw: String,
    },
    IndexRangeOutOfBounds {
        draw: String,
        first_index: u32,
        index_count: u32,
        capacity: u32,
    },
    WhenGateNotOptional {
        when: String,
    },
    ValueKindMismatch {
        value: String,
        expected: &'static str,
        found: &'static str,
    },
    UploadTargetKind {
        upload: String,
        kind: &'static str,
    },
    UploadTooLarge {
        upload: String,
        max_len: u32,
        capacity: u32,
    },
    BindingCountMismatch {
        uniform: String,
        expected: usize,
        found: usize,
    },
    BindingKindMismatch {
        uniform: String,
        field: u16,
    },
    UnsupportedInPhase1 {
        feature: UnsupportedFeature,
        at: String,
    },
    NestedControlFlow {
        outer: ScopeKind,
        inner: ScopeKind,
    },
    UniformSourceConflict {
        slot: usize,
    },
    MultiplePickingNodes,
    PickingWithoutDraw,
    EmptyOptionalScope,
    MutableExternalImport,
    BufferOffsetOverflow {
        offset: u64,
    },
    LayoutFieldOutOfBounds {
        schema: String,
        field: u16,
        offset: u32,
        len: u32,
        size: u32,
    },
    LayoutFieldsOverlap {
        schema: String,
        first: u16,
        second: u16,
    },
    LayoutResourceFieldSize {
        schema: String,
        field: u16,
        len: u32,
    },
    LayoutResourceFieldMismatch {
        schema: String,
        field: u16,
    },
    LayoutDataWithoutValue {
        block: String,
    },
    LayoutDataOutOfBounds {
        block: String,
        field: u16,
        src_offset: u32,
        len: u32,
        data_size: u32,
    },
    PushConstantSize {
        schema: String,
        size: u32,
    },
    ValueScopeInvalid {
        value: String,
        reason: &'static str,
    },
    UploadElemSizeMismatch {
        upload: String,
        value_elem_size: u32,
        buffer_elem_size: u32,
    },
    BufferAccessMismatch {
        block: String,
        field: u16,
        kind: BufferKind,
    },
    PickingCursorShape,
}

impl fmt::Display for GraphError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::IdOutOfRange { table, id } => write!(f, "{table} id {id} is undeclared"),
            Self::DuplicateWrite { command, tex } => {
                write!(f, "{command} writes texture {tex} more than once")
            }
            Self::MutateAndRead { command, tex } => {
                write!(f, "{command} mutates and reads texture {tex}")
            }
            Self::MutateAndWrite { command, tex } => {
                write!(f, "{command} mutates and writes texture {tex}")
            }
            Self::WriteAndPrevRead { command, tex } => write!(
                f,
                "{command} writes texture {tex} and reads its previous version"
            ),
            Self::PrevReadWithoutWrite { command, tex } => write!(
                f,
                "{command} reads texture {tex}'s previous version, but no command writes {tex}"
            ),
            Self::RepeatUniformRotatesTexture {
                repeat,
                uniform,
                tex,
            } => write!(
                f,
                "{repeat}: {uniform}'s uniform block references rotating texture {tex}; move the reference into the node's push block"
            ),
            Self::SharedUniformRotatesTexture {
                uniform,
                writer,
                tex,
            } => write!(
                f,
                "{uniform}'s uniform block is shared by several commands, but {writer} writes \
                 texture {tex}, which the block references, between them; bind {tex} through \
                 each command's push block instead"
            ),
            Self::RasterInRepeat { repeat } => {
                write!(f, "a draw node cannot appear inside repeat {repeat}")
            }
            Self::PassAfterMainRaster { pass } => {
                write!(f, "pass {pass} must precede the main raster pass")
            }
            Self::DrawWritesTexture { draw, tex } => write!(
                f,
                "draw {draw} can only read graph textures (texture {tex})"
            ),
            Self::TextureExtentZero {
                texture,
                width,
                height,
            } => write!(f, "texture {texture} has a zero extent: {width}x{height}"),
            Self::TextureExtentTooLarge {
                texture,
                width,
                height,
                max,
            } => write!(
                f,
                "texture {texture} extent {width}x{height} exceeds the device maximum {max}"
            ),
            Self::PipelineKindMismatch { command, expected } => write!(
                f,
                "{command} uses the wrong pipeline kind; expected {expected:?}"
            ),
            Self::IndirectArgsNotImmutable { draw } => {
                write!(f, "draw {draw}'s indirect argument buffer is not immutable")
            }
            Self::DrawIndicesNotIndexBuffer { draw } => {
                write!(f, "draw {draw}'s indices are not an index buffer")
            }
            Self::IndexRangeOutOfBounds {
                draw,
                first_index,
                index_count,
                capacity,
            } => write!(
                f,
                "draw {draw}'s index range [{first_index}, {first_index} + {index_count}) is out \
                 of bounds for an index buffer of {capacity} indices"
            ),
            Self::WhenGateNotOptional { when } => write!(f, "when {when} gate is not optional"),
            Self::ValueKindMismatch {
                value,
                expected,
                found,
            } => write!(f, "value {value} has kind {found}, expected {expected}"),
            Self::UploadTargetKind { upload, kind } => {
                write!(f, "upload {upload} cannot target {kind}")
            }
            Self::UploadTooLarge {
                upload,
                max_len,
                capacity,
            } => write!(
                f,
                "upload {upload} maximum {max_len} exceeds capacity {capacity}"
            ),
            Self::BindingCountMismatch {
                uniform,
                expected,
                found,
            } => write!(f, "{uniform} has {found} bindings, expected {expected}"),
            Self::BindingKindMismatch { uniform, field } => {
                write!(f, "{uniform} binding {field} has the wrong kind")
            }
            Self::UnsupportedInPhase1 { feature, at } => {
                write!(f, "{at}: {feature} is unsupported in phase 1")
            }
            Self::NestedControlFlow { outer, inner } => {
                write!(f, "nested {inner} inside {outer} is not supported")
            }
            Self::UniformSourceConflict { slot } => {
                write!(f, "uniform slot {slot} has more than one data source")
            }
            Self::MultiplePickingNodes => write!(f, "at most one picking node is allowed"),
            Self::PickingWithoutDraw => write!(f, "a picking node needs at least one draw node"),
            Self::EmptyOptionalScope => write!(f, "optional scope has no frame value"),
            Self::MutableExternalImport => {
                write!(f, "mutable external texture imports are unsupported")
            }
            Self::BufferOffsetOverflow { offset } => {
                write!(f, "buffer offset {offset} exceeds u32")
            }
            Self::LayoutFieldOutOfBounds {
                schema,
                field,
                offset,
                len,
                size,
            } => write!(
                f,
                "schema {schema} field {field} [{offset}, {offset} + {len}) does not fit in its \
                 {size}-byte block"
            ),
            Self::LayoutFieldsOverlap {
                schema,
                first,
                second,
            } => write!(f, "schema {schema} fields {first} and {second} overlap"),
            Self::LayoutResourceFieldSize { schema, field, len } => write!(
                f,
                "schema {schema} resource field {field} is {len} bytes; handles and addresses \
                 are 8 bytes"
            ),
            Self::LayoutResourceFieldMismatch { schema, field } => write!(
                f,
                "schema {schema} layout field {field} does not match its resource fields; each \
                 resource field needs exactly one layout field of the same kind"
            ),
            Self::LayoutDataWithoutValue { block } => {
                write!(f, "{block} lays out data fields but names no data value")
            }
            Self::LayoutDataOutOfBounds {
                block,
                field,
                src_offset,
                len,
                data_size,
            } => write!(
                f,
                "{block} data field {field} reads [{src_offset}, {src_offset} + {len}) past its \
                 {data_size}-byte data value"
            ),
            Self::PushConstantSize { schema, size } => write!(
                f,
                "push block {schema} is {size} bytes; vkCmdPushConstants needs a nonzero \
                 multiple of 4 of at most {MAX_PUSH_CONSTANT_BYTES}"
            ),
            Self::ValueScopeInvalid { value, reason } => {
                write!(f, "value {value} has an invalid scope: {reason}")
            }
            Self::UploadElemSizeMismatch {
                upload,
                value_elem_size,
                buffer_elem_size,
            } => write!(
                f,
                "upload {upload} elements are {value_elem_size} bytes, but its buffer's elements \
                 are {buffer_elem_size} bytes"
            ),
            Self::BufferAccessMismatch { block, field, kind } => write!(
                f,
                "{block} binding {field} names a {kind:?} buffer with an access or slot it does \
                 not support"
            ),
            Self::PickingCursorShape => {
                write!(f, "the picking cursor must be an 8-byte bytes value")
            }
        }
    }
}

impl std::error::Error for GraphError {}

pub(crate) fn validation_message(errors: &[GraphError]) -> String {
    let mut message = String::from("render graph validation failed:");
    for error in errors {
        message.push_str("\n  - ");
        message.push_str(&error.to_string());
    }

    message
}

/// Vulkan's guaranteed `maxPushConstantsSize`.
pub(crate) const MAX_PUSH_CONSTANT_BYTES: u32 = 128;

/// The byte size of every resource field: a bindless `DescriptorHandle` is a
/// Slang `uint2`, and a buffer device address is a `uint64_t`.
pub(crate) const RESOURCE_FIELD_BYTES: u32 = 8;

fn layout_resource_kind(kind: &SchemaFieldKind) -> Option<ResourceFieldKind> {
    match kind {
        SchemaFieldKind::Data { .. } => None,
        SchemaFieldKind::SampledTex => Some(ResourceFieldKind::SampledTex),
        SchemaFieldKind::StorageTex => Some(ResourceFieldKind::StorageTex),
        SchemaFieldKind::BufAddr => Some(ResourceFieldKind::BufAddr),
    }
}

/// A layout's fields fit in the block without overlapping, and its resource
/// fields are 8-byte slots that correspond one-to-one with `resource_fields`.
fn check_layout(schema: &SchemaDesc, errors: &mut Vec<GraphError>) {
    let Some(layout) = &schema.layout else {
        return;
    };

    let mut resource_seen = vec![false; schema.resource_fields.len()];
    let mut spans = vec![];
    for field in &layout.fields {
        let end = field.offset.checked_add(field.len);
        let fits = end.is_some_and(|end| end <= schema.size);
        if !fits {
            errors.push(GraphError::LayoutFieldOutOfBounds {
                schema: schema.name.clone(),
                field: field.key.0,
                offset: field.offset,
                len: field.len,
                size: schema.size,
            });
        }
        spans.push((field.offset, end.unwrap_or(u32::MAX), field.key.0));

        let Some(kind) = layout_resource_kind(&field.kind) else {
            continue;
        };

        if field.len != RESOURCE_FIELD_BYTES {
            errors.push(GraphError::LayoutResourceFieldSize {
                schema: schema.name.clone(),
                field: field.key.0,
                len: field.len,
            });
        }
        let index = field.key.0 as usize;
        let first_match = schema.resource_fields.get(index) == Some(&kind)
            && resource_seen.get(index) == Some(&false);
        if first_match {
            resource_seen[index] = true;
        } else {
            errors.push(GraphError::LayoutResourceFieldMismatch {
                schema: schema.name.clone(),
                field: field.key.0,
            });
        }
    }
    for (index, seen) in resource_seen.iter().enumerate() {
        if !seen {
            errors.push(GraphError::LayoutResourceFieldMismatch {
                schema: schema.name.clone(),
                field: index as u16,
            });
        }
    }

    spans.sort();
    let mut furthest: Option<(u32, u16)> = None;
    for (start, end, key) in spans {
        if let Some((furthest_end, furthest_key)) = furthest
            && start < furthest_end
        {
            errors.push(GraphError::LayoutFieldsOverlap {
                schema: schema.name.clone(),
                first: furthest_key,
                second: key,
            });
        }
        let extends = furthest.is_none_or(|(furthest_end, _)| end > furthest_end);
        if extends {
            furthest = Some((end, key));
        }
    }
}

/// `vkCmdPushConstants` takes a nonzero size that is a multiple of 4, and
/// Vulkan guarantees 128 bytes.
fn push_size_valid(size: u32) -> bool {
    size > 0 && size.is_multiple_of(4) && size <= MAX_PUSH_CONSTANT_BYTES
}

/// Every schema a pipeline or command uses as a push block, once each.
fn check_push_sizes(desc: &GraphDesc, schemas: &SchemaTable, errors: &mut Vec<GraphError>) {
    let mut push_schemas: Vec<SchemaId> = desc
        .pipelines
        .iter()
        .filter_map(|pipeline| pipeline.push)
        .chain(
            desc.leaves()
                .flat_map(LeafPass::commands)
                .filter_map(|command| command.push.map(|push| push.schema)),
        )
        .collect();
    push_schemas.sort_by_key(|schema| schema.0);
    push_schemas.dedup();
    for schema in push_schemas.iter().filter_map(|id| schemas.get(*id)) {
        if !push_size_valid(schema.size) {
            errors.push(GraphError::PushConstantSize {
                schema: schema.name.clone(),
                size: schema.size,
            });
        }
    }
}

/// A value's scope names a gate or count of the right shape: a `When` gate is
/// an optional value gated by itself, and a `Repeat` count is a top-level
/// count. Values inside a repeat cannot be optional, as `repeat` and
/// `optional` do not nest.
fn check_value_scopes(desc: &GraphDesc, errors: &mut Vec<GraphError>) {
    for value in &desc.values {
        let reason = match value.scope {
            ValueScope::Top => None,
            ValueScope::When { gate } => match desc.values.get(gate.0 as usize) {
                None => Some("its gate is undeclared"),
                Some(_) if !value.optional => Some("a value in an optional scope must be optional"),
                Some(gate_decl) if gate_decl.scope != (ValueScope::When { gate }) => {
                    Some("its gate is not gated by itself")
                }
                Some(_) => None,
            },
            ValueScope::Repeat { count } => match desc.values.get(count.0 as usize) {
                None => Some("its repeat count is undeclared"),
                Some(_) if value.optional => Some("a value in a repeat cannot be optional"),
                Some(count_decl)
                    if !matches!(count_decl.kind, ValueKind::Count)
                        || count_decl.scope != ValueScope::Top =>
                {
                    Some("its repeat count is not a top-level count")
                }
                Some(_) => None,
            },
        };
        if let Some(reason) = reason {
            errors.push(GraphError::ValueScopeInvalid {
                value: value.name.clone(),
                reason,
            });
        }
    }
}

/// A layout's data fields read inside the block's bytes data value.
fn check_layout_data(
    block: &str,
    schema: SchemaId,
    data: Option<ValueId>,
    desc: &GraphDesc,
    schemas: &SchemaTable,
    errors: &mut Vec<GraphError>,
) {
    let Some(layout) = schemas
        .get(schema)
        .and_then(|schema| schema.layout.as_ref())
    else {
        return;
    };

    let data_fields = layout.fields.iter().filter_map(|field| match field.kind {
        SchemaFieldKind::Data { src_offset } => Some((field, src_offset)),
        _ => None,
    });
    let Some(data) = data else {
        if data_fields.count() > 0 {
            errors.push(GraphError::LayoutDataWithoutValue {
                block: block.into(),
            });
        }
        return;
    };

    // a missing or mistyped value is reported where the value is checked
    let data_size = desc
        .values
        .get(data.0 as usize)
        .and_then(|value| match value.kind {
            ValueKind::Bytes { schema } => schemas.get(schema),
            _ => None,
        })
        .map(|data_schema| data_schema.size);
    let Some(data_size) = data_size else {
        return;
    };

    for (field, src_offset) in data_fields {
        let fits = src_offset
            .checked_add(field.len)
            .is_some_and(|end| end <= data_size);
        if !fits {
            errors.push(GraphError::LayoutDataOutOfBounds {
                block: block.into(),
                field: field.key.0,
                src_offset,
                len: field.len,
                data_size,
            });
        }
    }
}

/// A buffer binding's slot and access must be ones its buffer kind provides:
/// index buffers have no address, only GPU-updated buffers keep a previous
/// slot, and immutable and singleton buffers are read-only.
fn buffer_access_supported(kind: BufferKind, buffer: &BufferRef, access: BufAccess) -> bool {
    match kind {
        BufferKind::Index => false,
        BufferKind::Singleton | BufferKind::Immutable => {
            buffer.slot == SlotSel::Current && access == BufAccess::Read
        }
        BufferKind::Storage => buffer.slot == SlotSel::Current,
        BufferKind::GpuUpdatedFlight => {
            buffer.slot == SlotSel::Current || access == BufAccess::Read
        }
    }
}

/// A uniform block is written once on the CPU before any command executes,
/// so every consumer must resolve it to the same bytes. A `Write` between a
/// shared block's first and last consumer moves the version its texture
/// references resolve to. A repeat body's own rotation is reported as
/// [`GraphError::RepeatUniformRotatesTexture`] instead.
fn check_shared_uniforms(desc: &GraphDesc, errors: &mut Vec<GraphError>) {
    let commands: Vec<_> = desc.leaves().flat_map(LeafPass::commands).collect();
    for (id, uniform) in desc.uniforms.iter().enumerate() {
        let consumers: Vec<usize> = commands
            .iter()
            .enumerate()
            .filter(|(_, command)| command.uniform.0 as usize == id)
            .map(|(position, _)| position)
            .collect();
        let (Some(&first), Some(&last)) = (consumers.first(), consumers.last()) else {
            continue;
        };

        let mut reported: Vec<TexId> = vec![];
        for writer in &commands[first..last] {
            for (_, resource) in writer.bindings(desc) {
                let ResourceRef::Tex(tex, TexAccess::Write) = resource else {
                    continue;
                };

                let referenced =
                    uniform.source.bindings.iter().any(
                        |(_, bound)| matches!(bound, ResourceRef::Tex(bound, _) if bound == tex),
                    );
                let tex_name = tex_name(desc, *tex);
                let already_reported = reported.contains(tex) || errors.iter().any(|error| {
                    matches!(
                        error,
                        GraphError::RepeatUniformRotatesTexture { uniform: name, tex: rotating, .. }
                            if *name == uniform.name && *rotating == tex_name
                    )
                });
                if referenced && !already_reported {
                    reported.push(*tex);
                    errors.push(GraphError::SharedUniformRotatesTexture {
                        uniform: uniform.name.clone(),
                        writer: writer.name.into(),
                        tex: tex_name,
                    });
                }
            }
        }
    }
}

fn tex_name(desc: &GraphDesc, tex: TexId) -> String {
    match desc.textures.get(tex.0 as usize) {
        Some(decl) => decl.name.clone(),
        None => format!("tex{}", tex.0),
    }
}

/// `max` is the device's `maxImageDimension2D`; device-dependent checks stay
/// out of [`validate`].
pub(crate) fn extent_limit_errors(textures: &[TexDecl], max: u32) -> Vec<GraphError> {
    let mut errors = vec![];
    for tex in textures {
        if let SizeClass::Fixed(width, height) = tex.size
            && (width > max || height > max)
        {
            errors.push(GraphError::TextureExtentTooLarge {
                texture: tex.name.clone(),
                width,
                height,
                max,
            })
        }
    }

    errors
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Analysis {
    /// physical textures per logical texture, by index
    pub(crate) tex_phys: Vec<u32>,
}

fn kind(value: &ValueKind) -> &'static str {
    match value {
        ValueKind::Count => "count",
        ValueKind::Groups => "groups",
        ValueKind::Bytes { .. } => "bytes",
        ValueKind::Array { .. } => "array",
    }
}

fn check_shape(
    name: &str,
    schema: SchemaId,
    bindings: &[(FieldKey, ResourceRef)],
    schemas: &SchemaTable,
    errors: &mut Vec<GraphError>,
) {
    let Some(schema_desc) = schemas.get(schema) else {
        return;
    };

    if bindings.len() != schema_desc.resource_fields.len() {
        errors.push(GraphError::BindingCountMismatch {
            uniform: name.into(),
            expected: schema_desc.resource_fields.len(),
            found: bindings.len(),
        });
    }
    for (i, (key, resource)) in bindings.iter().enumerate() {
        let ok = key.0 as usize == i
            && schema_desc
                .resource_fields
                .get(i)
                .is_some_and(|field_kind| {
                    matches!(
                        (field_kind, resource),
                        (
                            ResourceFieldKind::SampledTex,
                            ResourceRef::Tex(_, TexAccess::Read | TexAccess::ReadPrevious)
                                | ResourceRef::External(_)
                        ) | (
                            ResourceFieldKind::StorageTex,
                            ResourceRef::Tex(_, TexAccess::Write | TexAccess::Mutate)
                        ) | (ResourceFieldKind::BufAddr, ResourceRef::Buf(_, _))
                    )
                });
        if !ok {
            errors.push(GraphError::BindingKindMismatch {
                uniform: name.into(),
                field: key.0,
            });
        }
    }
}

/// An indexed draw must name an index buffer, and an index sub-range must fit
/// in it. The index count is the buffer's `capacity`.
fn validate_draw_indices(draw: &DrawDesc, desc: &GraphDesc, errors: &mut Vec<GraphError>) {
    let Some(indices) = draw.call.index_buffer() else {
        return;
    };

    let Some(buffer) = desc.buffers.get(indices.0 as usize) else {
        errors.push(GraphError::IdOutOfRange {
            table: TableKind::Buffer,
            id: indices.0,
        });
        return;
    };

    let is_index_buffer = buffer.kind == BufferKind::Index;
    if !is_index_buffer {
        errors.push(GraphError::DrawIndicesNotIndexBuffer {
            draw: draw.name.clone(),
        });
        return;
    }

    if let DrawCall::IndexRange {
        first_index,
        index_count,
        ..
    } = draw.call
    {
        let capacity = buffer.capacity.unwrap_or(0);
        let in_bounds = first_index
            .checked_add(index_count)
            .is_some_and(|end| end <= capacity);
        if !in_bounds {
            errors.push(GraphError::IndexRangeOutOfBounds {
                draw: draw.name.clone(),
                first_index,
                index_count,
                capacity,
            });
        }
    }
}

pub(crate) fn validate(
    desc: &GraphDesc,
    schemas: &SchemaTable,
) -> Result<Analysis, Vec<GraphError>> {
    let mut errors = vec![];
    let valid = |id: u32, len: usize, table: TableKind, errors: &mut Vec<GraphError>| {
        if id as usize >= len {
            errors.push(GraphError::IdOutOfRange { table, id });

            false
        } else {
            true
        }
    };
    // Resource ids are checked on the declaration that carries them: a
    // zero-consumer source is still checked, a shared source reports once.
    let valid_bindings =
        |block: &str, bindings: &[(FieldKey, ResourceRef)], errors: &mut Vec<GraphError>| {
            for (key, resource) in bindings {
                match resource {
                    ResourceRef::Tex(tex, _) => {
                        valid(tex.0, desc.textures.len(), TableKind::Texture, errors);
                    }
                    ResourceRef::Buf(buffer, access) => {
                        if valid(
                            buffer.buffer.0,
                            desc.buffers.len(),
                            TableKind::Buffer,
                            errors,
                        ) {
                            let kind = desc.buffers[buffer.buffer.0 as usize].kind;
                            if !buffer_access_supported(kind, buffer, *access) {
                                errors.push(GraphError::BufferAccessMismatch {
                                    block: block.into(),
                                    field: key.0,
                                    kind,
                                });
                            }
                        }
                    }
                    ResourceRef::External(import) => {
                        valid(import.0, desc.imports.len(), TableKind::Import, errors);
                    }
                }
            }
        };
    let valid_command =
        |command: Command<'_>, expected: PipelineKind, errors: &mut Vec<GraphError>| {
            if valid(
                command.pipeline.0,
                desc.pipelines.len(),
                TableKind::Pipeline,
                errors,
            ) && desc.pipelines[command.pipeline.0 as usize].kind != expected
            {
                errors.push(GraphError::PipelineKindMismatch {
                    command: command.name.into(),
                    expected,
                })
            }
            valid(
                command.uniform.0,
                desc.uniforms.len(),
                TableKind::Uniform,
                errors,
            );
            if let Some(push) = command.push {
                valid(
                    push.schema.0,
                    schemas.schemas.len(),
                    TableKind::Schema,
                    errors,
                );
                if let Some(value) = push.data
                    && valid(value.0, desc.values.len(), TableKind::Value, errors)
                    && !matches!(desc.values[value.0 as usize].kind, ValueKind::Bytes { .. })
                {
                    let value = &desc.values[value.0 as usize];
                    errors.push(GraphError::ValueKindMismatch {
                        value: value.name.clone(),
                        expected: "bytes",
                        found: kind(&value.kind),
                    });
                }
                valid_bindings(command.name, &push.bindings, errors);
                check_layout_data(command.name, push.schema, push.data, desc, schemas, errors);

                check_shape(command.name, push.schema, &push.bindings, schemas, errors)
            }
        };
    for tex in &desc.textures {
        match tex.size {
            SizeClass::Window | SizeClass::WindowDiv(_) => {
                errors.push(GraphError::UnsupportedInPhase1 {
                    feature: UnsupportedFeature::WindowSizeClass,
                    at: tex.name.clone(),
                })
            }
            SizeClass::Fixed(width, height) if width == 0 || height == 0 => {
                errors.push(GraphError::TextureExtentZero {
                    texture: tex.name.clone(),
                    width,
                    height,
                })
            }
            _ => {}
        };
        match tex.usage {
            TexUsage::Color => errors.push(GraphError::UnsupportedInPhase1 {
                feature: UnsupportedFeature::ColorAttachmentUsage,
                at: tex.name.clone(),
            }),
            TexUsage::Depth => errors.push(GraphError::UnsupportedInPhase1 {
                feature: UnsupportedFeature::DepthAttachmentUsage,
                at: tex.name.clone(),
            }),
            _ => {}
        }
    }
    for schema in &schemas.schemas {
        check_layout(schema, &mut errors);
    }
    check_push_sizes(desc, schemas, &mut errors);
    check_value_scopes(desc, &mut errors);
    for value in &desc.values {
        let schema = match value.kind {
            ValueKind::Bytes { schema } => Some(schema),
            ValueKind::Array { elem, .. } => Some(elem),
            ValueKind::Count | ValueKind::Groups => None,
        };
        if let Some(schema) = schema {
            valid(
                schema.0,
                schemas.schemas.len(),
                TableKind::Schema,
                &mut errors,
            );
        }
    }
    for pipeline in &desc.pipelines {
        valid(
            pipeline.params.0,
            schemas.schemas.len(),
            TableKind::Schema,
            &mut errors,
        );
        if let Some(push) = pipeline.push {
            valid(
                push.0,
                schemas.schemas.len(),
                TableKind::Schema,
                &mut errors,
            );
        }
    }
    for uniform in &desc.uniforms {
        valid(
            uniform.schema.0,
            schemas.schemas.len(),
            TableKind::Schema,
            &mut errors,
        );
        if let Some(value) = uniform.source.data
            && valid(value.0, desc.values.len(), TableKind::Value, &mut errors)
            && !matches!(desc.values[value.0 as usize].kind, ValueKind::Bytes { .. })
        {
            errors.push(GraphError::ValueKindMismatch {
                value: desc.values[value.0 as usize].name.clone(),
                expected: "bytes",
                found: kind(&desc.values[value.0 as usize].kind),
            })
        }
        valid_bindings(&uniform.name, &uniform.source.bindings, &mut errors);
        check_layout_data(
            &uniform.name,
            uniform.schema,
            uniform.source.data,
            desc,
            schemas,
            &mut errors,
        );
        check_shape(
            &uniform.name,
            uniform.schema,
            &uniform.source.bindings,
            schemas,
            &mut errors,
        );
    }
    for upload in &desc.uploads {
        let buffer_valid = valid(
            upload.buffer.0,
            desc.buffers.len(),
            TableKind::Buffer,
            &mut errors,
        );
        let value_valid = valid(
            upload.value.0,
            desc.values.len(),
            TableKind::Value,
            &mut errors,
        );
        if buffer_valid && value_valid {
            let buffer = &desc.buffers[upload.buffer.0 as usize];
            let value = &desc.values[upload.value.0 as usize];
            // only CPU-updated storage takes per-frame writes; an immutable
            // buffer is never written while a frame may read it
            if buffer.kind != BufferKind::Storage {
                errors.push(GraphError::UploadTargetKind {
                    upload: upload.name.clone(),
                    kind: match buffer.kind {
                        BufferKind::GpuUpdatedFlight => "gpu-updated",
                        BufferKind::Singleton => "singleton",
                        BufferKind::Index => "index",
                        BufferKind::Immutable => "immutable",
                        BufferKind::Storage => "storage",
                    },
                })
            }
            if let ValueKind::Array { max_len, elem } = value.kind {
                if let Some(buffer_elem_size) = buffer.elem_size
                    && let Some(elem_schema) = schemas.get(elem)
                    && elem_schema.size != buffer_elem_size
                {
                    errors.push(GraphError::UploadElemSizeMismatch {
                        upload: upload.name.clone(),
                        value_elem_size: elem_schema.size,
                        buffer_elem_size,
                    })
                }
                if let Some(cap) = buffer.capacity
                    && max_len > cap
                {
                    errors.push(GraphError::UploadTooLarge {
                        upload: upload.name.clone(),
                        max_len,
                        capacity: cap,
                    })
                }
            } else {
                errors.push(GraphError::ValueKindMismatch {
                    value: value.name.clone(),
                    expected: "array",
                    found: kind(&value.kind),
                })
            }
        }
    }
    let mut raster_seen = false;
    for pass in &desc.passes {
        let pass_name = match pass {
            PassDesc::Leaf(LeafPass::Compute(compute)) => &compute.name,
            PassDesc::Leaf(LeafPass::Raster(raster)) => &raster.name,
            PassDesc::When { name, .. } | PassDesc::Repeat { name, .. } => name,
        };
        if raster_seen && !matches!(pass, PassDesc::Leaf(LeafPass::Raster(_))) {
            errors.push(GraphError::PassAfterMainRaster {
                pass: pass_name.clone(),
            });
        }
        match pass {
            PassDesc::When { name, value, body } => {
                if valid(value.0, desc.values.len(), TableKind::Value, &mut errors) {
                    let gate = &desc.values[value.0 as usize];
                    if !gate.optional {
                        errors.push(GraphError::WhenGateNotOptional { when: name.clone() })
                    } else if gate.scope != (ValueScope::When { gate: *value }) {
                        errors.push(GraphError::ValueScopeInvalid {
                            value: gate.name.clone(),
                            reason: "a when pass's gate must be gated by itself",
                        })
                    }
                }
                if body.iter().any(|leaf| matches!(leaf, LeafPass::Raster(_))) {
                    errors.push(GraphError::UnsupportedInPhase1 {
                        feature: UnsupportedFeature::RasterInWhen,
                        at: name.clone(),
                    })
                }
            }
            PassDesc::Repeat { name, count, body } => {
                if valid(count.0, desc.values.len(), TableKind::Value, &mut errors)
                    && !matches!(desc.values[count.0 as usize].kind, ValueKind::Count)
                {
                    let value = &desc.values[count.0 as usize];
                    errors.push(GraphError::ValueKindMismatch {
                        value: value.name.clone(),
                        expected: "count",
                        found: kind(&value.kind),
                    })
                }
                if body.iter().any(|leaf| matches!(leaf, LeafPass::Raster(_))) {
                    errors.push(GraphError::RasterInRepeat {
                        repeat: name.clone(),
                    })
                }
                let rotating: Vec<_> = body
                    .iter()
                    .flat_map(LeafPass::commands)
                    .flat_map(|command| command.bindings(desc))
                    .filter_map(|(_, resource)| match resource {
                        ResourceRef::Tex(tex, TexAccess::Write) => Some(*tex),
                        _ => None,
                    })
                    .collect();
                // a uniform shared by several body commands reports once
                let mut reported: Vec<(UniformId, TexId)> = vec![];
                for leaf in body {
                    if let LeafPass::Compute(compute) = leaf
                        && let Some(uniform) = desc.uniforms.get(compute.uniform.0 as usize)
                    {
                        for (_, resource) in &uniform.source.bindings {
                            if let ResourceRef::Tex(tex, _) = resource
                                && rotating.contains(tex)
                                && !reported.contains(&(compute.uniform, *tex))
                            {
                                reported.push((compute.uniform, *tex));
                                errors.push(GraphError::RepeatUniformRotatesTexture {
                                    repeat: name.clone(),
                                    uniform: uniform.name.clone(),
                                    tex: tex_name(desc, *tex),
                                })
                            }
                        }
                    }
                }
            }
            PassDesc::Leaf(LeafPass::Raster(raster)) => {
                if raster_seen {
                    errors.push(GraphError::UnsupportedInPhase1 {
                        feature: UnsupportedFeature::MultipleRasterPasses,
                        at: raster.name.clone(),
                    })
                }
                raster_seen = true;
            }
            _ => {}
        }
    }
    let mut phys = vec![1; desc.textures.len()];
    let mut prev_readers: Vec<(TexId, String)> = vec![];
    let mut written_anywhere: Vec<TexId> = vec![];
    for leaf in desc.leaves() {
        for command in leaf.commands() {
            let resources = command.bindings(desc);
            for (_, resource) in resources.clone() {
                if let ResourceRef::Tex(tex, TexAccess::ReadPrevious) = resource
                    && (tex.0 as usize) < phys.len()
                {
                    phys[tex.0 as usize] = 2
                }
            }
            let tex_ids = |access| {
                resources
                    .clone()
                    .filter_map(move |(_, resource)| match resource {
                        ResourceRef::Tex(tex, tex_access) if *tex_access == access => Some(*tex),
                        _ => None,
                    })
                    .collect::<Vec<_>>()
            };
            let reads = tex_ids(TexAccess::Read);
            let prev_reads = tex_ids(TexAccess::ReadPrevious);
            let writes = tex_ids(TexAccess::Write);
            let mutates = tex_ids(TexAccess::Mutate);
            for tex in &prev_reads {
                if (tex.0 as usize) < phys.len() && !prev_readers.iter().any(|(t, _)| t == tex) {
                    prev_readers.push((*tex, command.name.into()))
                }
            }
            for (i, tex) in writes.iter().enumerate() {
                if !written_anywhere.contains(tex) {
                    written_anywhere.push(*tex)
                }
                if writes[i + 1..].contains(tex) {
                    errors.push(GraphError::DuplicateWrite {
                        command: command.name.into(),
                        tex: tex_name(desc, *tex),
                    })
                }
                if reads.contains(tex) && (tex.0 as usize) < phys.len() {
                    phys[tex.0 as usize] = 2
                }
                if prev_reads.contains(tex) {
                    errors.push(GraphError::WriteAndPrevRead {
                        command: command.name.into(),
                        tex: tex_name(desc, *tex),
                    })
                }
            }
            for tex in mutates {
                if reads.contains(&tex) {
                    errors.push(GraphError::MutateAndRead {
                        command: command.name.into(),
                        tex: tex_name(desc, tex),
                    })
                }
                if writes.contains(&tex) {
                    errors.push(GraphError::MutateAndWrite {
                        command: command.name.into(),
                        tex: tex_name(desc, tex),
                    })
                }
            }
        }
        match leaf {
            LeafPass::Compute(compute) => {
                if let GroupSource::Value(value) = compute.groups {
                    if valid(value.0, desc.values.len(), TableKind::Value, &mut errors)
                        && !matches!(desc.values[value.0 as usize].kind, ValueKind::Groups)
                    {
                        let value = &desc.values[value.0 as usize];
                        errors.push(GraphError::ValueKindMismatch {
                            value: value.name.clone(),
                            expected: "groups",
                            found: kind(&value.kind),
                        });
                    }
                    errors.push(GraphError::UnsupportedInPhase1 {
                        feature: UnsupportedFeature::GroupSourceValue,
                        at: compute.name.clone(),
                    })
                }
                valid_command(Command::from(compute), PipelineKind::Compute, &mut errors);
            }
            LeafPass::Raster(raster) => {
                if let RasterTargets::Offscreen { color, depth } = &raster.targets {
                    errors.push(GraphError::UnsupportedInPhase1 {
                        feature: UnsupportedFeature::OffscreenTargets,
                        at: raster.name.clone(),
                    });
                    for tex in color.iter().chain(depth.iter()) {
                        valid(tex.0, desc.textures.len(), TableKind::Texture, &mut errors);
                    }
                }
                for draw in &raster.draws {
                    let mut written: Vec<TexId> = vec![];
                    for (_, resource) in Command::from(draw).bindings(desc) {
                        if let ResourceRef::Tex(tex, TexAccess::Write | TexAccess::Mutate) =
                            resource
                            && !written.contains(tex)
                        {
                            written.push(*tex)
                        }
                    }

                    for tex in written {
                        errors.push(GraphError::DrawWritesTexture {
                            draw: draw.name.clone(),
                            tex: tex_name(desc, tex),
                        })
                    }

                    let expected = match draw.call {
                        DrawCall::IndexedIndirect { .. } => PipelineKind::GraphicsIndirect,
                        _ => PipelineKind::Graphics,
                    };
                    valid_command(Command::from(draw), expected, &mut errors);
                    validate_draw_indices(draw, desc, &mut errors);
                    if let DrawCall::IndexedIndirect { args, .. } = &draw.call
                        && valid(
                            args.buffer.0,
                            desc.buffers.len(),
                            TableKind::Buffer,
                            &mut errors,
                        )
                        && desc.buffers[args.buffer.0 as usize].kind != BufferKind::Immutable
                    {
                        errors.push(GraphError::IndirectArgsNotImmutable {
                            draw: draw.name.clone(),
                        })
                    }
                }
            }
        }
    }
    if let Some(picking) = &desc.picking {
        if valid(
            picking.cursor.0,
            desc.values.len(),
            TableKind::Value,
            &mut errors,
        ) {
            let cursor_is_8_bytes = match desc.values[picking.cursor.0 as usize].kind {
                ValueKind::Bytes { schema } => schemas.get(schema).is_some_and(|s| s.size == 8),
                _ => false,
            };
            if !cursor_is_8_bytes {
                errors.push(GraphError::PickingCursorShape);
            }
        }
        let has_draw = desc
            .leaves()
            .any(|leaf| matches!(leaf, LeafPass::Raster(raster) if !raster.draws.is_empty()));
        if !has_draw {
            errors.push(GraphError::PickingWithoutDraw);
        }
    }
    check_shared_uniforms(desc, &mut errors);
    // Only a `Write` advances the run-state cursor, so a previous version
    // exists only for a texture some command writes; a mutate edits in place.
    for (tex, command) in prev_readers {
        if !written_anywhere.contains(&tex) {
            errors.push(GraphError::PrevReadWithoutWrite {
                command,
                tex: tex_name(desc, tex),
            })
        }
    }

    if errors.is_empty() {
        Ok(Analysis { tex_phys: phys })
    } else {
        Err(errors)
    }
}

#[cfg(test)]
mod tests {
    use super::super::desc::{
        BufferId, BufferKind, DrawCall, FieldKey, GroupSource, LeafPass, PassDesc, PipelineKind,
        RasterTargets, ResourceFieldKind, ResourceRef, SchemaDesc, SchemaField, SchemaFieldKind,
        SchemaLayout, SizeClass, TexAccess, TexId, TexUsage, UniformDecl, UniformId,
        UniformSourceDesc, UploadDesc, ValueKind,
    };
    use super::super::test_desc::{
        DescBuilder, bindings, mutate, raster, read, read_previous, write,
    };
    use super::{
        GraphError, TableKind, UnsupportedFeature, extent_limit_errors, validation_message,
    };

    /// one command reading and writing a texture cannot alias the two, so the
    /// texture needs a second physical image
    #[test]
    fn read_write_same_node_needs_two_images() {
        let mut builder = DescBuilder::new(2);
        let dispatch = builder.dispatch("d0", &[read(0), write(0)]);
        builder.leaf(dispatch);

        assert_eq!(builder.tex_phys(), vec![2, 1]);
    }

    #[test]
    fn read_and_write_across_nodes_stays_single_image() {
        let mut builder = DescBuilder::new(1);
        let writer = builder.dispatch("d0", &[write(0)]);
        builder.leaf(writer);
        let reader = builder.dispatch("d1", &[read(0)]);
        builder.leaf(reader);

        assert_eq!(builder.tex_phys(), vec![1]);
    }

    /// the partition spans a command's uniform and push bindings together
    #[test]
    fn push_block_read_write_also_needs_two() {
        let mut builder = DescBuilder::new(1);
        let dispatch = builder.dispatch_with_push("d0", &[read(0)], &[write(0)]);
        builder.leaf(dispatch);

        assert_eq!(builder.tex_phys(), vec![1 + 1]);
    }

    #[test]
    fn prev_read_forces_two_images() {
        let mut builder = DescBuilder::new(1);
        let dispatch = builder.dispatch("d0", &[read_previous(0)]);
        builder.leaf(dispatch);
        let writer = builder.dispatch("d1", &[write(0)]);
        builder.leaf(writer);

        assert_eq!(builder.tex_phys(), vec![2]);
    }

    #[test]
    fn prev_read_and_write_same_node_is_an_error() {
        let mut builder = DescBuilder::new(1);
        let dispatch = builder.dispatch("d0", &[read_previous(0), write(0)]);
        builder.leaf(dispatch);

        assert!(
            builder
                .errors()
                .iter()
                .any(|e| matches!(e, GraphError::WriteAndPrevRead { tex, .. } if tex == "tex0"))
        );
    }

    /// a mutate edits the current version in place, so it does not move the
    /// previous version out from under the read
    #[test]
    fn prev_read_and_mutate_same_node_is_allowed() {
        let mut builder = DescBuilder::new(1);
        let dispatch = builder.dispatch("d0", &[read_previous(0), mutate(0)]);
        builder.leaf(dispatch);
        let writer = builder.dispatch("d1", &[write(0)]);
        builder.leaf(writer);

        assert_eq!(builder.tex_phys(), vec![2]);
    }

    #[test]
    fn mutate_alone_stays_single_image() {
        let mut builder = DescBuilder::new(1);
        let dispatch = builder.dispatch("d0", &[mutate(0)]);
        builder.leaf(dispatch);

        assert_eq!(builder.tex_phys(), vec![1]);
    }

    #[test]
    fn mutate_and_read_same_node_is_an_error() {
        let mut builder = DescBuilder::new(1);
        let dispatch = builder.dispatch("d0", &[read(0), mutate(0)]);
        builder.leaf(dispatch);

        assert!(
            builder
                .errors()
                .iter()
                .any(|e| matches!(e, GraphError::MutateAndRead { tex, .. } if tex == "tex0"))
        );
    }

    #[test]
    fn mutate_and_write_same_node_is_an_error() {
        let mut builder = DescBuilder::new(1);
        let dispatch = builder.dispatch("d0", &[write(0), mutate(0)]);
        builder.leaf(dispatch);

        assert!(
            builder
                .errors()
                .iter()
                .any(|e| matches!(e, GraphError::MutateAndWrite { tex, .. } if tex == "tex0"))
        );
    }

    #[test]
    fn double_write_same_node_is_an_error() {
        let mut builder = DescBuilder::new(1);
        let dispatch = builder.dispatch("d0", &[write(0), write(0)]);
        builder.leaf(dispatch);

        assert!(
            builder
                .errors()
                .iter()
                .any(|e| matches!(e, GraphError::DuplicateWrite { tex, .. } if tex == "tex0"))
        );
    }

    #[test]
    fn duplicate_write_across_uniform_and_push_is_an_error() {
        let mut builder = DescBuilder::new(1);
        let dispatch = builder.dispatch_with_push("dispatch", &[write(0)], &[write(0)]);
        builder.leaf(dispatch);

        assert_eq!(
            builder.errors(),
            vec![GraphError::DuplicateWrite {
                command: "dispatch".into(),
                tex: "tex0".into(),
            }]
        );
    }

    #[test]
    fn missing_command_uniforms_report_errors_without_panicking() {
        let mut builder = DescBuilder::new(0);
        let mut dispatch = builder.dispatch("dispatch", &[]);
        let mut draw = builder.draw("draw", &[]);
        dispatch.uniform = UniformId(9);
        draw.uniform = UniformId(10);
        builder.leaf(dispatch);
        builder.leaf_raster(raster("main", vec![draw]));

        assert_eq!(
            builder.errors(),
            vec![
                GraphError::IdOutOfRange {
                    table: TableKind::Uniform,
                    id: 9
                },
                GraphError::IdOutOfRange {
                    table: TableKind::Uniform,
                    id: 10
                },
            ]
        );
    }

    #[test]
    fn undeclared_texture_is_an_error() {
        let mut builder = DescBuilder::new(1);
        let dispatch = builder.dispatch("d0", &[read(1)]);
        builder.leaf(dispatch);

        assert!(builder.errors().contains(&GraphError::IdOutOfRange {
            table: TableKind::Texture,
            id: 1,
        }));
    }

    /// the jacobi shape: the uniform block is written once for the whole loop,
    /// so it cannot name a texture the loop rotates per iteration
    #[test]
    fn loop_body_uniform_may_not_reference_rotating_texture() {
        let mut builder = DescBuilder::new(1);
        let dispatch = builder.dispatch_with_push("d0", &[read(0)], &[write(0)]);
        builder.repeat("repeat0", vec![LeafPass::Compute(dispatch)]);

        assert!(builder.errors().iter().any(|e| matches!(
            e,
            GraphError::RepeatUniformRotatesTexture { tex, .. } if tex == "tex0"
        )));
    }

    #[test]
    fn later_body_write_also_makes_an_earlier_uniform_stale() {
        let mut builder = DescBuilder::new(1);
        let reader = builder.dispatch("d0", &[read(0)]);
        let writer = builder.dispatch_with_push("d1", &[], &[write(0)]);
        builder.repeat(
            "repeat0",
            vec![LeafPass::Compute(reader), LeafPass::Compute(writer)],
        );

        assert!(builder.errors().iter().any(|e| matches!(
            e,
            GraphError::RepeatUniformRotatesTexture { uniform, tex, .. }
                if uniform == "uniform0" && tex == "tex0"
        )));
    }

    #[test]
    fn loop_body_uniform_may_reference_loop_stable_texture() {
        let mut builder = DescBuilder::new(2);
        let dispatch = builder.dispatch_with_push("d0", &[read(1)], &[read(0), write(0)]);
        builder.repeat("repeat0", vec![LeafPass::Compute(dispatch)]);

        assert_eq!(builder.tex_phys(), vec![2, 1]);
    }

    #[test]
    fn textures_rotated_in_an_earlier_loop_are_stable_in_the_next() {
        let mut builder = DescBuilder::new(2);
        let first = builder.dispatch_with_push("d0", &[], &[read(0), write(0)]);
        builder.repeat("repeat0", vec![LeafPass::Compute(first)]);
        let second = builder.dispatch_with_push("d1", &[read(0)], &[read(1), write(1)]);
        builder.repeat("repeat1", vec![LeafPass::Compute(second)]);

        assert_eq!(builder.tex_phys(), vec![2, 2]);
    }

    /// two dispatches share one uniform block that samples a texture the
    /// first writes through its push block: the second would restage the
    /// block at the next version before the first executes
    #[test]
    fn shared_uniform_may_not_reference_a_texture_written_between_consumers() {
        let mut builder = DescBuilder::new(1);
        let shared = builder.uniform(&[read(0)]);
        let mut first = builder.dispatch_with_push("d0", &[], &[write(0)]);
        first.uniform = shared;
        let mut second = builder.dispatch("d1", &[]);
        second.uniform = shared;
        builder.leaf(first);
        builder.leaf(second);

        assert_eq!(
            builder.errors(),
            vec![GraphError::SharedUniformRotatesTexture {
                uniform: "uniform0".into(),
                writer: "d0".into(),
                tex: "tex0".into(),
            }]
        );
    }

    /// a write before the first consumer or after the last one moves no
    /// consumer's version apart from another's
    #[test]
    fn shared_uniform_writes_outside_its_consumers_are_allowed() {
        let mut builder = DescBuilder::new(1);
        let shared = builder.uniform(&[read(0)]);
        let before = builder.dispatch_with_push("before", &[], &[write(0)]);
        let mut first = builder.dispatch("d0", &[]);
        first.uniform = shared;
        let mut second = builder.dispatch("d1", &[]);
        second.uniform = shared;
        let after = builder.dispatch_with_push("after", &[], &[write(0)]);
        builder.leaf(before);
        builder.leaf(first);
        builder.leaf(second);
        builder.leaf(after);

        assert!(builder.validate().is_ok());
    }

    /// a repeat body's rotation reports once, as the repeat rule
    #[test]
    fn repeat_rotation_of_a_shared_uniform_reports_once() {
        let mut builder = DescBuilder::new(1);
        let shared = builder.uniform(&[read(0)]);
        let mut first = builder.dispatch_with_push("d0", &[], &[write(0)]);
        first.uniform = shared;
        let mut second = builder.dispatch("d1", &[]);
        second.uniform = shared;
        builder.repeat(
            "repeat0",
            vec![LeafPass::Compute(first), LeafPass::Compute(second)],
        );

        let errors = builder.errors();
        assert_eq!(errors.len(), 1, "{errors:?}");
        assert!(matches!(
            errors[0],
            GraphError::RepeatUniformRotatesTexture { .. }
        ));
    }

    #[test]
    fn draw_inside_repeat_is_an_error() {
        let mut builder = DescBuilder::new(1);
        let draw = builder.draw("draw0", &[read(0)]);
        let pass = raster("main", vec![draw]);
        builder.repeat("repeat0", vec![LeafPass::Raster(pass)]);

        assert!(
            builder
                .errors()
                .iter()
                .any(|e| matches!(e, GraphError::RasterInRepeat { .. }))
        );
    }

    #[test]
    fn compute_after_draw_is_an_error() {
        let mut builder = DescBuilder::new(1);
        let draw = builder.draw("draw0", &[read(0)]);
        let pass = raster("main", vec![draw]);
        builder.leaf_raster(pass);
        let dispatch = builder.dispatch("d0", &[mutate(0)]);
        builder.leaf(dispatch);

        assert!(
            builder
                .errors()
                .iter()
                .any(|e| matches!(e, GraphError::PassAfterMainRaster { pass } if pass == "d0"))
        );
    }

    /// a fragment shader's writes are outside the graph's version tracking, so
    /// neither `write` nor `mutate` is legal from a draw
    #[test]
    fn draw_node_may_not_write() {
        for access in [write(0), mutate(0)] {
            let mut builder = DescBuilder::new(1);
            let draw = builder.draw("draw0", &[access]);
            let pass = raster("main", vec![draw]);
            builder.leaf_raster(pass);

            assert!(builder.errors().iter().any(|e| matches!(
                e,
                GraphError::DrawWritesTexture { draw, tex } if draw == "draw0" && tex == "tex0"
            )));
        }
    }

    /// a command is one draw, not the pass that coalesces them: two draws that
    /// each only read cannot conflict with each other
    #[test]
    fn independent_draws_in_one_pass_do_not_merge_accesses() {
        let mut builder = DescBuilder::new(1);
        let writer = builder.dispatch("d0", &[write(0)]);
        builder.leaf(writer);
        let reader = builder.draw("draw0", &[read(0)]);
        let prev_reader = builder.draw("draw1", &[read_previous(0)]);
        let pass = raster("main", vec![reader, prev_reader]);
        builder.leaf_raster(pass);

        assert_eq!(builder.tex_phys(), vec![2]);
    }

    #[test]
    fn two_consumers_of_one_uniform_id_is_legal() {
        let mut builder = DescBuilder::new(1);
        let shared = builder.uniform(&[read(0)]);
        let mut first = builder.dispatch("d0", &[]);
        first.uniform = shared;
        let mut second = builder.dispatch("d1", &[]);
        second.uniform = shared;
        builder.leaf(first);
        builder.leaf(second);

        assert_eq!(builder.tex_phys(), vec![1]);
    }

    /// resource identity is checked on the declaration, so a source with zero
    /// consumers is still checked
    #[test]
    fn unconsumed_uniform_source_is_still_checked() {
        let mut builder = DescBuilder::new(1);
        let schema = builder.schemas.push(SchemaDesc {
            name: "orphan".into(),
            size: 16,
            resource_fields: vec![ResourceFieldKind::SampledTex],
            layout: None,
        });
        builder.desc.uniforms.push(UniformDecl {
            name: "orphan".into(),
            schema,
            source: UniformSourceDesc {
                data: None,
                bindings: bindings(&[write(0)]),
            },
        });

        assert!(builder.errors().contains(&GraphError::BindingKindMismatch {
            uniform: "orphan".into(),
            field: 0,
        }));
    }

    #[test]
    fn window_size_class_rejected_until_phase5() {
        let mut builder = DescBuilder::new(1);
        builder.desc.textures[0].size = SizeClass::Window;

        assert!(builder.errors().contains(&GraphError::UnsupportedInPhase1 {
            feature: UnsupportedFeature::WindowSizeClass,
            at: "tex0".into(),
        }));
    }

    #[test]
    fn window_div_rejected_until_phase5() {
        let mut builder = DescBuilder::new(1);
        builder.desc.textures[0].size = SizeClass::WindowDiv(2);

        assert!(matches!(
            builder.desc.textures[0].size,
            SizeClass::WindowDiv(2)
        ));
        assert!(builder.errors().contains(&GraphError::UnsupportedInPhase1 {
            feature: UnsupportedFeature::WindowSizeClass,
            at: "tex0".into(),
        }));
    }

    #[test]
    fn color_usage_rejected_until_phase5() {
        let mut builder = DescBuilder::new(1);
        builder.desc.textures[0].usage = TexUsage::Color;

        assert!(builder.errors().contains(&GraphError::UnsupportedInPhase1 {
            feature: UnsupportedFeature::ColorAttachmentUsage,
            at: "tex0".into(),
        }));
    }

    #[test]
    fn depth_usage_rejected_until_phase5() {
        let mut builder = DescBuilder::new(1);
        builder.desc.textures[0].usage = TexUsage::Depth;

        assert!(builder.errors().contains(&GraphError::UnsupportedInPhase1 {
            feature: UnsupportedFeature::DepthAttachmentUsage,
            at: "tex0".into(),
        }));
    }

    #[test]
    fn offscreen_targets_rejected_until_phase5() {
        let mut builder = DescBuilder::new(1);
        let draw = builder.draw("draw0", &[read(0)]);
        let mut pass = raster("main", vec![draw]);
        pass.targets = RasterTargets::Offscreen {
            color: vec![TexId(0)],
            depth: Some(TexId(0)),
        };
        builder.leaf_raster(pass);

        let PassDesc::Leaf(LeafPass::Raster(stored)) = &builder.desc.passes[0] else {
            panic!("expected a raster pass")
        };
        let RasterTargets::Offscreen { color, depth } = &stored.targets else {
            panic!("expected offscreen targets")
        };
        assert_eq!(color, &vec![TexId(0)]);
        assert_eq!(depth, &Some(TexId(0)));
        assert!(builder.errors().contains(&GraphError::UnsupportedInPhase1 {
            feature: UnsupportedFeature::OffscreenTargets,
            at: "main".into(),
        }));
    }

    #[test]
    fn second_raster_pass_rejected_until_phase5() {
        let mut builder = DescBuilder::new(1);
        let first = builder.draw("draw0", &[read(0)]);
        let first_pass = raster("main", vec![first]);
        builder.leaf_raster(first_pass);
        let second = builder.draw("draw1", &[read(0)]);
        let second_pass = raster("second", vec![second]);
        builder.leaf_raster(second_pass);

        assert!(builder.errors().contains(&GraphError::UnsupportedInPhase1 {
            feature: UnsupportedFeature::MultipleRasterPasses,
            at: "second".into(),
        }));
    }

    #[test]
    fn raster_in_when_rejected_until_phase5() {
        let mut builder = DescBuilder::new(1);
        let draw = builder.draw("draw0", &[read(0)]);
        let pass = raster("main", vec![draw]);
        builder.when("when0", vec![LeafPass::Raster(pass)]);

        assert!(builder.errors().contains(&GraphError::UnsupportedInPhase1 {
            feature: UnsupportedFeature::RasterInWhen,
            at: "when0".into(),
        }));
    }

    #[test]
    fn group_source_value_rejected_until_phase3a() {
        let mut builder = DescBuilder::new(1);
        let groups = builder.value(ValueKind::Groups, false);
        let mut dispatch = builder.dispatch("d0", &[read(0)]);
        dispatch.groups = GroupSource::Value(groups);
        builder.leaf(dispatch);

        assert!(builder.errors().contains(&GraphError::UnsupportedInPhase1 {
            feature: UnsupportedFeature::GroupSourceValue,
            at: "d0".into(),
        }));
    }

    #[test]
    fn upload_to_gpu_updated_rejects() {
        let mut builder = DescBuilder::new(0);
        let buffer = builder.buffer_decl(BufferKind::GpuUpdatedFlight, Some(4));
        let elem = builder.schemas.push(SchemaDesc {
            name: "elem".into(),
            size: 4,
            resource_fields: vec![],
            layout: None,
        });
        let value = builder.value(ValueKind::Array { elem, max_len: 4 }, false);
        builder.desc.uploads.push(UploadDesc {
            name: "upload0".into(),
            buffer: super::super::desc::BufferId(buffer),
            value,
        });

        assert!(builder.errors().contains(&GraphError::UploadTargetKind {
            upload: "upload0".into(),
            kind: "gpu-updated",
        }));
    }

    #[test]
    fn upload_to_singleton_rejects() {
        let mut builder = DescBuilder::new(0);
        let buffer = builder.buffer_decl(BufferKind::Singleton, Some(4));
        let elem = builder.schemas.push(SchemaDesc {
            name: "elem".into(),
            size: 4,
            resource_fields: vec![],
            layout: None,
        });
        let value = builder.value(ValueKind::Array { elem, max_len: 4 }, false);
        builder.desc.uploads.push(UploadDesc {
            name: "upload0".into(),
            buffer: super::super::desc::BufferId(buffer),
            value,
        });

        assert!(builder.errors().contains(&GraphError::UploadTargetKind {
            upload: "upload0".into(),
            kind: "singleton",
        }));
    }

    #[test]
    fn when_gate_must_be_optional() {
        let mut builder = DescBuilder::new(1);
        let dispatch = builder.dispatch("d0", &[read(0)]);
        let value = builder.value(ValueKind::Count, false);
        builder.desc.passes.push(PassDesc::When {
            name: "when0".into(),
            value,
            body: vec![LeafPass::Compute(dispatch)],
        });

        assert!(builder.errors().contains(&GraphError::WhenGateNotOptional {
            when: "when0".into(),
        }));
    }

    #[test]
    fn repeat_count_must_be_count_kind() {
        let mut builder = DescBuilder::new(1);
        let dispatch = builder.dispatch("d0", &[read(0)]);
        let count = builder.value(ValueKind::Groups, false);
        builder.desc.passes.push(PassDesc::Repeat {
            name: "repeat0".into(),
            count,
            body: vec![LeafPass::Compute(dispatch)],
        });

        assert!(builder.errors().iter().any(|e| matches!(
            e,
            GraphError::ValueKindMismatch {
                expected: "count",
                found: "groups",
                ..
            }
        )));
    }

    /// indirect args must be immutable, because the graph does not version them
    #[test]
    fn indirect_args_must_be_immutable() {
        let mut builder = DescBuilder::new(0);
        let indices = builder.buffer_decl(BufferKind::Index, Some(3));
        let buffer = builder.buffer_decl(BufferKind::Storage, Some(4));
        let mut draw = builder.draw("draw0", &[]);
        draw.call = DrawCall::IndexedIndirect {
            indices: BufferId(indices),
            args: match super::super::test_desc::buffer(buffer) {
                ResourceRef::Buf(args, _) => args,
                _ => unreachable!(),
            },
            draw_count: 1,
        };
        let pass = raster("main", vec![draw]);
        builder.leaf_raster(pass);

        assert!(
            builder
                .errors()
                .contains(&GraphError::IndirectArgsNotImmutable {
                    draw: "draw0".into(),
                })
        );
    }

    /// an indexed draw's indices must name a hardware index buffer
    #[test]
    fn draw_indices_must_be_an_index_buffer() {
        let mut builder = DescBuilder::new(0);
        let storage = builder.buffer_decl(BufferKind::Storage, Some(4));
        let mut draw = builder.draw("draw0", &[]);
        draw.call = DrawCall::WholeIndexed {
            indices: BufferId(storage),
        };
        builder.leaf_raster(raster("main", vec![draw]));

        assert!(
            builder
                .errors()
                .contains(&GraphError::DrawIndicesNotIndexBuffer {
                    draw: "draw0".into(),
                })
        );
    }

    /// a compute node bound to a graphics pipeline, and the reverse
    #[test]
    fn pipeline_kind_must_match_the_command() {
        let mut builder = DescBuilder::new(1);
        let mut dispatch = builder.dispatch("d0", &[read(0)]);
        let graphics = builder.draw("draw0", &[read(0)]);
        dispatch.pipeline = graphics.pipeline;
        builder.leaf(dispatch);

        assert!(
            builder
                .errors()
                .contains(&GraphError::PipelineKindMismatch {
                    command: "d0".into(),
                    expected: PipelineKind::Compute,
                })
        );
    }

    /// `TexAccess` is the version-tracking vocabulary; the display uses it
    #[test]
    fn error_messages_keep_their_key_phrases() {
        assert!(
            GraphError::MutateAndRead {
                command: "d0".into(),
                tex: "tex3".into(),
            }
            .to_string()
            .contains("mutates and reads texture tex3")
        );
        assert!(
            GraphError::DrawWritesTexture {
                draw: "draw0".into(),
                tex: "tex1".into(),
            }
            .to_string()
            .contains("can only read graph textures")
        );
        assert!(
            GraphError::IdOutOfRange {
                table: TableKind::Texture,
                id: 7,
            }
            .to_string()
            .contains("texture id 7 is undeclared")
        );
        let _ = (TexAccess::Read, UniformId(0));
    }

    #[test]
    fn zero_extent_texture_is_an_error() {
        let mut builder = DescBuilder::new(1);
        builder.desc.textures[0].size = SizeClass::Fixed(0, 8);

        assert!(builder.errors().contains(&GraphError::TextureExtentZero {
            texture: "tex0".into(),
            width: 0,
            height: 8,
        }));
    }

    #[test]
    fn extent_over_device_limit_is_an_error() {
        let builder = DescBuilder::new(1);

        assert!(extent_limit_errors(&builder.desc.textures, 8).is_empty());
        assert_eq!(
            extent_limit_errors(&builder.desc.textures, 4),
            vec![GraphError::TextureExtentTooLarge {
                texture: "tex0".into(),
                width: 8,
                height: 8,
                max: 4,
            }]
        );
    }

    #[test]
    fn prev_read_without_any_write_is_an_error() {
        let mut builder = DescBuilder::new(1);
        let dispatch = builder.dispatch("d0", &[read_previous(0)]);
        builder.leaf(dispatch);

        assert!(
            builder
                .errors()
                .contains(&GraphError::PrevReadWithoutWrite {
                    command: "d0".into(),
                    tex: "tex0".into(),
                })
        );
    }

    /// a mutate edits in place and never advances the cursor, so it cannot
    /// produce a previous version
    #[test]
    fn prev_read_with_mutate_only_is_an_error() {
        let mut builder = DescBuilder::new(1);
        let producer = builder.dispatch("d0", &[mutate(0)]);
        builder.leaf(producer);
        let reader = builder.dispatch("d1", &[read_previous(0)]);
        builder.leaf(reader);

        assert!(
            builder
                .errors()
                .contains(&GraphError::PrevReadWithoutWrite {
                    command: "d1".into(),
                    tex: "tex0".into(),
                })
        );
    }

    /// frame N's write is frame N+1's previous version, so the write may
    /// follow the read in pass order
    #[test]
    fn prev_read_before_the_write_is_allowed() {
        let mut builder = DescBuilder::new(1);
        let reader = builder.dispatch("d0", &[read_previous(0)]);
        builder.leaf(reader);
        let writer = builder.dispatch("d1", &[write(0)]);
        builder.leaf(writer);

        assert_eq!(builder.tex_phys(), vec![2]);
    }

    /// an orphan uniform never executes, so its write produces no version
    #[test]
    fn orphan_uniform_write_does_not_satisfy_prev_read() {
        let mut builder = DescBuilder::new(1);
        builder.uniform(&[write(0)]);
        let reader = builder.dispatch("d0", &[read_previous(0)]);
        builder.leaf(reader);

        assert!(
            builder
                .errors()
                .contains(&GraphError::PrevReadWithoutWrite {
                    command: "d0".into(),
                    tex: "tex0".into(),
                })
        );
    }

    #[test]
    fn unconsumed_uniform_ids_are_bounds_checked() {
        let mut builder = DescBuilder::new(1);
        builder.uniform(&[read(5)]);

        assert!(builder.errors().contains(&GraphError::IdOutOfRange {
            table: TableKind::Texture,
            id: 5,
        }));
    }

    #[test]
    fn consumed_uniform_ids_report_once() {
        let mut builder = DescBuilder::new(1);
        let shared = builder.uniform(&[read(9)]);
        let mut first = builder.dispatch("d0", &[]);
        first.uniform = shared;
        let mut second = builder.dispatch("d1", &[]);
        second.uniform = shared;
        builder.leaf(first);
        builder.leaf(second);

        let out_of_range = builder
            .errors()
            .iter()
            .filter(|e| {
                matches!(
                    e,
                    GraphError::IdOutOfRange {
                        table: TableKind::Texture,
                        id: 9,
                    }
                )
            })
            .count();
        assert_eq!(out_of_range, 1);
    }

    #[test]
    fn push_binding_ids_are_bounds_checked() {
        let mut builder = DescBuilder::new(1);
        let dispatch = builder.dispatch_with_push("d0", &[], &[read(3)]);
        builder.leaf(dispatch);

        let out_of_range = builder
            .errors()
            .iter()
            .filter(|e| {
                matches!(
                    e,
                    GraphError::IdOutOfRange {
                        table: TableKind::Texture,
                        id: 3,
                    }
                )
            })
            .count();
        assert_eq!(out_of_range, 1);
    }

    #[test]
    fn compute_and_draw_push_data_require_declared_bytes() {
        use super::super::desc::{PushDesc, ValueId};

        #[derive(Clone, Copy, Debug)]
        enum PushDataCase {
            Missing,
            WrongKind,
            Bytes,
            None,
        }

        for graphics in [false, true] {
            for case in [
                PushDataCase::Missing,
                PushDataCase::WrongKind,
                PushDataCase::Bytes,
                PushDataCase::None,
            ] {
                let mut builder = DescBuilder::new(0);
                let mut dispatch = builder.dispatch("dispatch", &[]);
                let schema = builder.desc.uniforms[dispatch.uniform.0 as usize].schema;
                let (data, expected) = match case {
                    PushDataCase::Missing => (
                        Some(ValueId(99)),
                        Some(GraphError::IdOutOfRange {
                            table: TableKind::Value,
                            id: 99,
                        }),
                    ),
                    PushDataCase::WrongKind => {
                        let value = builder.value(ValueKind::Count, false);

                        (
                            Some(value),
                            Some(GraphError::ValueKindMismatch {
                                value: builder.desc.values[value.0 as usize].name.clone(),
                                expected: "bytes",
                                found: "count",
                            }),
                        )
                    }
                    PushDataCase::Bytes => (
                        Some(builder.value(ValueKind::Bytes { schema }, false)),
                        None,
                    ),
                    PushDataCase::None => (None, None),
                };
                let push = Some(PushDesc {
                    data,
                    schema,
                    bindings: vec![],
                });
                if graphics {
                    let mut draw = builder.draw("draw", &[]);
                    draw.push = push;
                    builder.leaf_raster(raster("main", vec![draw]));
                } else {
                    dispatch.push = push;
                    builder.leaf(dispatch);
                }

                match expected {
                    Some(error) => {
                        assert_eq!(builder.errors(), vec![error], "{graphics}: {case:?}")
                    }
                    None => assert!(builder.validate().is_ok(), "{graphics}: {case:?}"),
                }
            }
        }
    }

    #[test]
    fn offscreen_target_ids_are_bounds_checked() {
        let mut builder = DescBuilder::new(1);
        let draw = builder.draw("draw0", &[read(0)]);
        let mut pass = raster("main", vec![draw]);
        pass.targets = RasterTargets::Offscreen {
            color: vec![TexId(7)],
            depth: Some(TexId(8)),
        };
        builder.leaf_raster(pass);

        let errors = builder.errors();
        for id in [7, 8] {
            assert!(errors.contains(&GraphError::IdOutOfRange {
                table: TableKind::Texture,
                id,
            }));
        }
    }

    #[test]
    fn offscreen_targets_in_when_body_rejected() {
        let mut builder = DescBuilder::new(1);
        let draw = builder.draw("draw0", &[read(0)]);
        let mut pass = raster("main", vec![draw]);
        pass.targets = RasterTargets::Offscreen {
            color: vec![TexId(0)],
            depth: None,
        };
        builder.when("when0", vec![LeafPass::Raster(pass)]);

        assert!(builder.errors().contains(&GraphError::UnsupportedInPhase1 {
            feature: UnsupportedFeature::OffscreenTargets,
            at: "main".into(),
        }));
    }

    #[test]
    fn offscreen_targets_in_repeat_body_rejected() {
        let mut builder = DescBuilder::new(1);
        let draw = builder.draw("draw0", &[read(0)]);
        let mut pass = raster("main", vec![draw]);
        pass.targets = RasterTargets::Offscreen {
            color: vec![TexId(0)],
            depth: None,
        };
        builder.repeat("repeat0", vec![LeafPass::Raster(pass)]);

        assert!(builder.errors().contains(&GraphError::UnsupportedInPhase1 {
            feature: UnsupportedFeature::OffscreenTargets,
            at: "main".into(),
        }));
    }

    /// a second raster pass is one defect with one diagnostic
    #[test]
    fn second_raster_pass_reports_one_error() {
        let mut builder = DescBuilder::new(1);
        let first = builder.draw("draw0", &[read(0)]);
        builder.leaf_raster(raster("main", vec![first]));
        let second = builder.draw("draw1", &[read(0)]);
        builder.leaf_raster(raster("second", vec![second]));

        let errors = builder.errors();
        assert!(errors.contains(&GraphError::UnsupportedInPhase1 {
            feature: UnsupportedFeature::MultipleRasterPasses,
            at: "second".into(),
        }));
        assert!(
            errors
                .iter()
                .all(|e| !matches!(e, GraphError::PassAfterMainRaster { .. }))
        );
    }

    #[test]
    fn unsupported_feature_display_names_phase_and_ledger() {
        assert_eq!(
            UnsupportedFeature::OffscreenTargets.to_string(),
            "offscreen raster targets (phase 5, ledger S6)"
        );
        assert_eq!(
            UnsupportedFeature::GroupSourceValue.to_string(),
            "per-frame dispatch group counts (phase 3a, ledger S1)"
        );
    }

    fn layout_field(key: u16, offset: u32, len: u32, kind: SchemaFieldKind) -> SchemaField {
        SchemaField {
            key: FieldKey(key),
            offset,
            len,
            kind,
        }
    }

    /// a dispatch whose uniform schema carries `fields`; returns the builder
    /// and the uniform's name
    fn laid_out_dispatch(resources: &[ResourceRef], fields: Vec<SchemaField>) -> DescBuilder {
        let mut builder = DescBuilder::new(2);
        let dispatch = builder.dispatch("d0", resources);
        let schema = builder.desc.uniforms[dispatch.uniform.0 as usize].schema;
        builder.schemas.schemas[schema.0 as usize].layout = Some(SchemaLayout { fields });
        builder.leaf(dispatch);

        builder
    }

    #[test]
    fn a_well_formed_layout_validates() {
        let builder = laid_out_dispatch(
            &[read(0)],
            vec![
                layout_field(0, 0, 8, SchemaFieldKind::SampledTex),
                layout_field(1, 8, 4, SchemaFieldKind::Data { src_offset: 12 }),
            ],
        );

        assert!(builder.validate().is_ok());
    }

    #[test]
    fn layout_fields_must_fit_the_block() {
        for (offset, len) in [(12, 8), (u32::MAX, 2)] {
            let builder = laid_out_dispatch(
                &[],
                vec![layout_field(
                    0,
                    offset,
                    len,
                    SchemaFieldKind::Data { src_offset: 0 },
                )],
            );

            assert!(
                builder
                    .errors()
                    .contains(&GraphError::LayoutFieldOutOfBounds {
                        schema: "uniform0".into(),
                        field: 0,
                        offset,
                        len,
                        size: 16,
                    })
            );
        }
    }

    /// overlap is measured against the furthest earlier field, not only the
    /// adjacent one
    #[test]
    fn layout_fields_must_not_overlap() {
        let builder = laid_out_dispatch(
            &[],
            vec![
                layout_field(0, 0, 12, SchemaFieldKind::Data { src_offset: 0 }),
                layout_field(1, 4, 2, SchemaFieldKind::Data { src_offset: 0 }),
                layout_field(2, 8, 4, SchemaFieldKind::Data { src_offset: 0 }),
                layout_field(3, 12, 4, SchemaFieldKind::Data { src_offset: 0 }),
            ],
        );

        assert_eq!(
            builder.errors(),
            vec![
                GraphError::LayoutFieldsOverlap {
                    schema: "uniform0".into(),
                    first: 0,
                    second: 1,
                },
                GraphError::LayoutFieldsOverlap {
                    schema: "uniform0".into(),
                    first: 0,
                    second: 2,
                },
            ]
        );
    }

    /// `DescriptorHandle` is a `uint2` and a device address a `uint64_t`
    #[test]
    fn resource_fields_are_eight_bytes() {
        for kind in [SchemaFieldKind::SampledTex, SchemaFieldKind::StorageTex] {
            let resource = match kind {
                SchemaFieldKind::SampledTex => read(0),
                _ => write(0),
            };
            let builder = laid_out_dispatch(&[resource], vec![layout_field(0, 0, 4, kind)]);

            assert_eq!(
                builder.errors(),
                vec![GraphError::LayoutResourceFieldSize {
                    schema: "uniform0".into(),
                    field: 0,
                    len: 4,
                }]
            );
        }
    }

    #[test]
    fn layout_resource_fields_match_the_resource_list() {
        let missing = laid_out_dispatch(&[read(0)], vec![]);
        assert_eq!(
            missing.errors(),
            vec![GraphError::LayoutResourceFieldMismatch {
                schema: "uniform0".into(),
                field: 0,
            }]
        );

        let wrong_kind = laid_out_dispatch(
            &[read(0)],
            vec![layout_field(0, 0, 8, SchemaFieldKind::BufAddr)],
        );
        assert!(
            wrong_kind
                .errors()
                .contains(&GraphError::LayoutResourceFieldMismatch {
                    schema: "uniform0".into(),
                    field: 0,
                })
        );

        let duplicate = laid_out_dispatch(
            &[read(0)],
            vec![
                layout_field(0, 0, 8, SchemaFieldKind::SampledTex),
                layout_field(0, 8, 8, SchemaFieldKind::SampledTex),
            ],
        );
        assert_eq!(
            duplicate.errors(),
            vec![GraphError::LayoutResourceFieldMismatch {
                schema: "uniform0".into(),
                field: 0,
            }]
        );
    }

    #[test]
    fn data_fields_read_inside_the_data_value() {
        let builder = laid_out_dispatch(
            &[],
            vec![layout_field(
                0,
                0,
                8,
                SchemaFieldKind::Data { src_offset: 12 },
            )],
        );

        assert_eq!(
            builder.errors(),
            vec![GraphError::LayoutDataOutOfBounds {
                block: "uniform0".into(),
                field: 0,
                src_offset: 12,
                len: 8,
                data_size: 16,
            }]
        );
    }

    #[test]
    fn data_fields_need_a_data_value() {
        let mut builder = laid_out_dispatch(
            &[],
            vec![layout_field(
                0,
                0,
                4,
                SchemaFieldKind::Data { src_offset: 0 },
            )],
        );
        builder.desc.uniforms[0].source.data = None;

        assert_eq!(
            builder.errors(),
            vec![GraphError::LayoutDataWithoutValue {
                block: "uniform0".into(),
            }]
        );
    }

    #[test]
    fn push_blocks_fit_the_guaranteed_budget() {
        let mut builder = DescBuilder::new(1);
        let dispatch = builder.dispatch_with_push("d0", &[], &[read(0)]);
        let schema = dispatch.push.as_ref().unwrap().schema;
        let pipeline = dispatch.pipeline;
        builder.leaf(dispatch);
        // the pipeline names the same schema; it still reports once
        builder.desc.pipelines[pipeline.0 as usize].push = Some(schema);
        for size in [4, 128] {
            builder.schemas.schemas[schema.0 as usize].size = size;
            assert!(builder.validate().is_ok(), "{size}");
        }

        for size in [0, 6, 132] {
            builder.schemas.schemas[schema.0 as usize].size = size;
            assert_eq!(
                builder.errors(),
                vec![GraphError::PushConstantSize {
                    schema: "d0.push".into(),
                    size,
                }]
            );
        }
    }

    /// a pipeline's push schema is checked even when no command uses it
    #[test]
    fn unused_pipeline_push_blocks_are_checked() {
        let mut builder = DescBuilder::new(0);
        let dispatch = builder.dispatch("d0", &[]);
        let schema = builder.schemas.push(SchemaDesc {
            name: "orphan.push".into(),
            size: 2,
            resource_fields: vec![],
            layout: None,
        });
        builder.desc.pipelines[dispatch.pipeline.0 as usize].push = Some(schema);

        assert_eq!(
            builder.errors(),
            vec![GraphError::PushConstantSize {
                schema: "orphan.push".into(),
                size: 2,
            }]
        );
    }

    #[test]
    fn value_scopes_name_well_formed_gates_and_counts() {
        use super::super::desc::{ValueId, ValueScope};

        let scope_error = |scope: ValueScope, optional: bool| {
            let mut builder = DescBuilder::new(0);
            let gate = builder.value(ValueKind::Count, true);
            let count = builder.value(ValueKind::Count, false);
            let groups = builder.value(ValueKind::Groups, false);
            let value = builder.value(ValueKind::Count, optional);
            builder.desc.values[value.0 as usize].scope = match scope {
                ValueScope::When { gate: ValueId(0) } => ValueScope::When { gate },
                ValueScope::Repeat { count: ValueId(1) } => ValueScope::Repeat { count },
                ValueScope::Repeat { count: ValueId(2) } => ValueScope::Repeat { count: groups },
                other => other,
            };

            builder.validate().err().unwrap_or_default()
        };
        let reason = |errors: Vec<GraphError>| match errors.as_slice() {
            [GraphError::ValueScopeInvalid { reason, .. }] => *reason,
            other => panic!("expected one scope error: {other:?}"),
        };

        assert!(scope_error(ValueScope::When { gate: ValueId(0) }, true).is_empty());
        assert!(scope_error(ValueScope::Repeat { count: ValueId(1) }, false).is_empty());
        assert_eq!(
            reason(scope_error(ValueScope::When { gate: ValueId(0) }, false)),
            "a value in an optional scope must be optional"
        );
        assert_eq!(
            reason(scope_error(ValueScope::When { gate: ValueId(1) }, true)),
            "its gate is not gated by itself"
        );
        assert_eq!(
            reason(scope_error(ValueScope::When { gate: ValueId(9) }, true)),
            "its gate is undeclared"
        );
        assert_eq!(
            reason(scope_error(ValueScope::Repeat { count: ValueId(1) }, true)),
            "a value in a repeat cannot be optional"
        );
        assert_eq!(
            reason(scope_error(ValueScope::Repeat { count: ValueId(2) }, false)),
            "its repeat count is not a top-level count"
        );
        assert_eq!(
            reason(scope_error(ValueScope::Repeat { count: ValueId(9) }, false)),
            "its repeat count is undeclared"
        );
    }

    /// a when pass gates on a value that is gated by itself
    #[test]
    fn when_gates_are_self_gated() {
        use super::super::desc::ValueScope;

        let mut builder = DescBuilder::new(0);
        let outer = builder.value(ValueKind::Count, true);
        let dispatch = builder.dispatch("d0", &[]);
        let gate = builder.when("when0", vec![LeafPass::Compute(dispatch)]);
        builder.desc.values[gate.0 as usize].scope = ValueScope::When { gate: outer };

        assert!(builder.errors().contains(&GraphError::ValueScopeInvalid {
            value: format!("value{}", gate.0),
            reason: "a when pass's gate must be gated by itself",
        }));
    }

    /// direct draws and indexed-indirect draws use different pipeline kinds
    #[test]
    fn indirect_draws_need_graphics_indirect_pipelines() {
        let mut builder = DescBuilder::new(0);
        let indices = builder.buffer_decl(BufferKind::Index, Some(3));
        let args = builder.buffer_decl(BufferKind::Immutable, Some(1));
        let mut indirect = builder.draw("indirect", &[]);
        indirect.call = DrawCall::IndexedIndirect {
            indices: BufferId(indices),
            args: match super::super::test_desc::buffer(args) {
                ResourceRef::Buf(args, _) => args,
                _ => unreachable!(),
            },
            draw_count: 1,
        };
        let mut direct = builder.draw("direct", &[]);
        direct.pipeline = indirect.pipeline;
        builder.leaf_raster(raster("main", vec![indirect, direct]));

        assert_eq!(
            builder.errors(),
            vec![GraphError::PipelineKindMismatch {
                command: "indirect".into(),
                expected: PipelineKind::GraphicsIndirect,
            }]
        );

        let PassDesc::Leaf(LeafPass::Raster(raster)) = &builder.desc.passes[0] else {
            unreachable!()
        };
        let pipeline = raster.draws[0].pipeline;
        builder.desc.pipelines[pipeline.0 as usize].kind = PipelineKind::GraphicsIndirect;
        assert_eq!(
            builder.errors(),
            vec![GraphError::PipelineKindMismatch {
                command: "direct".into(),
                expected: PipelineKind::Graphics,
            }]
        );
    }

    #[test]
    fn upload_to_immutable_rejects() {
        let mut builder = DescBuilder::new(0);
        let buffer = builder.buffer_decl(BufferKind::Immutable, Some(4));
        let elem = builder.schemas.push(SchemaDesc {
            name: "elem".into(),
            size: 4,
            resource_fields: vec![],
            layout: None,
        });
        let value = builder.value(ValueKind::Array { elem, max_len: 4 }, false);
        builder.desc.uploads.push(UploadDesc {
            name: "upload0".into(),
            buffer: BufferId(buffer),
            value,
        });

        assert_eq!(
            builder.errors(),
            vec![GraphError::UploadTargetKind {
                upload: "upload0".into(),
                kind: "immutable",
            }]
        );
    }

    #[test]
    fn upload_elements_match_the_buffer_stride() {
        let mut builder = DescBuilder::new(0);
        let buffer = builder.buffer_decl(BufferKind::Storage, Some(4));
        let elem = builder.schemas.push(SchemaDesc {
            name: "elem".into(),
            size: 12,
            resource_fields: vec![],
            layout: None,
        });
        let value = builder.value(ValueKind::Array { elem, max_len: 4 }, false);
        builder.desc.uploads.push(UploadDesc {
            name: "upload0".into(),
            buffer: BufferId(buffer),
            value,
        });

        assert_eq!(
            builder.errors(),
            vec![GraphError::UploadElemSizeMismatch {
                upload: "upload0".into(),
                value_elem_size: 12,
                buffer_elem_size: 4,
            }]
        );
    }

    #[test]
    fn buffer_bindings_use_supported_slots_and_accesses() {
        use super::super::desc::{BufAccess, BufferRef, SlotSel};

        let cases = [
            (BufferKind::Index, SlotSel::Current, BufAccess::Read, false),
            (
                BufferKind::Immutable,
                SlotSel::Current,
                BufAccess::Mutate,
                false,
            ),
            (
                BufferKind::Singleton,
                SlotSel::Previous,
                BufAccess::Read,
                false,
            ),
            (
                BufferKind::Storage,
                SlotSel::Previous,
                BufAccess::Read,
                false,
            ),
            (
                BufferKind::GpuUpdatedFlight,
                SlotSel::Previous,
                BufAccess::Mutate,
                false,
            ),
            (
                BufferKind::GpuUpdatedFlight,
                SlotSel::Previous,
                BufAccess::Read,
                true,
            ),
            (
                BufferKind::GpuUpdatedFlight,
                SlotSel::Current,
                BufAccess::Mutate,
                true,
            ),
            (
                BufferKind::Storage,
                SlotSel::Current,
                BufAccess::Mutate,
                true,
            ),
            (
                BufferKind::Immutable,
                SlotSel::Current,
                BufAccess::Read,
                true,
            ),
        ];
        for (kind, slot, access, supported) in cases {
            let mut builder = DescBuilder::new(0);
            let buffer = builder.buffer_decl(kind, Some(4));
            let binding = ResourceRef::Buf(
                BufferRef {
                    buffer: BufferId(buffer),
                    slot,
                    offset: 0,
                    range: None,
                },
                access,
            );
            let dispatch = builder.dispatch("d0", &[binding]);
            builder.leaf(dispatch);

            let expected = if supported {
                vec![]
            } else {
                vec![GraphError::BufferAccessMismatch {
                    block: "uniform0".into(),
                    field: 0,
                    kind,
                }]
            };
            assert_eq!(
                builder.validate().err().unwrap_or_default(),
                expected,
                "{kind:?} {slot:?} {access:?}"
            );
        }
    }

    #[test]
    fn picking_needs_an_eight_byte_cursor_and_a_draw() {
        use super::super::desc::PickingDesc;

        let mut builder = DescBuilder::new(0);
        let cursor_schema = builder.schemas.push(SchemaDesc {
            name: "cursor".into(),
            size: 8,
            resource_fields: vec![],
            layout: None,
        });
        let cursor = builder.value(
            ValueKind::Bytes {
                schema: cursor_schema,
            },
            false,
        );
        builder.desc.picking = Some(PickingDesc { cursor });
        assert_eq!(builder.errors(), vec![GraphError::PickingWithoutDraw]);

        let draw = builder.draw("draw0", &[]);
        builder.leaf_raster(raster("main", vec![draw]));
        assert!(builder.validate().is_ok());

        builder.schemas.schemas[cursor_schema.0 as usize].size = 4;
        assert_eq!(builder.errors(), vec![GraphError::PickingCursorShape]);
    }

    #[test]
    fn validation_message_has_header_and_bullets() {
        let errors = vec![
            GraphError::MultiplePickingNodes,
            GraphError::EmptyOptionalScope,
        ];

        assert_eq!(
            validation_message(&errors),
            "render graph validation failed:\n  - at most one picking node is allowed\n  - optional scope has no frame value"
        );
    }
}
