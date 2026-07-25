# Vulkan/Slang Renderer

This started out as an implementation of the standard [Vulkan Tutorial](https://docs.vulkan.org/tutorial/latest/00_Introduction.html).
It's become an experiment in integrating [Slang](https://shader-slang.org/) and the [Slang compile-time reflection API](https://docs.shader-slang.org/en/latest/external/slang/docs/user-guide/09-reflection.html).

The idea is to provide generated type-safe CPU bindings for an arbitrary slang shader, so you could have a workflow where both languages are hot reloaded and typechecked against each other. For now, I'm generating Rust (without hot reload), and many resource types aren't supported. In the future I'm hoping to target other languages as well. The renderer also has some other quality-of-life features like hot reloading shaders, and in-shader printf debugging.

![2D SDFs](./screenshots/sdf_demo.gif)
![Vulkan Tutorial Viking Room](./screenshots/viking-room.png)
![Ray-marched Spheres](./screenshots/raymarch-spheres.png)

## setup

For now, only linux and windows are supported.

When cloning this repo, use `git clone --recursive` to pick up the slang submodule.

You'll need the following dependencies installed:
- rust/cargo
- just
- direnv
- clang
- cmake

Then run:

``` sh
direnv allow # allow loading env vars
just build-slang # build slang from source (this will take a while)
just dev # run the default triangle example
```

## layout

This is a cargo workspace:

| path | package | what |
|---|---|---|
| `crates/renderer` | `mltrs-renderer` | Vulkan engine, slang compilation, reflection types |
| `crates/mltrs` | `mltrs` | what consumers depend on: the `Game` trait, app loop, asset helpers |
| `crates/cli` | `mltrs-cli` | slang → Rust codegen; the binary is named `mltrs` |
| `crates/convert-link` | `convert-link` | unrelated Wind Waker asset converter |
| `examples/<name>` | `<name>` | one crate per example |

Each example is a standalone crate that depends on `mltrs` the same way an outside
project would, and owns its own `shaders/source`, `shaders/compiled`, `src/generated`
and assets. They are the first consumers of the workflow below.

## using mltrs in your own project

```sh
cargo add mltrs                # path or git dependency for now
cargo add ash serde serde_json glam   # named directly by the generated bindings

cargo install --path crates/cli   # provides the `mltrs` binary

mltrs shaders init             # seeds shaders/source with the engine slang modules
# write shaders/source/my_game.shader.slang
mltrs shaders compile          # -> shaders/compiled/ + src/generated/
```

Then in `src/main.rs`:

```rust
mod generated;

use generated::shader_atlas::ShaderAtlas;
use mltrs::game::Game;

fn main() -> anyhow::Result<()> {
    MyGame::run()
}
```

`shaders/source` and `shaders/compiled` are conventions relative to your crate root;
override them with `--source-dir` / `--compiled-dir` / `--rust-dir` if you need to.
Generated code finds its data through `env!("CARGO_MANIFEST_DIR")`, which expands in
*your* crate, so shader hot reload works out of the box in debug builds.

`examples/basic_triangle` is the smallest complete example of this layout.

