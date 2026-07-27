#!/usr/bin/env bash
#
# Headless validation sweep — the implementation behind `just headless-all`.
# (design: llm_notes/offscreen_testing.md; findings: llm_notes/build_reproducibility.md §7)
#
# Runs each example under a software Vulkan driver with no display, and fails
# if any of them emits Vulkan validation output.
#
#   just headless-all                  # all examples
#   just headless-all basic_triangle   # just these
#   SWEEP_TIMEOUT=20 just headless-all
#
# Container packages required (see build_reproducibility.md §4):
#   just install-deps-debian && just install-deps-headless-debian
# No audio package is needed; sdf_2d degrades to silent playback.
#
# Verified to catch injected faults at all three points in the lifecycle
# (device init, per-frame command recording, and teardown) — see §7.2.
# If you change this script, re-check that it still DETECTS a fault: a sweep
# that has silently stopped working looks exactly like a passing one.

set -u
cd "$(dirname "$0")/.."

# The examples are run as bare binaries below rather than through cargo, so
# they need the slang paths that .cargo/config.toml would otherwise supply.
# shellcheck source=./load-env.sh
. ./scripts/load-env.sh

# --- the four settings this sweep must OWN rather than inherit --------------
# Each of these, left to the ambient environment, makes a broken example pass
# silently. See build_reproducibility.md §7.3 for the measurements.
#
# 1. No GPU and no display: software ICD + offscreen SDL video driver.
export SDL_VIDEODRIVER=offscreen
: "${SWEEP_ICD:=/usr/share/vulkan/icd.d/lvp_icd.json}"
export VK_DRIVER_FILES="$SWEEP_ICD"     # loaders >= 1.3.207
export VK_ICD_FILENAMES="$SWEEP_ICD"    # older loaders; ignored when the above is set
#
# 2. RUST_LOG. The debug callback routes WARNING-severity validation through
#    log::warn! (renderer/debug.rs), and env_logger's default with RUST_LOG
#    unset keeps only error! -- so warnings vanish. An inherited RUST_LOG
#    naming some other module (or `off`) hides *everything*, errors included,
#    and .env sets one, so load-env.sh above has just supplied it.
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
: "${SWEEP_LOG_DIR:=target/sweep-logs}"
# Examples that cannot run on a machine without machine-local assets.
# toon_link needs assets/link/converted, which are gitignored and derived from
# a Wind Waker disc image (llm_notes/link_rendering/follow_up.md) -- it bails
# with a helpful message anywhere else. Set SWEEP_SKIP= to sweep it anyway on
# a machine where `just convert-link` has been run.
: "${SWEEP_SKIP:=toon_link}"
mkdir -p "$SWEEP_LOG_DIR"

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
  case " $SWEEP_SKIP " in *" $e "*) echo "skip: $e (needs machine-local assets)"; continue ;; esac

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
  # (toon_link legitimately exits 1 without its gitignored converted assets.)
  if [ "$code" -ne 124 ] && [ "$code" -ne 0 ]; then
    echo "FAIL(exit $code): $e"
    # drop backtrace frames, alsa's device-probe noise and blank lines; what's
    # left starts with the anyhow message or the panic line
    grep -viE '^ +[0-9]+:|^ +at |ALSA lib|^$' "$log" | head -3 | sed 's/^/    /'
    fail=1
    continue
  fi

  echo "ok: $e"
done

echo "--- sweep $([ $fail -eq 0 ] && echo PASS || echo FAIL) ---"
exit $fail
