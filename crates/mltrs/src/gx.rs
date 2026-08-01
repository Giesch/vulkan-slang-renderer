//! GX (GameCube GPU) emulation support for the `toon_link` example.
//!
//! Interim facade during the workspace migration: the manifest schema lives
//! in the `gx` crate (shared with `convert-link`), and the TEV uniform
//! packing stays here because it depends on the interim generated bindings.
//! Both move into the `toon_link` example crate when it splits out.

pub use ::gx::model_manifest;

pub mod tev_pack;
