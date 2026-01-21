set windows-shell := ["powershell.exe", "-NoLogo", "-Command"]

# NOTE this does nothing on windows
set dotenv-load := true

# list all available just recipes
list:
    @ just --list --unsorted


# compiler/linter watch via bacon
check:
    bacon check-all


# run dev build with shader hot reload
dev example="basic_triangle":
    cargo run --example {{example}}


# run with shader printf and vk validation layers at 'info'
[unix]
shader-debug example="viking_room":
    RUST_LOG=info VK_LAYER_PRINTF_ONLY_PRESET=1 \
      cargo run --example {{example}}

# run with shader printf and vk validation layers at 'info'
[windows]
shader-debug example="viking_room":
    pwsh -Command { \
      $env:RUST_LOG='info'; \
      $env:VK_LAYER_PRINTF_ONLY_PRESET='1'; \
      cargo run --example {{example}}; \
    }

# run a release build
release: shaders
    cargo run --release


# write precompiled shader bytecode, json metadata, and generated rust source to disk
[unix]
shaders:
    GENERATE_RUST_SOURCE=true cargo run --bin prepare_shaders
    cargo fmt

# write precompiled shader bytecode, json metadata, and generated rust source to disk
[windows]
shaders:
    pwsh -Command { \
      $env:GENERATE_RUST_SOURCE='true'; \
      cargo run --bin prepare_shaders; \
      cargo fmt; \
    }

# export space invaders aseprite files as one sprite sheet
[unix]
sprites:
    cd textures/space_invaders && aseprite --batch *.aseprite \
        --sheet sprite_sheet.png \
        --data sprite_sheet.json \
        --filename-format "{title} {frame}" \
        --format json-array

# run all unit tests
test:
    INSTA_UPDATE=no cargo test

# run and review snapshot tests interactively
[unix] # currently broken on windows, see build_tasks.rs
insta:
    cargo insta test --review


# lint in debug and release, with warnings denied
lint:
    cargo clippy -- -D warnings
    cargo clippy --release -- -D warnings


# set up git pre-commit hook
[unix]
setup-precommit:
    cp scripts/pre-commit.sh .git/hooks/pre-commit
    chmod +x .git/hooks/pre-commit

# lint and test for git pre-commit hook
pre-commit: shaders && lint test
    git add shaders/compiled

# get the slang git submodule and its submodules
init-submodules:
  git submodule update --init --recursive

# build slang as a static library (requires cmake and ninja)
[unix]
build-slang:
  cd vendor/slang && \
    cmake --preset default -DSLANG_LIB_TYPE=STATIC && \
    cmake --build --preset release

# build slang as a static library (requires cmake, ninja, python3, and visual studio)
[windows]
build-slang:
    pwsh -Command { \
      $env:SLANG_LIB_DIR="$PWD/vendor/slang/build/Release/lib"; \
      $env:SLANG_INCLUDE_DIR="$PWD/vendor/slang/build/Release/include"; \
      $env:SLANG_EXTERNAL_DIR="$PWD/vendor/slang/build/external"; \
      cd vendor/slang; \
      cmake --preset vs2022 -DSLANG_LIB_TYPE=STATIC; \
      cmake --build --preset release; \
    }
