#!/usr/bin/env bash
#
# Headless validation sweep — prototype for `just headless-all`
#
# Usage, and when it is worth running: docs/testing.md
# Design: llm_notes/offscreen_testing.md. Findings: build_reproducibility.md §7.
#
# Runs each example under a software Vulkan driver with no display, and fails
# if any of them emits Vulkan validation output.
#
#   scripts/headless-sweep.sh                 # all examples
#   scripts/headless-sweep.sh basic_triangle  # just these
#   SWEEP_TIMEOUT=20 scripts/headless-sweep.sh
#   SWEEP_SKIP="toon_link watercolor" scripts/headless-sweep.sh   # force a skip
#
# Examples needing machine-local, gitignored assets are skipped or swept based
# on whether those assets are actually present (see assets_missing below), so
# the same invocation is correct on a dev machine and in a bare container.
#
# Container packages required (see build_reproducibility.md §4):
#   mesa-vulkan-drivers vulkan-validationlayers libvulkan-dev
# No audio package is needed; sdf_2d degrades to silent playback.
#
# Verified to catch injected faults at all three points in the lifecycle
# (device init, per-frame command recording, and teardown) — see §7.2.
# If you change this script, re-check that it still DETECTS a fault: a sweep
# that has silently stopped working looks exactly like a passing one.

set -u
cd "$(dirname "$0")/.."

export SLANG_LIB_DIR="$PWD/slang/build/Release/lib"
export SLANG_INCLUDE_DIR="$PWD/slang/build/Release/include"
export SLANG_EXTERNAL_DIR="$PWD/slang/build/external"

# --- the four settings this sweep must OWN rather than inherit --------------
# Each of these, left to the ambient environment, makes a broken example pass
# silently. See build_reproducibility.md §7.3 for the measurements.
#
# 1. No GPU and no display: software ICD + offscreen SDL video driver. Pinning
#    the ICD keeps the sweep on lavapipe even on a machine that has a real GPU,
#    so results are comparable across machines. Bail rather than let the loader
#    fall back to the system default: an unreadable VK_ICD_FILENAMES turns into
#    16 identical device-init failures that read like a renderer bug.
export SDL_VIDEODRIVER=offscreen
lvp_icd=
for candidate in /usr/share/vulkan/icd.d/lvp_icd*.json; do
  [ -r "$candidate" ] && lvp_icd=$candidate && break
done
if [ -z "$lvp_icd" ]; then
  echo "FAIL: no lavapipe ICD in /usr/share/vulkan/icd.d (install mesa-vulkan-drivers)" >&2
  exit 1
fi
export VK_ICD_FILENAMES=$lvp_icd
#
# 2. RUST_LOG. The debug callback routes WARNING-severity validation through
#    log::warn! (renderer/debug.rs), and env_logger's default with RUST_LOG
#    unset keeps only error! -- so warnings vanish. An inherited RUST_LOG
#    naming some other module (or `off`) hides *everything*, errors included.
export RUST_LOG=warn
#
# 3. SDL's signal handlers must stay ON. SDL converts SIGTERM into SDL_QUIT,
#    which is what makes `timeout` below a *clean* shutdown -- and teardown is
#    where leaked-object errors (tech_debt.md §1) report themselves. With
#    SDL_NO_SIGNAL_HANDLERS=1, or under SIGKILL, they are never seen.
unset SDL_NO_SIGNAL_HANDLERS
#
# 4. Debug build only: ENABLE_VALIDATION is cfg!(debug_assertions), so a
#    --release sweep validates nothing and passes everything. No --release below.

: "${SWEEP_TIMEOUT:=10}"
: "${SWEEP_LOG_DIR:=/tmp/sweep-logs}"
# Force-skip by name, space separated. Empty by default: an example that cannot
# run here is detected below rather than listed here, so this is only for
# temporarily excluding one that otherwise would run.
: "${SWEEP_SKIP:=}"
mkdir -p "$SWEEP_LOG_DIR"

