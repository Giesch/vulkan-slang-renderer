# phase 2 — stub generator

Sub-plan of [`roc_platform_release.md`](../roc_platform_release.md) §2.
That section holds the rationale: why stubs, why no symbol versions, why the
glibc ≥ 2.34 floor, the measured symbol set, the copy-relocation hazard.
This document is the implementation spec.

> **Implemented 2026-08-17. Read this banner before acting on anything below.**
>
> The phase shipped, but it did not ship what this document specifies. Two
> classes of divergence:
>
> 1. **The floor moved to Ubuntu 24.04 / glibc 2.39, and there is no
>    container.** Every part of this spec that builds artifacts inside a
>    digest-pinned `ubuntu:22.04` image is unimplemented and should not be
>    built: `ci/floor.Dockerfile`, `stubs/generate_in_container.sh`, the SDL3
>    apt dependency list, and `stubs/above_floor.txt`. `stubs/generate.sh`
>    runs on the dev machine and asserts its glibc matches
>    `REQUIRED_GLIBC=2.39`. That assertion replaces the container's
>    by-construction guarantee. The reach this trades away, and the three
>    routes back to a lower floor, are recorded in
>    [`../tech_debt.md`](../tech_debt.md) §18. This document's own §"Floor
>    image", §"Determinism" apt caveat, and the `ubuntu:22.04` references in
>    §"Done criteria" are all moot as written.
> 2. **Five specific claims in the algorithm are wrong**, and four of them
>    fail silently or produce a stub set that does not link. Each is annotated
>    in place below as **Wrong.** Do not implement a step without reading its
>    annotation.
>
> Two hazards this document does not anticipate, both hit during
> implementation, are recorded in §"Found during implementation" at the end.
>
> The parts that were right and are worth keeping: the algorithm's overall
> shape, the `libc_forward.a` design and its four C sources verbatim, the
> emitted assembly shape, the `.size` requirement and the reasoning behind it,
> the `--build-id=none` / `ar rcD` determinism rules, and the
> `stubs/`-outside-`targets/` layout.

Deliverables:

- A generator that measures the host's undefined symbols and emits four stub
  shared objects with correct SONAMEs.
- Committed link inputs in `platform/targets/x64glibc/`: four `.so` stubs,
  `libc_forward.a`, `libstdc++.a`, `Scrt1.o`, `crti.o`, `crtn.o`.
- A `build.sh` with no `gcc -print-file-name` call and no gcc requirement.
- Every committed artifact generated inside the `ubuntu:22.04` floor image.

## Result state

`platform/targets/x64glibc/` after this phase:

| file | status | origin |
| --- | --- | --- |
| `libc.so` | committed | generator, SONAME `libc.so.6` |
| `libm.so` | committed | generator, SONAME `libm.so.6` |
| `libgcc_s.so` | committed | generator, SONAME `libgcc_s.so.1` |
| `libvulkan.so` | committed | generator, SONAME `libvulkan.so.1` |
| `libc_forward.a` | committed | generator, from `stubs/forward/*.c` |
| `libstdc++.a` | committed | floor image's gcc copy |
| `Scrt1.o`, `crti.o`, `crtn.o` | committed | floor image's glibc copies |
| `libhost.a` | gitignored | `build.sh` |

`libc_nonshared.a` leaves the directory and the `inputs` list. No symlinks
remain.

> **This table is what shipped**, with two wording corrections: the origins read
> "local gcc/glibc copies", not "floor image's", and `libc_forward.a` is built
> from `stubs/forward/*.c` compiled in a temp directory. `libc_nonshared.a` does
> not leave on its own — the generator has to prune it, see the note under
> §"File layout".

## Decisions

**The generator is one bash script.** The logic is set arithmetic
(`sort -u`, `comm`), `nm`/`readelf` parsing, text emission, and `gcc`/`ar`
calls. Every other script in `roc-platform/` is bash. The script runs inside
the floor container, so a host-built rust binary cannot run it: the dev
machine's glibc 2.39 exceeds the floor's 2.35.

**The measured `libhost.a` is built inside the container.** An archive
compiled against glibc 2.39 headers can reference symbols the floor's
`libc.so.6` does not export (the `__isoc23_*` redirects), and the generator
would fail on them. The container-built archive is the one the release links,
so it is the one to measure. It is measured, not committed; `build.sh` keeps
building the dev machine's own.

**This phase replaces the committed `libstdc++.a` with the floor image's
copy** and rewrites `built_with_toolchain.txt`. GCC 13's archive can
reference `arc4random`, which is glibc 2.36. The floor's gcc cannot, by
construction (§7 of the parent plan). The generator's fail-on-unassigned
check then enforces the floor forever. This amends the phase 1 artifact.

