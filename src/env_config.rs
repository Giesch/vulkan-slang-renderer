//! Every environment variable this process reads, parsed once at startup.
//!
//! Nothing outside [`EnvConfig::from_env`] may call `std::env::var`. A variable
//! read mid-frame costs a lookup per frame, is invisible at the call site, and
//! makes behavior depend on state no reader of that code can see. Parsing here
//! also means one place to look for the whole list.
//!
//! Variables read by libraries rather than by us are deliberately absent:
//! `RUST_LOG` belongs to `pretty_env_logger` (captured below only so a failure
//! message can mention it), and `SDL_VIDEODRIVER`, `VK_ICD_FILENAMES` and
//! `VK_LAYER_*` belong to SDL and the Vulkan loader.

/// The parsed environment. Build one with [`EnvConfig::from_env`] at startup
/// and pass it down; don't re-read the environment later.
#[derive(Debug, Clone, Default)]
pub struct EnvConfig {
    /// `VKR_SWEEP=1` — this run is part of `scripts/headless-sweep.sh`.
    ///
    /// Turns on startup and exit checks that are right for an automated sweep
    /// and wrong for an interactive run: a person closing a window after zero
    /// frames is not an error, but an example doing it under the sweep is.
    pub sweep: bool,

    /// `VKR_INJECT_VALIDATION_FAULT=1` — record a deliberately invalid viewport.
    ///
    /// Exists so the sweep can prove its own detector still fires; a sweep that
    /// has silently stopped detecting looks exactly like a passing one. Debug
    /// builds only, like validation itself.
    pub inject_validation_fault: bool,

    /// `GENERATE_RUST_SOURCE` — whether `prepare_shaders` writes `src/generated`.
    pub generate_rust_source: bool,

    /// `RUST_LOG` — consumed by `pretty_env_logger`, captured for reporting.
    ///
    /// Not load-bearing: validation counting keys off message severity, not the
    /// log level, so a filtered `RUST_LOG` can hide the detail of a failure but
    /// not the failure itself.
    pub rust_log: Option<String>,
}

impl EnvConfig {
    pub fn from_env() -> Self {
        Self {
            sweep: flag("VKR_SWEEP"),
            inject_validation_fault: flag("VKR_INJECT_VALIDATION_FAULT"),
            generate_rust_source: flag("GENERATE_RUST_SOURCE"),
            rust_log: std::env::var("RUST_LOG").ok(),
        }
    }
}

/// The exit codes `scripts/headless-sweep.sh` keys off.
///
/// 0 and 1 are the ones Rust already gives us — success, and `main` returning
/// `Err`, which is how a validation failure reports itself. These are the two
/// extras, both meaningful only under [`EnvConfig::sweep`]. Keep them in sync
/// with the table in `docs/testing.md`.
pub mod exit_code {
    /// Validation is compiled out, so this run would pass everything.
    pub const VALIDATION_DISABLED: i32 = 2;
    /// The run ended without ever presenting a frame.
    pub const NO_FRAMES: i32 = 3;
}

/// Unset, empty and `"false"` are false; anything else is true.
///
/// Matches what `GENERATE_RUST_SOURCE` has always accepted, so `just shaders`
/// keeps working unchanged.
fn flag(name: &str) -> bool {
    match std::env::var(name) {
        Err(_) => false,
        Ok(value) => {
            let value = value.trim();
            !value.is_empty() && !value.eq_ignore_ascii_case("false")
        }
    }
}
