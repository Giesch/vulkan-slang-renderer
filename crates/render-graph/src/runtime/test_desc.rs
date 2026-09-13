//! A hand-written [`GraphDesc`] builder for validate, compile, and expand
//! tests. It fabricates the value, uniform, pipeline, and schema rows around a
//! test's `ResourceRef` sets, so a test states only the accesses it is about.

use super::desc::{
    BufAccess, BufferDecl, BufferKind, BufferRef, DispatchDesc, DrawCall, DrawDesc, FieldKey,
    GraphDesc, GraphFormat, GroupSource, LeafPass, PassDesc, PipelineDecl, PipelineKind, PushDesc,
    RasterDesc, RasterTargets, ResourceFieldKind, ResourceRef, SchemaDesc, SchemaTable, SizeClass,
    SlotSel, TexAccess, TexDecl, TexId, TexUsage, UniformDecl, UniformId, UniformSourceDesc,
    ValueDecl, ValueId, ValueKind,
};
use super::validate::{Analysis, GraphError, validate};

/// The schema field a resource must be bound to for `check_shape` to accept it.
pub(crate) fn field_kind(resource: &ResourceRef) -> ResourceFieldKind {
    match resource {
        ResourceRef::Tex(_, TexAccess::Read | TexAccess::ReadPrevious)
        | ResourceRef::External(_) => ResourceFieldKind::SampledTex,
        ResourceRef::Tex(_, TexAccess::Write | TexAccess::Mutate) => ResourceFieldKind::StorageTex,
        ResourceRef::Buf(..) => ResourceFieldKind::BufAddr,
    }
}

/// `check_shape` requires binding `i` to carry `FieldKey(i)`.
pub(crate) fn bindings(resources: &[ResourceRef]) -> Vec<(FieldKey, ResourceRef)> {
    resources
        .iter()
        .enumerate()
        .map(|(i, resource)| (FieldKey(i as u16), resource.clone()))
        .collect()
}

pub(crate) fn read(tex: u32) -> ResourceRef {
    ResourceRef::Tex(TexId(tex), TexAccess::Read)
}

pub(crate) fn read_previous(tex: u32) -> ResourceRef {
    ResourceRef::Tex(TexId(tex), TexAccess::ReadPrevious)
}

pub(crate) fn write(tex: u32) -> ResourceRef {
    ResourceRef::Tex(TexId(tex), TexAccess::Write)
}

pub(crate) fn mutate(tex: u32) -> ResourceRef {
    ResourceRef::Tex(TexId(tex), TexAccess::Mutate)
}

pub(crate) fn buffer(index: u32) -> ResourceRef {
    ResourceRef::Buf(
        BufferRef {
            buffer: super::desc::BufferId(index),
            slot: SlotSel::Current,
            offset: 0,
            range: None,
        },
        BufAccess::Read,
    )
}

/// Every draw of one raster pass, in one `RasterDesc`.
pub(crate) fn raster(name: &str, draws: Vec<DrawDesc>) -> RasterDesc {
    RasterDesc {
        name: name.into(),
        targets: RasterTargets::Main,
        draws,
    }
}

pub(crate) struct DescBuilder {
    pub(crate) desc: GraphDesc,
    pub(crate) schemas: SchemaTable,
}

impl DescBuilder {
    /// `textures` fixed-size storage textures named `tex0..texN`.
    pub(crate) fn new(textures: usize) -> Self {
        let mut desc = GraphDesc::default();
        for i in 0..textures {
            desc.textures.push(TexDecl {
                name: format!("tex{i}"),
                format: GraphFormat::R32Float,
                size: SizeClass::Fixed(8, 8),
                usage: TexUsage::Storage,
            })
        }

        Self {
            desc,
            schemas: SchemaTable::default(),
        }
    }

    pub(crate) fn buffer_decl(&mut self, kind: BufferKind, capacity: Option<u32>) -> u32 {
        let index = self.desc.buffers.len() as u32;
        self.desc.buffers.push(BufferDecl {
            name: format!("buf{index}"),
            kind,
            capacity,
            elem_size: Some(4),
        });

        index
    }

    pub(crate) fn value(&mut self, kind: ValueKind, optional: bool) -> ValueId {
        let id = ValueId(self.desc.values.len() as u32);
        self.desc.values.push(ValueDecl {
            name: format!("value{}", id.0),
            kind,
            optional,
        });

        id
    }