> **The three decisions above are superseded.** Bash was the right call and
> stands. The container did not survive: the floor became Ubuntu 24.04, so the
> dev machine *is* the floor, the archive is measured where it is built, and the
> `libstdc++.a` replacement is a no-op.
>
> The analysis that drove the container is still correct and still worth
> reading, because it is the argument for going back to 22.04. Thirteen symbols
> in a glibc-2.39-built host archive are absent from glibc 2.35: eight
> `__isoc23_*` redirects plus `strlcpy`, `strlcat`, `wcslcpy`, `wcslcat` (all
> 2.38, from `SDL_string.c.o`, `SDL_cpuinfo.c.o` and `SDL_hidapi.c.o`), and
> `arc4random` (2.36, from gcc 13's `libstdc++.a` `random.o`). All three SDL
> objects are certainly extracted in any link, so the problem is real and not a
> lazy-extraction edge case. No LTS base image sits at 2.38, which makes the
> floor choice effectively binary: 24.04 and none of the thirteen matter, or
> 22.04 and all thirteen need a reviewed allowlist plus a container to build
> in. [`../tech_debt.md`](../tech_debt.md) §18 owns that decision.

**The forwarding archive is named `libc_forward.a`.** `libc_nonshared.a`
names a glibc file this artifact is not. The rename costs one string in
`main.roc`; roc treats inputs as opaque paths.

**The forwarders are C, one file per symbol.** C documents the signatures.
One file per symbol gives one archive member per symbol, so lazy extraction
pulls only what the link references. The sources include no glibc headers:
declared externs avoid `__REDIRECT` renames and conflicts with the
forwarders' own definitions. Compile with `gcc -O2 -fno-stack-protector -c`,
archive with `ar rcD`. All four members ship unconditionally, so the archive
is independent of the measurement.

## File layout

```
roc-platform/
  stubs/
    generate.sh              # runs inside the container; the whole algorithm
    generate_in_container.sh # host wrapper: docker build + docker run
    libc_stub.s              # generated, committed
    libm_stub.s              # generated, committed
    libgcc_s_stub.s          # generated, committed
    libvulkan_stub.s         # generated, committed
    forward/
      atexit.c
      at_quick_exit.c
      pthread_atfork.c
      stack_chk_fail_local.c
  ci/
    floor.Dockerfile         # the ubuntu:22.04 floor image; phase 4 reuses it
```

> **What shipped**, with `generate_in_container.sh` and `ci/floor.Dockerfile`
> dropped:
>
> ```
> roc-platform/
>   stubs/
>     generate.sh              # runs locally; asserts REQUIRED_GLIBC=2.39
>     libc_stub.s              # generated, committed
>     libm_stub.s              # generated, committed
>     libgcc_s_stub.s          # generated, committed
>     libvulkan_stub.s         # generated, committed
>     forward/
>       atexit.c
>       at_quick_exit.c
>       pthread_atfork.c
>       stack_chk_fail_local.c
> ```
>
> The forwarder `.o` files are compiled into a `mktemp -d` work directory, not
> beside their sources. That matters because dropping the
> `platform/targets/*/*.o` line from `.gitignore` makes any stray `.o` in the
> tree newly trackable.
>
> The generator also prunes `targets/x64glibc/` down to the files it owns plus
> `libhost.a`. Without that step the stale `libc_nonshared.a` symlink survives,
> and roc's targets validator would never notice: it checks only that each
> *declared* input exists, so an undeclared leftover is invisible to it and
> would still land in phase 3's bundle glob.

The `.s` sources and `forward/*.c` live outside `targets/`, so phase 3's
`roc bundle platform/targets/x64glibc/*` glob does not ship them (parent
plan, "Constraints from roc").

## Floor image

> **Unimplemented in full. Do not build from this section.** There is no
> Dockerfile, no container, and no `generate_in_container.sh`. It is kept as the
> starting point for [`../tech_debt.md`](../tech_debt.md) §18 route 1, which is
> the route back to a 22.04 floor.
>
> Three things this section leaves unresolved, and they are why the floor moved
> instead:
>
> - **The SDL3 apt package list is not settled**, and the list is load-bearing:
>   it decides which SDL features compile in, which decides the host archive's
>   undefined set, which decides the stub set. "Settle it during
>   implementation" is the least deterministic instruction in the document.
> - **The cargo cache mounts are wrong as written.** `slang-sys/build.rs`
>   extracts its vendored `.tar.xz` into `CARGO_MANIFEST_DIR`, which lives
>   under `~/.cargo/git/checkouts/`, so mounting only
>   `/root/.cargo/registry` is not enough. And `CARGO_TARGET_DIR` must point
>   outside the bind-mounted repo, or the glibc-2.35 build clobbers the host's
>   `target/`.
> - **The container build is the phase's long pole** — SDL3 from vendored C
>   plus a 14 MB slang archive, on every regeneration.
>
> One thing this section gets right and route 1 still needs: gcc 11's
> `libstdc++.a` does satisfy the prebuilt slang archive. The only recent marker
> slang references is `_ZSt28__throw_bad_array_new_lengthv`, which is
> GLIBCXX_3.4.29, i.e. GCC 11. If that ever stops being true, the generator's
> fail-on-unassigned check names the symbol before anything is committed.

`ci/floor.Dockerfile` starts `FROM ubuntu:22.04@sha256:<digest>`. Pin the
digest; record it in `built_with_toolchain.txt`. Installed packages:

- `build-essential` — gcc-11/g++-11 (`libstdc++.a`), binutils (`nm`,
  `readelf`, `objdump`, `ar`), `libc6-dev` (CRT objects,
  `libc_nonshared.a`).
- `libvulkan1` — the real `libvulkan.so.1`, the probe target for
  `vkGetInstanceProcAddr`.
- `curl` + rustup, toolchain per `rust-toolchain.toml`.
- SDL3 build deps: `cmake`, `pkg-config`, and the X11/Wayland dev packages.
  Settle the exact list during implementation from the sdl3-sys CMake
  configure summary. Assert the summary reports the X11 and Wayland video
  drivers enabled.

`libc.so.6`, `libm.so.6` and `libgcc_s.so.1` ship in the base image.

`stubs/generate_in_container.sh` builds the image, then runs:

```bash
docker run --rm -v "$repo_root":/work -w /work/roc-platform \
    -v mltrs-cargo-cache:/root/.cargo/registry \
    <image> bash stubs/generate.sh
```

Outputs land in the mounted repo. Committing them is a manual step.

## Generator algorithm

`stubs/generate.sh`, cwd `roc-platform/`. `set -euo pipefail`; every failure
message names the symbols involved. `LC_ALL=C` throughout.

1. **Assert the environment.** `/etc/os-release` reports 22.04.
   `ldd --version` reports 2.35. `nm`, `readelf`, `objdump`, `ar`, `gcc`
   and `cargo` are on PATH. Every probe library resolves through
   `gcc -print-file-name` to an absolute path. These are the
   `-print-file-name` calls that leave `build.sh`.
2. **Build the measurement input.**
   `cargo build --release --lib --target x86_64-unknown-linux-gnu`.
3. **Copy floor artifacts** into `platform/targets/x64glibc/`:
   `libstdc++.a`, `Scrt1.o`, `crti.o`, `crtn.o`. Use `cp`; the files are
   committed and go into the bundle.
4. **Build `libc_forward.a`** from `stubs/forward/*.c` into
   `platform/targets/x64glibc/`.
5. **Measure.** U = the union of `nm --undefined-only` over three archives:
   `libhost.a`, `libstdc++.a`, `libc_forward.a`. The third archive puts
   `__cxa_atexit`, `__cxa_at_quick_exit`, `__register_atfork` and
   `__stack_chk_fail` into the measured set. Exclude weak-undefined symbols
   (`nm` letters `w`, `v`); they need no provider. D = the defined-symbol
   union of the same three archives. The candidate set S = U ∖ D, via
   `sort -u` and `comm -23`.

   > **Wrong, three ways.**
   >
   > **(a) D must be `nm --defined-only --extern-only`.** Without
   > `--extern-only`, local symbols enter D. `SDL_gpu_vulkan.c.o` defines a
   > *local* bss symbol named `vkGetInstanceProcAddr` — SDL's own function
   > pointer — and it cancels ash's genuine undefined reference to the global of
   > the same name. The `libvulkan.so` stub then comes out **empty**, and the
   > generator still reports zero unassigned symbols. That is the one symbol the
   > Vulkan stub exists for, so the bug disables exactly the check that would
   > have caught it.
   >
   > **(b) Three archives is not enough. The CRT objects are link inputs too.**
   > `Scrt1.o` references `__libc_start_main`, and nothing else in the link
   > does. Measuring only the archives produces a stub set that fails the first
   > link with `undefined symbol: __libc_start_main`. Measure every input except
   > `app`: `Scrt1.o`, `crti.o`, `crtn.o`, `libhost.a`, `libstdc++.a`,
   > `libc_forward.a`. The three objects add exactly that one symbol.
   >
   > **(c) Filter to strong `U`; do not subtract a weak set.** A symbol that is
   > weak-undefined in one object and strong-undefined in another still needs a
   > provider. `pthread_mutex_lock`, `pthread_mutex_unlock` and `pthread_once`
   > are each in that state. Taking `nm` letter `U` only drops the weak cases
   > for free and keeps the strong ones.

6. **Subtract linker-defined symbols.** LLD synthesizes a fixed set for an
   executable link: `__dso_handle`, `__ehdr_start`, `_end`, `_edata`,
   `__bss_start`, `_DYNAMIC`, `_GLOBAL_OFFSET_TABLE_`, the
   `__init_array_start`/`__init_array_end` and
   `__fini_array_start`/`__fini_array_end` pairs. Keep the list as a
   maintained allowlist in the script. Without it, `__dso_handle` alone
   fails the unassigned check.

   > **Incomplete.** The LLD set is right, but it is not the only source of
   > symbols another input defines. The `app` object supplies whatever the
   > platform header's `provides` block names — `roc_init` today — and that
   > symbol otherwise fails the unassigned check in step 8. Parse the names out
   > of `platform/main.roc` rather than hardcoding them, so the allowlist
   > tracks the header. In practice only three of these ever appear:
   > `_GLOBAL_OFFSET_TABLE_`, `__dso_handle` and `roc_init`.

7. **Route nonshared symbols.** For each s ∈ S that the real
   `libc_nonshared.a` defines and `objdump -T libc.so.6` does not export:
   s must be one of `atexit`, `at_quick_exit`, `pthread_atfork`,
   `__stack_chk_fail_local`. Fail otherwise — a nonshared symbol with no
   forwarder. For each of the four that S references, assert its forwarding
   target is a dynamic export of the floor `libc.so.6`. Warn when the
   target's only version is `GLIBC_PRIVATE` (`__register_atfork` is; an
   unversioned reference binds to the default version, but `GLIBC_PRIVATE`
   carries no stability promise). Remove the routed symbols from S;
   `libc_forward.a` provides them.

   > **Wrong: route from U, not from S.** Step 5 puts `libc_forward.a` in the
   > measured set, so the archive's own definitions land in D and the four
   > forwarded symbols never reach S. Routing from S therefore selects nothing,
   > the named-forwarder assertion is dead code, and the report shows
   > `libc_forward.a  0` while `atexit` is silently handled.
   >
   > The safety property still holds by accident — a nonshared symbol with no
   > forwarder lands in S, no provider defines it, and step 8 fails — but the
   > message degrades from "nonshared symbol with no forwarder, here it is" to a
   > generic "no provider defines these symbols". Routing from U keeps the
   > specific diagnostic. S needs no change either way.

