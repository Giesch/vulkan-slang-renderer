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

fn kind(v: &ValueKind) -> &'static str {
    match v {
        ValueKind::Count => "count",
        ValueKind::Groups => "groups",
        ValueKind::Bytes { .. } => "bytes",
        ValueKind::Array { .. } => "array",
    }
}
fn refs<'a>(d: &'a GraphDesc, leaf: &'a LeafPass) -> Vec<(&'a str, &'a [(FieldKey, ResourceRef)])> {
    match leaf {
        LeafPass::Compute(c) => {
            let mut x = vec![];
            if let Some(u) = d.uniforms.get(c.uniform.0 as usize) {
                x.push((u.name.as_str(), u.source.bindings.as_slice()));
            }
            if let Some(p) = &c.push {
                x.push((c.name.as_str(), p.bindings.as_slice()));
            }
            x
        }
        LeafPass::Raster(r) => r
            .draws
            .iter()
            .flat_map(|c| {
                let mut x = vec![];
                if let Some(u) = d.uniforms.get(c.uniform.0 as usize) {
                    x.push((u.name.as_str(), u.source.bindings.as_slice()));
                }
                if let Some(p) = &c.push {
                    x.push((c.name.as_str(), p.bindings.as_slice()));
                }
                x
            })
            .collect(),
    }
}
fn all_leaves(d: &GraphDesc) -> Vec<(&str, &LeafPass)> {
    let mut v = vec![];
    for p in &d.passes {
        match p {
            PassDesc::Leaf(l) => v.push(("", l)),
            PassDesc::When { name, body, .. } | PassDesc::Repeat { name, body, .. } => {
                for l in body {
                    v.push((name, l))
                }
            }
        }
    }
    v
}
fn check_shape(
    name: &str,
    schema: SchemaId,
    bindings: &[(FieldKey, ResourceRef)],
    schemas: &SchemaTable,
    e: &mut Vec<GraphError>,
) {
    let Some(s) = schemas.get(schema) else { return };
    if bindings.len() != s.resource_fields.len() {
        e.push(GraphError::BindingCountMismatch {
            uniform: name.into(),
            expected: s.resource_fields.len(),
            found: bindings.len(),
        });
    }
    for (i, (key, r)) in bindings.iter().enumerate() {
        let ok = key.0 as usize == i
            && s.resource_fields.get(i).is_some_and(|k| {
                matches!(
                    (k, r),
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
            e.push(GraphError::BindingKindMismatch {
                uniform: name.into(),
                field: key.0,
            });
        }
    }
}

pub(crate) fn validate(d: &GraphDesc, schemas: &SchemaTable) -> Result<Analysis, Vec<GraphError>> {
    let mut e = vec![];
    let valid = |id: u32, n: usize, t: TableKind, e: &mut Vec<GraphError>| {
        if id as usize >= n {
            e.push(GraphError::IdOutOfRange { table: t, id });
            false
        } else {
            true
        }
    };
    for t in &d.textures {
        match t.size {
            SizeClass::Window | SizeClass::WindowDiv(_) => {
                e.push(GraphError::UnsupportedInPhase1 {
                    feature: UnsupportedFeature::WindowSizeClass,
                    at: t.name.clone(),
                })
            }
            _ => {}
        };
        match t.usage {
            TexUsage::Color => e.push(GraphError::UnsupportedInPhase1 {
                feature: UnsupportedFeature::ColorAttachmentUsage,
                at: t.name.clone(),
            }),
            TexUsage::Depth => e.push(GraphError::UnsupportedInPhase1 {
                feature: UnsupportedFeature::DepthAttachmentUsage,
                at: t.name.clone(),
            }),
            _ => {}
        }
    }
    for value in &d.values {
        let schema = match value.kind {
            ValueKind::Bytes { schema } => Some(schema),
            ValueKind::Array { elem, .. } => Some(elem),
            ValueKind::Count | ValueKind::Groups => None,
        };
        if let Some(schema) = schema {
            valid(schema.0, schemas.schemas.len(), TableKind::Schema, &mut e);
        }
    }
    for pipeline in &d.pipelines {
        valid(
            pipeline.params.0,
            schemas.schemas.len(),
            TableKind::Schema,
            &mut e,
        );
        if let Some(push) = pipeline.push {
            valid(push.0, schemas.schemas.len(), TableKind::Schema, &mut e);
        }
    }
    for u in &d.uniforms {
        valid(u.schema.0, schemas.schemas.len(), TableKind::Schema, &mut e);
        if let Some(v) = u.source.data
            && valid(v.0, d.values.len(), TableKind::Value, &mut e)
            && !matches!(d.values[v.0 as usize].kind, ValueKind::Bytes { .. })
        {
            e.push(GraphError::ValueKindMismatch {
                value: d.values[v.0 as usize].name.clone(),
                expected: "bytes",
                found: kind(&d.values[v.0 as usize].kind),
            })
        }
        check_shape(&u.name, u.schema, &u.source.bindings, schemas, &mut e);
    }
    for up in &d.uploads {
        let bv = valid(up.buffer.0, d.buffers.len(), TableKind::Buffer, &mut e);
        let vv = valid(up.value.0, d.values.len(), TableKind::Value, &mut e);
        if bv && vv {
            let b = &d.buffers[up.buffer.0 as usize];
            let v = &d.values[up.value.0 as usize];
            if !matches!(b.kind, BufferKind::Storage | BufferKind::Immutable) {
                e.push(GraphError::UploadTargetKind {
                    upload: up.name.clone(),
                    kind: match b.kind {
                        BufferKind::GpuOnlyFlight => "gpu-only",
                        BufferKind::Singleton => "singleton",
                        _ => "buffer",
                    },
                })
            }
            if let ValueKind::Array { max_len, .. } = v.kind {
                if let Some(cap) = b.capacity
                    && max_len > cap
                {
                    e.push(GraphError::UploadTooLarge {
                        upload: up.name.clone(),
                        max_len,
                        capacity: cap,
                    })
                }
            } else {
                e.push(GraphError::ValueKindMismatch {
                    value: v.name.clone(),
                    expected: "array",
                    found: kind(&v.kind),
                })
            }
        }
    }
    let mut raster_seen = false;
    for p in &d.passes {
        let pname = match p {
            PassDesc::Leaf(LeafPass::Compute(c)) => &c.name,
            PassDesc::Leaf(LeafPass::Raster(r)) => &r.name,
            PassDesc::When { name, .. } | PassDesc::Repeat { name, .. } => name,
        };
        if raster_seen {
            e.push(GraphError::PassAfterMainRaster {
                pass: pname.clone(),
            });
        }
        match p {
            PassDesc::When { name, value, body } => {
                if valid(value.0, d.values.len(), TableKind::Value, &mut e)
                    && !d.values[value.0 as usize].optional
                {
                    e.push(GraphError::WhenGateNotOptional { when: name.clone() })
                }
                if body.iter().any(|x| matches!(x, LeafPass::Raster(_))) {
                    e.push(GraphError::UnsupportedInPhase1 {
                        feature: UnsupportedFeature::RasterInWhen,
                        at: name.clone(),
                    })
                }
            }
            PassDesc::Repeat { name, count, body } => {
                if valid(count.0, d.values.len(), TableKind::Value, &mut e)
                    && !matches!(d.values[count.0 as usize].kind, ValueKind::Count)
                {
                    let v = &d.values[count.0 as usize];
                    e.push(GraphError::ValueKindMismatch {
                        value: v.name.clone(),
                        expected: "count",
                        found: kind(&v.kind),
                    })
                }
                if body.iter().any(|x| matches!(x, LeafPass::Raster(_))) {
                    e.push(GraphError::RasterInRepeat {
                        repeat: name.clone(),
                    })
                }
                let rotating: Vec<_> = body
                    .iter()
                    .flat_map(|l| refs(d, l))
                    .flat_map(|(_, r)| r)
                    .filter_map(|(_, r)| match r {
                        ResourceRef::Tex(t, TexAccess::Write) => Some(*t),
                        _ => None,
                    })
                    .collect();
                for l in body {
                    if let LeafPass::Compute(c) = l
                        && let Some(u) = d.uniforms.get(c.uniform.0 as usize)
                    {
                        for (_, r) in &u.source.bindings {
                            if let ResourceRef::Tex(t, _) = r
                                && rotating.contains(t)
                            {
                                e.push(GraphError::RepeatUniformRotatesTexture {
                                    repeat: name.clone(),
                                    uniform: u.name.clone(),
                                    tex: t.0,
                                })
                            }
                        }
                    }
                }
            }
            PassDesc::Leaf(LeafPass::Raster(r)) => {
                if raster_seen {
                    e.push(GraphError::UnsupportedInPhase1 {
                        feature: UnsupportedFeature::MultipleRasterPasses,
                        at: r.name.clone(),
                    })
                }
                raster_seen = true;
                if matches!(r.targets, RasterTargets::Offscreen { .. }) {
                    e.push(GraphError::UnsupportedInPhase1 {
                        feature: UnsupportedFeature::OffscreenTargets,
                        at: r.name.clone(),
                    })
                }
            }
            _ => {}
        }
    }
    let mut phys = vec![1; d.textures.len()];
    for (_, leaf) in all_leaves(d) {
        let command = match leaf {
            LeafPass::Compute(c) => c.name.clone(),
            LeafPass::Raster(r) => r.name.clone(),
        };
        let rr: Vec<_> = refs(d, leaf).into_iter().flat_map(|(_, r)| r).collect();
        for (_, r) in &rr {
            match r {
                ResourceRef::Tex(t, a) => {
                    if valid(t.0, d.textures.len(), TableKind::Texture, &mut e)
                        && matches!(a, TexAccess::ReadPrevious)
                    {
                        phys[t.0 as usize] = 2
                    }
                }
                ResourceRef::Buf(b, _) => {
                    valid(b.buffer.0, d.buffers.len(), TableKind::Buffer, &mut e);
                }
                ResourceRef::External(i) => {
                    valid(i.0, d.imports.len(), TableKind::Import, &mut e);
                }
            }
        }
        let tex = |a| {
            rr.iter()
                .filter_map(move |(_, r)| match r {
                    ResourceRef::Tex(t, x) if *x == a => Some(*t),
                    _ => None,
                })
                .collect::<Vec<_>>()
        };
        let reads = tex(TexAccess::Read);
        let prev = tex(TexAccess::ReadPrevious);
        let writes = tex(TexAccess::Write);
        let muts = tex(TexAccess::Mutate);
        for (i, t) in writes.iter().enumerate() {
            if writes[i + 1..].contains(t) {
                e.push(GraphError::DuplicateWrite {
                    command: command.clone(),
                    tex: t.0,
                })
            }
            if reads.contains(t) && (t.0 as usize) < phys.len() {
                phys[t.0 as usize] = 2
            }
            if prev.contains(t) {
                e.push(GraphError::WriteAndPrevRead {
                    command: command.clone(),
                    tex: t.0,
                })
            }
        }
        for t in muts {
            if reads.contains(&t) {
                e.push(GraphError::MutateAndRead {
                    command: command.clone(),
                    tex: t.0,
                })
            }
            if writes.contains(&t) {
                e.push(GraphError::MutateAndWrite {
                    command: command.clone(),
                    tex: t.0,
                })
            }
        }
        match leaf {
            LeafPass::Compute(c) => {
                if let GroupSource::Value(v) = c.groups {
                    if valid(v.0, d.values.len(), TableKind::Value, &mut e)
                        && !matches!(d.values[v.0 as usize].kind, ValueKind::Groups)
                    {
                        let value = &d.values[v.0 as usize];
                        e.push(GraphError::ValueKindMismatch {
                            value: value.name.clone(),
                            expected: "groups",
                            found: kind(&value.kind),
                        });
                    }
                    e.push(GraphError::UnsupportedInPhase1 {
                        feature: UnsupportedFeature::GroupSourceValue,
                        at: c.name.clone(),
                    })
                }
                if valid(c.pipeline.0, d.pipelines.len(), TableKind::Pipeline, &mut e)
                    && d.pipelines[c.pipeline.0 as usize].kind != PipelineKind::Compute
                {
                    e.push(GraphError::PipelineKindMismatch {
                        command: c.name.clone(),
                        expected: PipelineKind::Compute,
                    })
                }
                valid(c.uniform.0, d.uniforms.len(), TableKind::Uniform, &mut e);
                if let Some(p) = &c.push {
                    valid(p.schema.0, schemas.schemas.len(), TableKind::Schema, &mut e);
                    if let Some(value) = p.data
                        && valid(value.0, d.values.len(), TableKind::Value, &mut e)
                        && !matches!(d.values[value.0 as usize].kind, ValueKind::Bytes { .. })
                    {
                        let value = &d.values[value.0 as usize];
                        e.push(GraphError::ValueKindMismatch {
                            value: value.name.clone(),
                            expected: "bytes",
                            found: kind(&value.kind),
                        });
                    }
                    check_shape(&c.name, p.schema, &p.bindings, schemas, &mut e)
                }
            }
            LeafPass::Raster(r) => {
                for draw in &r.draws {
                    if writes.len()
                        + rr.iter()
                            .filter(|(_, x)| matches!(x, ResourceRef::Tex(_, TexAccess::Mutate)))
                            .count()
                        > 0
                        && let Some(t) = writes.first()
                    {
                        e.push(GraphError::DrawWritesTexture {
                            draw: draw.name.clone(),
                            tex: t.0,
                        })
                    }
                    if valid(
                        draw.pipeline.0,
                        d.pipelines.len(),
                        TableKind::Pipeline,
                        &mut e,
                    ) && d.pipelines[draw.pipeline.0 as usize].kind != PipelineKind::Graphics
                    {
                        e.push(GraphError::PipelineKindMismatch {
                            command: draw.name.clone(),
                            expected: PipelineKind::Graphics,
                        })
                    }
                    valid(draw.uniform.0, d.uniforms.len(), TableKind::Uniform, &mut e);
                    if let Some(push) = &draw.push {
                        valid(
                            push.schema.0,
                            schemas.schemas.len(),
                            TableKind::Schema,
                            &mut e,
                        );
                        check_shape(&draw.name, push.schema, &push.bindings, schemas, &mut e);
                    }
                    if let DrawCall::IndexedIndirect { args, .. } = &draw.call
                        && valid(args.buffer.0, d.buffers.len(), TableKind::Buffer, &mut e)
                        && d.buffers[args.buffer.0 as usize].kind != BufferKind::Immutable
                    {
                        e.push(GraphError::IndirectArgsNotImmutable {
                            draw: draw.name.clone(),
                        })
                    }
                }
            }
        }
    }
    if e.is_empty() {
        Ok(Analysis { tex_phys: phys })
    } else {
        Err(e)
    }
}
