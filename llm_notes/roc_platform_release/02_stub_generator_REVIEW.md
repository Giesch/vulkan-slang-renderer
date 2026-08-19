# phase 2 — review of the implementation

Review of the uncommitted phase 2 changes against
[`02_stub_generator.md`](02_stub_generator.md), performed 2026-08-17, before
commit. Line references point at `roc-platform/stubs/generate.sh` as reviewed.

**Verdict: the implementation matches the annotated spec. The review found no
correctness bug.** It found one wrong comment, two robustness gaps, one
untracked follow-up, and one README wording nit.

## Verified

Every "Wrong" annotation in the spec has a matching fix in `generate.sh`:

- 5(a): D uses `nm --defined-only --extern-only` (`generate.sh:190`).
- 5(b): the measured set is six inputs, including the CRT objects
  (`generate.sh:171`).
- 5(c): U keeps strong `U` rows only (`generate.sh:186`).
- 6: the allowlist adds the `provides` names parsed from `platform/main.roc`
  (`generate.sh:217`).
- 7: routing reads from U, not S (`generate.sh:236`).
- 8(a): the provider probe strips `@…` version suffixes (`generate.sh:225`).
- 8(b): ld.so is the last provider, and its symbols fold into the libc stub
  (`generate.sh:85`, `generate.sh:359`).
- 9(a): the data-stub section comes from the `readelf` Ndx column mapped
  through the section table (`generate.sh:309`, `generate.sh:332`).
- 9(b): classification keeps `@@` rows, falls back to unversioned rows, and
  skips single-`@` compat rows (`generate.sh:301`).

Both hazards from the spec's §"Found during implementation" are handled:

- `cp --remove-destination` for the toolchain copies (`generate.sh:126`) and
  `rm -f` before every stub link (`generate.sh:405`).
- `sed -n 1p` instead of `head -1` (`generate.sh:47`) and grep-on-file
  instead of `grep -q` on a large pipe (`generate.sh:259`).

Checks run on the committed artifacts:

- `readelf -d` per stub: exact SONAME, zero `DT_NEEDED`. `readelf -SW` per
  stub: no `.gnu.version_d` or `.gnu.version_r` section.
- Dynamic-export counts match the committed `.s` sources: 420 (`libc.so`),
  39 (`libm.so`), 15 (`libgcc_s.so`), 1 (`libvulkan.so`). 420 is the spec's
  419 libc symbols plus the folded `__tls_get_addr`.
- `libvulkan.so` exports exactly `vkGetInstanceProcAddr`.
- `build.sh`'s `COMMITTED_INPUTS` lists the same 9 names as `main.roc`'s
  `inputs`, minus `libhost.a` and `app`.
- The `.gitignore` deletion, the `just roc-platform stubs` recipe, and the
  README edits match the spec's amended §"Edits to existing files",
  including the two README staleness fixes (`app [game]` example,
  `platform/Game.roc` in the layout list).

## Findings

Most important first.

### 1. The provider-order comment states the opposite of the behavior

`generate.sh:82` reads "Earlier providers win, so libm never claims a symbol
libc and libm both export." libm precedes libc in `PROVIDERS`, so libm *does*
claim the dual-exported symbols. Measured: 7 of the 39 libm-stub symbols are
also dynamic exports of the real `libc.so.6` — `frexp`, `frexpl`, `ldexp`,
`modf`, `modff`, `scalbn`, `scalbnf`.

The behavior is correct: both libraries export these at run time, so either
assignment resolves. The comment has libm and libc swapped, and it is the
comment the next floor change reads. Fix the comment, not the code.

### 2. The `provides` extraction is fragile in two ways

The sed at `generate.sh:217` captures only the first quoted name on the line,
and it matches only the current one-line `provides { "…": … }` format.

- A second `provides` entry fails step 8 with the misleading "a new
  dependency entered the host" message.
- A reformat of the block onto multiple lines extracts nothing, so `roc_init`
  fails step 8 the same way.

Both failures are loud. The gap is diagnostic quality, not silence.
Hardening: extract every quoted name from the `provides` block, and fail with
"could not parse provides from platform/main.roc" when the extraction is
empty.

### 3. The floor assertion has no test, and the gap is untracked

The spec's done-criteria annotation says the mismatched-glibc refusal "needs
a test of its own", because the assertion is the only thing holding the
floor. Nothing in the change adds that test. Neither
[`../tech_debt.md`](../tech_debt.md) §18 nor the phase 4 note records it as
pending. Phase 4's regen-diff on an `ubuntu-24.04` runner exercises only the
happy path. Record the gap in §18 or the phase 4 plan.

### 4. Latent: steps 8 and 9 disagree about compat-only exports

