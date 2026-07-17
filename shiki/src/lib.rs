#![doc = include_str!("../README.md")]

pub mod ansi;
mod definition;
mod error;
mod grammar;
mod highlighter;
mod matcher;
mod raw;
pub mod renderer;
mod snapshot;
mod theme;
mod tokenizer;

pub use definition::{
    LanguageBundle, LanguageDefinition, LanguageGroup, ThemeBundle,
    ThemeDefinition,
};
pub use error::{Error, Result};
pub use grammar::{RawGrammar, RawRule};
pub use highlighter::{
    Highlighter, HighlighterBuilder, HighlighterEngine, LanguageInput,
    LanguageSession, ResolvedStyle, ThemeInfo, ThemeInput, split_lines,
};
pub use raw::{RawList, RawMap, RawMapEntry, RawString};
pub use renderer::{HtmlOptions, HtmlRenderer, Renderer};
pub use theme::{
    FontStyle, RawTheme, RawThemeRule, RawThemeScope, RawThemeSettings, Theme,
};
pub use tokenizer::{
    GrammarState, MultiThemedToken, RegexLimits, ScopeStackId, ScopeToken,
    ThemeId, ThemeTokenStyle, ThemedToken, TokenizerCacheStats,
};

#[doc(hidden)]
pub mod __private {
    pub use crate::{
        grammar::{
            Capture, CompiledGrammar, Injection, Rule, RuleKind, ScopeName,
            ScopePart, ScopeTemplate,
        },
        matcher::{Expression, Priority, ScopeSelector},
        theme::{ColorId, Style, Theme, ThemeRule},
    };
}
