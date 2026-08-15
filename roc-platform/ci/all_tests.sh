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

for roc_file in examples/*.roc; do
    name=$(basename "$roc_file" .roc)
    echo ""
    echo "--- $name ---"

    if ! roc build --no-cache "$roc_file"; then
        echo "FAIL(build): $name"
        failed=1
        continue
    fi

    timeout --signal=TERM --preserve-status "$RUN_TIMEOUT" "./$name"
    code=$?
    rm -f "./$name"

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
