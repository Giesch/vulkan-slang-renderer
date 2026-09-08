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

#[cfg(test)]
mod tests {
    use super::super::desc::{
        DrawCall, FieldKey, GroupSource, LeafPass, PipelineId, ResourceRef, SchemaDesc,
        SchemaField, SchemaFieldKind, SchemaLayout, TexAccess, TexId, ValueId,
    };
    use super::super::test_desc::{DescBuilder, bindings, buffer, raster, read, write};
    use super::{
        AsmId, AssemblyProgram, AssemblySrc, AssemblyStep, BarrierKind, CompiledLeaf,
        CompiledLeafKind, CompiledPass, build_assembly, compile,
    };

    fn data(key: u16, offset: u32, len: u32, src_offset: u32) -> SchemaField {
        SchemaField {
            key: FieldKey(key),
            offset,
            len,
            kind: SchemaFieldKind::Data { src_offset },
        }
    }

    fn resource(key: u16, offset: u32, len: u32, kind: SchemaFieldKind) -> SchemaField {
        SchemaField {
            key: FieldKey(key),
            offset,
            len,
            kind,
        }
    }

    fn schema(fields: Vec<SchemaField>) -> SchemaDesc {
        SchemaDesc {
            name: "block".into(),
            size: 36,
            resource_fields: vec![],
            layout: Some(SchemaLayout { fields }),
        }
    }

    /// interior padding is its own step, so the assembler never reads bytes the
    /// layout does not name
    #[test]
    fn assembly_interleaved_data_resource_and_padding() {
        let schema = schema(vec![
            data(0, 0, 8, 0),
            data(1, 8, 8, 8),
            resource(2, 16, 4, SchemaFieldKind::SampledTex),
            resource(3, 24, 8, SchemaFieldKind::BufAddr),
            data(4, 32, 4, 16),
        ]);
        let value = ValueId(5);
        let program = build_assembly(
            &schema,
            Some(value),
            &bindings(&[read(1), buffer(2)])
                .into_iter()
                .enumerate()
                .map(|(i, (_, resource))| (FieldKey(i as u16 + 2), resource))
                .collect::<Vec<_>>(),
        );

        let AssemblyProgram::Steps(steps) = program else {
            panic!("a laid-out schema assembles eagerly")
        };
        assert_eq!(
            steps,
            vec![
                AssemblyStep {
                    dst_offset: 0,
                    src: AssemblySrc::FrameBytes {
                        value,
                        src_offset: 0,
                        len: 8,
                    },
                },
                AssemblyStep {
                    dst_offset: 8,
                    src: AssemblySrc::FrameBytes {
                        value,
                        src_offset: 8,
                        len: 8,
                    },
                },
                AssemblyStep {
                    dst_offset: 16,
                    src: AssemblySrc::ResolveTex {
                        tex: TexId(1),
                        access: TexAccess::Read,
                    },
                },
                AssemblyStep {
                    dst_offset: 24,
                    src: match &bindings(&[buffer(2)])[0].1 {
                        ResourceRef::Buf(args, _) => AssemblySrc::ResolveBuf(args.clone()),
                        _ => unreachable!(),
                    },
                },
                AssemblyStep {
                    dst_offset: 32,
                    src: AssemblySrc::FrameBytes {
                        value,
                        src_offset: 16,
                        len: 4,
                    },
                },
            ]
        );
        assert!(
            steps
                .iter()
                .all(|step| !(20..24).contains(&step.dst_offset))
        );
    }

    #[test]
    fn assembly_data_only_schema() {
        let schema = schema(vec![data(0, 0, 4, 0), data(1, 4, 12, 4)]);
        let value = ValueId(0);

        assert_eq!(
            build_assembly(&schema, Some(value), &[]),
            AssemblyProgram::Steps(vec![
                AssemblyStep {
                    dst_offset: 0,
                    src: AssemblySrc::FrameBytes {
                        value,
                        src_offset: 0,
                        len: 4,
                    },
                },
                AssemblyStep {
                    dst_offset: 4,
                    src: AssemblySrc::FrameBytes {
                        value,
                        src_offset: 4,
                        len: 12,
                    },
                },
            ])
        );
    }

    /// resources resolve in field order, not binding order
    #[test]
    fn assembly_resource_only_schema() {
        let schema = schema(vec![
            resource(0, 0, 4, SchemaFieldKind::SampledTex),
            resource(1, 8, 8, SchemaFieldKind::BufAddr),
            resource(2, 16, 4, SchemaFieldKind::StorageTex),
        ]);
        let bound = bindings(&[read(3), buffer(0), write(4)]);
        let AssemblyProgram::Steps(steps) = build_assembly(&schema, None, &bound) else {
            panic!("a laid-out schema assembles eagerly")
        };

        assert_eq!(steps.len(), 3);
        assert_eq!(
            steps[0].src,
            AssemblySrc::ResolveTex {
                tex: TexId(3),
                access: TexAccess::Read,
            }
        );
        assert!(matches!(steps[1].src, AssemblySrc::ResolveBuf(_)));
        assert_eq!(
            steps[2].src,
            AssemblySrc::ResolveTex {
                tex: TexId(4),
                access: TexAccess::Write,
            }
        );
    }

