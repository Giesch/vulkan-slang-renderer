//! The renderer's view of the slang machinery.
//!
//! The compiler and reflector themselves live in `mltrs-slang-reflection`, the
//! workspace's only direct `shader-slang` dependency. This module re-exports
//! them so `mltrs::shaders::…` stays a valid path for generated code and the
//! cli, and owns the parts that need vulkan: the atlas traits, and turning
//! reflected data into vulkan objects.

pub mod atlas;
pub mod json;

pub use mltrs_slang_reflection::*;

/// Compiled spv as the `u32`s vulkan wants.
///
/// An extension trait because `CompiledShader` is defined in
/// `mltrs-slang-reflection`, which deliberately has no `ash` dependency. Only
/// the hot-reload path needs this — the cli writes the raw
/// `shader_bytecode: Vec<u8>` straight to disk.
#[cfg(debug_assertions)]
pub trait SpvBytes {
    fn spv_bytes(&self) -> Result<Vec<u32>, std::io::Error>;
}

#[cfg(debug_assertions)]
impl SpvBytes for CompiledShader {
    fn spv_bytes(&self) -> Result<Vec<u32>, std::io::Error> {
        let byte_reader = &mut std::io::Cursor::new(self.shader_bytecode.as_slice());
        ash::util::read_spv(byte_reader)
    }
}
