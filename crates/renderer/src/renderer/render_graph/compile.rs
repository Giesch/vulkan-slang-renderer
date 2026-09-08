//! Pure compilation. Phase 1 keeps this separate from execution.
use super::desc::*;
use super::validate::Analysis;

#[derive(Debug)]
pub(crate) struct CompiledGraph {
    pub(crate) passes: Vec<CompiledPass>,
    pub(crate) assemblies: Vec<AssemblyProgram>,
    pub(crate) tex_phys: Vec<u32>,
    pub(crate) value_count: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct AsmId(pub(crate) u32);

#[derive(Debug)]
pub(crate) enum CompiledPass {
    Leaf(CompiledLeaf),
    Repeat {
        count: ValueId,
        body: Vec<CompiledLeaf>,
    },
    When {
        gate: ValueId,
        body: Vec<CompiledLeaf>,
    },
}

#[derive(Debug)]
pub(crate) struct CompiledLeaf {
    pub(crate) kind: CompiledLeafKind,
    pub(crate) barrier_before: BarrierKind,
    pub(crate) access: LeafAccess,
}

#[derive(Debug)]
pub(crate) enum CompiledLeafKind {
    Dispatch {
        pipeline: PipelineId,
        groups: GroupSource,
        uniform: UniformId,
        asm: AsmId,
        push_asm: Option<AsmId>,
    },
    Raster {
        draws: Vec<CompiledDraw>,
    },
}

#[derive(Debug)]
pub(crate) struct CompiledDraw {
    pub(crate) pipeline: PipelineId,
    pub(crate) uniform: UniformId,
    pub(crate) asm: AsmId,
    pub(crate) push_asm: Option<AsmId>,
    pub(crate) call: DrawCall,
}

#[derive(Debug, Default)]
pub(crate) struct LeafAccess {
    pub(crate) reads: Vec<TexId>,
    pub(crate) prev_reads: Vec<TexId>,
    pub(crate) writes: Vec<TexId>,
    pub(crate) mutates: Vec<TexId>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BarrierKind {
    ComputeSync,
    ComputeToGraphics,
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum AssemblyProgram {
    Steps(Vec<AssemblyStep>),
    Deferred,
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct AssemblyStep {
    pub(crate) dst_offset: u32,
    pub(crate) src: AssemblySrc,
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum AssemblySrc {
    FrameBytes {
        value: ValueId,
        src_offset: u32,
        len: u32,
    },
    ResolveTex {
        tex: TexId,
        access: TexAccess,
    },
    ResolveBuf(BufferRef),
    External(ImportId),
}

pub(crate) fn build_assembly(
    schema: &SchemaDesc,
    data: Option<ValueId>,
    bindings: &[(FieldKey, ResourceRef)],
) -> AssemblyProgram {
    let Some(layout) = &schema.layout else {
        return AssemblyProgram::Deferred;
    };

    let mut out = vec![];
    for field in &layout.fields {
        let src = match field.kind {
            SchemaFieldKind::Data { src_offset } => {
                let Some(value) = data else {
                    debug_assert!(false, "data field without data value");
                    continue;
                };
                AssemblySrc::FrameBytes {
                    value,
                    src_offset,
                    len: field.len,
                }
            }
            SchemaFieldKind::SampledTex
            | SchemaFieldKind::StorageTex
            | SchemaFieldKind::BufAddr => {
                let Some((_, resource)) = bindings.iter().find(|(key, _)| *key == field.key) else {
                    continue;
                };
                match resource {
                    ResourceRef::Tex(tex, access) => AssemblySrc::ResolveTex {
                        tex: *tex,
                        access: *access,
                    },
                    ResourceRef::Buf(buffer, _) => AssemblySrc::ResolveBuf(buffer.clone()),
                    ResourceRef::External(import) => AssemblySrc::External(*import),
                }
            }
        };
        out.push(AssemblyStep {
            dst_offset: field.offset,
            src,
        })
    }

    AssemblyProgram::Steps(out)
}

fn access(desc: &GraphDesc, pass: &LeafPass) -> LeafAccess {
    let mut access = LeafAccess::default();
    let mut add = |resource: &ResourceRef| {
        if let ResourceRef::Tex(tex, tex_access) = resource {
            match tex_access {
                TexAccess::Read => &mut access.reads,
                TexAccess::ReadPrevious => &mut access.prev_reads,
                TexAccess::Write => &mut access.writes,
                TexAccess::Mutate => &mut access.mutates,
            }
            .push(*tex)
        }
    };
    match pass {
        LeafPass::Compute(compute) => {
            if let Some(uniform) = desc.uniforms.get(compute.uniform.0 as usize) {
                for (_, resource) in &uniform.source.bindings {
                    add(resource)
                }
            }
            if let Some(push) = &compute.push {
                for (_, resource) in &push.bindings {
                    add(resource)
                }
            }
        }
        LeafPass::Raster(raster) => {
            for draw in &raster.draws {
                if let Some(uniform) = desc.uniforms.get(draw.uniform.0 as usize) {
                    for (_, resource) in &uniform.source.bindings {
                        add(resource)
                    }
                }
                if let Some(push) = &draw.push {
                    for (_, resource) in &push.bindings {
                        add(resource)
                    }
                }
            }
        }
    }

    access
}

pub(crate) fn compile(
    desc: &GraphDesc,
    analysis: &Analysis,
    schemas: &SchemaTable,
) -> CompiledGraph {
    let mut assemblies = desc
        .uniforms
        .iter()
        .map(|uniform| {
            build_assembly(
                schemas.get(uniform.schema).unwrap(),
                uniform.source.data,
                &uniform.source.bindings,
            )
        })
        .collect::<Vec<_>>();

    fn leaf(
        desc: &GraphDesc,
        schemas: &SchemaTable,
        assemblies: &mut Vec<AssemblyProgram>,
        pass: &LeafPass,
    ) -> CompiledLeaf {
        let leaf_access = access(desc, pass);

        match pass {
            LeafPass::Compute(compute) => {
                let push_asm = compute.push.as_ref().map(|push| {
                    let id = AsmId(assemblies.len() as u32);
                    assemblies.push(build_assembly(
                        schemas.get(push.schema).unwrap(),
                        push.data,
                        &push.bindings,
                    ));
                    id
                });
                CompiledLeaf {
                    kind: CompiledLeafKind::Dispatch {
                        pipeline: compute.pipeline,
                        groups: compute.groups,
                        uniform: compute.uniform,
                        asm: AsmId(compute.uniform.0),
                        push_asm,
                    },
                    barrier_before: BarrierKind::ComputeSync,
                    access: leaf_access,
                }
            }
            LeafPass::Raster(raster) => {
                let draws = raster
                    .draws
                    .iter()
                    .map(|draw| {
                        let push_asm = draw.push.as_ref().map(|push| {
                            let id = AsmId(assemblies.len() as u32);
                            assemblies.push(build_assembly(
                                schemas.get(push.schema).unwrap(),
                                push.data,
                                &push.bindings,
                            ));
                            id
                        });
                        CompiledDraw {
                            pipeline: draw.pipeline,
                            uniform: draw.uniform,
                            asm: AsmId(draw.uniform.0),
                            push_asm,
                            call: draw.call.clone(),
                        }
                    })
                    .collect();
                CompiledLeaf {
                    kind: CompiledLeafKind::Raster { draws },
                    barrier_before: BarrierKind::ComputeToGraphics,
                    access: leaf_access,
                }
            }
        }
    }

    let passes = desc
        .passes
        .iter()
        .map(|pass| match pass {
            PassDesc::Leaf(body_leaf) => {
                CompiledPass::Leaf(leaf(desc, schemas, &mut assemblies, body_leaf))
            }
            PassDesc::Repeat { count, body, .. } => CompiledPass::Repeat {
                count: *count,
                body: body
                    .iter()
                    .map(|body_leaf| leaf(desc, schemas, &mut assemblies, body_leaf))
                    .collect(),
            },
            PassDesc::When { value, body, .. } => CompiledPass::When {
                gate: *value,
                body: body
                    .iter()
                    .map(|body_leaf| leaf(desc, schemas, &mut assemblies, body_leaf))
                    .collect(),
            },
        })
        .collect();

    CompiledGraph {
        passes,
        assemblies,
        tex_phys: analysis.tex_phys.clone(),
        value_count: desc.values.len() as u32,
    }
}
