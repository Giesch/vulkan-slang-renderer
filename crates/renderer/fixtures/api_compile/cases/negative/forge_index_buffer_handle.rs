//! Negative control: only `Renderer::create_index_buffer` mints index buffer
//! handles. A caller cannot build one with an index count of its own choosing,
//! because the fields are private.

#![allow(dead_code)]

use mltrs_renderer::renderer::{IndexBufferHandle, IndexBufferIndex};

fn forge(index: IndexBufferIndex) -> IndexBufferHandle {
    IndexBufferHandle {
        index,
        index_count: u32::MAX,
    }
}

fn main() {}
