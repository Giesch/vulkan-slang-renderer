//! Hardware index buffers for indexed draws.
//!
//! Vertex data is pulled by the shader through a buffer device address, so an
//! index buffer is untyped: it holds `u32` indices into whatever vertex buffer
//! the draw's parameters point at. Binding it with `vkCmdBindIndexBuffer`
//! keeps the GPU's post-transform vertex reuse cache.

use ash::vk;

/// Index into `Renderer::index_buffers`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct IndexBufferIndex(usize);

impl IndexBufferIndex {
    pub(super) fn raw(self) -> usize {
        self.0
    }
}

/// A device-local buffer of `u32` indices created by
/// `Renderer::create_index_buffer`. Indexed draws bind it; the vertices it
/// indexes are read by the shader through an address binding.
#[derive(Debug)]
pub struct IndexBufferHandle {
    index: IndexBufferIndex,
    index_count: u32,
}

impl IndexBufferHandle {
    /// The number of indices in the buffer.
    pub fn index_count(&self) -> u32 {
        self.index_count
    }

    pub(super) fn index(&self) -> IndexBufferIndex {
        self.index
    }
}

pub(super) struct RawIndexBuffer {
    pub(super) buffer: vk::Buffer,
    pub(super) allocation: vk_mem::Allocation,
    pub(super) index_count: u32,
}

/// Index buffers ordered by handle index. Like singleton buffers they are
/// written once at creation and have no per-frame copies.
pub(super) struct IndexBufferStorage(Vec<Option<RawIndexBuffer>>);

impl IndexBufferStorage {
    pub fn new() -> Self {
        Self(Default::default())
    }

    pub(super) fn contains(&self, index: usize) -> bool {
        self.0.get(index).is_some_and(Option::is_some)
    }

    pub fn add(&mut self, buffer: RawIndexBuffer) -> IndexBufferHandle {
        let handle = IndexBufferHandle {
            index: IndexBufferIndex(self.0.len()),
            index_count: buffer.index_count,
        };

        self.0.push(Some(buffer));

        handle
    }

    /// The live buffer at `index`, or `None` once it has been dropped.
    pub(super) fn get_by_index(&self, index: usize) -> Option<&RawIndexBuffer> {
        self.0.get(index).and_then(Option::as_ref)
    }

    #[expect(dead_code)]
    pub fn take(&mut self, handle: IndexBufferHandle) -> RawIndexBuffer {
        self.0[handle.index.raw()].take().unwrap()
    }

    pub fn take_all(&mut self) -> Vec<RawIndexBuffer> {
        self.0
            .iter_mut()
            .filter_map(|option| option.take())
            .collect()
    }
}

/// The setup-time checks `Renderer::create_index_buffer` runs before any GPU
/// allocation. Vulkan cannot allocate an empty buffer, so emptiness is always
/// rejected; the O(n) index scan runs only when `check_bounds` is set, which
/// the renderer ties to debug builds.
pub(super) fn validate_indices(
    indices: &[u32],
    vertex_count: u32,
    check_bounds: bool,
) -> anyhow::Result<()> {
    anyhow::ensure!(
        !indices.is_empty(),
        "an index buffer needs at least one index; vulkan cannot allocate an empty buffer"
    );
    anyhow::ensure!(
        vertex_count > 0,
        "an index buffer needs a nonzero vertex count to index into"
    );
    u32::try_from(indices.len())
        .map_err(|_| anyhow::anyhow!("{} indices exceed u32::MAX", indices.len()))?;

    if !check_bounds {
        return Ok(());
    }

    let out_of_bounds = indices
        .iter()
        .enumerate()
        .find(|&(_, &vertex_index)| vertex_index >= vertex_count);
    if let Some((position, vertex_index)) = out_of_bounds {
        anyhow::bail!(
            "index {position} of the index buffer is {vertex_index}, which is out of bounds \
             for a vertex count of {vertex_count}"
        );
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::validate_indices;

    #[test]
    fn in_bounds_indices_are_accepted() {
        validate_indices(&[0, 1, 2, 2, 1, 3], 4, true).unwrap();
    }

    #[test]
    fn empty_indices_are_rejected() {
        let error = validate_indices(&[], 3, true).unwrap_err();
        assert!(error.to_string().contains("at least one index"), "{error}");

        // emptiness is checked even when the bounds scan is off
        assert!(validate_indices(&[], 3, false).is_err());
    }

    #[test]
    fn a_zero_vertex_count_is_rejected() {
        let error = validate_indices(&[0], 0, true).unwrap_err();
        assert!(
            error.to_string().contains("nonzero vertex count"),
            "{error}"
        );

        assert!(validate_indices(&[0], 0, false).is_err());
    }

    #[test]
    fn an_out_of_bounds_index_names_its_position_and_the_vertex_count() {
        let error = validate_indices(&[0, 1, 2, 3, 1], 3, true).unwrap_err();
        let message = error.to_string();
        assert!(message.contains("index 3"), "{message}");
        assert!(message.contains("is 3"), "{message}");
        assert!(message.contains("vertex count of 3"), "{message}");
    }

    #[test]
    fn the_bounds_scan_is_skipped_when_disabled() {
        validate_indices(&[0, 7], 3, false).unwrap();
    }
}
