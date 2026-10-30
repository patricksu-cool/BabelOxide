//! One native render thread; only owned RGBA bytes cross into the GUI.
mod native;
mod platform;
mod queue;
mod types;

pub use queue::{PreviewRenderer, Wakeup};
pub use types::{
    MAX_RENDER_EDGE, PreviewError, PreviewErrorKind, PreviewFailure, PreviewFrame, PreviewRequest,
    PreviewResult, PreviewTicket, RenderSize, render_size,
};

#[cfg(test)]
mod tests;