    fn schema(&mut self, name: String, resources: &[ResourceRef]) -> super::desc::SchemaId {
        self.schemas.push(SchemaDesc {
            name,
            size: 16,
            resource_fields: resources.iter().map(field_kind).collect(),
            layout: None,
        })
    }

    fn pipeline(&mut self, kind: PipelineKind) -> super::desc::PipelineId {
        let id = super::desc::PipelineId(self.desc.pipelines.len() as u32);
        let params = self.schema(format!("pipeline{}.params", id.0), &[]);
        self.desc.pipelines.push(PipelineDecl {
            name: format!("pipeline{}", id.0),
            kind,
            params,
            push: None,
        });

        id
    }

    /// A uniform decl whose schema matches `resources`, so only the accesses a
    /// test states can produce an error.
    pub(crate) fn uniform(&mut self, resources: &[ResourceRef]) -> UniformId {
        let id = UniformId(self.desc.uniforms.len() as u32);
        let schema = self.schema(format!("uniform{}", id.0), resources);
        let data_schema = self.schema(format!("uniform{}.data", id.0), &[]);
        let data = self.value(
            ValueKind::Bytes {
                schema: data_schema,
            },
            false,
        );
        self.desc.uniforms.push(UniformDecl {
            name: format!("uniform{}", id.0),
            schema,
            source: UniformSourceDesc {
                data: Some(data),
                bindings: bindings(resources),
            },
        });

        id
    }

    fn push_desc(&mut self, name: &str, resources: &[ResourceRef]) -> PushDesc {
        let schema = self.schema(format!("{name}.push"), resources);

        PushDesc {
            data: None,
            schema,
            bindings: bindings(resources),
        }
    }

    pub(crate) fn dispatch(&mut self, name: &str, resources: &[ResourceRef]) -> DispatchDesc {
        let uniform = self.uniform(resources);
        let pipeline = self.pipeline(PipelineKind::Compute);

        DispatchDesc {
            name: name.into(),
            pipeline,
            uniform,
            groups: GroupSource::Fixed([1, 1, 1]),
            push: None,
        }
    }

    pub(crate) fn dispatch_with_push(
        &mut self,
        name: &str,
        resources: &[ResourceRef],
        push: &[ResourceRef],
    ) -> DispatchDesc {
        let mut dispatch = self.dispatch(name, resources);
        dispatch.push = Some(self.push_desc(name, push));

        dispatch
    }

    pub(crate) fn draw(&mut self, name: &str, resources: &[ResourceRef]) -> DrawDesc {
        let uniform = self.uniform(resources);
        let pipeline = self.pipeline(PipelineKind::Graphics);

        DrawDesc {
            name: name.into(),
            pipeline,
            uniform,
            call: DrawCall::VertexCount(3),
            push: None,
        }
    }

    pub(crate) fn leaf(&mut self, dispatch: DispatchDesc) {
        self.desc
            .passes
            .push(PassDesc::Leaf(LeafPass::Compute(dispatch)))
    }

    pub(crate) fn leaf_raster(&mut self, raster: RasterDesc) {
        self.desc
            .passes
            .push(PassDesc::Leaf(LeafPass::Raster(raster)))
    }

    /// A repeat pass whose count value is minted here, so it always type-checks.
    /// Returns that value, which indexes the per-frame trip count.
    pub(crate) fn repeat(&mut self, name: &str, body: Vec<LeafPass>) -> ValueId {
        let count = self.value(ValueKind::Count, false);
        self.desc.passes.push(PassDesc::Repeat {
            name: name.into(),
            count,
            body,
        });

        count
    }

    /// A when pass whose gate value is minted optional, so it always
    /// type-checks. Returns that value, which indexes the per-frame gate.
    pub(crate) fn when(&mut self, name: &str, body: Vec<LeafPass>) -> ValueId {
        let value = self.value(ValueKind::Count, true);
        self.desc.passes.push(PassDesc::When {
            name: name.into(),
            value,
            body,
        });

        value
    }

    pub(crate) fn validate(&self) -> Result<Analysis, Vec<GraphError>> {
        validate(&self.desc, &self.schemas)
    }

    pub(crate) fn tex_phys(&self) -> Vec<u32> {
        self.validate()
            .expect("desc under test must validate")
            .tex_phys
    }

    pub(crate) fn errors(&self) -> Vec<GraphError> {
        self.validate()
            .expect_err("desc under test must fail validation")
    }
}