8. **Assign providers.** Probe with `nm -D --defined-only`, in priority
   order `libvulkan.so.1`, `libgcc_s.so.1`, `libm.so.6`, `libc.so.6`. Any
   leftover symbol fails the run, with the list printed. That is the signal
   a new dependency entered the host.

   > **Wrong, two ways.**
   >
   > **(a) `nm -D` prints versioned names, so the probe must strip `@…`.**
   > Output rows read `memcpy@@GLIBC_2.14`, never bare `memcpy`. Comparing
   > unstripped names against the symbol set matches **nothing**: every symbol
   > falls through all four providers and the run fails with the entire stub set
   > listed as unassigned. This is the first thing to get wrong and the easiest
   > to misdiagnose, because the failure looks like a missing dependency.
   >
   > **(b) The provider list is missing the dynamic linker.**
   > `__tls_get_addr` is exported by `ld-linux-x86-64.so.2` alone; `libc.so.6`
   > does not export it at any version. Probe ld.so last and emit its symbols
   > into the libc stub. They resolve at run time because glibc always keeps
   > ld.so in the global search scope, and `LD_BIND_NOW=1` on the built
   > executable proves it.
9. **Classify.** Read each symbol's TYPE from the provider's
   `readelf -sW --dyn-syms`:
   - `FUNC` or `IFUNC` → function stub. glibc exports `memcpy` and friends
     as `IFUNC`; `nm` letters cannot classify those, so `readelf` TYPE
     replaces the parent plan's `nm`-letter filter for classification.
   - `OBJECT` → data stub, size from the same `readelf` row. The `nm -D`
     letter selects the section: `B` → `.bss`, `D`/`G` → `.data`,
     `R` → `.rodata`. Warn when the size is not 8; those sizes can drift
     between glibc versions.
   - `TLS`, `NOTYPE`, anything else → fail. A TLS symbol cannot be stubbed
     as a function or an object.

   > **Wrong, two ways.**
   >
   > **(a) The `nm -D` letter cannot select the section.** `environ` is a
   > *weak* object, letter `V`, which the `B`/`D`/`G`/`R` set does not cover, so
   > the generator fails on it. Read the section from the same `readelf` row's
   > Ndx column instead, mapped through `readelf -SW` to a section name, then
   > onto `.bss`, `.data` or `.rodata`. Measured on glibc 2.39: `environ` and
   > `__libc_single_threaded` are `.bss`; `stdin`, `stdout` and `stderr` are
   > `.data`.
   >
   > **(b) Read the default-version (`@@`) row only.** A symbol can appear twice
   > in `.dynsym` with different TYPE *and* different size. `memcpy` is `FUNC`
   > size 44 at the compat `GLIBC_2.2.5` and `IFUNC` size 273 at the default
   > `GLIBC_2.14`. Ten of the 419 libc symbols are in that state. Taking the
   > first row misclassifies them, and for an `OBJECT` it would size the copy
   > relocation from the wrong row — the exact silent failure the `.size`
   > requirement exists to prevent. Prefer the `@@` row, fall back to an
   > unversioned row, and the counts reconcile exactly.
   >
   > The size-not-8 warning fires legitimately: `__libc_single_threaded` is a
   > 1-byte `char`. Word it as informational, not as a problem.

