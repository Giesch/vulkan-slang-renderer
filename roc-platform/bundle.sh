#!/bin/bash
set -eo pipefail

# Bundle the platform into dist/<hash>.tar.zst.
#
# The platform directory content equals the bundle content, so there is no
# staging directory. `roc bundle` records each path as given, and
# platform/main.roc names its link inputs with `inputs_dir: "targets/"`. The
# script therefore bundles from inside platform/, so the archive root is the
# platform directory.
#
# main.roc leads the argument list because `roc bundle` uses the first .roc
# path as its module-discovery entry point. The glob repeats it; roc sorts and
# deduplicates the list.

TARGET_NAME=x64glibc
TARGET_DIR="platform/targets/$TARGET_NAME"
DIST_DIR=dist

# Every input the x64glibc inputs list names, except app. build.sh writes
# libhost.a; the rest are committed.
BUNDLE_INPUTS=(
    Scrt1.o
    crti.o
    crtn.o
    libstdc++.a
    libvulkan.so
    libm.so
    libc.so
    libc_forward.a
    libgcc_s.so
    libhost.a
)

if ! command -v roc > /dev/null 2>&1; then
    echo "Error: roc not found on PATH." >&2
    exit 1
fi

missing=()
for name in "${BUNDLE_INPUTS[@]}"; do
    [ -f "$TARGET_DIR/$name" ] || missing+=("$name")
done
if [ ${#missing[@]} -ne 0 ]; then
    echo "Missing link inputs in $TARGET_DIR:" >&2
    printf '  %s\n' "${missing[@]}" >&2
    echo "Build them with \`just roc-platform build\`." >&2
    exit 1
fi

# One archive per run keeps the output glob unambiguous. roc bundle opens
# --output-dir rather than creating it.
rm -rf "$DIST_DIR"
mkdir -p "$DIST_DIR"

(cd platform && roc bundle main.roc *.roc "targets/$TARGET_NAME"/* --output-dir "../$DIST_DIR")

bundles=("$DIST_DIR"/*.tar.zst)
if [ ${#bundles[@]} -ne 1 ]; then
    echo "Error: expected one archive in $DIST_DIR, found ${#bundles[@]}." >&2
    exit 1
fi

echo ""
echo "Bundle: ${bundles[0]}"
echo "Size:   $(stat -c %s "${bundles[0]}") bytes"
