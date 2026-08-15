#!/bin/bash
set -eo pipefail

# The host links SDL3, the Vulkan loader, and the C++ runtime that slang and
# vk-mem need. Those are glibc-linked shared libraries on this machine, so the
# platform builds for x64glibc only. musl and macOS targets need a static
# Vulkan/SDL story that does not exist here.
#
# roc resolves every name in a target's `inputs` list against
# platform/targets/<target>/, including the glibc startup objects and the
# system shared libraries. This script links them into place. The linked files
# are gitignored: they point at whatever this machine provides.

TARGET_NAME=x64glibc
RUST_TRIPLE=x86_64-unknown-linux-gnu
TARGET_DIR="platform/targets/$TARGET_NAME"

arch=$(uname -m)
os=$(uname -s)
if [ "$os" != "Linux" ] || [ "$arch" != "x86_64" ]; then
    echo "Unsupported host: $os $arch (this platform builds x64glibc only)" >&2
    exit 1
fi

if ! command -v gcc > /dev/null 2>&1; then
    echo "gcc is required to locate the glibc startup objects and runtime libraries." >&2
    exit 1
fi

# `gcc -print-file-name=X` echoes X unchanged when it cannot find the file, so
# an absolute path is the success test.
link_system_file() {
    local link_name=$1
    local file_name=$2
    local source_path

    source_path=$(gcc -print-file-name="$file_name")
    if [ "${source_path#/}" = "$source_path" ] || [ ! -e "$source_path" ]; then
        echo "Missing system file: $file_name" >&2
        exit 1
    fi

    ln -sf "$source_path" "$TARGET_DIR/$link_name"
    echo "  $link_name -> $source_path"
}

mkdir -p "$TARGET_DIR"

echo "Linking system files into $TARGET_DIR..."
# glibc startup objects, in the order the inputs list uses them.
link_system_file Scrt1.o Scrt1.o
link_system_file crti.o crti.o
link_system_file crtn.o crtn.o
# `cargo rustc -- --print native-static-libs` reports what libhost.a leaves
# undefined. Everything it lists that is not folded into libc.so.6 appears here.
link_system_file libstdc++.so libstdc++.so.6
link_system_file libvulkan.so libvulkan.so.1
link_system_file libm.so libm.so.6
link_system_file libc.so libc.so.6
link_system_file libgcc_s.so libgcc_s.so.1
# glibc keeps atexit and a few other stubs out of libc.so.6.
link_system_file libc_nonshared.a libc_nonshared.a
echo ""

echo "Building for $TARGET_NAME ($RUST_TRIPLE)..."
cargo build --release --lib --target "$RUST_TRIPLE"

cp "target/$RUST_TRIPLE/release/libhost.a" "$TARGET_DIR/"
echo "  -> $TARGET_DIR/libhost.a"

echo ""
echo "Build complete!"