    /// lowering does not record field layouts, so the lowered path defers
    #[test]
    fn layoutless_schema_defers_assembly() {
        let schema = SchemaDesc {
            name: "block".into(),
            size: 16,
            resource_fields: vec![],
            layout: None,
        };

        assert_eq!(schema.size, 16);
        assert_eq!(
            build_assembly(&schema, Some(ValueId(0)), &[]),
            AssemblyProgram::Deferred
        );
    }

    #[test]
    fn dispatches_get_compute_sync_barrier_template() {
        let mut builder = DescBuilder::new(1);
        for i in 0..3 {
            let dispatch = builder.dispatch(&format!("d{i}"), &[read(0)]);
            builder.leaf(dispatch)
        }

        let analysis = builder.validate().expect("desc under test must validate");
        let graph = compile(&builder.desc, &analysis, &builder.schemas);

        assert_eq!(graph.passes.len(), 3);
        assert_eq!(graph.assemblies.len(), builder.desc.uniforms.len());
        assert!(
            graph
                .assemblies
                .iter()
                .all(|program| *program == AssemblyProgram::Deferred)
        );
        for (i, pass) in graph.passes.iter().enumerate() {
            let CompiledPass::Leaf(CompiledLeaf {
                barrier_before,
                kind:
                    CompiledLeafKind::Dispatch {
                        asm,
                        uniform,
                        pipeline,
                        groups,
                        push_asm,
                    },
                ..
            }) = pass
            else {
                panic!("expected a dispatch leaf")
            };
            assert_eq!(*barrier_before, BarrierKind::ComputeSync);
            assert_eq!(*asm, AsmId(uniform.0));
            assert_eq!(uniform.0, i as u32);
            assert_eq!(*pipeline, PipelineId(i as u32));
            assert!(matches!(groups, GroupSource::Fixed([1, 1, 1])));
            assert_eq!(*push_asm, None);
        }
    }

    #[test]
    fn raster_gets_compute_to_graphics_barrier_template() {
        let mut builder = DescBuilder::new(1);
        let dispatch = builder.dispatch("d0", &[write(0)]);
        builder.leaf(dispatch);
        let draw = builder.draw("draw0", &[read(0)]);
        let pass = raster("main", vec![draw]);
        builder.leaf_raster(pass);
        let analysis = builder.validate().expect("desc under test must validate");
        let graph = compile(&builder.desc, &analysis, &builder.schemas);

        let CompiledPass::Leaf(CompiledLeaf {
            barrier_before,
            kind: CompiledLeafKind::Raster { draws },
            ..
        }) = &graph.passes[1]
        else {
            panic!("expected a raster leaf")
        };
        assert_eq!(*barrier_before, BarrierKind::ComputeToGraphics);
        assert_eq!(draws.len(), 1);
        assert_eq!(draws[0].asm, AsmId(draws[0].uniform.0));
        assert_eq!(draws[0].pipeline, PipelineId(1));
        assert_eq!(draws[0].push_asm, None);
        assert!(matches!(draws[0].call, DrawCall::VertexCount(3)));
    }

    #[test]
    fn repeat_and_when_structure_survives_compile() {
        let mut builder = DescBuilder::new(2);
        let gated = builder.dispatch("gated", &[read(1)]);
        builder.when("when0", vec![LeafPass::Compute(gated)]);
        let rotating = builder.dispatch_with_push("jacobi", &[read(1)], &[read(0), write(0)]);
        builder.repeat("repeat0", vec![LeafPass::Compute(rotating)]);
        let analysis = builder.validate().expect("desc under test must validate");
        let graph = compile(&builder.desc, &analysis, &builder.schemas);

        assert_eq!(graph.tex_phys, vec![2, 1]);
        assert_eq!(graph.value_count, builder.desc.values.len() as u32);
        let CompiledPass::When { body, .. } = &graph.passes[0] else {
            panic!("expected a when pass")
        };
        assert_eq!(body.len(), 1);
        assert!(body[0].access.reads.contains(&TexId(1)));

        let CompiledPass::Repeat { body, .. } = &graph.passes[1] else {
            panic!("expected a repeat pass")
        };
        let leaf = &body[0];
        assert_eq!(leaf.access.reads, vec![TexId(1), TexId(0)]);
        assert_eq!(leaf.access.writes, vec![TexId(0)]);
        assert!(leaf.access.prev_reads.is_empty());
        assert!(leaf.access.mutates.is_empty());
        let CompiledLeafKind::Dispatch { push_asm, .. } = leaf.kind else {
            panic!("expected a dispatch leaf")
        };
        assert_eq!(push_asm, Some(AsmId(builder.desc.uniforms.len() as u32)));
    }
}
