//! Draw nodes built from runtime data instead of generated shader types.
//!
//! A host that receives its graph as plain data (the Roc platform) cannot
//! name a `GraphShaderParams` type per shader. A dynamic node carries the
//! parameter block's GPU size and takes its per-frame value as already
//! packed bytes. It lowers through the same `LowerCtx` and validates under
//! the same rules as a typed node.

use crate::commands::PendingDrawCommand;

use super::lower::{LowerCtx, LowerDrawCall, UniformInput};
use super::{
    BackendTypes, GraphBinding, GraphNode, GraphicsPipelineKey, IndexBufferSlot, NoPush, PlanCtx,
    UniformSlot, compatible,
};

/// The draw call of a dynamic node. Indirect draws stay typed: their argument
/// record is checked against the backend at the type level.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DynDrawCall {
    WholeIndexed {
        indices: IndexBufferSlot,
    },
    IndexRange {
        indices: IndexBufferSlot,
        first_index: u32,
        index_count: u32,
    },
    VertexCount(u32),
}

/// A draw whose uniform parameter block arrives as packed bytes each frame.
///
/// The constructors take a direct graphics pipeline key, so a dynamic draw
/// cannot name a compute or indirect pipeline. Each key is backend
/// integration: its index must name a live graphics pipeline with no push
/// block, and the uniform slot a live uniform buffer of exactly `gpu_size`
/// bytes. An index range past the end of its index buffer is a
/// `RenderGraph::new` error.
pub struct DynDrawNode {
    pipeline_index: usize,
    uniform_index: usize,
    gpu_size: u32,
    call: LowerDrawCall,
    bindings: Vec<GraphBinding>,
}

impl DynDrawNode {
    /// Draws every index in `indices`.
    pub fn indexed<T>(
        pipeline: impl Into<GraphicsPipelineKey<NoPush>>,
        indices: impl Into<IndexBufferSlot>,
        uniform: impl Into<UniformSlot<T>>,
        gpu_size: u32,
    ) -> Self {
        Self::new(
            pipeline.into().index(),
            uniform.into().index,
            gpu_size,
            LowerDrawCall::WholeIndexed {
                indices: indices.into(),
            },
        )
    }

    /// Draws `index_count` indices of `indices`, starting at `first_index`.
    pub fn index_range<T>(
        pipeline: impl Into<GraphicsPipelineKey<NoPush>>,
        indices: impl Into<IndexBufferSlot>,
        uniform: impl Into<UniformSlot<T>>,
        gpu_size: u32,
        first_index: u32,
        index_count: u32,
    ) -> Self {
        Self::new(
            pipeline.into().index(),
            uniform.into().index,
            gpu_size,
            LowerDrawCall::IndexRange {
                indices: indices.into(),
                first_index,
                index_count,
            },
        )
    }

    /// Draws `vertex_count` vertices with no index buffer.
    pub fn vertex_count<T>(
        pipeline: impl Into<GraphicsPipelineKey<NoPush>>,
        uniform: impl Into<UniformSlot<T>>,
        gpu_size: u32,
        vertex_count: u32,
    ) -> Self {
        Self::new(
            pipeline.into().index(),
            uniform.into().index,
            gpu_size,
            LowerDrawCall::VertexCount(vertex_count),
        )
    }

    fn new(
        pipeline_index: usize,
        uniform_index: usize,
        gpu_size: u32,
        call: LowerDrawCall,
    ) -> Self {
        Self {
            pipeline_index,
            uniform_index,
            gpu_size,
            call,
            bindings: vec![],
        }
    }

    pub fn call(&self) -> DynDrawCall {
        match self.call {
            LowerDrawCall::WholeIndexed { indices } => DynDrawCall::WholeIndexed { indices },
            LowerDrawCall::IndexRange {
                indices,
                first_index,
                index_count,
            } => DynDrawCall::IndexRange {
                indices,
                first_index,
                index_count,
            },
            LowerDrawCall::VertexCount(count) => DynDrawCall::VertexCount(count),
            LowerDrawCall::IndexedIndirect { .. } => {
                unreachable!("dynamic nodes have no indirect constructor")
            }
        }
    }

