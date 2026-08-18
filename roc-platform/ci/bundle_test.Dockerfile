# The image for ci/bundle_test.sh. It holds what a player's machine holds and
# nothing more: the Vulkan loader and the lavapipe software driver. binutils
# supplies nm and readelf for the executable checks.
#
# Deliberately absent: rust, cargo, cmake, gcc, SDL3, Vulkan headers and
# libvulkan-dev. The shipped executable must need none of them.
#
# 24.04 and not 22.04: the glibc floor is 2.39. A 22.04 container lacks
# thirteen symbols the host references. See llm_notes/tech_debt.md section 18.
#
# There is no COPY: ci/bundle_test.sh arrives on the /work mount, so the host
# and the container always run the same revision.

FROM ubuntu:24.04

RUN apt-get update \
    && DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends \
        binutils \
        libvulkan1 \
        mesa-vulkan-drivers \
    && rm -rf /var/lib/apt/lists/*