10. **Emit** the four `.s` files under `stubs/`, symbols sorted. Each file
    opens with a provenance header: glibc version, gcc version, image
    digest, `generated by stubs/generate.sh`. No timestamps. Per symbol:

    ```asm
    # function
    .text
    .balign 8
    .globl <sym>
    .type <sym>, @function
    <sym>: ret

    # object
    .section <.bss|.data|.rodata>
    .balign 8
    .globl <sym>
    .type <sym>, @object
    .size <sym>, <N>
    <sym>: .skip <N>
    ```

    No `.symver` anywhere. The `.size` directive is the point: a data stub
    with `st_size` 0 gives the loader a copy-relocation slot too small for
    the real object (parent plan, "Data symbols in the libc stub").
11. **Assemble.** Per stub:

    ```bash
    gcc -nostdlib -shared -Wl,-soname,<SONAME> -Wl,--build-id=none \
        -o platform/targets/x64glibc/<name>.so stubs/<name>_stub.s
    ```

    SONAMEs: `libc.so.6`, `libm.so.6`, `libgcc_s.so.1`, `libvulkan.so.1`.
    roc's linker passes no `-soname`, so the SONAME embedded here is what
    the executable records as `DT_NEEDED`.
12. **Self-verify.** Per stub, `readelf -d` shows the exact SONAME, zero
    `NEEDED` entries, and no `VERDEF`/`VERNEED` section. The `libvulkan.so`
    stub exports exactly `vkGetInstanceProcAddr`. `nm libc_forward.a` shows
    the four forwarder definitions.
