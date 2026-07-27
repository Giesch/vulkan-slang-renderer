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
- ninja (the slang cmake preset uses it)

Plus these system libraries, which the build links against:
- `libasound2-dev` (alsa, via rodio; without it the `alsa-sys` build script fails)
- `libvulkan-dev` (without it the link step fails with `unable to find library -lvulkan`)

On debian/ubuntu, `just install-deps-debian` installs that set. It's
best-effort and distro-specific — a convenience, not a promise.

Then run:

``` sh
direnv allow # allow loading env vars
just build-slang # build slang from source (this will take a while)
just dev # run the default triangle example
```

Note that the static slang build deliberately produces no `libslang.a`; what
`slang-sys` links is `libslang-compiler.a`, `libcompiler-core.a` and
`libcore.a`.

### without direnv

`.envrc` exports the slang paths, but only in an interactive shell with direnv
hooked. Two things cover the rest:

- `.cargo/config.toml` sets them for anything cargo runs, so a bare
  `bash -c 'cargo test'` works with no direnv at all. A value already in the
  environment still wins.
- `. ./scripts/load-env.sh` exports them into the current shell — the unix
  mirror of `scripts/load-env.ps1`. Needed only for processes cargo doesn't
  launch, e.g. running `target/debug/examples/<name>` directly.

### snapshot tests and headless runs

The snapshot workflow (`just insta`, `cargo insta test --accept`) needs
`cargo-insta`, which `just install-tools` installs.

`just headless-all` runs every example under a software Vulkan driver with no
display and fails on any validation error. It needs `mesa-vulkan-drivers` (the
lavapipe ICD) and `vulkan-validationlayers`, which
`just install-deps-headless-debian` installs. No audio device is required.

