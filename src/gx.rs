//! GX (GameCube GPU) emulation support for the `toon_link` example.
//!
//! Everything under this module exists to reproduce GX behavior — the manifest
//! schema for converted assets and the TEV uniform packing. The matching asset
//! converter binary lives in `src/gx/bin/convert_link/`.

pub mod model_manifest;
pub mod tev_pack;