13. **Record.** Rewrite `built_with_toolchain.txt`:

    ```
    image: ubuntu:22.04@sha256:<digest>
    glibc: <dpkg-query -W -f '${Version}' libc6>
    gcc: <dpkg-query -W -f '${Version}' gcc-11>
    binutils: <dpkg-query -W -f '${Version}' binutils>
    rustc: <rustc --version>
    libstdc++.a: <gcc -print-file-name=libstdc++.a>
    generated-by: stubs/generate.sh
    ```

    Print the summary table: undefined count, defined count, stub-set
    count, per-provider split — the shape of the parent plan's "Measured
    symbol set" table.

## Forwarding archive

`stubs/forward/`, one symbol per file. Signatures:

```c
/* atexit.c */
extern int __cxa_atexit(void (*func)(void *), void *arg, void *dso_handle);
extern void *__dso_handle;
int atexit(void (*func)(void)) {
    return __cxa_atexit((void (*)(void *))func, 0, __dso_handle);
}

/* at_quick_exit.c */
extern int __cxa_at_quick_exit(void (*func)(void *), void *dso_handle);
extern void *__dso_handle;
int at_quick_exit(void (*func)(void)) {
    return __cxa_at_quick_exit((void (*)(void *))func, __dso_handle);
}

/* pthread_atfork.c */
extern int __register_atfork(void (*prepare)(void), void (*parent)(void),
                             void (*child)(void), void *dso_handle);
extern void *__dso_handle;
int pthread_atfork(void (*prepare)(void), void (*parent)(void),
                   void (*child)(void)) {
    return __register_atfork(prepare, parent, child, __dso_handle);
}

/* stack_chk_fail_local.c */
extern void __stack_chk_fail(void) __attribute__((noreturn));
void __stack_chk_fail_local(void) __attribute__((noreturn));
void __stack_chk_fail_local(void) { __stack_chk_fail(); }
```

