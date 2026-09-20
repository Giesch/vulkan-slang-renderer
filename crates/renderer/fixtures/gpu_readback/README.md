# Generated GPU readback diagnostic

From the workspace root on Linux with lavapipe and Vulkan validation layers:

```sh
SDL_VIDEODRIVER=offscreen \
VK_ICD_FILENAMES=/usr/share/vulkan/icd.d/lvp_icd.json \
RUST_LOG=warn \
cargo test -p mltrs-renderer --test gpu_readback -- --ignored --nocapture
```

Use the installed lavapipe ICD path (some distributions call it
`lvp_icd.x86_64.json`). Slang and the normal renderer build dependencies must
be installed. This test is explicitly ignored by ordinary `cargo test`; the
command above executes it and fails on missing prerequisites, shader/codegen
failure, Vulkan initialization failure, mismatched output, or validation
warnings/errors. No assets, frame presentation, or animation setup are needed.

The harness copies these sources into a process-specific temporary crate,
generates SPIR-V/reflection/Rust with the existing CLI `write_precompiled_shaders`
API, and runs that crate against the real renderer. It seeds dependency
resolution from the workspace lockfile and uses offline cargo execution.
Successful runs remove the temporary crate; failures retain it for inspection.
No generated source is maintained here or edited by hand.

Two one-thread workgroups write two full output records. Independent numeric
assertions cover a nested struct, two enum variants, vectors, a fixed array,
and a nonsymmetric nonidentity matrix. A second actual dispatch writes enum
value 99 and must produce a checked decoder error. Validation message counting
is checked after renderer destruction, independently of the logging filter.

Matrix contract: row-major Slang storage is decoded into glam columns, matching
the inverse raw upload representation, not same-index mathematical entries.
The test asserts those explicit columns separately from the shader-computed
matrix/vector product. It does not claim animation skinning or frame-loop
coverage. Software Vulkan is real Vulkan execution, not hardware-driver coverage.
