# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Where the docs are

- **`docs/`** — current reference material, kept up to date. Trust it, and update
  it when you change what it describes.
- **`llm_notes/`** — historical plans and phase records, written before or during
  a piece of work. **Treat as possibly out of date**: much of it is a snapshot of
  what was believed at the time, some of it was superseded by the work it
  describes, and a few claims turned out to be wrong (those are annotated in
  place rather than deleted, so the record stays honest). Useful for *why* a
  thing is the way it is; verify against the code before acting on it.

## Build Commands

```bash
cargo check --all-targets  # Check source AND examples for type errors
just shaders               # Generate shader bindings (MUST run after .slang changes)
just test                  # Run tests (snapshot testing via insta)
cargo insta test --accept  # accept all modified snapshots
just lint                  # Clippy with warnings as errors
just watch EXAMPLE         # build, then run one example for a few seconds
just sweep                 # run EVERY example headlessly, fail on validation output
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
- Never call `std::env::var` outside `src/env_config.rs`. Every variable is
  parsed once at startup into `EnvConfig` and passed down from there.

## Shader System

**Workflow:**
1. Create/edit `shaders/source/*.shader.slang`
2. Run `just shaders`
3. Generates: SPIR-V bytecode + reflection JSON + Rust bindings in `src/generated/`

## Testing

```bash
just test                  # Non-interactive (CI)
just insta                 # Interactive review
cargo insta test --accept  # Re-run and accept every changed snapshot
just sweep                 # run all examples, checking for vulkan validation errors
just sweep-self-test       # check that the sweep still detects an injected fault
```

Always run `just test` when changing `src/shaders/build_tasks.rs`.

Run `just sweep` when a change could affect what the renderer records or destroys.

See [`docs/testing.md`](docs/testing.md) before writing a validation check or accepting a snapshot.