The function-pointer cast in `atexit` matches glibc's own
`stdlib/atexit.c`. `__dso_handle` resolves at link time: the link has no
`crtbegin.o`, and LLD synthesizes `__dso_handle` for executables. The
allowlist in step 6 depends on the same fact.

## Determinism

Phase 4's CI regenerates the committed outputs and fails on diff, so two
runs must produce identical bytes.

- `LC_ALL=C` and sorted symbol order everywhere.
- No timestamps in the `.s` headers.
- `-Wl,--build-id=none` on every stub link.
- `ar rcD` for archives: zeroed timestamps, uid, gid.
- The base image is digest-pinned.

One caveat stands: `apt-get` inside the pinned image installs the current
point release of each package, so an Ubuntu security update can change
`Scrt1.o` or `libstdc++.a` bytes between regenerations. `built_with_toolchain.txt`
pins the package versions, and a phase 4 regen-diff failure after a package
bump is a review-and-recommit signal, not a bug. `snapshot.ubuntu.com`
would close the gap; it is more machinery than the problem warrants.

> **The first four rules held. The fifth and the caveat are moot** — there is no
> image. The same drift arrives by another door: a local gcc or libc6-dev
> update changes `libstdc++.a` or `Scrt1.o`, and the response is the same, a
> review-and-recommit rather than a bug.
>
> Two rules worth adding, both learned the hard way:
>
> - **Keep package version strings out of the `.s` headers.** The emitted
>   headers carry the floor's `2.35`-style version and a pointer to
>   `built_with_toolchain.txt`, nothing finer. A point-release bump then touches
>   one file rather than five.
> - **Verify determinism from an emptied `targets/`, not just by re-running.**
>   Regenerating over existing output and bootstrapping from nothing are
>   different code paths; only the second is what a fresh clone hits. Both were
>   checked, and both reproduce identical bytes.

## Edits to existing files

- `platform/main.roc:17` — replace `"libc_nonshared.a"` with
  `"libc_forward.a"`, same position: after `"libc.so"`, before
  `"libgcc_s.so"`.
- `build.sh` — shrinks to: the Linux/x86_64 host check, an existence check
  over the committed link inputs (a missing file names it and points at
  `just stubs`), `cargo build --release --lib --target
  x86_64-unknown-linux-gnu`, `cp` of `libhost.a`. Delete the gcc check,
  `find_system_file`, `link_system_file`, `copy_system_file`, and every
  symlink call. `build.sh` needs no compiler; roc's own link works without
  one (parent plan, "Dynamic linker discovery").
- `.gitignore` — delete lines 8–15: the two comment lines and the six
  patterns `platform/targets/*/*.o`, `.../libc.so`,
  `.../libc_nonshared.a`, `.../libgcc_s.so`, `.../libm.so`,
  `.../libvulkan.so`. Keep `platform/targets/*/libhost.a`. Nothing left in
  `targets/` needs a `!` re-include.
- `justfile` — add:

  ```
  # regenerate the committed link inputs in the ubuntu:22.04 floor image (needs docker)
  stubs:
      bash stubs/generate_in_container.sh
  ```
- `built_with_toolchain.txt` — rewritten by the generator (step 13).
- `README.md` — Requirements: drop the C++ runtime and gcc; the Vulkan
  loader becomes a run-time requirement; add docker, for `just stubs` only.
  Targets: replace the symlink description with the committed-artifact
  story; delete the "cannot be bundled" sentence. Layout: add `stubs/`.

> **This list is accurate, with four corrections.**
>
> - The recipe is reached as `just roc-platform stubs`, not `just stubs` —
>   `roc-platform` is a `just` module off the root justfile. Its body is
>   `bash stubs/generate.sh`, and it needs gcc and binutils rather than docker.
> - `build.sh` still needs gcc *removed from its own requirements* but the
>   committed-inputs list has to be spelled out in it, one entry per name the
>   `inputs` list carries. Those two lists must be edited together.
> - The `README.md` Requirements section wants splitting in two: what builds
>   the platform, and what runs what it builds. The glibc floor belongs in the
>   second half, with a pointer to [`../tech_debt.md`](../tech_debt.md) §18.
> - Two unrelated staleness bugs sit in the same `README.md` sections and are
>   cheapest to fix in the same pass: the Platform API example still shows
>   `app [init!]` returning a bare record rather than `app [game]` with
>   `Game.Init`, and the Layout list omits `platform/Game.roc`.

## Found during implementation

Two hazards this document does not anticipate. Both cost real debugging time,
and the second is the kind that passes review and then fails a CI run.

