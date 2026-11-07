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
  `Game::setup` and `Game::run` return `eros::Result<T>`, so game code can `?`
  anything there, including typed graph errors.
- **`Game::draw`** returns `Result<(), DrawError>` (see below).
- **render-graph** never names backend errors. Each backend trait has an
  associated error type, which becomes the last member of the graph's typed
  sets (see `crates/render-graph/AGENTS.md`).

## `DrawError`: the frame error

`DrawError` (`crates/renderer/src/renderer/error.rs`) is the error of
`Game::draw`, `RuntimeGame::draw_frame`, the direct-draw methods
(`FrameRenderer::submit_draws`, `draw_indexed`, `draw_vertex_count`,
`draw_vertex_count_with_picking`) and the renderer's `FrameBackend::Error`.
It is a plain enum, not an eros union:

- `Upload(UploadRejected)`, `Indirect(IndirectRejected)`: rejected before
  anything was written, queued or submitted.
- `Frame(ErrorUnionAdapter)`: frame submission failed; holds the AnyError union
  with its context and backtrace.
- `Graph(ErrorUnionAdapter<GraphFrameErrors>)`: one of the render graph's own
  per-frame errors (dropped buffer, frame value count, upload size, uniform
  size, index range), still typed. Narrow it with `adapter.union().narrow::<E, _>()`
  or inspect with `adapter.as_ref().downcast_inner_ref::<E>()`.

A graph draw is `self.graph.execute(renderer, &params)?; Ok(())`. `?` uses
`From<ErrorUnion<GraphExecuteErrors>> for DrawError`: a held `DrawError` comes
back as is (context attached to the execute union moves onto a `Frame`/`Graph`
inner union; `Upload`/`Indirect` carry no diagnostics), and the graph's own
errors become `Graph` with every diagnostic kept. A direct draw is the tail
expression. Inside `draw`, an AnyError (`eros::ErrorUnion`) converts with `?`
into `Frame`.

Turning a `DrawError` into AnyError: call `DrawError::into_union()`, which
unwraps the `Frame`/`Graph` adapters so their context lines and backtrace stay
in the `{:#?}` report (the run loop does this). Plain `?` also compiles, through
eros's blanket `From<E: Error>`, but it makes the whole enum one root error and
buries the inner context. That blanket impl is also why `into_union` is a method
and not a `From` impl.

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
