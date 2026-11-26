#!/usr/bin/env bash
#
# Build the platform and run every example headlessly.
#
# The example runs under the software Vulkan driver with no display, the same
# way scripts/headless-sweep.sh runs the renderer examples. SDL turns SIGTERM
# into SDL_QUIT, so `timeout --signal=TERM` produces a clean shutdown and the
# example exits 0. An exit of 143 means the signal reached the process
# directly, so the loop never drained the GPU.
#
# The host is a release build, so Vulkan validation is compiled out. This
# script checks that the platform links, starts, renders its graphs, and shuts
# down cleanly, with shader hot reload forced off and on. It does not check
# validation output; `just sweep` covers that for the renderer.

set -uo pipefail
cd "$(dirname "$0")/.." || exit 1

: "${RUN_TIMEOUT:=10}"

if ! command -v roc > /dev/null 2>&1; then
    echo "Error: roc not found on PATH." >&2
    exit 1
fi

export SDL_VIDEODRIVER=offscreen

lvp_icd=
for candidate in /usr/share/vulkan/icd.d/lvp_icd*.json; do
    [ -r "$candidate" ] && lvp_icd=$candidate && break
done
if [ -z "$lvp_icd" ]; then
    echo "Error: no lavapipe ICD in /usr/share/vulkan/icd.d (install mesa-vulkan-drivers)." >&2
    exit 1
fi
export VK_ICD_FILENAMES=$lvp_icd

# SDL only converts SIGTERM into SDL_QUIT while its signal handlers are on.
unset SDL_NO_SIGNAL_HANDLERS

echo "Using $(roc version)"
echo ""
echo "=== Building platform ==="
bash build.sh || exit 1

failed=0

# The glibc floor assertion in stubs/generate.sh is the only thing holding the
# floor, so it needs its own test. The probe stops in step 1, ahead of cargo.
# `mktemp -p stubs`: generate.sh derives its working directory from $0, so a
# probe in /tmp would cd to / and fail for another reason. The message grep is
# load-bearing for the same reason: any failure gives a non-zero exit.
echo ""
echo "--- floor assertion ---"
probe=$(mktemp -p stubs floor_probe.XXXXXX.sh)
sed 's/^REQUIRED_GLIBC=.*/REQUIRED_GLIBC=0.0/' stubs/generate.sh > "$probe"
probe_out=$(bash "$probe" 2>&1)
probe_code=$?
rm -f "$probe"
if [ $probe_code -eq 0 ]; then
    echo "FAIL(floor): generate.sh accepted a mismatched glibc floor"
    failed=1
elif ! echo "$probe_out" | grep -q "does not match the floor"; then
    echo "FAIL(floor): generate.sh failed for another reason:"
    echo "$probe_out" | sed 's/^/    /'
    failed=1
else
    echo "PASS: generate.sh refuses a mismatched glibc floor"
fi

# The host's CPU orchestration builds without roc or the host's link inputs.
echo ""
echo "--- host CPU tests ---"
if cargo test --manifest-path tests/lifecycle/Cargo.toml; then
    echo "PASS: graph lifecycle and shaders_source_dir resolution"
else
    echo "FAIL(host CPU tests): cargo test failed"
    failed=1
fi

# The fixtures live beside basic-triangle so they share its generated shader
# modules. Each negative fixture is valid but for one mistake, so `roc check`
# must report exactly one error, of the expected kind, at the expected site.
check_rejects() {
    local fixture=$1 kind=$2 site=$3 reason=$4
    echo ""
    echo "--- $fixture ---"
    local out code
    out=$(cd examples/basic-triangle && roc check "$fixture.roc" 2>&1)
    code=$?
    if [ $code -eq 0 ]; then
        echo "FAIL($fixture): roc check accepted it"
        failed=1
    elif ! grep -q "1 error and" <<< "$out" || ! grep -qi "$kind" <<< "$out" || ! grep -qF "$site" <<< "$out"; then
        echo "FAIL($fixture): roc check failed for another reason:"
        sed 's/^/    /' <<< "$out"
        failed=1
    else
        echo "PASS: $reason"
    fi
}

check_accepts() {
    local fixture=$1 reason=$2
    echo ""
    echo "--- $fixture ---"
    if (cd examples/basic-triangle && roc check "$fixture.roc" && roc test "$fixture.roc"); then
        echo "PASS: $reason"
    else
        echo "FAIL($fixture): roc check or roc test failed"
        failed=1
    fi
}

# A graph that fails validation fails `roc check` when its top-level Ok
# destructure meets the Err during constant evaluation.
check_rejects invalid_graph "non exhaustive destructure" "Ok(graphs)" \
    "an index range past its index buffer fails compilation"
check_rejects invalid_index_bounds "non exhaustive destructure" "Ok(graphs)" \
    "an index past its vertex count fails compilation"

check_accepts two_draws "two draws pack their values in declaration order"
check_accepts tuple_render_graphs "tuple render graphs preserve value order at every arity"
check_accepts local_graphs "selected graphs keep their set's type"

# Frame values must match the render graph's frame type at the draw call.
for fixture in invalid_values invalid_selected_values invalid_extra_values invalid_value_type; do
    check_rejects "$fixture" "type mismatch" "draw(" "$fixture rejects an invalid frame value at draw"
done

# Game.new ties the draw function's submissions to the registered set.
check_rejects invalid_game "type mismatch" "Game.new" \
    "Game.new rejects a draw function submitting from another set"

# Runs an example until the timeout's SIGTERM, which SDL turns into a clean quit.
run_example() {
    local label=$1 executable=$2
    timeout --signal=TERM --preserve-status "$RUN_TIMEOUT" "$executable"
    local code=$?

    case $code in
        0)
            echo "PASS: $label"
            ;;
        143 | 137)
            echo "FAIL(no clean teardown): $label died on a signal (exit $code)"
            failed=1
            ;;
        101)
            echo "FAIL(panic): $label"
            failed=1
            ;;
        *)
            echo "FAIL(exit $code): $label"
            failed=1
            ;;
    esac
}

for roc_file in examples/*/main.roc; do
    example_dir=$(dirname "$roc_file")
    name=$(basename "$example_dir")
    echo ""
    echo "--- $name ---"

    if ! (cd "$example_dir" && roc test main.roc && roc build --no-cache main.roc); then
        echo "FAIL(build): $name"
        failed=1
        continue
    fi

    # Forced off, pipelines load the precompiled SPIR-V, whatever the app's
    # Game.Init asks for.
    VKR_SHADER_HOT_RELOAD=0 run_example "$name (precompiled shaders)" "./$example_dir/main"

    # Forced on, every shader compiles from shaders_source_dir at startup,
    # resolved here against the executable's directory.
    VKR_SHADER_HOT_RELOAD=1 run_example "$name (shader hot reload)" "./$example_dir/main"
    rm -f "./$example_dir/main"
done

echo ""
if [ $failed -eq 0 ]; then
    echo "=== All tests passed! ==="
else
    echo "=== Some tests failed ==="
    exit 1
fi