# True when $1 needs machine-local assets that aren't on this machine.
#
# /assets/ is gitignored wholesale, so an example fed from it runs on a machine
# where the assets have been generated and cannot run anywhere else. Testing for
# the assets beats a hard-coded skip list: the same sweep covers toon_link on a
# dev machine and skips it in a container, with no env var to remember.
#
# Every other example loads from tracked textures/, models/ or audio/, so this
# is the whole set. Add a case here alongside any new gitignored-asset example.
assets_missing() {
  case "$1" in
    # examples/toon_link.rs:155 reads this first and bails if it is absent;
    # produced by `just extract-link && just convert-link` from a Wind Waker
    # disc image (llm_notes/link_rendering/phase_00.md).
    toon_link) [ ! -f assets/link/converted/link.manifest.json ] ;;
    *) return 1 ;;
  esac
}

if [ "$#" -gt 0 ]; then
  examples=("$@")
else
  mapfile -t examples < <(ls examples/*.rs | xargs -n1 basename | sed 's/\.rs$//')
fi

# Build FIRST, untimed, and run the binaries directly below.
#
# `timeout N cargo run` times the compile as well as the run. On a cold build
# the timeout expires during compilation: cargo is killed, exit code is 124 --
# indistinguishable from "the example ran for its whole window" -- and the log
# is empty. Every example then reports ok and the whole sweep is vacuous. This
# is easy to hit, since any source edit immediately before a sweep triggers it.
echo "building examples..."
if ! cargo build --examples; then
  echo "FAIL: examples did not build" >&2
  exit 1
fi

fail=0
for e in "${examples[@]}"; do
  case " $SWEEP_SKIP " in *" $e "*) echo "skip: $e (SWEEP_SKIP)"; continue ;; esac

  # A skip, not a failure, even when named explicitly on the command line: in a
  # container there is nothing to fix, and a red sweep there would be noise.
  if assets_missing "$e"; then
    echo "skip: $e (assets absent; run \`just extract-link && just convert-link\`)"
    continue
  fi

  log="$SWEEP_LOG_DIR/$e.log"
  bin="target/debug/examples/$e"

  if [ ! -x "$bin" ]; then
    echo "FAIL(no binary): $e"
    fail=1
    continue
  fi

  # SIGTERM (timeout's default) on purpose -- see note 3 above. Timing the
  # binary rather than `cargo run` keeps the compile out of the budget.
  timeout -s TERM "$SWEEP_TIMEOUT" "./$bin" >"$log" 2>&1
  code=$?

  # The debug callback returns VK_FALSE, so the process exits 0 even with
  # hundreds of validation errors. Grep the log; the exit code says nothing
  # about validation.
  if grep -qiE '\[Validation\]|VUID-' "$log"; then
    n=$(grep -ciE '\[Validation\]|VUID-' "$log")
    echo "FAIL(validation, $n lines): $e"
    grep -oiE 'VUID-[A-Za-z0-9_-]+' "$log" | sort -u | head -3 | sed 's/^/    /'
    fail=1
    continue
  fi

  # 124 == timed out == ran its whole window without dying, which is success
  # for an example that would otherwise loop forever. Anything else nonzero is
  # a crash or an early bail, reported separately from a validation failure.
  # Every example that reaches here has the assets it needs, so an early bail
  # is a real failure rather than a missing-asset message.
  if [ "$code" -ne 124 ] && [ "$code" -ne 0 ]; then
    echo "FAIL(exit $code): $e"
    grep -viE '^ +[0-9]+:|^ +at |ALSA lib' "$log" | tail -2 | sed 's/^/    /'
    fail=1
    continue
  fi

  echo "ok: $e"
done

echo "--- sweep $([ $fail -eq 0 ] && echo PASS || echo FAIL) ---"
exit $fail
