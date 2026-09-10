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
        outer: &'static str,
        inner: &'static str,
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

fn refs<'a>(desc: &'a GraphDesc, leaf: &'a LeafPass) -> Vec<&'a [(FieldKey, ResourceRef)]> {
    match leaf {
        LeafPass::Compute(compute) => {
            let mut out = vec![];
            if let Some(uniform) = desc.uniforms.get(compute.uniform.0 as usize) {
                out.push(uniform.source.bindings.as_slice());
            }
            if let Some(push) = &compute.push {
                out.push(push.bindings.as_slice());
            }

            out
        }
        LeafPass::Raster(raster) => raster
            .draws
            .iter()
            .flat_map(|draw| {
                let mut out = vec![];
                if let Some(uniform) = desc.uniforms.get(draw.uniform.0 as usize) {
                    out.push(uniform.source.bindings.as_slice());
                }
                if let Some(push) = &draw.push {
                    out.push(push.bindings.as_slice());
                }

                out
            })
            .collect(),
    }
}

fn command_bindings<'a>(
    desc: &'a GraphDesc,
    uniform: UniformId,
    push: Option<&'a PushDesc>,
) -> Vec<&'a (FieldKey, ResourceRef)> {
    let mut out = vec![];
    if let Some(decl) = desc.uniforms.get(uniform.0 as usize) {
        out.extend(&decl.source.bindings);
    }
    if let Some(push) = push {
        out.extend(&push.bindings);
    }

    out
}

/// A command is one dispatch or one draw. Hazard checks partition by command,
/// not by pass: a raster pass holds several independent draws.
fn commands<'a>(
    desc: &'a GraphDesc,
    leaf: &'a LeafPass,
) -> Vec<(&'a str, Vec<&'a (FieldKey, ResourceRef)>)> {
    match leaf {
        LeafPass::Compute(compute) => vec![(
            compute.name.as_str(),
            command_bindings(desc, compute.uniform, compute.push.as_ref()),
        )],
        LeafPass::Raster(raster) => raster
            .draws
            .iter()
            .map(|draw| {
                (
                    draw.name.as_str(),
                    command_bindings(desc, draw.uniform, draw.push.as_ref()),
                )
            })
            .collect(),
    }
}

