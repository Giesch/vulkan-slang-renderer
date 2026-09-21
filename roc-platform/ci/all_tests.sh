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
# script checks that the platform links, starts, and shuts down cleanly. It
# does not check validation output; `just sweep` covers that for the renderer.

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

# An invalid graph must fail `roc check` while the app's constants evaluate,
# with the same aggregated message the Rust validator reports. The fixture
# lives beside the example so it shares its generated shader modules.
echo ""
echo "--- invalid graph ---"
invalid_out=$(cd examples/basic-triangle && roc check invalid_graph.roc 2>&1)
invalid_code=$?
if [ $invalid_code -eq 0 ]; then
    echo "FAIL(invalid graph): roc check accepted an invalid render graph"
    failed=1
elif ! echo "$invalid_out" | grep -q "render graph validation failed"; then
    echo "FAIL(invalid graph): roc check failed for another reason:"
    echo "$invalid_out" | sed 's/^/    /'
    failed=1
else
    echo "PASS: an invalid render graph fails compilation"
fi

# A two-node graph consumes a value tuple in declaration order.
echo ""
echo "--- two draws ---"
if (cd examples/basic-triangle && roc test two_draws.roc); then
    echo "PASS: two draws of one pipeline pack both values"
else
    echo "FAIL(two draws): roc test failed"
    failed=1
fi

# Exercise every supported tuple arity, mixed types, and nested composition.
echo ""
echo "--- tuple render graphs ---"
if (cd examples/basic-triangle && roc test tuple_render_graphs.roc); then
    echo "PASS: tuple render graphs preserve declaration and payload order"
else
    echo "FAIL(tuple render graphs): roc test failed"
    failed=1
fi

# Packing still verifies the generated packer's byte-length invariant.
echo ""
echo "--- invalid packer ---"
packer_out=$(cd examples/basic-triangle && roc check invalid_packer.roc 2>&1)
packer_code=$?
if [ $packer_code -eq 0 ] || ! echo "$packer_out" | grep -q 'packed 1 bytes for a 192-byte uniform'; then
    echo "FAIL(invalid packer): expected the uniform-size diagnostic:"
    echo "$packer_out" | sed 's/^/    /'
    failed=1
else
    echo "PASS: an invalid packer fails with the uniform-size diagnostic"
fi

# Local record collections infer selected frame types without draw annotations.
echo ""
echo "--- local graphs ---"
if (cd examples/basic-triangle && roc test local_graphs.roc); then
    echo "PASS: local graph selection retains the collection type"
else
    echo "FAIL(local graphs): roc test failed"
    failed=1
fi

# Frame tuple arity and element types must match the render graph at draw.
for fixture in invalid_values invalid_selected_values invalid_extra_values invalid_value_type; do
    echo ""
    echo "--- $fixture ---"
    values_out=$(cd examples/basic-triangle && roc check "$fixture.roc" 2>&1)
    values_code=$?
    if [ $values_code -eq 0 ]; then
        echo "FAIL($fixture): roc check accepted an invalid frame tuple"
        failed=1
    elif ! echo "$values_out" | grep -qi "type mismatch" || ! echo "$values_out" | grep -q 'draw('; then
        echo "FAIL($fixture): roc check failed for another reason:"
        echo "$values_out" | sed 's/^/    /'
        failed=1
    else
        echo "PASS: $fixture rejects an invalid frame tuple at draw"
    fi
done

# A shader with a vertex input declared as a vertex-count pipeline must fail
# type checking: `shader.vertex` is a nominal marker.
echo ""
echo "--- invalid vertex count ---"
vc_out=$(cd examples/basic-triangle && roc check invalid_vertex_count.roc 2>&1)
vc_code=$?
if [ $vc_code -eq 0 ]; then
    echo "FAIL(invalid vertex count): roc check accepted a vertex-input shader without vertices"
    failed=1
elif ! echo "$vc_out" | grep -qi "type mismatch"; then
    echo "FAIL(invalid vertex count): roc check failed for another reason:"
    echo "$vc_out" | sed 's/^/    /'
    failed=1
else
    echo "PASS: a vertex-input shader without vertices fails compilation"
fi

# The Game constructor must tie the draw result to the collection's type.
echo ""
echo "--- invalid game ---"
game_out=$(cd examples/basic-triangle && roc check invalid_game.roc 2>&1)
game_code=$?
if [ $game_code -eq 0 ]; then
    echo "FAIL(invalid game): Game.new accepted mismatched draw and graphs types"
    failed=1
elif ! echo "$game_out" | grep -qi "type mismatch" || ! echo "$game_out" | grep -q 'Game.new'; then
    echo "FAIL(invalid game): roc check failed for another reason:"
    echo "$game_out" | sed 's/^/    /'
    failed=1
else
    echo "PASS: Game.new rejects mismatched draw and graphs types"
fi

for roc_file in examples/*/main.roc; do
    example_dir=$(dirname "$roc_file")
    name=$(basename "$example_dir")
    echo ""
    echo "--- $name ---"

    if ! (cd "$example_dir" && roc build --no-cache main.roc); then
        echo "FAIL(build): $name"
        failed=1
        continue
    fi

    timeout --signal=TERM --preserve-status "$RUN_TIMEOUT" "./$example_dir/main"
    code=$?
    rm -f "./$example_dir/main"

    case $code in
        0)
            echo "PASS: $name"
            ;;
        143 | 137)
            echo "FAIL(no clean teardown): $name died on a signal (exit $code)"
            failed=1
            ;;
        101)
            echo "FAIL(panic): $name"
            failed=1
            ;;
        *)
            echo "FAIL(exit $code): $name"
            failed=1
            ;;
    esac
done

echo ""
if [ $failed -eq 0 ]; then
    echo "=== All tests passed! ==="
else
    echo "=== Some tests failed ==="
    exit 1
fi
