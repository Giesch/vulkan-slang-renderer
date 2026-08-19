#!/usr/bin/env bash
#
# Prove the bundled platform runs from a URL on a stock ubuntu:24.04 machine.
#
# Host side: build, bundle, serve dist/ on loopback, write a test app whose
# header names the served archive, then run the container side.
#
# Container side (--container): download and link the platform from that URL,
# check the executable against the container's libraries, and run it. The image
# has no rust, no cargo, no cmake, no gcc, no SDL3, no Vulkan headers and no
# libvulkan-dev, so a green run pins the runtime floor to the executable.
#
# roc accepts a plain-http package URL on loopback only
# (../roc/src/base/url.zig:290), so the container needs --network=host. A
# bridge network needs the host LAN address, which that gate rejects.
#
# The app exits on SIGTERM only, so a passing run always consumes its whole
# timeout window. The download and the link therefore happen under
# BUILD_TIMEOUT in `roc build`, ahead of the timed runs.

set -uo pipefail

: "${RUN_TIMEOUT:=10}"
: "${ROC_RUN_TIMEOUT:=60}"
: "${BUILD_TIMEOUT:=900}"

IMAGE=mltrs-roc-platform-bundle-test

failed=0

# SDL turns SIGTERM into SDL_QUIT, so a healthy run exits 0. An exit of 143
# means the signal reached the process directly, so the loop never drained the
# GPU.
check_run() {
    case $2 in
        0)
            echo "PASS: $1"
            ;;
        143 | 137)
            echo "FAIL(no clean teardown): $1 died on a signal (exit $2)"
            failed=1
            ;;
        101)
            echo "FAIL(panic): $1"
            failed=1
            ;;
        *)
            echo "FAIL(exit $2): $1"
            failed=1
            ;;
    esac
}

# `roc <app>.roc` runs the app as a child process, and timeout signals the whole
# process group. The app therefore receives SIGTERM and shuts down cleanly, but
# roc installs no SIGTERM handler and dies on the default disposition.
# --preserve-status reports roc's status, so a healthy run of this shape is 143
# and the app's own exit 0 is invisible. The two executable runs verify
# teardown.
check_roc_run() {
    case $2 in
        0 | 143)
            echo "PASS: $1"
            ;;
        101)
            echo "FAIL(panic): $1"
            failed=1
            ;;
        *)
            echo "FAIL(exit $2): $1"
            failed=1
            ;;
    esac
}

die() {
    echo "FAIL($1): $2"
    echo ""
    echo "=== Some tests failed ==="
    exit 1
}

# =============================== container side ==============================
#
# The dispatch precedes any cd: inside the container $0 is /work/bundle_test.sh,
# so `cd "$(dirname "$0")/.."` would land on /.

