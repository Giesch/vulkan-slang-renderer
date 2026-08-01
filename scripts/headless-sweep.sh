#!/usr/bin/env bash
#
# Headless validation sweep — run it as `just sweep`
#
# Usage, and when it is worth running: docs/testing.md
# Design: llm_notes/offscreen_testing.md. Findings: build_reproducibility.md §7.
#
# Runs each example under a software Vulkan driver with no display, and fails
# if any of them emits Vulkan validation output.
#
#   scripts/headless-sweep.sh                 # all examples
#   scripts/headless-sweep.sh basic_triangle  # just these
#   scripts/headless-sweep.sh --self-test     # only prove the detector works
#   SWEEP_TIMEOUT=20 scripts/headless-sweep.sh
#   SWEEP_SKIP="toon_link watercolor" scripts/headless-sweep.sh   # force a skip
#   SWEEP_SELF_TEST=0 scripts/headless-sweep.sh                   # skip the self-test
#
# Examples needing machine-local, gitignored assets are skipped or swept based
# on whether those assets are actually present (see assets_missing below), so
# the same invocation is correct on a dev machine and in a bare container.
#
# Container packages required (see build_reproducibility.md §4):
#   mesa-vulkan-drivers vulkan-validationlayers libvulkan-dev
# No audio package is needed; sdf_2d degrades to silent playback.
#
# The verdict comes from each example's EXIT CODE (see the table below), which
# the renderer now makes meaningful: renderer/debug.rs counts validation
# messages by severity and Game::run turns a nonzero count into a nonzero exit.
# The log is still grepped, but as a cross-check -- if the two detectors ever
# disagree, that is itself reported as a failure.

set -u
cd "$(dirname "$0")/.." || exit 1

# Defaults for the in-repo slang build, only where the environment (direnv, a
# custom slang location) hasn't already set them. Unlike the sweep-owned
# settings below, these are build configuration and an existing value wins.
: "${SLANG_LIB_DIR:=$PWD/slang/build/Release/lib}"
: "${SLANG_INCLUDE_DIR:=$PWD/slang/build/Release/include}"
: "${SLANG_EXTERNAL_DIR:=$PWD/slang/build/external}"
export SLANG_LIB_DIR SLANG_INCLUDE_DIR SLANG_EXTERNAL_DIR

# --- the settings this sweep must OWN rather than inherit -------------------
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
# 2. RUST_LOG, for a readable log. No longer load-bearing for the verdict:
#    the validation count keys off the severity Vulkan reports rather than the
#    log level, so a filtered RUST_LOG can hide the detail of a failure but not
#    the failure itself.
export RUST_LOG=warn
#
# 3. SDL's signal handlers must stay ON. SDL converts SIGTERM into SDL_QUIT,
#    which is what makes `timeout` below a *clean* shutdown -- and teardown is
#    where leaked-object errors (tech_debt.md §1) report themselves. With
#    SDL_NO_SIGNAL_HANDLERS=1, or under SIGKILL, they are never seen. This no
#    longer fails silently: the exit code distinguishes the two (see 143 below).
unset SDL_NO_SIGNAL_HANDLERS
#
# 4. VKR_SWEEP turns on the checks that are right for an automated sweep and
#    wrong interactively: exit 2 if validation is compiled out (a --release
#    build validates nothing and would pass everything), exit 3 if the example
#    ends without drawing a frame.
export VKR_SWEEP=1

: "${SWEEP_TIMEOUT:=10}"
: "${SWEEP_LOG_DIR:=/tmp/sweep-logs}"
: "${SWEEP_SELF_TEST:=1}"
: "${SWEEP_SELF_TEST_TIMEOUT:=5}"
# Force-skip by name, space separated. Empty by default: an example that cannot
# run here is detected below rather than listed here, so this is only for
# temporarily excluding one that otherwise would run.
: "${SWEEP_SKIP:=}"
mkdir -p "$SWEEP_LOG_DIR"

