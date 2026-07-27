# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Build Commands

```bash
cargo check --all-targets  # Check source AND examples for type errors
just shaders               # Generate shader bindings (MUST run after .slang changes)
just test                  # Run tests (snapshot testing via insta)
cargo insta test --accept  # accept all modified snapshots
just lint                  # Clippy with warnings as errors
timeout 3 just dev EXAMPLE # run an example to check for vulkan validation errors
cat shaders/compiled/EXAMPLE.json | jq '.' # inspect shader reflection json
```

### After changes
- Always run `just shaders` after modifying any `.slang` files to regenerate Rust bindings.
- Always use `cargo check --all-targets` when changing rust files as a first pass.
  NOTE `--all-targets`, not `--all`: `--all` means "all workspace members" and
  silently skips examples, so a broken example passes. `--all-targets` covers
  examples, benches and `#[cfg(test)]` code. Same distinction applies to clippy
  (`just lint` uses `--all-targets`).
- Always use `just test` when making changes to shaders/build_tasks.rs
- Run `cargo fmt` after a set of rust file changes are complete
- Never edit `src/generated/` by hand — `just shaders` regenerates it.

## Shader System

**Workflow:**
1. Create/edit `shaders/source/*.shader.slang`
2. Run `just shaders`
3. Generates: SPIR-V bytecode + reflection JSON + Rust bindings in `src/generated/`

## Testing

Uses insta for snapshot testing of generated code:
```bash
just test                  # Non-interactive (CI)
just insta                 # Interactive review
cargo insta test --accept  # Re-run and accept every changed snapshot
```

NOTE `cargo insta accept` on its own does nothing after `just test`: that recipe
sets `INSTA_UPDATE=no`, so no `.snap.new` files are written for it to review.
Use `cargo insta test --accept`, which re-runs the tests and writes the
snapshots in one step. Review the diffs `just test` prints before accepting.
