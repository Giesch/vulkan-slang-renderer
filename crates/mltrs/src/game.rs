pub(crate) mod traits;

pub use traits::{Game, Input, Key, MouseButton, WindowDescription};

// defined in the renderer crate (it needs it for swapchain setup), re-exported
// here so `Game::max_msaa_samples` reads from one place
pub use crate::renderer::MaxMSAASamples;
