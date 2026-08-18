# roc `PT_INTERP` fix — plan

> **Done 2026-08-18.** Merged upstream as roc-lang/roc PR #10838
> (issue #10835, merge `a4a3c344`). The landed change is the
> `RocTarget` variant this plan prefers, not the minimal per-architecture
> constants: `src/target/mod.zig` gains `glibcProgramInterpreter`, the
> native `.gnu` branch of `linker.zig` emits it, and
> `src/cli/libc_finder.zig` is deleted. `local-install` is rebased onto
> the merge and the installed roc (`release-fast-62a50c46`) carries it.
> Every done criterion is met: `readelf -l` on a `basic_triangle` linked
> on this Debian-family machine names `/lib64/ld-linux-x86-64.so.2`, the
> executable runs to a clean exit 0 in a `fedora:latest` container with
> lavapipe, roc's test suite passed on the PR, and the `readelf -l` item
> in [`roc_platform_release.md`](roc_platform_release.md) §Verification
> is reinstated — `ci/bundle_test.sh` now asserts the path.

Make roc write the ABI-constant dynamic-linker path into every native glibc
executable. The work happens in the `../roc` checkout and lands as an
upstream PR to `roc-lang/roc`. Line references are against commit
`9a1cdae6`.

[`roc_platform_release/03_bundle.md`](roc_platform_release/03_bundle.md)
depends on this plan: until the fix lands, executables linked on
Debian-family machines run on Debian-family machines only.

## The defect

Every dynamically linked ELF executable carries a `PT_INTERP` header. The
header holds one absolute path: the dynamic linker. The kernel loads that
file at `exec`. A missing path fails with `ENOENT`, reported against the
executable, before any library loads.

The x86-64 glibc ABI defines one canonical path:
`/lib64/ld-linux-x86-64.so.2`. Every glibc distro provides it. Fedora and
Arch store the real file there. Debian-family distros store the real file
at the multiarch path `/usr/lib/x86_64-linux-gnu/ld-linux-x86-64.so.2` and
provide `/lib64/ld-linux-x86-64.so.2` as a `libc6` compatibility symlink.
The multiarch path exists only on Debian-family distros. GCC hardcodes the
canonical path in its target definition and searches for nothing.

roc discovers a path per link machine instead:

- `src/cli/libc_finder.zig` — `findViaCompiler` runs
  `gcc -print-file-name=ld-linux-x86-64.so.2` and discards the output. A
  TODO above the call (`libc_finder.zig:91-96`) says the intent was lost in
  a refactor. The code keeps only the directory of
  `gcc -print-file-name=libc.so`.
- `findDynamicLinker` (`libc_finder.zig:178-224`) probes that directory
  first. On a Debian-family machine the multiarch copy is there, so the
  probe stops. `/lib64` is the second probe and is never reached.
- `linker.zig:625-646` passes the result to LLD as `-dynamic-linker`. The
  hardcoded `/lib64/ld-linux-x86-64.so.2` fallback fires only when
  discovery fails completely.

The discarded compiler output is not the root cause. The compiler's answer
on Ubuntu is also the multiarch path. The discovery approach is the root
cause: the path is an ABI constant, so there is nothing to discover.

The consequence: an executable linked with `roc build` on a Debian-family
machine records the multiarch path and fails with `ENOENT` on Fedora, Arch
and SteamOS. The failure is one-way. An executable linked on Fedora or Arch
records the canonical path and runs everywhere.

## The fix

Replace discovery with the constant.

- `src/cli/linker.zig:625-646`, the native `.gnu` branch: delete the
  `findLibc` call and the fallback branch. Always pass the per-architecture
  constant that the fallback already contains
  (`/lib64/ld-linux-x86-64.so.2` for x86-64, `/lib/ld-linux-aarch64.so.1`
  for aarch64).
- `RocTarget.getDynamicLinkerPath` (`src/target/mod.zig:876`) already maps
  every target to the canonical path and has zero callers. Prefer it as the
  source of truth if a `RocTarget` is reachable at the call site;
  `LinkConfig` carries `target_arch` but no `RocTarget`, so the
  per-architecture constants are the minimal change.
- `libc_finder.zig` has exactly one caller (`linker.zig:629`). Delete the
  file with the call.

Two paths stay untouched:

- Cross-compilation. Callers pass `-dynamic-linker` through `extra_args`,
  and the changed branch runs only when `extra_args.len == 0`.
- musl targets. They link `-static` and have no `PT_INTERP`.

One edge case changes for the better. A musl host (Alpine) that links a gnu
target natively discovers musl's `ld-musl-x86_64.so.1` today, which is
wrong for a glibc-target binary. The constant is correct for the target
ABI.

## Steps

1. Branch off `local-install` in `../roc`.
2. Make the change. Run roc's test suite.
3. Link an example against `roc-platform` and check
   `readelf -l` shows `PT_INTERP` = `/lib64/ld-linux-x86-64.so.2`.
4. Open the PR to `roc-lang/roc`.
5. Rebase `local-install` onto the fix. Reinstall roc. Run
   `just roc-platform record-roc-version`.

Step 5 removes the local limitation ahead of the upstream merge: the
installed roc is built from this checkout (`built_with_roc_version.txt`
matches its HEAD), so the fix takes effect on this machine as soon as it is
rebased in.

## Done criteria

- `readelf -l` on a roc-linked executable, built on this (Debian-family)
  machine, names `/lib64/ld-linux-x86-64.so.2`.
- The executable runs in a Fedora or Arch container.
- roc's test suite passes.
- `src/cli/libc_finder.zig` is deleted.
- The waived `PT_INTERP` item in
  [`roc_platform_release.md`](roc_platform_release.md) §Verification is
  reinstated.