self_test_only=0
examples=()
for arg in "$@"; do
  case "$arg" in
    --self-test) self_test_only=1 ;;
    -*) echo "unknown option: $arg" >&2; exit 1 ;;
    *) examples+=("$arg") ;;
  esac
done

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

# Run one example binary, capturing its log. Returns the exit code; sets
# $elapsed. --preserve-status is what makes the exit code reach us at all:
# plain `timeout` reports 124 whenever the window is used up, discarding
# whatever the process exited with, which is why this sweep used to have no
# choice but to grep. -k covers a process that ignores SIGTERM outright.
run_example() {
  local bin=$1 log=$2 window=$3
  local start=$SECONDS
  timeout --preserve-status -k 5 -s TERM "$window" "./$bin" >"$log" 2>&1
  local code=$?
  elapsed=$((SECONDS - start))
  return $code
}

# Count of lines the old log-grep detector would have caught.
validation_lines() {
  grep -ciE '\[Validation\]|VUID-' "$1"
}

# Prove the sweep still detects a fault, by injecting one.
#
# This matters more than it sounds: a sweep that has silently stopped working
# looks exactly like a passing one, which is why this used to be a manual
# procedure in docs/testing.md that someone had to remember. VKR_INJECT_-
# VALIDATION_FAULT records an invalid viewport width (renderer.rs
# viewport_width), so a detector in working order reports exit 1 with
# VUID-VkViewport-width-01771.
self_test() {
  local bin="target/debug/examples/basic_triangle"
  local log="$SWEEP_LOG_DIR/self-test.log"

  if [ ! -x "$bin" ]; then
    echo "FAIL: self-test: no $bin"
    return 1
  fi

  # exported rather than prefixed onto the call: a `VAR=x func` prefix sets the
  # variable for the shell function, not for the process it spawns.
  local code=0
  export VKR_INJECT_VALIDATION_FAULT=1
  run_example "$bin" "$log" "$SWEEP_SELF_TEST_TIMEOUT" || code=$?
  unset VKR_INJECT_VALIDATION_FAULT

  if [ "$code" -eq 0 ]; then
    echo "FAIL: self-test: the injected fault was NOT detected."
    echo "    The sweep is not currently checking anything. Fix this before"
    echo "    trusting a pass; see docs/testing.md."
    return 1
  fi

  if ! grep -q 'VUID-VkViewport-width' "$log"; then
    echo "FAIL: self-test: exit $code, but not from the injected viewport fault."
    echo "    Something else is broken; see $log."
    return 1
  fi

  echo "self-test ok: injected fault detected (exit $code)"
}

echo "building examples..."
# Build FIRST, untimed, and run the binaries directly below.
#
# `timeout N cargo run` times the compile as well as the run. On a cold build
# the timeout expires during compilation: cargo is killed, exit code is 124 --
# indistinguishable from "the example ran for its whole window" -- and the log
# is empty. Every example then reports ok and the whole sweep is vacuous. This
# is easy to hit, since any source edit immediately before a sweep triggers it.
if ! cargo build -p mltrs --examples; then
  echo "FAIL: examples did not build" >&2
  exit 1
fi

if [ "$self_test_only" -eq 1 ]; then
  self_test || exit 1
  exit 0
fi

if [ "$SWEEP_SELF_TEST" != "0" ]; then
  # Abort rather than continue: a sweep whose detector is broken would report
  # a clean pass for every example, which is worse than not running at all.
  self_test || exit 1
fi

