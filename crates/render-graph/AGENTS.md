# Render graph

Last verified: 2026-09-13

## Purpose and dependencies

This crate owns graph behavior without a graphics stack. Renderer implements the
backend interfaces; graph does not depend on renderer, ash, vk-mem, SDL, or Slang.
`docs/render_graph.md` describes the public lifecycle and integration API.

## Contracts

- Keep `BindingResolver` and generated shader parameter interfaces concrete.
  Backend lookups resolve flight-slot addresses without exposing storage.
- ABI address and bindless wrappers have one nominal identity, owned here and
  reexported by renderer. Keep moved-type conversions here as well.
- `IndexedIndirectArgs` exposes values, not an ABI guarantee. Accept indirect
  nodes only for the backend's exact associated record through sealed recursive
  `CompatibleWith` bounds. Erased request and batch construction remains private.
- Retain upload staging as `MaybeUninit` bytes; do not read Rust padding as `u8`.
- Prepared graphs retain backend resource ownership. Preparation rejects extent
  limits before allocation; partial failures preserve backend registration policy.
- Frame submission applies writes after the flight-slot wait. Commit texture
  cursors once after successful submission, before presentation; a presentation
  error does not undo submitted state.

## Verification

Run `cargo test -p mltrs-render-graph` for logical, fake-backend, dependency-closure,
and doc tests. Renderer owns ABI/range and production submission-coordinator tests.
The renderer API compile harness tests exact backend compatibility and sealing;
`just sweep` verifies actual Vulkan recording and teardown. See `docs/testing.md`.