    pub fn gpu_size(&self) -> u32 {
        self.gpu_size
    }
}

impl GraphNode for DynDrawNode {
    type Frame = Vec<u8>;

    fn lower(&self, cx: &mut LowerCtx) {
        cx.draw(
            self.pipeline_index,
            self.call,
            UniformInput {
                slot: self.uniform_index,
                gpu_size: self.gpu_size,
                data_size: self.gpu_size,
                bindings: self.bindings.clone(),
            },
            None,
        )
    }

    fn plan(&self, frame_data: &Self::Frame, cx: &mut PlanCtx<'_>) -> anyhow::Result<()> {
        anyhow::ensure!(
            frame_data.len() == self.gpu_size as usize,
            "render graph: uniform buffer slot {} received {} bytes; its parameter block is {} bytes",
            self.uniform_index,
            frame_data.len(),
            self.gpu_size,
        );
        cx.stage_uniform_bytes(self.uniform_index, frame_data);

        let pipeline_index = self.pipeline_index;
        let draw_call = cx.draw_call(self.call)?;
        cx.draws.push(PendingDrawCommand {
            pipeline_index,
            draw_call,
            push_constants: None,
        });

        Ok(())
    }
}

impl<B: BackendTypes> compatible::Sealed<B> for DynDrawNode {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{RenderGraph, ResourcePlanner};

    #[test]
    fn dynamic_draws_lower_into_one_main_pass() {
        let indices = IndexBufferSlot::from_backend(2, 36);
        let nodes = vec![
            DynDrawNode::indexed(
                GraphicsPipelineKey::<NoPush>::new(3),
                indices,
                UniformSlot::<()>::from_backend(7),
                192,
            ),
            DynDrawNode::vertex_count(
                GraphicsPipelineKey::<NoPush>::new(4),
                UniformSlot::<()>::from_backend(8),
                16,
                3,
            ),
        ];
        let graph = RenderGraph::new(ResourcePlanner::new(), nodes).unwrap();
        assert_eq!(graph.uniform_slots, vec![7, 8]);
        assert_eq!(graph.nodes[0].call(), DynDrawCall::WholeIndexed { indices });
        assert_eq!(graph.nodes[1].call(), DynDrawCall::VertexCount(3));
    }

    #[test]
    fn one_slot_with_two_sizes_is_a_source_conflict() {
        let indices = IndexBufferSlot::from_backend(2, 36);
        let nodes = vec![
            DynDrawNode::indexed(
                GraphicsPipelineKey::<NoPush>::new(3),
                indices,
                UniformSlot::<()>::from_backend(7),
                192,
            ),
            DynDrawNode::indexed(
                GraphicsPipelineKey::<NoPush>::new(3),
                indices,
                UniformSlot::<()>::from_backend(7),
                64,
            ),
        ];
        let Err(err) = RenderGraph::new(ResourcePlanner::new(), nodes) else {
            panic!("conflicting sizes were accepted");
        };
        assert!(err.to_string().contains("uniform slot 7"), "{err}");
    }

    #[test]
    fn an_index_range_past_the_index_buffer_is_rejected() {
        let nodes = vec![DynDrawNode::index_range(
            GraphicsPipelineKey::<NoPush>::new(3),
            IndexBufferSlot::from_backend(2, 36),
            UniformSlot::<()>::from_backend(7),
            192,
            30,
            7,
        )];
        let Err(err) = RenderGraph::new(ResourcePlanner::new(), nodes) else {
            panic!("an out-of-bounds index range was accepted");
        };
        assert!(err.to_string().contains("out of bounds"), "{err}");
    }
}
