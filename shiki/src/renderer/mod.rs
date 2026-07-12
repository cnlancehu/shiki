use crate::{Highlighter, Result};

mod html;

pub use html::{HtmlOptions, HtmlRenderer};

/// Renders highlighted source code into a concrete output format.
///
/// Renderers may use the high-level token APIs on [`Highlighter`]. Renderers
/// implemented by this crate can additionally use its streaming internals to
/// avoid materializing owned tokens.
pub trait Renderer {
    type Output;

    fn render(
        &mut self,
        highlighter: &mut Highlighter,
        code: &str,
        language: &str,
    ) -> Result<Self::Output>;
}
