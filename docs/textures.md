# Textures

Every example texture is a KTX2 file with pre-baked mip levels. Each file is
encoded at build time by [`ctt`](https://github.com/cwfitzgerald/ctt). Each file
is committed next to the source image it came from.

```bash
cargo install ctt-cli    # one-time; only needed to change an asset
just textures            # re-encode every example's images
just EXAMPLE textures    # re-encode one example's
```

`cargo build` and `cargo run` do not need `ctt`. The `.ktx2` files are
committed, the same arrangement as `shaders/compiled/`.

## The two format groups

The format is chosen per asset, by what the content is:

| group | flags | why |
|---|---|---|
| models / photographic | `-f bc7 --mipmap --quality very-slow --zstd=19` | 4× less VRAM; the loss is invisible on this content |
| pixel art | `-f rgba8unorm --zstd=19` (no `--mipmap`) | bit-exact, and smaller on disk than the source PNG |

Pixel art uses the lossless group for these reasons:

- No GPU block format is lossless. BC1–BC7, ETC/EAC and ASTC are fixed-rate by
  construction. A constant byte budget per block is what buys constant-time
  random texel access.
- BC7 mode 6 represents any 4×4 block of 2 or fewer distinct 8-bit RGBA colors
  exactly. That covers almost all upscaled pixel art.
- No available tool decodes BC7 back to RGBA. There is no way to prove the
  encoder took the mode 6 path.

The lossless group also beats decoding a PNG at startup. The file is smaller,
the load is a zstd inflate plus a memcpy instead of a PNG decode, and the VRAM
cost is the same.

## Device support

- BC7 is the desktop-universal family. ETC and ASTC are not considered.
- BC7 requires the `textureCompressionBC` device feature. `create_logical_device`
  enables it. `choose_physical_device` requires it.
- `choose_physical_device` also checks that every format in `TEXTURE_FORMATS`
  supports `SAMPLED_IMAGE | TRANSFER_DST | SAMPLED_IMAGE_FILTER_LINEAR`. The
  list holds both RGBA8 spellings and both BC7 spellings.
- No conformant device fails these checks. The Vulkan mandatory-format table
  covers RGBA8, and `textureCompressionBC` covers BC7. A device that does fail is
  named at startup, so no texture load has to check device support.

## Mip levels

- The renderer never generates mip levels. Every level it uploads comes from the
  file. There is no mip flag in the options.
- Pixel art gets a single level on purpose. `Nearest`-filtered 2D content never
  minifies, so a mip chain is wasted.

## The upload path

`create_texture_from_levels` in `renderer.rs` is the one upload path. Every
public entry point calls into it:

| entry point | levels | format from | sampling |
|---|---|---|---|
| `create_texture_with_mips` | the file's chain | the file's `vkFormat` | `SamplerOptions` |
| `create_texture{,_with_options}` | one | `TextureOptions::color_space` | `TextureOptions::sampler` |

`create_texture_with_options` is the one-level RGBA8 case of a mip upload. Its
`color_space` is how a caller that holds raw bytes names the format a KTX2 file
would carry. The rest of the work is shared:

- the staging buffer and its block-aligned per-level offsets
- the layout transitions
- the image view
- the sampler

The only format gate in the upload path is membership of `TEXTURE_FORMATS`,
because device support is settled at startup. The sampler reads the level count.
A one-level image gets `max_lod = 0` and anisotropy off, rather than a LOD clamp
that points at a level that is not there.

Each level's byte length is checked against what its extent implies in the
format's block layout (`level_byte_len`). A short slice fails before the copy,
rather than reading past the level it was given. The KTX2 loader sizes its zstd
output with the same helper.

## Adding or changing a texture

1. Put the source image in the example's `textures/` (or `models/`).
2. Add a `textures` recipe to the example's justfile. Add a `mod` line in the
   root justfile if the example does not have one. Copy the closest existing
   example. The BC7 examples are `viking_room`, `depth_texture`, `koch_curve`
   and `suzanne`. The lossless examples are `serenity_crt`, `sprite_batch` and
   `space_invaders`.
3. Add the example to `examples_with_textures` in the root justfile. `just
   textures` with no argument skips any example that is not in that list.
4. Run `just EXAMPLE textures`.
5. Load the file with `mltrs::ktx::load_ktx2_texture(renderer, &file_path, filter)`.
6. Commit the source image and the `.ktx2`.

Where an encode reads another generated file, chain the two with just's deferred
dependency. `examples/space_invaders/justfile` does `sprites: && textures`.

## Not on this path

- **`watercolor`** — `paper_height.png` is decoded on the CPU into a `Vec<f32>`
  luma buffer and written into an `R32_SFLOAT` storage texture. It does not go
  through the KTX2 path, so no container or block format applies.
- **`multi_mesh`** — generates its 8×8 images in memory. There are no files to
  convert. A single level suits it: a mip chain would average the checkerboard to
  flat gray at distance and destroy the wrap/filter test its panels exist for.
- **`toon_link`** — needs more than a recipe. Its 44 PNGs are gitignored and
  produced by `convert_link`. It needs `BC7_UNORM_BLOCK` rather than sRGB. It
  needs per-entry `wrap_u`/`wrap_v`, which `ktx::load_ktx2` plus
  `create_texture_with_mips` carries through `SamplerOptions`.

## Determinism

`ctt` output is byte-deterministic, across both format groups and including
multithreaded BC7 at `--quality very-slow`. That is what makes committing the
artifacts safe. Treat `just textures` followed by a dirty `git status` as a
regression. Investigate it rather than committing the churn.

`--quality very-slow` costs about half a second for a 1024² image with a full mip
chain. The artifacts are regenerated rarely and committed, so that cost does not
matter. Do not encode at a lower quality.

## Inspecting a file

```bash
xxd -l 48 examples/viking_room/textures/viking_room.ktx2
```

| offset | field | |
|---|---|---|
| `0x0C` | `vkFormat` | `146` = `BC7_SRGB_BLOCK`, `43` = `R8G8B8A8_SRGB` |
| `0x14` / `0x18` | width / height | |
| `0x28` | `levelCount` | `1` for the lossless group |
| `0x2C` | `supercompressionScheme` | `0` = none, `2` = zstd |

The loader is `crates/mltrs/src/ktx.rs`. It accepts a KTX2 file that is 2D,
non-array and non-cubemap, whose `vkFormat` is a concrete Vulkan format known to
`format_block_info`, and that uses either no supercompression or zstd. It checks
each level's decompressed size against the size implied by the dimensions and the
block layout. A truncated or mislabelled file fails at load rather than sampling
garbage.
