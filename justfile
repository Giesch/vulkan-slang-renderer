set windows-shell := ["powershell.exe", "-NoLogo", "-Command"]

# NOTE this does nothing on windows
set dotenv-load := true

# list all available just recipes
list:
    @ just --list --unsorted


# compiler/linter watch via bacon
check:
    bacon check-all
alias c := check


# run dev build with shader hot reload
dev example="basic_triangle":
    cargo run --example {{example}}
alias d := dev


# run with shader printf and vk validation layers at 'info'
[unix]
shader-debug example="viking_room":
    RUST_LOG=info VK_LAYER_PRINTF_ONLY_PRESET=1 cargo run --example {{example}}

[windows]
shader-debug example="viking_room":
    pwsh -Command { $env:RUST_LOG='info'; $env:VK_LAYER_PRINTF_ONLY_PRESET='1'; cargo run --example {{example}}; }

# run a release build
release: shaders
    cargo run --release
alias r := release


# write precompiled shader bytecode, json metadata, and generated rust source to disk
shaders:
    GENERATE_RUST_SOURCE=true cargo run --bin prepare_shaders
    cargo fmt
alias s := shaders

# export space invaders aseprite files as one sprite sheet
[unix]
sprites:
    cd textures/space_invaders && aseprite --batch *.aseprite --sheet sprite_sheet.png --data sprite_sheet.json --format json-array

# run all unit tests
test:
    INSTA_UPDATE=no cargo test
alias t := test

# run and review snapshot tests interactively
insta:
  cargo insta test --review
alias i := insta


# lint in debug and release with warnings denied
lint:
    cargo clippy -- -D warnings
    cargo clippy --release -- -D warnings
alias l := lint


# set up git pre-commit hook
[unix]
setup:
    cp scripts/pre-commit.sh .git/hooks/pre-commit
    chmod +x .git/hooks/pre-commit

# lint and test for git pre-commit hook
pre-commit: shaders && lint test
    git add shaders/compiled
