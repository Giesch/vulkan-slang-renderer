set windows-shell := ["powershell.exe", "-NoLogo", "-Command"]

# Example-specific recipes live in the example's own crate, alongside the
# scripts and assets they touch. Reach them as `just <example> <recipe>`, e.g.
# `just toon_link link-verify-p1`. just runs a submodule's recipes with the
# working directory set to that submodule's dir, which is what their relative
# paths assume.
mod depth_texture 'examples/depth_texture'
mod koch_curve 'examples/koch_curve'
mod sdf_2d 'examples/sdf_2d'
mod serenity_crt 'examples/serenity_crt'
mod space_invaders 'examples/space_invaders'
mod sprite_batch 'examples/sprite_batch'
mod suzanne 'examples/suzanne'
mod toon_link 'examples/toon_link'
mod viking_room 'examples/viking_room'
mod watercolor 'examples/watercolor'


# list all available just recipes, including the per-example modules
_default:
    @ just --list --unsorted --list-submodules


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


# write precompiled shader bytecode, json metadata, and generated rust source to disk
[unix]
shaders example="all":
    #!/usr/bin/env bash
    set -euo pipefail
    if [ "{{example}}" = "all" ]; then
        for d in examples/*/; do cargo run -p mltrs-cli -- shaders compile --crate-dir "$d"; done
    else
        cargo run -p mltrs-cli -- shaders compile --crate-dir "examples/{{example}}"
    fi
    cargo fmt

# the examples with a `textures` recipe; add new ones here
examples_with_textures := "depth_texture koch_curve serenity_crt space_invaders sprite_batch suzanne viking_room"

# re-encode every example's source images to ktx2 (needs `cargo install ctt-cli`)
[unix]
textures:
    #!/usr/bin/env bash
    set -euo pipefail
    # NOTE the artifacts are committed, so this is only needed after changing a
    # source image -- it is deliberately not part of `just pre-commit`.
    for e in {{examples_with_textures}}; do just "$e" textures; done

# re-seed every example's vendored engine slang modules from the cli's canonical copies
[unix]
vendor-shaders:
    #!/usr/bin/env bash
    set -euo pipefail
    for d in examples/*/; do cargo run -p mltrs-cli -- shaders init --dir "$d/shaders/source" --force; done
    cargo fmt

# e.g. `just mltrs shaders compile --crate-dir examples/sdf_2d`
# run the mltrs cli directly, passing all arguments through
[unix]
mltrs *args:
    cargo run -p mltrs-cli -- {{args}}

# run the mltrs cli directly, passing all arguments through
[windows]
mltrs *args:
    pwsh -Command { \
      . ./scripts/load-env.ps1; \
      cargo run -p mltrs-cli -- {{args}}; \
    }

# write precompiled shader bytecode, json metadata, and generated rust source to disk
[windows]
shaders example="all":
    pwsh -Command { \
      . ./scripts/load-env.ps1; \
      if ('{{example}}' -eq 'all') { \
        Get-ChildItem -Directory examples | ForEach-Object { cargo run -p mltrs-cli -- shaders compile --crate-dir $_.FullName }; \
      } else { \
        cargo run -p mltrs-cli -- shaders compile --crate-dir "examples/{{example}}"; \
      } \
      cargo fmt; \
    }

# build one example, then run it for a few seconds and exit
#
# NOTE build and run are separate on purpose. `timeout N cargo run` times the
# compile as well as the run, so on a cold build the timeout expires during
# compilation and the example never starts -- with no output to say so.
[unix]
watch example="basic_triangle" seconds="5":
    cargo build -p {{example}}
    timeout --preserve-status -k 5 -s TERM {{seconds}} ./target/debug/{{example}}


# run every example headlessly, failing on vulkan validation output
[unix]
sweep *examples:
    ./scripts/headless-sweep.sh {{examples}}

# check that the sweep still detects an injected validation fault
[unix]
sweep-self-test:
    ./scripts/headless-sweep.sh --self-test


# run all unit tests
test:
    INSTA_UPDATE=no cargo test --workspace

# run and review snapshot tests interactively
[unix] # currently broken on windows, see build_tasks.rs
insta:
    cargo insta test --workspace --review


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
    git add 'examples/*/shaders/compiled/*' 'examples/*/src/generated/*'