if [ "${#examples[@]}" -eq 0 ]; then
  mapfile -t examples < <(ls crates/mltrs/examples/*.rs | xargs -n1 basename | sed 's/\.rs$//')
fi

fail=0
ran=0
passed=0
skipped=0
for e in "${examples[@]}"; do
  case " $SWEEP_SKIP " in *" $e "*) echo "skip: $e (SWEEP_SKIP)"; skipped=$((skipped + 1)); continue ;; esac

  # A skip, not a failure, even when named explicitly on the command line: in a
  # container there is nothing to fix, and a red sweep there would be noise.
  if assets_missing "$e"; then
    echo "skip: $e (assets absent; run \`just extract-link && just convert-link\`)"
    skipped=$((skipped + 1))
    continue
  fi

  log="$SWEEP_LOG_DIR/$e.log"
  bin="target/debug/examples/$e"

  if [ ! -x "$bin" ]; then
    echo "FAIL(no binary): $e"
    fail=1
    continue
  fi

  elapsed=0
  code=0
  run_example "$bin" "$log" "$SWEEP_TIMEOUT" || code=$?
  ran=$((ran + 1))
  lines=$(validation_lines "$log")

  case $code in
    0)
      # The two detectors must agree. If the log has validation output the exit
      # code didn't account for, one of them is broken, and a sweep whose
      # detector is broken reports a clean pass for everything.
      if [ "$lines" -gt 0 ]; then
        echo "FAIL(detector disagreement): $e exited 0 with $lines validation lines in $log"
        fail=1
        continue
      fi
      # A clean exit well inside the window means it quit on its own rather
      # than on SIGTERM, so most of the run wasn't observed. --preserve-status
      # gives up 124-means-full-window, so the duration is what tells us.
      if [ "$elapsed" -lt "$((SWEEP_TIMEOUT - 1))" ]; then
        echo "FAIL(exited early): $e ran ${elapsed}s of its ${SWEEP_TIMEOUT}s window"
        grep -viE '^ +[0-9]+:|^ +at |ALSA lib' "$log" | tail -2 | sed 's/^/    /'
        fail=1
        continue
      fi
      echo "ok: $e"
      passed=$((passed + 1))
      ;;
    1)
      # Exit 1 is any error out of main; the log says which kind.
      if [ "$lines" -gt 0 ]; then
        echo "FAIL(validation, $lines lines): $e"
        grep -oiE 'VUID-[A-Za-z0-9_-]+' "$log" | sort -u | head -3 | sed 's/^/    /'
      else
        echo "FAIL(error): $e"
        grep -viE '^ +[0-9]+:|^ +at |ALSA lib' "$log" | tail -2 | sed 's/^/    /'
      fi
      fail=1
      ;;
    2)
      echo "FAIL(validation disabled): $e — a --release build validates nothing"
      fail=1
      ;;
    3)
      echo "FAIL(no frames): $e exited without drawing"
      grep -viE '^ +[0-9]+:|^ +at |ALSA lib' "$log" | tail -2 | sed 's/^/    /'
      fail=1
      ;;
    124)
      # Only reachable if --preserve-status stopped working.
      echo "FAIL(timeout, no status): $e — the exit code did not survive \`timeout\`"
      fail=1
      ;;
    143 | 137)
      # SIGTERM/SIGKILL reached the process itself, so SDL never turned it into
      # SDL_QUIT: the loop didn't exit normally, drain_gpu and Drop for Renderer
      # never ran, and teardown -- where leaked objects report -- went unchecked.
      # This was indistinguishable from a pass before --preserve-status.
      echo "FAIL(no clean teardown): $e died on a signal (exit $code)"
      fail=1
      ;;
    101)
      echo "FAIL(panic): $e"
      grep -viE '^ +[0-9]+:|^ +at |ALSA lib' "$log" | tail -2 | sed 's/^/    /'
      fail=1
      ;;
    *)
      echo "FAIL(exit $code): $e"
      grep -viE '^ +[0-9]+:|^ +at |ALSA lib' "$log" | tail -2 | sed 's/^/    /'
      fail=1
      ;;
  esac
done

# A sweep that ran nothing is not a pass. Reachable via a typo in an example
# name, or a skip list that swallowed everything.
if [ "$ran" -eq 0 ]; then
  echo "FAIL: no examples ran"
  fail=1
fi

echo "--- sweep $([ $fail -eq 0 ] && echo PASS || echo FAIL):" \
  "$passed ok / $skipped skip / $((ran - passed)) fail ---"
exit $fail
