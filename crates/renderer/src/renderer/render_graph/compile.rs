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
    s: &SchemaDesc,
    data: Option<ValueId>,
    bindings: &[(FieldKey, ResourceRef)],
) -> AssemblyProgram {
    let Some(layout) = &s.layout else {
        return AssemblyProgram::Deferred;
    };
    let mut out = vec![];
    for f in &layout.fields {
        let src = match f.kind {
            SchemaFieldKind::Data { src_offset } => {
                let Some(value) = data else {
                    debug_assert!(false, "data field without data value");
                    continue;
                };
                AssemblySrc::FrameBytes {
                    value,
                    src_offset,
                    len: f.len,
                }
            }
            SchemaFieldKind::SampledTex
            | SchemaFieldKind::StorageTex
            | SchemaFieldKind::BufAddr => {
                let Some((_, r)) = bindings.iter().find(|(k, _)| *k == f.key) else {
                    continue;
                };
                match r {
                    ResourceRef::Tex(tex, access) => AssemblySrc::ResolveTex {
                        tex: *tex,
                        access: *access,
                    },
                    ResourceRef::Buf(b, _) => AssemblySrc::ResolveBuf(b.clone()),
                    ResourceRef::External(i) => AssemblySrc::External(*i),
                }
            }
        };
        out.push(AssemblyStep {
            dst_offset: f.offset,
            src,
        })
    }
    AssemblyProgram::Steps(out)
}
fn access(d: &GraphDesc, l: &LeafPass) -> LeafAccess {
    let mut a = LeafAccess::default();
    let mut add = |r: &ResourceRef| {
        if let ResourceRef::Tex(t, x) = r {
            match x {
                TexAccess::Read => &mut a.reads,
                TexAccess::ReadPrevious => &mut a.prev_reads,
                TexAccess::Write => &mut a.writes,
                TexAccess::Mutate => &mut a.mutates,
            }
            .push(*t)
        }
    };
    match l {
        LeafPass::Compute(c) => {
            if let Some(u) = d.uniforms.get(c.uniform.0 as usize) {
                for (_, r) in &u.source.bindings {
                    add(r)
                }
            }
            if let Some(p) = &c.push {
                for (_, r) in &p.bindings {
                    add(r)
                }
            }
        }
        LeafPass::Raster(r) => {
            for x in &r.draws {
                if let Some(u) = d.uniforms.get(x.uniform.0 as usize) {
                    for (_, r) in &u.source.bindings {
                        add(r)
                    }
                }
                if let Some(p) = &x.push {
                    for (_, r) in &p.bindings {
                        add(r)
                    }
                }
            }
        }
    }
    a
}
pub(crate) fn compile(d: &GraphDesc, a: &Analysis, s: &SchemaTable) -> CompiledGraph {
    let mut assemblies = d
        .uniforms
        .iter()
        .map(|u| build_assembly(s.get(u.schema).unwrap(), u.source.data, &u.source.bindings))
        .collect::<Vec<_>>();
    fn leaf(
        d: &GraphDesc,
        s: &SchemaTable,
        assemblies: &mut Vec<AssemblyProgram>,
        l: &LeafPass,
    ) -> CompiledLeaf {
        let ac = access(d, l);
        match l {
            LeafPass::Compute(c) => {
                let push_asm = c.push.as_ref().map(|p| {
                    let id = AsmId(assemblies.len() as u32);
                    assemblies.push(build_assembly(
                        s.get(p.schema).unwrap(),
                        p.data,
                        &p.bindings,
                    ));
                    id
                });
                CompiledLeaf {
                    kind: CompiledLeafKind::Dispatch {
                        pipeline: c.pipeline,
                        groups: c.groups,
                        uniform: c.uniform,
                        asm: AsmId(c.uniform.0),
                        push_asm,
                    },
                    barrier_before: BarrierKind::ComputeSync,
                    access: ac,
                }
            }
            LeafPass::Raster(r) => {
                let draws = r
                    .draws
                    .iter()
                    .map(|x| {
                        let push_asm = x.push.as_ref().map(|p| {
                            let id = AsmId(assemblies.len() as u32);
                            assemblies.push(build_assembly(
                                s.get(p.schema).unwrap(),
                                p.data,
                                &p.bindings,
                            ));
                            id
                        });
                        CompiledDraw {
                            pipeline: x.pipeline,
                            uniform: x.uniform,
                            asm: AsmId(x.uniform.0),
                            push_asm,
                            call: x.call.clone(),
                        }
                    })
                    .collect();
                CompiledLeaf {
                    kind: CompiledLeafKind::Raster { draws },
                    barrier_before: BarrierKind::ComputeToGraphics,
                    access: ac,
                }
            }
        }
    }
    let passes = d
        .passes
        .iter()
        .map(|p| match p {
            PassDesc::Leaf(l) => CompiledPass::Leaf(leaf(d, s, &mut assemblies, l)),
            PassDesc::Repeat { count, body, .. } => CompiledPass::Repeat {
                count: *count,
                body: body
                    .iter()
                    .map(|x| leaf(d, s, &mut assemblies, x))
                    .collect(),
            },
            PassDesc::When { value, body, .. } => CompiledPass::When {
                gate: *value,
                body: body
                    .iter()
                    .map(|x| leaf(d, s, &mut assemblies, x))
                    .collect(),
            },
        })
        .collect();
    CompiledGraph {
        passes,
        assemblies,
        tex_phys: a.tex_phys.clone(),
        value_count: d.values.len() as u32,
    }
}
