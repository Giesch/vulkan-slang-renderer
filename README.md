# Vulkan/Slang Renderer

This started out as an implementation of the standard [Vulkan Tutorial](https://docs.vulkan.org/tutorial/latest/00_Introduction.html).
It's become an experiment in integrating [Slang](https://shader-slang.org/) and the [Slang compile-time reflection API](https://docs.shader-slang.org/en/latest/external/slang/docs/user-guide/09-reflection.html).

The idea is to provide a generated type-safe interface for an arbitrary slang shader, so you could have a workflow where both languages are hot reloaded and typechecked against each other. For now, I'm generating Rust (without hot reload), and many resource types aren't supported. In the future I'm hoping to target other languages as well. The renderer also has some other quality-of-life features like hot reloading shaders, and in-shader println debugging.
