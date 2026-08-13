# Phase 8b — tie the push block to its pipeline in the type system

Detailed plan for Phase 8b of [../bindless_textures.md](../bindless_textures.md).
**Status: done**, landed 2026-08-12. Written against `0662b19` (the commit that
landed Phase 8); the line numbers below are that snapshot.
See [§6 Outcome](#6-outcome) for what shipped and what the plan got wrong.

## Why this phase exists

Phase 8 shipped the per-draw push constant channel and paid for it with four
runtime checks: three `assert!`/`panic!` arms in `cmd_push_constants`
(renderer.rs:2461-2508) and an `anyhow::ensure!` in `create_picking_pipeline`
(:1283). **All four exist for one reason** — `PipelineHandle<T>`
(pipeline.rs:93) is parameterized on the *draw-call marker*, not on the shader,
so nothing in the type system knows which push block a handle refers to, or
whether it has one at all.

Phase 8 §4 recorded this as the known limitation. Its `PushConstantBlock` marker
got the cheap half (a `u32` or a vertex struct no longer compiles at the call
site) but could not tie `P` to *this pipeline's own* block — two different blocks
of the same size both passed, and the size assert was the entire runtime check.

**Before Phase 9, not after.** `toon_link` is the first real consumer; written
against the untyped API it would have to be migrated afterwards.

## 1. The encoding, and why it takes two markers

The non-obvious constraint: the *plain* `queue_draw_*` must **reject** a
push-declaring pipeline, and Rust has no negative bounds — there is no way to say
"`P` is not a `PushConstantBlock`". So the parameter cannot be the block type
itself. It has to be a two-variant marker, which makes both signatures concrete
shape matches needing no negative reasoning:

```rust
pub struct NoPush;
pub struct PushBlock<P: PushConstantBlock>(PhantomData<P>);

pub struct PipelineHandle<T, P = NoPush> { … }

pub fn queue_draw_indexed(&mut self, p: &PipelineHandle<DrawIndexed>);           // = NoPush
pub fn queue_draw_indexed_with_push_constants<P: PushConstantBlock>(
    &mut self, p: &PipelineHandle<DrawIndexed, PushBlock<P>>, push: &P);
```

Attempting it with a bare `P` and a `PushConstantBlock` bound will *look* like it
works and will not reject the missing-push case. Start from the two markers.

**`PushBlock<P>`, not the `Block<P>` the parent doc named.** Every generated
shader file glob-imports `renderer::*` (shader_atlas_entry.rs.askama:20); a bare
`Block` is too generic a name to put in that namespace.

### The default type parameter is what makes this cheap

Phase 8 §4 called closing this gap "a much larger refactor than this phase". That
estimate missed `P = NoPush`: `PipelineHandle<DrawIndexed>` still resolves to
`PipelineHandle<DrawIndexed, NoPush>`, so **every existing example compiles
untouched** — all ~14 queue call sites, every stored handle field, and every
`PipelineHandle<Compute>`. The churn collapses to the renderer's own generics.

Apply the same default to `PipelineConfig` and `IndexedPipelineConfig`. That is
also what makes the picking win free (§3).

## 2. What this deletes

| Phase 8 check | after 8b |
|---|---|
| payload size ≠ `range.size` | impossible — `P` *is* the pipeline's block |
| range, no payload | plain method won't take a `PushBlock<P>` handle |
| payload, no range | push method won't take a `NoPush` handle |
| neither | the only remaining branch, and not an error |
| picking `ensure!` (:1283) | a **compile** error at the call site |
| `debug_assert_eq!(range.offset, 0)` | **stays** — a reflection invariant, not a caller state |

**The size check becomes redundant legitimately, and the chain was verified
rather than inherited.** `range.size` is
`element_type_layout.size(ParameterCategory::Uniform)`
(slang-reflection/src/reflection/pipeline_layout.rs:58,71). Codegen reads the
*same* expression into `PushConstantGlobalParameter::element_size`
(reflection/parameters.rs:206), asserts its own computed std430 size equals it
(build_tasks.rs:1050-1054), and emits
`const _: () = assert!(size_of::<DrawConstants>() == 96)`. So
`size_of::<P>() == range.size` is already a compile-time fact. Hot reload cannot
drift them apart either: `assert_shader_interface_unchanged` (renderer.rs:5116)
compares the *entire* serialized reflection, so adding or removing a push block
panics loudly rather than silently re-ranging a live pipeline.

## 3. The work

- **Thread `P` through four types**, all in pipeline.rs: `PipelineHandle` (:93),
  `PipelineConfig` (:288), `IndexedPipelineConfig` (:319), plus
  `PipelineStorage::add`/`get`/`take` (:117, :138, :156) and the two terminal
  builder calls `build_indexed` (:388) / `build_vertex_count` (:405). Also
  `Renderer::create_pipeline` (renderer.rs:1209), `init_pipeline` (:1500) and
  `renderer_pipeline` (:493). **`P` is erased at the storage boundary** —
  `GraphicsPipelineIndex` stays untyped and only the *handle* carries it.
  `PipelineConfigBuilder` (:376) needs nothing: it has no phantom, and the
  terminal call is where the slot is chosen.
- **Codegen emits the parameter** from `GeneratedShaderImpl::config_return_type`
  (build_tasks.rs:538), which builds the return type string by hand and already
  has `push_constant_type_name` on the same struct. No template change.
- **Delete** the three `assert!`/`panic!` arms in `cmd_push_constants` and the
  picking `ensure!`, leaving the `(None, None)` early return and the offset
  debug assert.
- **The stub crate is the easiest thing here to miss** — the same trap Phase 8
  hit with `gpu_write.rs`. `crates/cli/fixtures/check_crate/src/renderer/mod.rs`
  is hand-maintained and must gain `NoPush`, `PushBlock<P>`, the defaulted
  parameter on both config types and the parameter on both terminal calls, or
  `alignment_tests`' `cargo check` of the generated `push_constants.rs` fails.

### Three details that only show up while writing it

1. **Function generics cannot carry defaults** (`invalid_type_param_default` is
   deny-by-default). `build_indexed<V, P>` is fine anyway: `P` is inferred from
   the generated `pipeline_config()`'s *declared return type*, which is what keeps
   the call site turbofish-free — the same mechanism `V` already relied on.
2. **`PipelineHandle`'s `#[derive(Debug)]` has to become a hand-written impl.**
   The derive would put `P: Debug` on the bound, and `PushBlock<P>` has no reason
   to satisfy it. The manual impl over `<T, P>` prints the index and takes no
   bounds at all.
3. **Codegen writes the slot out in both cases**, `NoPush` included — see §7.
   The alternative (emit nothing, lean on the default) keeps every non-push
   generated file byte-identical and so preserves Phase 8's one-named-snapshot
   tripwire; that is how this phase was first written and reviewed, and it is
   why the tripwire held. It was then deliberately given up.

## 4. Verification

`cargo check --workspace --all-targets` is the real test here — a type-level
change that compiles across every example with **zero example edits** is most of
the claim. Plus `just test`, `just lint`, `cargo fmt`, `just sweep`.

**Exactly one snapshot moves** *if* codegen emits nothing for the no-push case:
`…alignment_tests@src__generated__shader_atlas__push_constants.rs.snap`'s
`pipeline_config()` return type gains `, PushBlock<DrawConstants>`. Review it
(`just insta`), do not blind-accept — *which* snapshot moved is the claim. That
is how this phase was reviewed; §7 then traded the tripwire away on purpose, in
a separate commit, so this check still applies to the 8b diff itself.

**The negative half is the point**, and needs the same `multi_mesh` scaffolding
Phase 8 used (phase_08.md §6): a temporary `MultiMeshDraw { float4 tint }` block
with the two `P_CUBE` draws tinted differently. Re-run Phase 8's four control
cases and confirm each is now a **compile** error rather than a panic.

## 5. Out of scope

- **Compute — Phase 12.** `PipelineHandle<Compute>` defaults to `NoPush`, which
  is exactly right while reflection rejects compute push blocks. Phase 12 gets
  the encoding for free and only has to add the dispatch-path push call.
- **Phase 13 shrinks to the API shape.** Its "delete the creation-time
  `ensure!`" bullet is done here, and by a stronger mechanism. What remains is
  only how to supply *two* payloads once picking joins the multi-draw queue.

## 6. Outcome

Everything above shipped as planned; no design decision was reversed. What
follows is the parts worth recording.

**The `P = NoPush` estimate held exactly.** Not one example file was edited —
`cargo check --workspace --all-targets` passed clean on the first run after the
renderer and codegen changes, with no warnings. The whole diff is five files:
`pipeline.rs`, `renderer.rs`, `build_tasks.rs`, the check_crate stub, and the one
snapshot.

**The snapshot tripwire came out as predicted.** `cargo insta test --workspace`
reported *one* snapshot to review and its diff was one line:

```
-    ) -> PipelineConfig<'a, NoVertex, DrawVertexCount> {
+    ) -> PipelineConfig<'a, NoVertex, DrawVertexCount, PushBlock<DrawConstants>> {
```

`just shaders` across every example changed nothing, and `just test` was 163
passed / 0 failed.

### The indexed + push path has no snapshot, and the scaffolding is why that is fine

The `push_constants` fixture is **vertex-count only**, so the
`IndexedPipelineConfig<'a, V, PushBlock<B>>` branch of `config_return_type` is
not covered by any snapshot. The `multi_mesh` scaffolding covers it end to end
instead — that example is indexed, and adding the block produced

```rust
) -> IndexedPipelineConfig<'a, Vertex, PushBlock<MultiMeshDraw>> {
```

which then flowed through `with_shared_mesh` → `create_pipeline` → the stored
handle field → `queue_draw_index_range_with_push_constants`. Worth knowing before
anyone "simplifies" that fixture set.

### All four controls, forced

| Phase 8 control | Phase 8 result | now |
|---|---|---|
| push-declaring pipeline through plain `queue_draw_index_range` | assert fires at record time | `expected &PipelineHandle<_, NoPush>`, found `PushBlock<MultiMeshDraw>` |
| wrong-size payload | assert fires (size mismatch) | `expected &MultiMeshDraw`, found `&ImposterBlock` |
| **same-size** payload of a different block | **rendered — nothing caught it** | same type error as above |
| `basic_triangle` (no block) queued with a payload | assert fires | `expected &PipelineHandle<_, PushBlock<NoBlockHere>>`, found `NoPush` |
| picking shader declaring a block | `ensure!` → `Err` at creation | type error at `create_picking_pipeline` |

**Row three is the one Phase 8 could not do at all** and is worth forcing
explicitly: a local `#[repr(C, align(16))] struct ImposterBlock { tint: Vec4 }`
with hand-written `GPUWrite` + `PushConstantBlock` impls is byte-identical to
`MultiMeshDraw`, so every Phase 8 check passed it and it rendered. It is now a
type error naming both structs.

### The positive control still holds

With the scaffolding in place and `elapsed` frozen to 0, the tinted and
equal-tint captures differ in **exactly one 199×84 region at +0+313** — the
cube's top face, the second `P_CUBE` draw — measured by `compare -metric AE`
(13505 pixels) plus a difference bounding box. Byte-for-byte the same region
Phase 8 measured, so typing the channel did not perturb it. Every other pixel in
the frame is identical: same pipeline, same descriptor set, same params uniform
buffer, two index ranges.

Reverting every scaffolding line and re-running `just shaders multi_mesh` brought
the committed artifacts back byte-identical, including both the marker impl and
the push slot disappearing from the generated return type.

### One deviation from the plan above

`cmd_push_constants`'s mixed arms could not simply be *deleted* — the match still
has to be exhaustive. They collapse to a single `unreachable!` arm whose message
names the invariant (`PipelineHandle`'s push slot) that makes them unreachable,
which is the honest encoding of §2's table rather than a surviving check.

The size check also came back as a `debug_assert_eq!` rather than vanishing. §2's
chain shows it is redundant, but it is now the same *kind* of thing as the
surviving offset assert — an internal reflection invariant, free in release —
and it is the tripwire if the codegen chain it depends on ever changes.

## 7. Follow-up: the push slot is written out even when it is `NoPush`

Landed immediately after 8b, as its own commit. `config_return_type` originally
emitted *nothing* for a shader with no block, leaning on `P = NoPush` — which is
what held §4's one-named-snapshot tripwire while the phase was reviewed. Once
that review was done, the tripwire had served its purpose and the trade came out
the other way:

```rust
None => "NoPush".to_string(),   // was: String::new()
```

**Why.** Generated code should state what reflection found rather than leave it
to be inferred — the same standard that already produces explicit `_padding_N`
fields, explicit size/offset asserts and explicit `impl GPUWrite` rather than
relying on anything implicit. It also decouples codegen from the default's
existence, and it puts `NoPush` in front of a reader who hits
`expected PushBlock<_>, found NoPush` and opens the generated file looking for
where that came from. **The default's remaining job is hand-written code** —
examples' stored `PipelineHandle<DrawIndexed>` fields, `create_picking_pipeline`,
`draw_indexed`, `dispatch` — which is what it is for.

**Cost, paid once:** 26 codegen snapshots and 17 example generated files, one
line each, every diff of the identical shape
(`PipelineConfig<'a, NoVertex, DrawVertexCount>` → `…, NoPush>`). The
`push_constants` snapshot does **not** move — it was already explicit — which is
the cheap confirmation that only the `None` branch changed.

**What this gives up**, and it is worth knowing before the next codegen phase: a
non-push generated file changing *at all* is no longer by itself a signal that a
conditional emit broke. Both branches now emit something, so the check becomes
the weaker "it should say `NoPush`". Phase 12 and 13 both touch this string.
