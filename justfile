set windows-shell := ["powershell.exe", "-NoLogo", "-Command"]


# list all available just recipes
list:
    @ just --list --unsorted


# compiler/linter watch via bacon
check:
    bacon check-all


# run dev build with shader hot reload
[unix]
dev example="basic_triangle":
    cargo run -p {{example}}

# run dev build with shader hot reload
[windows]
dev example="basic_triangle":
    pwsh -Command { \
      . ./scripts/load-env.ps1; \
      cargo run -p {{example}}; \
    }


# run with shader printf and vk validation layers at 'info'
[unix]
shader-debug example="viking_room":
    RUST_LOG=info VK_LAYER_PRINTF_ONLY_PRESET=1 \
      cargo run -p {{example}}

# run with shader printf and vk validation layers at 'info'
[windows]
shader-debug example="viking_room":
    pwsh -Command { \
      . ./scripts/load-env.ps1; \
      $env:RUST_LOG='info'; \
      $env:VK_LAYER_PRINTF_ONLY_PRESET='1'; \
      cargo run -p {{example}}; \
    }

# run a release build
release example="basic_triangle": shaders
    cargo run --release -p {{example}}


# regenerate spirv, reflection json and rust bindings for one example, or all
[unix]
shaders example="all":
    #!/usr/bin/env bash
    set -euo pipefail
    if [ "{{example}}" = "all" ]; then
        for d in examples/*/; do cargo run -q -p mltrs-cli -- shaders compile --crate-dir "$d"; done
    else
        cargo run -q -p mltrs-cli -- shaders compile --crate-dir "examples/{{example}}"
    fi
    cargo fmt

# regenerate spirv, reflection json and rust bindings for one example, or all
[windows]
shaders example="all":
    pwsh -Command { \
      . ./scripts/load-env.ps1; \
      if ('{{example}}' -eq 'all') { \
        Get-ChildItem -Directory examples | ForEach-Object { \
          cargo run -q -p mltrs-cli -- shaders compile --crate-dir $_.FullName; \
        } \
      } else { \
        cargo run -q -p mltrs-cli -- shaders compile --crate-dir "examples/{{example}}"; \
      } \
      cargo fmt; \
    }

# re-sync the vendored engine slang modules into every example
[unix]
vendor-shaders:
    #!/usr/bin/env bash
    set -euo pipefail
    for d in examples/*/; do
        cargo run -q -p mltrs-cli -- shaders init --dir "$d/shaders/source" --force
    done

# re-sync the vendored engine slang modules into every example
[windows]
vendor-shaders:
    pwsh -Command { \
      . ./scripts/load-env.ps1; \
      Get-ChildItem -Directory examples | ForEach-Object { \
        cargo run -q -p mltrs-cli -- shaders init --dir "$($_.FullName)/shaders/source" --force; \
      } \
    }

# generate watercolor paper height map texture
paper-texture:
    cargo run -p watercolor --bin generate_paper_texture --release

# export space invaders aseprite files as one sprite sheet
[unix]
sprites:
    cd examples/space_invaders/textures/space_invaders && aseprite --batch *.aseprite \
        --sheet sprite_sheet.png \
        --data sprite_sheet.json \
        --filename-format "{title} {frame}" \
        --format json-array

# run all unit tests
test:
    INSTA_UPDATE=no cargo test --workspace

# run and review snapshot tests interactively
[unix] # currently broken on windows, see build_tasks.rs
insta:
    cargo insta test -p mltrs-cli --review


# lint in debug and release, with warnings denied
# NOTE --all-targets is required to cover examples, benches and test cfg code;
# plain `cargo clippy` checks the lib and bins only, so example-only breakage
# slips through (same trap applies to `cargo check`)
lint:
    cargo clippy --workspace --all-targets -- -D warnings
    cargo clippy --workspace --all-targets --release -- -D warnings