**The pre-stub symlinks alias their own source, and one write path follows
them.** Before this phase, `build.sh` filled `targets/x64glibc/` with symlinks
into `/usr/lib/gcc/...`. The generator overwrites those paths, and on the first
run they still exist:

- `cp` refuses with `'/usr/lib/.../Scrt1.o' and 'targets/.../Scrt1.o' are the
  same file`. Loud, harmless, and the obvious fix is
  `cp --remove-destination`.
- `gcc -shared -o targets/x64glibc/libvulkan.so` is the dangerous one. It
  **follows the symlink and overwrites `/usr/lib/x86_64-linux-gnu/libvulkan.so.1`**
  with a one-symbol stub, silently, breaking every Vulkan program on the
  machine. `rm -f` the target before every stub link.

Both writes need the destination removed first. Neither is obvious from reading
this document, because it describes the end state and not the migration into
it.

**`set -o pipefail` plus an early-exiting pipe consumer makes the generator
fail intermittently.** `head -1` and `grep -q` both close the pipe as soon as
they have what they need. The producer then takes SIGPIPE, and `pipefail`
promotes that to a pipeline failure, which `set -e` turns into a silent exit
141 with no output at all. Whether it fires depends on whether the producer's
output fits the 64 KB pipe buffer, so the same script passes six times and
fails the seventh.

Two idioms avoid it:

- `sed -n 1p` instead of `head -1`. It reads its input to EOF.
- For `grep -q`, write the producer's output to a file in the work directory
  once and grep the file. `nm -D --defined-only libc.so.6 | grep -q …` is the
  worst case at ~2900 symbols; `readelf -d` on a 14 KB stub is small enough to
  hide the bug indefinitely.

A generator that shells out this much should be run five times in a row before
it is believed.

## Amendments to the parent plan

This spec changes six details of §2.

- Phase 2 replaces phase 1's `libstdc++.a` with a floor-image copy. The
  parent's phase list omits it; §7's by-construction argument requires it.
- The measurement union covers three archives, not two: `libc_forward.a`
  joins `libhost.a` and `libstdc++.a`.
- Classification uses the `readelf --dyn-syms` TYPE column. `nm` letters
  select only the data-stub section. `nm` has no letter for `IFUNC`, and
  glibc exports `IFUNC` symbols.
- Fail-on-unassigned needs two exclusions the parent does not name:
  weak-undefined symbols and LLD's linker-synthesized set.
- Reproducibility needs the base image pinned by digest, with the
  apt-point-release caveat recorded.
- `stdin`, `stdout` and `stderr` export as `FILE *` — 8-byte objects, not
  `FILE` structs. The copy-relocation mechanism and the `.size` requirement
  stand unchanged.

> **Two of these six did not survive implementation.**
>
> - The `libstdc++.a` replacement is moot. With the floor at the dev machine's
>   own gcc 13, the committed archive from phase 1 is already the right file,
>   and the generator's `cp` shows no diff. `arc4random` — the symbol that
>   motivated the replacement — is simply a normal `libc.so.6` export at glibc
>   2.39.
> - The digest-pinned image and its apt-point-release caveat are moot. The
>   floor is now an assertion inside `stubs/generate.sh`, not an image. The
>   determinism it needed came instead from `--build-id=none`, `ar rcD`,
>   `LC_ALL=C` and sorted symbol order, all of which held: a run from an
>   emptied `targets/` reproduced byte-identical output.
>
> The measurement union is right in spirit and wrong in count — six inputs, not
> three. See the annotation on step 5.

## Measured result

What the generator actually reported on first correct run, glibc 2.39 / gcc
13.3. Numbers to compare against, not to trust blindly — they move with the
dependency tree.

| provider | count | breakdown |
| --- | --- | --- |
| `libc.so.6` | 419 | 414 func, 5 object |
| `libm.so.6` | 39 | 26 FUNC, 13 IFUNC |
| `libgcc_s.so.1` | 15 | all `_Unwind_*` |
| `libvulkan.so.1` | 1 | `vkGetInstanceProcAddr` |
| `ld-linux-x86-64.so.2` | 1 | `__tls_get_addr` |
| `libc_forward.a` | 1 | `atexit` |

Stub set 475, from 16,542 undefined and 54,326 defined-extern. Three symbols
allowlisted rather than stubbed: `_GLOBAL_OFFSET_TABLE_`, `__dso_handle`,
`roc_init`. Zero unassigned.

The parent plan's §"Measured symbol set" predicted 400 after static libstdc++.
475 is the real figure at this floor, and the gap is mostly the 13
glibc-2.36-and-later symbols a 2.35 floor would have had to exclude.

