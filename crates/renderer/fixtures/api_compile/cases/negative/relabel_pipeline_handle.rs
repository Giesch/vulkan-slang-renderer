//! Negative control: only the renderer mints pipeline handles. A caller cannot
//! rebuild a `Graphics` handle as a `GraphicsIndirect` one, because the fields
//! are private.

#![allow(dead_code)]

use std::marker::PhantomData;

use mltrs_renderer::renderer::pipeline::{Graphics, GraphicsIndirect, PipelineHandle};

fn relabel(handle: PipelineHandle<Graphics>) -> PipelineHandle<GraphicsIndirect> {
    let PipelineHandle { index, .. } = handle;

    PipelineHandle {
        index,
        _phantom_data: PhantomData,
    }
}

fn main() {}