fn all_leaves(desc: &GraphDesc) -> Vec<&LeafPass> {
    let mut leaves = vec![];
    for pass in &desc.passes {
        match pass {
            PassDesc::Leaf(leaf) => leaves.push(leaf),
            PassDesc::When { body, .. } | PassDesc::Repeat { body, .. } => {
                for leaf in body {
                    leaves.push(leaf)
                }
            }
        }
    }

    leaves
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
    let valid_bindings = |bindings: &[(FieldKey, ResourceRef)], errors: &mut Vec<GraphError>| {
        for (_, resource) in bindings {
            match resource {
                ResourceRef::Tex(tex, _) => {
                    valid(tex.0, desc.textures.len(), TableKind::Texture, errors);
                }
                ResourceRef::Buf(buffer, _) => {
                    valid(
                        buffer.buffer.0,
                        desc.buffers.len(),
                        TableKind::Buffer,
                        errors,
                    );
                }
                ResourceRef::External(import) => {
                    valid(import.0, desc.imports.len(), TableKind::Import, errors);
                }
            }
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
        valid_bindings(&uniform.source.bindings, &mut errors);
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
            if !matches!(buffer.kind, BufferKind::Storage | BufferKind::Immutable) {
                errors.push(GraphError::UploadTargetKind {
                    upload: upload.name.clone(),
                    kind: match buffer.kind {
                        BufferKind::GpuOnlyFlight => "gpu-only",
                        BufferKind::Singleton => "singleton",
                        _ => "buffer",
                    },
                })
            }
            if let ValueKind::Array { max_len, .. } = value.kind {
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
                if valid(value.0, desc.values.len(), TableKind::Value, &mut errors)
                    && !desc.values[value.0 as usize].optional
                {
                    errors.push(GraphError::WhenGateNotOptional { when: name.clone() })
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
                    .flat_map(|leaf| refs(desc, leaf))
                    .flatten()
                    .filter_map(|(_, resource)| match resource {
                        ResourceRef::Tex(tex, TexAccess::Write) => Some(*tex),
                        _ => None,
                    })
                    .collect();
                for leaf in body {
                    if let LeafPass::Compute(compute) = leaf
                        && let Some(uniform) = desc.uniforms.get(compute.uniform.0 as usize)
                    {
                        for (_, resource) in &uniform.source.bindings {
                            if let ResourceRef::Tex(tex, _) = resource
                                && rotating.contains(tex)
                            {
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
    for leaf in all_leaves(desc) {
        for (command, resources) in commands(desc, leaf) {
            for (_, resource) in &resources {
                if let ResourceRef::Tex(tex, TexAccess::ReadPrevious) = resource
                    && (tex.0 as usize) < phys.len()
                {
                    phys[tex.0 as usize] = 2
                }
            }
            let tex_ids = |access| {
                resources
                    .iter()
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
                    prev_readers.push((*tex, command.into()))
                }
            }
            for (i, tex) in writes.iter().enumerate() {
                if !written_anywhere.contains(tex) {
                    written_anywhere.push(*tex)
                }
                if writes[i + 1..].contains(tex) {
                    errors.push(GraphError::DuplicateWrite {
                        command: command.into(),
                        tex: tex_name(desc, *tex),
                    })
                }
                if reads.contains(tex) && (tex.0 as usize) < phys.len() {
                    phys[tex.0 as usize] = 2
                }
                if prev_reads.contains(tex) {
                    errors.push(GraphError::WriteAndPrevRead {
                        command: command.into(),
                        tex: tex_name(desc, *tex),
                    })
                }
            }
            for tex in mutates {
                if reads.contains(&tex) {
                    errors.push(GraphError::MutateAndRead {
                        command: command.into(),
                        tex: tex_name(desc, tex),
                    })
                }
                if writes.contains(&tex) {
                    errors.push(GraphError::MutateAndWrite {
                        command: command.into(),
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
                if valid(
                    compute.pipeline.0,
                    desc.pipelines.len(),
                    TableKind::Pipeline,
                    &mut errors,
                ) && desc.pipelines[compute.pipeline.0 as usize].kind != PipelineKind::Compute
                {
                    errors.push(GraphError::PipelineKindMismatch {
                        command: compute.name.clone(),
                        expected: PipelineKind::Compute,
                    })
                }
                valid(
                    compute.uniform.0,
                    desc.uniforms.len(),
                    TableKind::Uniform,
                    &mut errors,
                );
                if let Some(push) = &compute.push {
                    valid(
                        push.schema.0,
                        schemas.schemas.len(),
                        TableKind::Schema,
                        &mut errors,
                    );
                    if let Some(value) = push.data
                        && valid(value.0, desc.values.len(), TableKind::Value, &mut errors)
                        && !matches!(desc.values[value.0 as usize].kind, ValueKind::Bytes { .. })
                    {
                        let value = &desc.values[value.0 as usize];
                        errors.push(GraphError::ValueKindMismatch {
                            value: value.name.clone(),
                            expected: "bytes",
                            found: kind(&value.kind),
                        });
                    }
                    valid_bindings(&push.bindings, &mut errors);
                    check_shape(
                        &compute.name,
                        push.schema,
                        &push.bindings,
                        schemas,
                        &mut errors,
                    )
                }
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
                    for (_, resource) in command_bindings(desc, draw.uniform, draw.push.as_ref()) {
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

                    if valid(
                        draw.pipeline.0,
                        desc.pipelines.len(),
                        TableKind::Pipeline,
                        &mut errors,
                    ) && desc.pipelines[draw.pipeline.0 as usize].kind != PipelineKind::Graphics
                    {
                        errors.push(GraphError::PipelineKindMismatch {
                            command: draw.name.clone(),
                            expected: PipelineKind::Graphics,
                        })
                    }
                    valid(
                        draw.uniform.0,
                        desc.uniforms.len(),
                        TableKind::Uniform,
                        &mut errors,
                    );
                    if let Some(push) = &draw.push {
                        valid(
                            push.schema.0,
                            schemas.schemas.len(),
                            TableKind::Schema,
                            &mut errors,
                        );
                        valid_bindings(&push.bindings, &mut errors);
                        check_shape(
                            &draw.name,
                            push.schema,
                            &push.bindings,
                            schemas,
                            &mut errors,
                        );
                    }
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
        BufferKind, DrawCall, GroupSource, LeafPass, PassDesc, PipelineKind, RasterTargets,
        ResourceFieldKind, ResourceRef, SchemaDesc, SizeClass, TexAccess, TexId, TexUsage,
        UniformDecl, UniformId, UniformSourceDesc, UploadDesc, ValueKind,
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
    fn upload_to_gpu_only_rejects() {
        let mut builder = DescBuilder::new(0);
        let buffer = builder.buffer_decl(BufferKind::GpuOnlyFlight, Some(4));
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
            kind: "gpu-only",
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

    /// the only draw-call variant that names a buffer; indirect args must be
    /// immutable, because the graph does not version them
    #[test]
    fn indirect_args_must_be_immutable() {
        let mut builder = DescBuilder::new(0);
        let buffer = builder.buffer_decl(BufferKind::Storage, Some(4));
        let mut draw = builder.draw("draw0", &[]);
        draw.call = DrawCall::IndexedIndirect {
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
