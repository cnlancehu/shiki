mod builder;
mod engine;
mod instance;

#[allow(unused_imports)]
pub use builder::{HighlighterBuilder, LanguageInput, ThemeInput, css_name};
#[allow(unused_imports)]
pub(crate) use engine::{CompiledLanguage, EngineInner, NamedTheme};
pub use engine::{
    HighlighterEngine, LanguageSession, ResolvedStyle, ThemeInfo,
};
pub use instance::{Highlighter, split_lines};
