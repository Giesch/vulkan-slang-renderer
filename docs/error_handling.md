# Error handling

The workspace uses [`eros`](https://crates.io/crates/eros) for errors. anyhow
is not a dependency; don't reintroduce it.

## Which error shape to use

- **Library-like modules** use granular typed errors:
  `eros::Result<T, (ErrA, ErrB, ..)>`. Library-like means reflection, codegen,
  render-graph validation, parsers, KTX loading, GPU readback decoding, and
  resource creation from user input. Each member is a small `thiserror` type
  that carries the data needed to act on it, or a foreign error such as
  `io::Error` or `vk::Result`. Name sets with aliases (`PrepareShaderErrors`,
  `ExecuteErrors<E>`, `Ktx2Errors`, …). Each crate keeps its types in an
  `error` module.
- **The renderer** uses `eros::Result<T>` (AnyError) only where crashing is the
  only option: init, swapchain recreation, frame submission, hot reload.
- **Example game code, binaries and tests** use `eros::Result<T>` (AnyError).
  The `Game` trait (`setup`, `draw`, `run`) returns `eros::Result<T>`, so game
  code can `?` anything, including typed graph errors.
- **render-graph** never names backend errors. Each backend trait has an
  associated error type, which becomes the last member of the graph's typed
  sets (see `crates/render-graph/AGENTS.md`).

An eros set holds at most 26 types. When a set would exceed that, group related
failures into a small enum.

## Context

A fallible function describes its own operation and relevant inputs with
`#[context("Load model {}", path.display())]`. Add call-site `.context(..)` only
where the caller knows something the callee doesn't. Avoid `#[context]` on
per-frame or per-element hot paths.

`#[context]` formats lazily, after the body runs. That has three consequences:

- An owned parameter the body consumes needs `#[eros::eager_context]`.
- `self.field` shows its value *after* the body mutated it.
- A `&mut self` method that returns a borrow can't reference `self` fields.

eros-macros 0.8.1 also has two bugs:

- `#[context]` on an associated function without `self` fails with E0425.
  Use call-site `.context` instead.
- A parameter used only in the context string warns as unused. Prefix it with
  `_`.

## Printing errors

Display (`{}`) prints the root error and its `source()` chain. `{:#}` prints the
root only. **Context appears only in Debug output** (`{:?}`, `{:#?}`). Use
`{:#?}` for user-facing reports and for test assertions about context text.
Returning an error from `main` prints Debug, which includes a backtrace when
`RUST_BACKTRACE` is set.

To inspect a typed error, use `narrow::<E, _>()` on a union. On AnyError, use
`downcast_inner_ref::<E>()`. Use `recover::<E, _>(..)` to handle an error
type and remove it from the set.
