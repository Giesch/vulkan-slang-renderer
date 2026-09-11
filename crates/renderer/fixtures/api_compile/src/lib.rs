//! Fixture crate for the render-graph API compile checks
//! (`crates/renderer/tests/render_graph_api_compile.rs`, phase-2 step P2.1 of
//! `llm_notes/render-graph/08_plain_data_graph/02_schemas_and_compile_checks.md`).
//!
//! [`generated`] is real shader codegen output, produced from
//! `shaders/source/*.slang` by `just shaders api_compile`. Nothing here is
//! hand-written, so the cases cannot drift from what
//! `crates/cli/templates/graph_split.rs.askama` emits.
//!
//! The CLI stub fixture (`crates/cli/fixtures/check_crate`) compiles generated
//! code against stub renderer types. This crate compiles it against the real
//! `mltrs-renderer`.

pub mod generated;