`dynsyms` (`generate.sh:223`) strips both `@` and `@@` suffixes, so a
provider whose only export of a name is a compat version still claims it.
`dyn_rows` (`generate.sh:297`) drops single-`@` rows, so classification then
fails with "`$sym` has no .dynsym row". The failure is loud, but it
attributes the symbol to the wrong provider, and the fix — let the symbol
fall through to a later provider with a default-version export — is not what
the message suggests. No symbol hits this at glibc 2.39. Low priority.

### 5. README wording

The Requirements section says `just roc-platform stubs` "runs only when the
link inputs change", which reads as automatic. It should say "run it only
when the link inputs change".

## Consistency

The spec's numbers reconcile with the artifacts:

- Stub set 475 = 419 (`libc.so.6`) + 39 (`libm.so.6`) + 15 (`libgcc_s.so.1`)
  + 1 (`libvulkan.so.1`) + 1 (`ld-linux-x86-64.so.2`).
- Three allowlisted symbols match what the LLD list plus the `provides`
  parse produces: `_GLOBAL_OFFSET_TABLE_`, `__dso_handle`, `roc_init`.
- `built_with_toolchain.txt` matches the amended step 13 format, with the
  container fields dropped as annotated.

## Remediation plan

Written after re-verifying every finding above against the artifacts. All five
are real.

**One amendment to this review's own ranking.** Findings 1, 2, 3 and 5 stand as
written. Finding 4 does not: it is re-rated from "low priority" to **first**.
The same root cause puts a hole in step 7's nonshared routing, and the victim is
`pthread_atfork` — one of the four names the spec's §"Risks" says the forwarding
archive exists to protect. Detail under fix 1.

The verdict of "no correctness bug" still holds. Every problem below is latent
at glibc 2.39, and the committed artifacts are correct as they stand. These are
fixes against the next floor change, not against today's output.

Apply in the order given: fix 1, then 2 and 5, then 3, then 4.

> **Applied 2026-08-17. Two snippets below are wrong as written; the applied
> code differs.**
>
> - **Fix 1** needs `$5!="LOCAL"` as well as `$7!="UND"`. `readelf --dyn-syms`
>   lists the LOCAL `_DYNAMIC` row of a stub, which `nm -D` omits, so the
>   snippet below reads `libvulkan.so`'s exports as
>   `_DYNAMIC vkGetInstanceProcAddr` and step 12 fails. The extra filter changes
>   no row of any of the five providers.
> - **Fix 3**'s sed range does not stop at the end of a one-line `provides`
>   block. sed searches for the end address from the next line, so the range
>   runs to the closing brace of `hosted` and the extraction returns
>   `roc_stderr_line`, `roc_stdin_line` and `roc_stdout_line` as well as
>   `roc_init`. An awk brace counter reads both formats. The assignment also
>   needs `|| provides=`, because `grep -o` exits 1 on no match and pipefail
>   aborts the script before the `fail` message.
>
> Artifacts are byte-identical, as fix 1 predicts. Five consecutive
> `just roc-platform stubs` runs exit 0 at stub set 475.

### Fix 1 — one definition of "usably exported"

Addresses finding 4 and a second consequence the finding does not name.

**Root cause.** The script holds two notions of "exported", and they disagree.
`dynsyms` (`generate.sh:223`) strips `@` and `@@` alike, so a compat-only export
counts as usable. `dyn_rows` (`generate.sh:297`) drops single-`@` rows. An
unversioned reference cannot bind to a compat-only version — glibc matches an
unversioned reference against the default version only — so `dynsyms` is the
wrong one.

Two consequences, not one.

- **The step 7 routing hole, and it is the important half.** `libc_dyn.txt`
  comes from `dynsyms` (`generate.sh:228`). `libc.so.6` exports
  `pthread_atfork@GLIBC_2.2.5` with no `@@` row, and `libc_nonshared.a` defines
  `pthread_atfork` weakly. Step 7 therefore reads `pthread_atfork` as a libc
  export and does **not** route it to `libc_forward.a`. A host that references
  it gets a declaration in the libc stub, a clean link, and a run-time
  resolution failure on every machine. That is exactly the failure the
  forwarding archive exists to prevent, applied to one of the four names it
  carries. `atexit`, `at_quick_exit` and `__stack_chk_fail_local` have no
  `libc.so.6` row at all, so they route correctly.
- **The step 8/9 disagreement**, as finding 4 describes it.

**The fix.** One helper, feeding both assignment and classification:

```bash
# Defined, default-version exports only. A compat-only export (single @) cannot
# satisfy an unversioned reference, and a UND row is an import, not an export.
dyn_rows() {
    readelf -sW --dyn-syms "$1" | awk '
        NF>=8 && $1 ~ /^[0-9]+:$/ && $7!="UND" {
            n=$8
            if (n ~ /@@/)     { sub(/@@.*/, "", n) }
            else if (n ~ /@/) { next }
            print n, $4, $3, $7
        }' | sort -u
}
dynsyms() { dyn_rows "$1" | awk '{print $1}' | sort -u; }
```

