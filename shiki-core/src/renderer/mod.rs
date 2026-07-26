use crate::{Highlighter, Result};

mod html;

pub use html::{HtmlOptions, HtmlRenderer};

/// Renders highlighted source code into a concrete output format.
///
/// Custom renderers can use [`Highlighter::tokenize_line_into`] with a reused
/// token buffer and [`Highlighter::token_styles`] to stream output without
/// materializing an owned themed-token document. ANSI renderers can use
/// [`crate::ansi::AnsiParser`] directly.
pub trait Renderer {
    type Output;

    fn render(
        &mut self,
        highlighter: &mut Highlighter,
        code: &str,
        language: &str,
    ) -> Result<Self::Output>;
}
