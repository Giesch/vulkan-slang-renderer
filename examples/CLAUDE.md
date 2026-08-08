# Examples

## Per-example recipes

An example that needs its own build tasks carries a `justfile` (and a
`scripts/` dir) inside its crate; the root justfile declares each one as a
`mod`, so they run as `just <example> <recipe>` — e.g.
`just toon_link link-verify-p1`, `just sdf_2d beats`. `just --list` at the root
shows them all. **just sets the working directory to the example's crate dir**
when running a submodule recipe, so paths in those justfiles and scripts are
crate-relative, not repo-relative.

`toon_link` also keeps its gitignored, machine-local Wind Waker assets inside
the crate at `examples/toon_link/assets/link/`.
