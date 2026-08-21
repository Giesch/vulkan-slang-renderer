# Bindless

The renderer reaches buffers and textures as plain data. These are the
preferred defaults:

- A buffer field is a device address: `mltrs::Addr<T>`, `mltrs::ReadAddr<T>`,
  or `mltrs::ImmutableAddr<T>`. Reflection rejects `StructuredBuffer` fields.
- A texture field is a heap handle: `Sampler2D.Handle` to read,
  `RWTexture2D.Handle` to write. It indexes a global texture heap. Every
  texture joins the heap at creation.

Both are uniform data, not descriptors. Both can sit in a `ParameterBlock`,
in a push constant block, or in a std430 struct behind a pointer. A texture
reached through a handle is not welded to a pipeline, so one pipeline can
draw many textures.

One descriptor remains: the `ParameterBlock` itself, because a uniform buffer
must carry the handles.

## Texture handles

Declare a handle where a `Sampler2D` or `RWTexture2D` field would go:

```slang
struct Material {
    Sampler2D.Handle tex0;
    RWTexture2D<float4>.Handle target;
    float4 tint;
}
```

- A handle is 8 bytes of uniform data (a `uint2`). It consumes no
  descriptor, so the generated `Resources` struct has no field for it.
- Codegen emits the field as `BindlessHandle<Sampler2D>` or
  `BindlessHandle<RwTexture2D>`. Mint the value with
  `TextureHandle::bindless_handle()` or
  `StorageTextureHandle::bindless_handle()`.
- Reflection detects handle declarations, and the renderer binds the heap
  for those pipelines. No app code binds anything.
- Reflection accepts two handle shapes: `Sampler2D` at binding 1 and
  `RWTexture2D` at binding 3. Each shape matches a heap binding the
  renderer declares. Reflection rejects other handle shapes and handle
  arrays.
- One storage-image heap array serves every element type. Slang emits one
  image type per access class, and the format is not part of that type. So
  `RWTexture2D<float>` and `RWTexture2D<float4>` share the one binding.
- The compiler pins the Slang `None` bindless preset. Each descriptor type
  then gets its own heap binding: 0 sampler, 1 combined image sampler, 2
  sampled image, 3 storage image. This renderer uses `None` rather than
  the Slang default preset. The default puts every non-sampler type on one
  binding, and needs a `VK_DESCRIPTOR_TYPE_MUTABLE_EXT` binding.
- The access site does not change. `material.tex0.Sample(uv)` works as if
  the field were a `Sampler2D`. `material.target[pixel] = c` works as if
  the field were an `RWTexture2D`. `Sampler2D tex = material.tex0;`
  converts at a boundary, so helper functions stay handle-free.

`examples/watercolor` is the reference for storage handles. A per-dispatch
write target lets one compute pipeline write both textures of a ping-pong
pair. `wc_pressure_jacobi` is the reference for per-dispatch handles: both
sides of the pair live in its push block, so one pipeline serves two
dispatches that swap them within one frame.

`examples/depth_texture` is the minimal form: one handle in the
`ParameterBlock`, written each frame from `bindless_handle()`.

## Per-draw data: push constants

A push constant block is the per-draw and per-dispatch channel:

```slang
[[vk::push_constant]] ConstantBuffer<MyDraw> draw;
```

- One block per shader, at most 128 bytes, std430 layout. Codegen emits the
  Rust struct and a compile-time size assert.
- Queue with `queue_draw_indexed_with_push_constants`,
  `queue_draw_index_range_with_push_constants`,
  `queue_draw_vertex_count_with_push_constants`, or
  `dispatch_with_push_constants`. Every pipeline handle carries the block
  type (`PipelineHandle<D, PushBlock<P>>` versus
  `PipelineHandle<D, NoPush>`), graphics and compute alike, so a missing,
  extra, or wrong-type payload is a compile error.
- The payload is captured at queue time, so two dispatches of one pipeline
  in one frame each read the value in hand when they were queued.
- A picking pipeline accepts only `NoPush` handles.
- A push block can carry a device address. `FrameRenderer` mints addresses
  at queue time: `singleton_addr_at` for singleton buffers,
  `current_immutable_addr_at` for ringed immutable buffers.

## The uniformity rule

A texture handle — and any index or pointer that selects the struct that
carries it — must be dynamically uniform within one draw.

- Do not read a handle, or an index that selects one, from vertex or
  instance data.
- Do not select between handles per invocation, for example a per-pixel
  ternary between two material textures.

Nothing enforces this rule. Slang compiles a divergent heap access without
`NonUniformEXT` and without complaint. Validation cannot see it, because the
divergence is data-dependent. Reflection sees declarations, not indexing
expressions. `just sweep` stays green. The failure is wrong-texture
rendering on wave-scalarizing hardware (AMD), while other hardware renders
correctly.

The renderer has no support for an index that varies within a draw. That
needs a `getDescriptorFromHandle` override plus the
`shaderSampledImageArrayNonUniformIndexing` device feature, and neither is
implemented.

## The supported pattern: one draw per material

Satisfy the rule with a push constant. A push constant is constant for the
draw by definition, so a handle selected by push data is uniform by
construction.

`examples/toon_link` is the reference:

- `Material` (two `Sampler2D.Handle` fields plus TEV state) lives in a
  singleton buffer, one element per material, in `MaterialSlot` order.
- The push block carries a pointer at one element:
  `ToonLinkDraw { mltrs::ImmutableAddr<Material> material; }`.
- The draw loop issues one draw per batch. `MaterialSlot::push`
  (`examples/toon_link/src/main.rs`) mints the element address with
  `singleton_addr_at`, and the draw queues with
  `queue_draw_index_range_with_push_constants`.
- Both entry points read `draw.material[0]` directly. No interstage
  varying, no vertex-input change.

The pointer form is the strongest shape: the shader holds no selecting
expression that could diverge, only a pushed address.

For upload-once data such as a material table, use a singleton buffer
(`create_singleton_buffer`): one allocation, a stable address, and no
`Gpu` write accessor.
