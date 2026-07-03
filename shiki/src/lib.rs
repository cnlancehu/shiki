//! Native TextMate syntax highlighting with compile-time language bundles.
//!
//! The crate compiles TextMate grammars into numeric runtime structures and
//! uses Oniguruma for regex matching. Use [`Highlighter::builder`] with a
//! [`LanguageBundle`] and one or more [`ThemeDefinition`] values. The resulting
//! highlighter can stream HTML, return themed tokens, or preserve
//! [`GrammarState`] between lines for incremental highlighting.
//!
//! Bundled language and theme definitions are provided by the companion
//! `shiki-langs` and `shiki-themes` crates.

mod definition;
mod error;
mod grammar;
mod highlighter;
mod matcher;
mod theme;
mod tokenizer;

pub use definition::{
    LanguageBundle, LanguageDefinition, LanguageGroup, ThemeBundle, ThemeDefinition,
};
pub use error::{Error, Result};
pub use grammar::{RawGrammar, RawRule, StaticRawGrammar, StaticRawMapEntry, StaticRawRule};
pub use highlighter::{Highlighter, HighlighterBuilder, HtmlOptions, RawLanguage};
pub use theme::{
    FontStyle, RawTheme, RawThemeRule, RawThemeScope, RawThemeSettings, StaticRawTheme,
    StaticRawThemeRule, StaticRawThemeScope, StaticRawThemeSettings, Theme,
};
pub use tokenizer::{
    GrammarState, MultiThemedToken, ScopeStackId, ScopeToken, ThemeId, ThemeTokenStyle, ThemedToken,
};
