# Link model scaling metadata

The model converter exports the parsed INF1 matrix scaling convention as
`skeleton.scaling_rule`, with JSON values `BASIC`, `SOFTIMAGE`, or `MAYA`.
Each joint exports `scale_compensate`, the boolean value of the JNT1
no-inherit-scale flag (`no_inherit_scale != 0`).

The shared Rust types are `gx::model_manifest::ScalingRule`,
`Skeleton::scaling_rule: ScalingRule`, and
`SkeletonJoint::scale_compensate: bool`. Both fields are required; manifests
missing either field fail deserialization. Regenerate manifests produced before
scaling metadata was exported with `just toon_link convert-link`. The model
manifest version remains 1. Animation consumers must validate their required
scaling convention.

For the extracted `cl.bdl`, INF1's low flag nibble is 2 (Maya). There are 42
joints, with compensation enabled on 12. Every bind scale remains exactly
`[1, 1, 1]`. This export does not change bind baking, geometry, skin weights,
textures, or animation manifests/conversion.

## Regeneration and checks

Run `just toon_link convert-link`, then run the model gates separately:

```sh
just toon_link link-verify-p1
just toon_link link-verify-p2
just toon_link link-verify-p3
cargo test -p gx -p convert-link -- --include-ignored
```

The schema tests reject missing metadata and cover explicit values for every rule
and both compensation flags. The real `real_bake_and_manifest` test checks Maya,
42 joints, 12 enabled flags, exact per-joint source mapping, and unit bind scales.

### Golden review status

The metadata export produces model manifest SHA-256
`00e139c571035e48dbd81a1d8b447c0bbc1b8bf017a0f87607f2ed6090970777`.
Removing only the new metadata lines reproduces the pre-amendment local manifest
byte-for-byte, SHA-256
`fba3c4365dfbdcb84e5f7ddd35aba6279f5fe43012ad3d045b5cdd1f63a792e7`.
All other entries in `examples/toon_link/scripts/link_converted.sha256` pass,
including vertex/index/skin binaries, MAT3 dump, and textures.

The previous tracked manifest golden expected
`f5d82177f19824055a50c2f49c40242a523df3e6710e2386f24b0162d260b74e`.
Its mismatch already existed before the metadata amendment: historical commit
`c6ed71e` removed texture `mipmaps` fields without updating that golden. Removing
the new metadata and restoring `mipmaps: false` after each texture filter,
including the original JSON comma placement, reproduces that old hash exactly.
The single manifest golden was explicitly promoted after this byte-level review:
its differences are only the historical removal of unused mipmap fields and the
new scaling metadata. No other model golden entry changed. Animation golden
files are unchanged.