**`$7!="UND"` is mandatory.** Do not take finding 4's wording — "let the symbol
fall through to a later provider" — and point assignment at the existing
`dyn_rows`. That function keeps undefined rows, and `libvulkan.so.1` has 61 of
them out of 311, including `__isoc23_sscanf`, `__fread_chk` and
`__ctype_tolower_loc`. Without the filter, libvulkan claims libc symbols it
merely imports and they land in the libvulkan stub. `libc.so.6` has 19 such
rows.

The same filter closes a third defect. Classification reads the first row after
`sort -u`. For a name both imported and exported by one provider, `FUNC` and
`NOTYPE` sort before `OBJECT`, so the UND row wins and a data stub gets
`.size 0` — the copy-relocation defect the `.size` directive exists to prevent,
silent and visible only on the player's machine. Unreachable today: all five
data objects have exactly one `.dynsym` row each.

**Two things must not change.** `libc_dyn_versioned.txt` stays on raw `nm -D`
output, because the `GLIBC_PRIVATE` warning needs the version strings. And
`dynsyms` stays correct for step 12's libvulkan export check, since stub exports
are defined and unversioned.

**Expected outcome: byte-identical artifacts.** All 475 stubbed symbols have a
defined default-version export, `libc.so.6`'s 287 compat-only names are all
outside the stub set, and none of the three unrouted forwarders is referenced by
`libhost.a`. Verify with `md5sum`, not by reading a diff.

### Fix 2 — the provider-order comment

Addresses finding 1. Comment only. The order matches the spec, and both
libraries are in `DT_NEEDED`, so either assignment resolves at run time.

Replace the two sentences at `generate.sh:82` with the true behavior: earlier
providers win, so libm claims the names libc and libm both export. Measured, 7
of the 39 libm-stub symbols: `frexp`, `frexpl`, `ldexp`, `modf`, `modff`,
`scalbn`, `scalbnf`.

### Fix 3 — harden the `provides` parse

Addresses finding 2. Replace `generate.sh:217`:

```bash
provides=$(sed -n '/provides[[:space:]]*{/,/}/p' platform/main.roc |
    grep -o '"[^"]*"' | tr -d '"' | sort -u)
[ -n "$provides" ] || fail "could not parse a provides entry from platform/main.roc"
echo "$provides" >> "$work/allow.txt"
```

The range address depends on the block's opening and closing braces both
matching, so test the one-line format `main.roc` uses today and a multi-line
reformat.

### Fix 4 — test the floor assertion

Addresses finding 3. Close the gap rather than only recording it. With no
container, the assertion is the only thing holding the floor, and it runs in
step 1 ahead of `cargo`, so the test costs milliseconds. Add to
`ci/all_tests.sh`:

```bash
probe=$(mktemp)
sed 's/^REQUIRED_GLIBC=.*/REQUIRED_GLIBC=0.0/' stubs/generate.sh > "$probe"
if bash "$probe" > /dev/null 2>&1; then
    echo "FAIL: generate.sh accepted a mismatched glibc floor"
    failed=1
fi
rm -f "$probe"
```

Add one line to [`../tech_debt.md`](../tech_debt.md) §18 recording that phase
4's regen-diff on an `ubuntu-24.04` runner exercises the happy path only.

### Fix 5 — README wording

Addresses finding 5. `roc-platform/README.md:29`: "and it runs only when the
link inputs change" → "Run it only when the link inputs change."

### Considered and declined

`build.sh`'s `COMMITTED_INPUTS` duplicates `main.roc`'s `inputs` list, and
nothing enforces the match. Leave it. `build.sh` is the script that must never
break, and a nine-name list that changes about once per phase is cheap to keep
in sync. The two-list invariant is recorded in
[`02_stub_generator.md`](02_stub_generator.md) §"Edits to existing files".
Revisit if it drifts once.

### Verification

- `md5sum` the four `stubs/*_stub.s`, the four `.so` stubs and
  `libc_forward.a` before and after fix 1. Expect no change.
- Run `bash stubs/generate.sh` five times. Expect exit 0 and stub set 475 every
  time. Five runs because the SIGPIPE failure class is timing-dependent.
- Re-run the phase 2 done-criteria list in
  [`02_stub_generator.md`](02_stub_generator.md). Fix 1 touches provider
  assignment, which every number in the summary table depends on.
- `just roc-platform build && just roc-platform test`.
- Confirm the new floor-assertion test fails when the assertion is deleted, not
  only that it passes when the assertion is present.