if [ "${1:-}" = "--container" ]; then
    mkdir -p /out || exit 1
    cp /work/bundle_app.roc /out/ || exit 1
    cd /out || exit 1

    # /work is read-only, so every write goes to the container's own layer. The
    # container filesystem is fresh on each --rm run, so the cache starts empty
    # and the download path always runs.
    export ROC_CACHE_DIR=/roc-cache

    export SDL_VIDEODRIVER=offscreen

    lvp_icd=
    for candidate in /usr/share/vulkan/icd.d/lvp_icd*.json; do
        [ -r "$candidate" ] && lvp_icd=$candidate && break
    done
    if [ -z "$lvp_icd" ]; then
        echo "Error: no lavapipe ICD in /usr/share/vulkan/icd.d." >&2
        exit 1
    fi
    export VK_ICD_FILENAMES=$lvp_icd

    # SDL only converts SIGTERM into SDL_QUIT while its signal handlers are on.
    unset SDL_NO_SIGNAL_HANDLERS

    exe=./bundle_app

    echo "=== container: $(sed -n 's/^PRETTY_NAME="\(.*\)"/\1/p' /etc/os-release) ==="
    echo "Using $(roc version)"
    sed -n 1p bundle_app.roc
    echo ""

    # --- platform download and link ------------------------------------------
    #
    # The platform edge is exempt from the per-package limit
    # (../roc/src/compile/package_resolution.zig:854) but not from the
    # per-direct-dependency transitive limit (checkTransitiveLimits, line 918).
    # The platform expands to 154 MiB against a 100 MB default. Attempt one
    # carries no flag, so the test records which limit actually bites.

    echo "--- platform download and link ---"
    limit_flags=()
    timeout "$BUILD_TIMEOUT" roc build bundle_app.roc > build.log 2>&1
    code=$?

    if [ $code -ne 0 ]; then
        # roc renders a diagnostic title in upper case.
        grep -qi "Dependency Tree Too Large" build.log && limit_flags+=(--max-transitive-mb=0)
        grep -qi "Package Too Large" build.log && limit_flags+=(--max-package-mb=0)
    fi

    if [ ${#limit_flags[@]} -ne 0 ]; then
        echo "MEASURED: the platform exceeds a default size limit."
        sed 's/^/    /' build.log
        echo "  Retrying with ${limit_flags[*]} from an empty cache."
        rm -rf "$ROC_CACHE_DIR"
        timeout "$BUILD_TIMEOUT" roc build "${limit_flags[@]}" bundle_app.roc > build.log 2>&1
        code=$?
    fi

    if [ $code -ne 0 ]; then
        echo "FAIL(build): the platform did not build from the URL (exit $code)"
        sed 's/^/    /' build.log
        echo ""
        echo "=== Container checks failed ==="
        exit 1
    fi

    if [ ${#limit_flags[@]} -eq 0 ]; then
        echo "PASS: the platform builds from the URL with no size flag"
        echo "MEASURED: an app needs neither --max-package-mb nor --max-transitive-mb."
    else
        echo "PASS: the platform builds from the URL with ${limit_flags[*]}"
        echo "MEASURED: an app that names this platform by URL needs ${limit_flags[*]}."
    fi

    # --- dynamic dependencies ------------------------------------------------
    #
    # readelf, not ldd, for the list: a Vulkan SDK on LD_LIBRARY_PATH adds
    # libdl and libpthread to ldd output.

    echo ""
    echo "--- dynamic dependencies ---"
    readelf -d "$exe" > readelf_d.txt 2>&1
    needed=$(grep NEEDED readelf_d.txt | sed 's/.*\[\(.*\)\]/\1/' | sort | tr '\n' ' ')
    expected="libc.so.6 libgcc_s.so.1 libm.so.6 libvulkan.so.1 "
    if [ "$needed" = "$expected" ]; then
        echo "PASS: DT_NEEDED is exactly [$expected]"
    else
        echo "FAIL(needed): got [$needed], want [$expected]"
        failed=1
    fi

    env -u LD_LIBRARY_PATH ldd "$exe" > ldd.txt 2>&1
    sed 's/^/    /' ldd.txt
    if grep -q "libstdc++" ldd.txt; then
        echo "FAIL(libstdc++): the executable links the shared C++ runtime"
        failed=1
    elif grep -q "not found" ldd.txt; then
        echo "FAIL(missing): the container does not have every library"
        failed=1
    else
        echo "PASS: ldd resolves every library and names no libstdc++.so.6"
    fi

    # --- symbol versions -----------------------------------------------------

    echo ""
    echo "--- symbol versions ---"
    # The stubs pin a symbol to its default version when the provider also
    # exports a compat version: ld.so binds an unversioned reference to the
    # oldest version node, which is the compat implementation. Every pin is a
    # version of the floor glibc (REQUIRED_GLIBC in stubs/generate.sh), so
    # the requirement must exist and must stay at or below the floor.
    GLIBC_FLOOR=2.39
    readelf --version-info "$exe" > versions.txt 2>&1
    required=$(grep -oE 'GLIBC_[0-9]+(\.[0-9]+)*' versions.txt | sed 's/^GLIBC_//' | sort -uV)
    if [ -z "$required" ]; then
        echo "FAIL(versions): the executable pins no glibc symbol versions"
        failed=1
    else
        newest=$(echo "$required" | tail -1)
        if [ "$(printf '%s\n%s\n' "$newest" "$GLIBC_FLOOR" | sort -V | tail -1)" != "$GLIBC_FLOOR" ]; then
            echo "FAIL(versions): the executable requires GLIBC_$newest, above the $GLIBC_FLOOR floor"
            failed=1
        else
            echo "PASS: version requirements stay at or below glibc $GLIBC_FLOOR ($(echo "$required" | tr '\n' ' '))"
        fi
    fi

    # --- undefined symbols ---------------------------------------------------

    echo ""
    echo "--- undefined symbols ---"
    nm -D --undefined-only "$exe" > undefined.txt 2>&1
    undefined=$(wc -l < undefined.txt)
    env -u LD_LIBRARY_PATH ldd -r "$exe" > ldd_r.txt 2>&1
    if grep -q "undefined symbol" ldd_r.txt; then
        echo "FAIL(undefined): the container does not define every symbol"
        grep "undefined symbol" ldd_r.txt | sed 's/^/    /'
        failed=1
    else
        echo "PASS: all $undefined undefined symbols resolve against container libraries"
    fi

    # --- copy relocations ----------------------------------------------------

    echo ""
    echo "--- copy relocations ---"
    readelf -r "$exe" > relocs.txt 2>&1
    grep R_X86_64_COPY relocs.txt > copies.txt
    copies=$(wc -l < copies.txt)
    if [ "$copies" -eq 0 ]; then
        echo "PASS (vacuous): the executable has no copy relocation"
    else
        sed 's/^/    /' copies.txt
        if grep -q "_Z" copies.txt; then
            echo "FAIL(copy): a C++ symbol has a copy relocation"
            failed=1
        else
            echo "PASS: $copies copy relocations, C symbols only. Compare each size by hand."
        fi
    fi

    # --- exported symbols ----------------------------------------------------

    echo ""
    echo "--- exported symbols ---"
    nm -D --defined-only "$exe" > exports.txt 2>&1
    if grep -q " _Z" exports.txt; then
        echo "FAIL(exports): the executable exports C++ symbols"
        grep " _Z" exports.txt | sed 's/^/    /'
        failed=1
    else
        echo "PASS: no _Z-prefixed export"
    fi

    # --- interpreter ---------------------------------------------------------

    echo ""
    echo "--- interpreter ---"
    readelf -l "$exe" > headers.txt 2>&1
    if grep -q "Requesting program interpreter: /lib64/ld-linux-x86-64.so.2" headers.txt; then
        echo "PASS: interpreter is /lib64/ld-linux-x86-64.so.2"
    else
        echo "FAIL(interpreter): not the ABI-constant path"
        grep "Requesting program interpreter" headers.txt | sed 's/^/    /'
        failed=1
    fi

    # --- runs ----------------------------------------------------------------

    echo ""
    echo "--- run ---"
    timeout --signal=TERM --preserve-status "$RUN_TIMEOUT" "$exe"
    check_run "plain run" $?

    echo ""
    echo "--- LD_BIND_NOW run ---"
    LD_BIND_NOW=1 timeout --signal=TERM --preserve-status "$RUN_TIMEOUT" "$exe"
    check_run "LD_BIND_NOW run" $?

    echo ""
    echo "--- roc bundle_app.roc ---"
    timeout --signal=TERM --preserve-status "$ROC_RUN_TIMEOUT" \
        roc "${limit_flags[@]}" bundle_app.roc
    check_roc_run "roc bundle_app.roc" $?

    echo ""
    if [ $failed -eq 0 ]; then
        echo "=== Container checks passed ==="
    else
        echo "=== Container checks failed ==="
        exit 1
    fi
    exit 0
fi

# ================================= host side =================================

script_dir=$(cd "$(dirname "$0")" && pwd)
cd "$script_dir/.." || exit 1

for tool in roc docker python3 git; do
    command -v "$tool" > /dev/null 2>&1 || die tool "$tool is not on PATH"
done

work=$(mktemp -d -t mltrs_bundle_test.XXXXXXXX) || exit 1
server_pid=
cleanup() {
    if [ -n "$server_pid" ]; then
        kill "$server_pid" 2> /dev/null
        wait "$server_pid" 2> /dev/null
    fi
    rm -rf "$work"
}
trap cleanup EXIT
trap 'exit 130' INT
trap 'exit 143' TERM

echo "=== mltrs Roc platform bundle proof ==="
echo "Using $(roc version)"

echo ""
echo "=== Building platform ==="
bash build.sh || die build "build.sh failed"

echo ""
echo "--- link inputs ---"
dirty=$(git status --porcelain -- platform/targets/x64glibc | grep -v 'libhost\.a$')
if [ -n "$dirty" ]; then
    echo "FAIL(dirty): build.sh changed a committed link input"
    echo "$dirty" | sed 's/^/    /'
    failed=1
else
    echo "PASS: platform/targets/x64glibc is clean except libhost.a"
fi

echo ""
echo "=== Bundling platform ==="
bash bundle.sh || die bundle "bundle.sh failed"

bundles=(dist/*.tar.zst)
[ ${#bundles[@]} -eq 1 ] ||
    die bundle "expected one archive in dist/, found ${#bundles[@]}"
bundle_name=$(basename "${bundles[0]}")

echo ""
echo "--- server ---"
# Port 0 lets the kernel choose, which removes every port-collision race. The
# server reports the real port on stdout; python3 -u keeps that line out of a
# block buffer.
python3 -u -m http.server 0 --bind 127.0.0.1 --directory dist > "$work/server.log" 2>&1 &
server_pid=$!

port=
for _ in $(seq 1 50); do
    kill -0 "$server_pid" 2> /dev/null || break
    port=$(sed -n 's/^Serving HTTP on .* port \([0-9][0-9]*\) .*/\1/p' "$work/server.log")
    [ -n "$port" ] && break
    sleep 0.1
done
if [ -z "$port" ]; then
    sed 's/^/    /' "$work/server.log"
    die server "python3 -m http.server did not report a port"
fi

url="http://127.0.0.1:$port/$bundle_name"
echo "PASS: serving dist/ at $url"

echo ""
echo "--- test app ---"
# Line 1 of the committed example is the app header. The test copy names the
# URL; the committed example keeps its relative path.
sed "1s|.*|app [game] { pf: platform \"$url\" }|" examples/basic_triangle.roc \
    > "$work/bundle_app.roc" || die app "could not write the test app"
cp "$script_dir/$(basename "$0")" "$work/bundle_test.sh" || exit 1
sed -n 1p "$work/bundle_app.roc" | sed 's/^/    /'

echo ""
echo "--- image ---"
docker build -f ci/bundle_test.Dockerfile -t "$IMAGE" ci || die image "docker build failed"

echo ""
echo "=== Container proof ==="
roc_bin=$(readlink -f "$(command -v roc)")
docker run --rm \
    --network=host \
    --volume "$roc_bin:/usr/local/bin/roc:ro" \
    --volume "$work:/work:ro" \
    --env "RUN_TIMEOUT=$RUN_TIMEOUT" \
    --env "ROC_RUN_TIMEOUT=$ROC_RUN_TIMEOUT" \
    --env "BUILD_TIMEOUT=$BUILD_TIMEOUT" \
    "$IMAGE" \
    bash /work/bundle_test.sh --container
[ $? -eq 0 ] || failed=1

echo ""
if [ $failed -eq 0 ]; then
    echo "=== All tests passed! ==="
else
    echo "=== Some tests failed ==="
    exit 1
fi
