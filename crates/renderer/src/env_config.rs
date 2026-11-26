// TODO review these docs?

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
    pub headless_sweep: bool,

    /// `VKR_INJECT_VALIDATION_FAULT=1` — record a deliberately invalid viewport.
    ///
    /// Exists so the sweep can prove its own detector still fires; a sweep that
    /// has silently stopped detecting looks exactly like a passing one. Debug
    /// builds only, like validation itself.
    pub inject_validation_fault: bool,

    /// `VKR_PREFER_INTEGRATED=1` — rank integrated GPUs above discrete when
    /// choosing a physical device. A preference, not a requirement: if no
    /// integrated GPU is suitable the renderer still starts on whatever is.
    /// Unset (or false) keeps the default discrete-first order.
    pub prefer_integrated_gpu: Option<bool>,

    /// `VKR_SHADER_HOT_RELOAD=1` / `=0` — force shader hot reload on or off,
    /// overriding the application's `RendererConfig::shader_hot_reload`
    /// (which `Game` defaults to on in debug builds and off in release).
    /// Unset keeps the application's choice; see
    /// [`EnvConfig::resolve_shader_hot_reload`].
    pub shader_hot_reload: Option<bool>,

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
            headless_sweep: flag("VKR_SWEEP"),
            inject_validation_fault: flag("VKR_INJECT_VALIDATION_FAULT"),
            prefer_integrated_gpu: optional_flag("VKR_PREFER_INTEGRATED"),
            shader_hot_reload: optional_flag("VKR_SHADER_HOT_RELOAD"),
            rust_log: std::env::var("RUST_LOG").ok(),
        }
    }

    /// Whether shader hot reload is on: `VKR_SHADER_HOT_RELOAD` when set,
    /// otherwise the application's default.
    pub fn resolve_shader_hot_reload(&self, app_default: bool) -> bool {
        self.shader_hot_reload.unwrap_or(app_default)
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

/// Unset, empty, `"0"` and `"false"` are false; anything else is true.
fn flag(name: &str) -> bool {
    optional_flag(name).unwrap_or(false)
}

/// Unset is `None`; otherwise the same truthiness rule as [`flag`].
fn optional_flag(name: &str) -> Option<bool> {
    let value = std::env::var(name).ok()?;

    Some(is_truthy(&value))
}

/// Empty, `"0"` and `"false"` (any case, surrounding whitespace ignored) are
/// false; anything else is true.
fn is_truthy(value: &str) -> bool {
    let value = value.trim();
    let is_falsey = value.is_empty() || value == "0" || value.eq_ignore_ascii_case("false");

    !is_falsey
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_truthy_treats_empty_zero_and_false_as_false() {
        for value in ["", "  ", "0", " 0 ", "false", "FALSE", "False"] {
            assert!(!is_truthy(value), "{value:?} should be false");
        }

        for value in ["1", "true", "yes", "on", "2"] {
            assert!(is_truthy(value), "{value:?} should be true");
        }
    }

    #[test]
    fn optional_flag_is_none_for_an_unset_variable() {
        assert_eq!(
            optional_flag("VKR_TEST_VARIABLE_THAT_IS_NEVER_SET_ANYWHERE"),
            None
        );
    }

    #[test]
    fn resolve_shader_hot_reload_prefers_the_environment_when_set() {
        let unset = EnvConfig {
            shader_hot_reload: None,
            ..EnvConfig::default()
        };
        assert!(unset.resolve_shader_hot_reload(true));
        assert!(!unset.resolve_shader_hot_reload(false));

        let forced_on = EnvConfig {
            shader_hot_reload: Some(true),
            ..EnvConfig::default()
        };
        assert!(forced_on.resolve_shader_hot_reload(false));

        let forced_off = EnvConfig {
            shader_hot_reload: Some(false),
            ..EnvConfig::default()
        };
        assert!(!forced_off.resolve_shader_hot_reload(true));
    }
}
