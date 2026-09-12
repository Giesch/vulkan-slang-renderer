//! GX (GameCube GPU) emulation support for the `toon_link` example.
//!
//! The manifest schemas for converted assets (`model_manifest` for the
//! model, `animation_manifest` for the extracted Link animations), shared
//! between the `convert_link`/`convert_link_animations` asset converters
//! and the example that renders their output. The TEV uniform packing lives
//! with the example itself (it depends on the example's generated shader
//! bindings).

pub mod animation_manifest;
pub mod model_manifest;
