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

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum GraphError {
    IdOutOfRange {
        table: TableKind,
        id: u32,
    },
    DuplicateWrite {
        command: String,
        tex: u32,
    },
    MutateAndRead {
        command: String,
        tex: u32,
    },
    MutateAndWrite {
        command: String,
        tex: u32,
    },
    WriteAndPrevRead {
        command: String,
        tex: u32,
    },
    RepeatUniformRotatesTexture {
        repeat: String,
        uniform: String,
        tex: u32,
    },
    RasterInRepeat {
        repeat: String,
    },
    PassAfterMainRaster {
        pass: String,
    },
    DrawWritesTexture {
        draw: String,
        tex: u32,
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
        write!(f, "render graph: ")?;
        match self {
            Self::IdOutOfRange { table, id } => write!(f, "{table:?} id {id} is undeclared"),
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
                write!(f, "{feature:?} at {at} is unsupported in phase 1")
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

fn refs<'a>(
    desc: &'a GraphDesc,
    leaf: &'a LeafPass,
) -> Vec<(&'a str, &'a [(FieldKey, ResourceRef)])> {
    match leaf {
        LeafPass::Compute(compute) => {
            let mut out = vec![];
            if let Some(uniform) = desc.uniforms.get(compute.uniform.0 as usize) {
                out.push((uniform.name.as_str(), uniform.source.bindings.as_slice()));
            }
            if let Some(push) = &compute.push {
                out.push((compute.name.as_str(), push.bindings.as_slice()));
            }

            out
        }
        LeafPass::Raster(raster) => raster
            .draws
            .iter()
            .flat_map(|draw| {
                let mut out = vec![];
                if let Some(uniform) = desc.uniforms.get(draw.uniform.0 as usize) {
                    out.push((uniform.name.as_str(), uniform.source.bindings.as_slice()));
                }
                if let Some(push) = &draw.push {
                    out.push((draw.name.as_str(), push.bindings.as_slice()));
                }

                out
            })
            .collect(),
    }
}

fn all_leaves(desc: &GraphDesc) -> Vec<(&str, &LeafPass)> {
    let mut leaves = vec![];
    for pass in &desc.passes {
        match pass {
            PassDesc::Leaf(leaf) => leaves.push(("", leaf)),
            PassDesc::When { name, body, .. } | PassDesc::Repeat { name, body, .. } => {
                for leaf in body {
                    leaves.push((name, leaf))
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
    for tex in &desc.textures {
        match tex.size {
            SizeClass::Window | SizeClass::WindowDiv(_) => {
                errors.push(GraphError::UnsupportedInPhase1 {
                    feature: UnsupportedFeature::WindowSizeClass,
                    at: tex.name.clone(),
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
        if raster_seen {
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
                    .flat_map(|(_, resource)| resource)
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
                                    tex: tex.0,
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
                if matches!(raster.targets, RasterTargets::Offscreen { .. }) {
                    errors.push(GraphError::UnsupportedInPhase1 {
                        feature: UnsupportedFeature::OffscreenTargets,
                        at: raster.name.clone(),
                    })
                }
            }
            _ => {}
        }
    }
    let mut phys = vec![1; desc.textures.len()];
    for (_, leaf) in all_leaves(desc) {
        let command = match leaf {
            LeafPass::Compute(compute) => compute.name.clone(),
            LeafPass::Raster(raster) => raster.name.clone(),
        };
        let resources: Vec<_> = refs(desc, leaf)
            .into_iter()
            .flat_map(|(_, resource)| resource)
            .collect();
        for (_, resource) in &resources {
            match resource {
                ResourceRef::Tex(tex, access) => {
                    if valid(tex.0, desc.textures.len(), TableKind::Texture, &mut errors)
                        && matches!(access, TexAccess::ReadPrevious)
                    {
                        phys[tex.0 as usize] = 2
                    }
                }
                ResourceRef::Buf(buffer, _) => {
                    valid(
                        buffer.buffer.0,
                        desc.buffers.len(),
                        TableKind::Buffer,
                        &mut errors,
                    );
                }
                ResourceRef::External(import) => {
                    valid(import.0, desc.imports.len(), TableKind::Import, &mut errors);
                }
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
        for (i, tex) in writes.iter().enumerate() {
            if writes[i + 1..].contains(tex) {
                errors.push(GraphError::DuplicateWrite {
                    command: command.clone(),
                    tex: tex.0,
                })
            }
            if reads.contains(tex) && (tex.0 as usize) < phys.len() {
                phys[tex.0 as usize] = 2
            }
            if prev_reads.contains(tex) {
                errors.push(GraphError::WriteAndPrevRead {
                    command: command.clone(),
                    tex: tex.0,
                })
            }
        }
        for tex in mutates {
            if reads.contains(&tex) {
                errors.push(GraphError::MutateAndRead {
                    command: command.clone(),
                    tex: tex.0,
                })
            }
            if writes.contains(&tex) {
                errors.push(GraphError::MutateAndWrite {
                    command: command.clone(),
                    tex: tex.0,
                })
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
                for draw in &raster.draws {
                    let draw_writes_texture = writes.len()
                        + resources
                            .iter()
                            .filter(|(_, resource)| {
                                matches!(resource, ResourceRef::Tex(_, TexAccess::Mutate))
                            })
                            .count()
                        > 0;
                    if draw_writes_texture && let Some(tex) = writes.first() {
                        errors.push(GraphError::DrawWritesTexture {
                            draw: draw.name.clone(),
                            tex: tex.0,
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

    if errors.is_empty() {
        Ok(Analysis { tex_phys: phys })
    } else {
        Err(errors)
    }
}
