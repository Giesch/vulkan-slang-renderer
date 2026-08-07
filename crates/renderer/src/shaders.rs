pub mod atlas;
pub mod json;

pub use mltrs_slang_reflection::*;

#[cfg(debug_assertions)]
pub trait SpvBytes {
    /// converts compiled spv to vulkan-readable u32s
    fn spv_bytes(&self) -> Result<Vec<u32>, std::io::Error>;
}

#[cfg(debug_assertions)]
impl SpvBytes for CompiledShader {
    fn spv_bytes(&self) -> Result<Vec<u32>, std::io::Error> {
        let byte_reader = &mut std::io::Cursor::new(self.shader_bytecode.as_slice());
        ash::util::read_spv(byte_reader)
    }
}