The four stubs are small: 44 KB, 14 KB, 14 KB and 13 KB, against `libhost.a`'s
155 MB.

## Done criteria

The phase 2 slice of the parent Verification list, run on the dev machine
unless marked.

> **Two entries read differently after implementation.**
>
> - `ubuntu:22.04` is `ubuntu:24.04` throughout, and the check passed with only
>   `libvulkan1` installed. `ldd -r` is the shorter spelling: it reports
>   unresolved symbols directly, so no `nm` is needed in the container.
> - `ldd ./basic_triangle` lists two extra entries, `libdl.so.2` and
>   `libpthread.so.0`, if a Vulkan SDK is on `LD_LIBRARY_PATH` — they are that
>   loader's own `DT_NEEDED`, not the executable's. Run
>   `env -u LD_LIBRARY_PATH ldd` to check this criterion, or read `readelf -d`
>   for `DT_NEEDED` instead, which is what the criterion means.
>
> Also worth knowing: `readelf -r` reported **zero** `R_X86_64_COPY`
> relocations. Nothing in the link takes the address of `stdout` or `environ`
> as data, so the copy-relocation hazard is currently latent. The correct
> `.size` on the five data stubs is insurance for when it stops being latent,
> not something the build exercises today. The same is true of the C++
> exception check — no host path throws.

- `just roc-platform build && just roc-platform test` passes headless with
  lavapipe.
- The generator reports zero unassigned symbols.
- `ldd ./basic_triangle` lists exactly `libvulkan.so.1`, `libgcc_s.so.1`,
  `libm.so.6`, `libc.so.6`, `linux-vdso` and `ld-linux`. No
  `libstdc++.so.6`.
- `readelf --version-info ./basic_triangle` shows no versioned glibc
  requirement.
- `LD_BIND_NOW=1 ./basic_triangle` runs.
- `readelf -r ./basic_triangle | grep R_X86_64_COPY` lists libc data
  symbols only, no `_Z`-prefixed name, and each entry's size matches the
  real system library.
- `nm -D ./basic_triangle` exports no `_Z`-prefixed symbol.
- Inside an `ubuntu:22.04` container, every symbol from
  `nm -D --undefined-only ./basic_triangle` resolves against the
  container's libraries. Copy the executable in; the bundle test is
  phase 3.
- `./basic_triangle` writes to stdout and survives a thrown-and-caught C++
  exception. If no host code path throws, note that and defer the explicit
  throw test to phase 3's container run.
- `git status` in `platform/targets/x64glibc/` is clean after `build.sh`,
  except for `libhost.a`.

Phase-2-specific:

- Per stub: `readelf -d` shows the exact SONAME, zero `DT_NEEDED`, and no
  version sections.
- `libvulkan.so` exports exactly `vkGetInstanceProcAddr`.
- `libc_forward.a` defines the four forwarders, and each referenced
  forwarder's target is a dynamic export of the floor `libc.so.6`.
- Two consecutive `just stubs` runs leave `git status` clean after the
  second.
- `built_with_toolchain.txt` records the floor's glibc 2.35 and the image
  digest, not the dev machine's 2.39.
- `find platform/targets/x64glibc -type l` prints nothing.

> Every criterion above passed, with these readings: `built_with_toolchain.txt`
> records glibc 2.39 and no digest, the container is `ubuntu:24.04`, and the
> recipe is `just roc-platform stubs`. One criterion needs a stronger
> replacement: **add a check that the generator refuses to run on a mismatched
> glibc.** With no container, that assertion is the only thing holding the floor,
> so it needs a test of its own.

## Out of scope

- The `PT_INTERP` multiarch-path decision and the bundle-from-URL test —
  phase 3.
- The CI regen-and-diff job — phase 4, reusing `ci/floor.Dockerfile` and
  `stubs/generate.sh`.

> **`PT_INTERP` is confirmed broken, and phase 3 should start from the
> measurement rather than re-deriving it.** The built executable records
> `/lib/x86_64-linux-gnu/ld-linux-x86-64.so.2` — the Debian multiarch path,
> which does not exist on Fedora or Arch. This is the parent plan's §2 "Dynamic
> linker discovery" hazard, and it lands exactly as predicted: roc's
> `libc_finder.zig` probes the libc directory before `/lib64`, and on a
> Debian-family machine the libc directory wins. It is invisible to the
> `ubuntu:24.04` container check, because Ubuntu has that path too.
>
> Phase 4 reuses `stubs/generate.sh` on an `ubuntu-24.04` runner. There is no
> `ci/floor.Dockerfile` to reuse.
