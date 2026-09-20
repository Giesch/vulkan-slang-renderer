# toon-link-bck execution record

Written 2026-09-19 during execution of the planning artifact
`plan-001.md` (session 0b6vjt-angel; execution session 0bdkzy-flaky). That
artifact is outside this repository and is kept unedited as the historical
record. Where its text conflicts with the amendments below, the amendments
govern and the shipped code follows them. Verify against the code before
acting on anything here.

## Operator-approved amendments

1. **GPU oracle infrastructure (AC10).** The plan's "stop for scope review"
   gate fired: the renderer had no numerical readback surface, only the
   single-pixel picking result. Operator approved a codegen-based readback
   API with Slang structs as the source of truth (`GPURead` generated per
   pure-data reflected struct; `Renderer::dispatch_readback`), initially for
   these tests only, possibly picking later. Implemented, independently
   reviewed, and verified under lavapipe. See `docs/testing.md`
   "Typed GPU readback".

2. **Animated scale — supersedes AC5's "unit/default/constant-1 scale only …
   reject all keyed scale" and the matching Compatibility text.** The
   real-asset audit rejected 158/594 clips for scale (77 with non-unit
   constant scale, 116 with varying keyed scale, 35 overlap), so the unit-scale
   boundary was wrong for the actual catalog. Operator approved: export
   explicit `Skeleton.scaling_rule: Option<ScalingRule>` and
   `SkeletonJoint.scale_compensate: Option<bool>` from the already-parsed
   INF1/JNT1 (absent ≠ false; `cl.bdl` is Maya with 12 of 42 joints
   compensating); runtime requires `Some(Maya)` and every flag `Some`,
   otherwise static rendering with a regeneration diagnostic; accept finite
   default/constant/keyed animation scale. Maya compensation order verified
   against tww `J3DJoint.cpp` (`J3DMtxCalcMaya::calcTransform`, revision
   e389a9b) is `local = T · inverse(parent LOCAL scale) · R · S` — the
   compensation scales the ROWS of the 3×3, translation untouched. The earlier
   research draft had the order wrong (post-rotation inverse scale); the
   rotated nonuniform parent/child test pins the correct one. 594/594 coverage
   remained the gate and now passes. See `docs/link_model_metadata.md`.

3. **Command ordering — earlier resolution rejected by final review.** The
   initial integration consumed buffered commands in the next update, after
   advancing the clock. Two earlier reviewers accepted this, but the Astra
   re-review correctly identified that the real app order is update → UI →
   draw: this left the very first draw after a UI command on the old pose.
   This was an implementation defect, not an operator-approved amendment.
   The required correction is to advance in update, consume UI commands once
   in the same frame's pre-draw seam, then evaluate/publish the requested pose.
   That correction is implemented and passed an independent Astra re-review.
   The same review found that accepted deferred commands cleared errors before
   their pose evaluation succeeded. Clearing now waits for successful publication
   and cannot erase a later command's failure diagnostic. Regression tests drive
   actual UI clicks and slider input between update and the first preparation,
   plus repeated unsafe requests and newer-error ordering.

## Resolved review questions

- Rotation sampling truncates toward zero *before* the decimal shift. This is
  not open: the plan pins it from the verified J3D s16 path, and the
  197.5 → 197 → 1576 oracle is the plan's own.
- The runtime audit asserts the 594 total and the 77/116/35 scale fingerprint
  on purpose as a tripwire coupled to the frozen converter goldens; documented
  in `docs/link_animations.md`.

## Final automated verification after lifecycle fixes

The final chained run completed with exit 0:

- `cargo check --workspace --all-targets`
- `cargo fmt --check`
- `just lint` (debug and release)
- `just test` (Toon Link: 84 passed, 0 failed, 2 explicitly ignored)
- Explicit runtime catalog audit: 594 accepted, 0 rejected; all five static
  clips accepted; scale fingerprint 77/116/35.
- `just toon_link test-skinning-gpu`: 11 production-shader cases passed,
  zero validation messages including teardown.
- Full `just sweep`: 17 passed, 0 skipped, 0 failed.

Final verification log:
`/home/danknutson/.local/share/polytoken/sessions-v1/0bdkzy-flaky/shell/call_qSAUcism03cMT11U1ccXIneM.stdout`.
The earlier Fable delta re-review failed because the provider model was not
available; it was not counted as a pass. Subsequent Astra review found the
lifecycle defects above, and a fresh independent Astra review passed their fixes.
The full-story acceptance remains incomplete until the visual cases below are
observed; neither the structural audit nor the compute oracle substitutes for
both rendering modes' call-site verification.

## Not performed by execution

The six interactive visual cases (`visual-bind-both`,
`visual-animated-fractional-both`, `visual-paused-mode-switch`,
`visual-static-five`, `visual-return-bind`,
`visual-facial-settings-regression`) are an operator step under
`just dev toon_link`; `docs/toon_link.md` lists them. The unchanged sweep is
GameCube bind-pose evidence only.
