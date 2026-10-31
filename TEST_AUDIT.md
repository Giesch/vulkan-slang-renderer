# Test audit

An audit of the workspace's ~525 Rust tests against one standard: a test is a
deletion candidate if its failure would not indicate broken production code.

## Standard

Flag a test if any of:

1. Its assertions hold for any correct stdlib/OS/runtime behavior,
   independent of this repo's own code.
2. Its assertions check output of test-support/fixture code, not the module
   under test.
3. No invariant, contract, or user-facing behavior backs the assertion.
4. It's redundant with a narrower, cheaper test proving the same thing.
5. It's tautological — it asserts a value against itself or a value it just
   set.

Each flagged test carries a confidence label: `CONFIRMED` (clearly low-value)
or `LIKELY` (borderline, needs a second look).

## Excluded by default

Three patterns `docs/testing.md` explicitly defends were scanned for outliers
only, not flagged wholesale:

- render-graph's compile-fail case harness
  (`crates/renderer/tests/render_graph_api_compile.rs`) — asserts the
  specific failure site/code, not just "it failed."
- insta snapshot tests — prove generated code is rustfmt-clean/byte-identical
  to committed output.
- the validation sweep's fault-injection self-tests — validate the detector
  against known-bad inputs.

No outliers were found in any of the three.

## Candidates

| Test | File:Line | Confidence | Reasoning | Introduced | Last changed |
|---|---|---|---|---|---|
| `texture_names_come_from_the_caller` | `crates/render-graph/src/runtime.rs:2081` | CONFIRMED | Asserts a trivial field-copy pass-through (the texture name echoed back); exercises no graph logic — id, phys-count, and format are untested here. | `a3780be` 2026-09-13 | same |
| `indirect_command_vulkan_layout` | `crates/renderer/src/renderer/indirect.rs:42` | CONFIRMED | Duplicates, verbatim, a `const _: () = { assert!(...) }` compile-time check already in the same file. A build failure already catches any violation; this test can't fail independently of one. | `d0d97a4` 2026-09-13 | same |
| `s16_shift8_uv_decodes` | `crates/convert-link/src/bmd/vtx1.rs:204` | CONFIRMED | Asserts `384_f32 / 256.0 == 1.5`, pure floating-point arithmetic that calls no code from this crate. The real UV-decode path is already covered by `parses_three_arrays` in the same file. | `7704292` 2026-07-05 | same |
| `invbind_identity_holds_for_pure_rotation` | `crates/convert-link/src/pose.rs:492` | CONFIRMED | Checks `M * M⁻¹ ≈ I` using glam's own `Mat4::inverse()`. This holds for any invertible matrix regardless of whether this crate's FK transform is correct. `two_joint_chain_fk` already tests the real transform against independently known values. | `7704292` 2026-07-05 | same |
| `live_channel_slots_past_the_pair_count_are_fine` | `crates/convert-link/src/tev_ir.rs:922` | LIKELY | Calls `describe` on the fixture used by `baseline_fixture_is_accepted`, completely unmodified. No mutation distinguishes this test's input, so it doesn't independently prove the invariant it claims to. | `97db02c` 2026-07-27 | same |
| `locations_match_the_reflected_varying_indices` | `crates/slang-reflection/src/json/vertex_layout.rs:273` | LIKELY | Re-derives expected locations from the same fixture already fully pinned by `vec3_fields_pack_tightly`. Adds no case the narrower test doesn't already cover. | `099f290` 2026-09-22 | same |
| `reflection_value_roundtrip_is_stable` | `crates/slang-reflection/src/json.rs:37` | CONFIRMED | Pure serde round-trip (parse → serialize → reparse → compare) with no assertion on field names or structure. A schema-breaking change that round-trips consistently would not be caught. | `e72b6a4` 2026-08-06 | same |
| `animation_schema_roundtrip_all_formats` | `crates/gx/src/animation_manifest.rs:377` | CONFIRMED | Builds clips, serializes, deserializes, and asserts equality to the original value with no check on actual JSON shape. A shape regression invisible to round-trip would pass. | `65213f7` 2026-09-12 | `a95e3ec` 2026-09-12 |
| `animation_schema_roundtrip` | `crates/gx/src/animation_manifest.rs:509` | CONFIRMED | Same tautological round-trip pattern as `animation_schema_roundtrip_all_formats`, and fully redundant with it. | `b02b679` 2026-09-12 | `a95e3ec` 2026-09-12 |
| `track_defaults_and_rotation_units` | `crates/gx/src/animation_manifest.rs:596` | CONFIRMED | Every assertion is inline arithmetic/stdlib (`matches!` on a unit variant, a literal bit-angle conversion, a byte shift) with no call into any production function. Nothing here can fail from a code regression in this crate. | `b02b679` 2026-09-12 | `08ef640` 2026-09-12 |
| `manifest_round_trips` | `crates/gx/src/model_manifest.rs:776` | CONFIRMED | Serializes/deserializes a `Manifest` and checks three scalar values survive; asserts nothing about JSON field names or nesting, so it only proves serde derive works. | `7704292` 2026-07-05 | same |
| `both_modes_satisfy_game_bound` | `examples/toon_link/src/main.rs:1743` | CONFIRMED | Calls a no-op generic function to check a trait bound already required to compile by the production `impl Game` blocks elsewhere in the same file. A violation would fail the build, not this test. | `973cf07` 2026-09-13 | same |

## Per-crate coverage

| Crate | Tests reviewed | Candidates |
|---|---|---|
| cli | 79 | 0 |
| render-graph | 132 | 1 |
| renderer | 22 | 1 |
| convert-link | 128 | 3 |
| slang-reflection / gx / mltrs / roc-platform / scripts | 45 | 5 |
| examples | 119 | 1 |

## Not included

`test_app_platform_sources_survive_generation_and_relocation`
(`crates/cli/src/roc_codegen.rs`) was deleted directly as part of this audit,
rather than listed here as a candidate: it mostly proved OS rename semantics
and a test-support helper's own output, not the behavior of
`write_precompiled_roc_shaders`.
