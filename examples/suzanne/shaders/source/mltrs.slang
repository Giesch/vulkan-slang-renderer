#language slang 2026

module mltrs;

// The engine's slang prelude: `import mltrs;` re-exports every engine module,
// with all declarations under `namespace mltrs` (eg. `mltrs::MVPMatrices`).
//
// Note the namespace is ergonomics, not isolation: reflection records type
// names unqualified into a flat map, so every public struct/enum name still
// has to be unique across all of shaders/source/.
__exported import mltrs.addr;
__exported import mltrs.mvp;
__exported import mltrs.projection;
__exported import mltrs.fullscreen_triangle;
__exported import mltrs.super_sample;
