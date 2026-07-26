#!/usr/bin/env bash
#
# Headless validation sweep — prototype for `just headless-all`
# (design: llm_notes/offscreen_testing.md; findings: llm_notes/build_reproducibility.md §7)
#
# Runs each example under a software Vulkan driver with no display, and fails
# if any of them emits Vulkan validation output.
#
#   scripts/headless-sweep.sh                 # all examples
#   scripts/headless-sweep.sh basic_triangle  # just these
#   SWEEP_TIMEOUT=20 scripts/headless-sweep.sh
#
# Container packages required (see build_reproducibility.md §4):
#   mesa-vulkan-drivers vulkan-validationlayers libvulkan-dev
# and, because sdf_2d opens an audio device, a null ALSA default in ~/.asoundrc:
#   pcm.!default { type null }
#   ctl.!default { type null }
#
# Verified to catch injected faults at all three points in the lifecycle
# (device init, per-frame command recording, and teardown) — see §7.2.

set -u
cd "$(dirname "$0")/.."

export SLANG_LIB_DIR="$PWD/slang/build/Release/lib"
export SLANG_INCLUDE_DIR="$PWD/slang/build/Release/include"
export SLANG_EXTERNAL_DIR="$PWD/slang/build/external"

# --- the four settings this sweep must OWN rather than inherit --------------
# Each of these, left to the ambient environment, makes a broken example pass
# silently. See build_reproducibility.md §7.3 for the measurements.
#
# 1. No GPU and no display: software ICD + offscreen SDL video driver.
export SDL_VIDEODRIVER=offscreen
export VK_ICD_FILENAMES=/usr/share/vulkan/icd.d/lvp_icd.json
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
mkdir -p "$SWEEP_LOG_DIR"

if [ "$#" -gt 0 ]; then
  examples=("$@")
else
  mapfile -t examples < <(ls examples/*.rs | xargs -n1 basename | sed 's/\.rs$//')
fi

fail=0
for e in "${examples[@]}"; do
  log="$SWEEP_LOG_DIR/$e.log"

  # SIGTERM (timeout's default) on purpose -- see note 3 above.
  timeout -s TERM "$SWEEP_TIMEOUT" cargo run --quiet --example "$e" >"$log" 2>&1
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
  # (toon_link legitimately exits 1 without its gitignored converted assets.)
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