# set up git pre-commit hook
[unix]
setup-precommit:
    cp scripts/pre-commit.sh .git/hooks/pre-commit
    chmod +x .git/hooks/pre-commit

# lint and test for git pre-commit hook
pre-commit: shaders && lint test
    git add examples/*/shaders/compiled examples/*/src/generated

# get the slang git submodule and its submodules
init-submodules:
  git submodule update --init --recursive

# build slang as a static library (requires cmake and ninja)
[unix]
build-slang:
  cd slang && \
    cmake --preset default -DSLANG_LIB_TYPE=STATIC && \
    cmake --build --preset release

# NOTE: the tests and slang-rhi dependency are disabled below.
# The slang-rhi fix this was waiting on (https://github.com/shader-slang/slang-rhi/pull/630)
# is included in the slang-rhi pinned by slang v2026.13.1, so these flags are
# likely removable; keeping them until that's verified on a Windows machine.
# build slang as a static library (requires cmake, ninja, python3, and visual studio)
[windows]
build-slang:
    pwsh -Command { \
      . ./scripts/load-env.ps1; \
      cd slang; \
      cmake --preset vs2022 '-DSLANG_LIB_TYPE=STATIC' '-DSLANG_ENABLE_SLANG_RHI=OFF' '-DSLANG_ENABLE_TESTS=OFF'; \
      cmake --build --preset vs2022-release; \
    }

[unix]
clean-slang:
    rm -rf slang/build

[windows]
clean-slang:
    pwsh -Command { \
      . ./scripts/load-env.ps1; \
      Remove-Item -Recurse -Force slang/build; \
    }


# write *.beats.json assets based on automatically extracted timestamps
[unix]
beats:
    ./scripts/extract_beats.py './examples/sdf_2d/audio/'


# extract Link assets from the tww disc image (needs ../tww; override with TWW_DIR)
[unix]
extract-link:
    ./scripts/extract_link.sh

# parse Link's BDL and emit converted assets (P1: chunk walk only)
[unix]
convert-link *args:
    cargo run -p convert-link -- assets/link/raw assets/link/converted {{args}}

# P1 gate: diff our --info chunk table against the gclib oracle, then run ignored tests
[unix]
link-verify-p1:
    #!/usr/bin/env bash
    set -euo pipefail
    diff <(just convert-link --info) <(./scripts/link_chunk_table.py assets/link/raw/cl.bdl)
    cargo test -p convert-link -- --include-ignored
    echo "P1 VERIFIED"

# P2 texture gate: pixel-diff every decoded texture against gclib
[unix]
link-verify-textures:
    #!/usr/bin/env bash
    set -euo pipefail
    just convert-link >/dev/null
    ./scripts/link_texture_diff.py assets/link/raw assets/link/converted/tex

# P2 MAT3 gate: diff our canonical --dump-mat3 against the gclib oracle
[unix]
link-verify-mat3:
    #!/usr/bin/env bash
    set -euo pipefail
    diff <(just convert-link --dump-mat3) <(./scripts/link_mat3_table.py assets/link/raw/cl.bdl)
    echo "MAT3 table matches oracle"

# P2 gate: textures + MAT3 + ignored real-file tests
[unix]
link-verify-p2: link-verify-textures link-verify-mat3
    cargo test -p convert-link -- --include-ignored
    echo "P2 VERIFIED"

# P3 geometry gate: diff our canonical --dump-geometry against the oracle,
# then run the full conversion (which runs the baking invariants)
[unix]
link-verify-geometry:
    #!/usr/bin/env bash
    set -euo pipefail
    diff <(just convert-link --dump-geometry) <(./scripts/link_geometry_table.py assets/link/raw/cl.bdl)
    just convert-link >/dev/null
    echo "geometry table matches oracle"

# P3 gate: geometry diff + ignored real-file tests
[unix]
link-verify-p3: link-verify-geometry
    cargo test -p convert-link -- --include-ignored
    echo "P3 VERIFIED"
