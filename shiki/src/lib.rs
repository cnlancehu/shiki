//! Native TextMate syntax highlighting with Shiki-compatible language and theme bundles.
//!
//! Grammars are compiled into numeric runtime structures and matched with Oniguruma.
//! Bundled definitions live in the companion `shiki-langs` and `shiki-themes` crates;
//! runtime TextMate JSON is supported through [`HighlighterBuilder::json_language`] and
//! [`HighlighterBuilder::json_theme`]. Reuse a [`Highlighter`] or [`HighlighterEngine`]
//! because compiled scanners and scope/style transitions are cached on demand.
//!
//! # Basic highlighting
//!
//! ```ignore
//! use shiki::{Highlighter, LanguageBundle};
//!
//! static LANGUAGES: LanguageBundle = shiki_langs::languages![rust];
//!
//! let mut highlighter = Highlighter::builder()
//!     .bundle(&LANGUAGES)
//!     .languages(["rust"])
//!     .theme(&shiki_themes::CATPPUCCIN_MOCHA)
//!     .build()?;
//!
//! let html = highlighter.code_to_html("fn main() {}", "rust")?;
//! assert!(html.contains("fn"));
//! # Ok::<(), shiki::Error>(())
//! ```
//!
//! # Enabling every bundled language
//!
//! `shiki_langs::all()` provides the convenient all-language bundle. Building it is
//! intentionally more expensive, so applications should normally create one shared engine.
//!
//! ```ignore
//! use shiki::Highlighter;
//!
//! let languages = shiki_langs::all();
//! let engine = Highlighter::builder()
//!     .bundle(&languages)
//!     .theme(&shiki_themes::CATPPUCCIN_MOCHA)
//!     .build_engine()?;
//!
//! let mut highlighter = engine.highlighter();
//! let html = highlighter.code_to_html("const value = 1", "javascript")?;
//! # Ok::<(), shiki::Error>(())
//! ```
//!
//! # Incremental documents and parallel sessions
//!
//! An engine shares immutable grammar IR and themes. Each session owns its dynamic scanner,
//! scope and style caches, so documents can advance independently.
//!
//! ```ignore
//! use shiki::{Highlighter, LanguageBundle};
//!
//! static LANGUAGES: LanguageBundle = shiki_langs::languages![rust];
//! let engine = Highlighter::builder()
//!     .bundle(&LANGUAGES)
//!     .languages(["rust"])
//!     .theme(&shiki_themes::GITHUB_DARK)
//!     .build_engine()?;
//!
//! let mut session = engine.session("rust")?;
//! let mut state = session.initial_state();
//! let tokens = session.tokenize_line("/* open", &mut state, true)?;
//! assert!(!tokens.is_empty());
//! # Ok::<(), shiki::Error>(())
//! ```
//!
//! # Custom output formats
//!
//! [`Renderer`] separates tokenization from output selection. [`HtmlRenderer`] is the
//! built-in streaming implementation; additional renderers can choose the scope-token or
//! themed-token APIs depending on whether they need borrowed styles or owned output.
//!
//! ```ignore
//! use shiki::{Highlighter, HtmlOptions, HtmlRenderer};
//!
//! # let mut highlighter: Highlighter = todo!();
//! let options = HtmlOptions::default().without_line_wrapper();
//! let mut renderer = HtmlRenderer::new(&options);
//! let html = highlighter.render("let value = 1", "rust", &mut renderer)?;
//! # Ok::<(), shiki::Error>(())
//! ```

mod definition;
mod error;
mod grammar;
mod highlighter;
mod matcher;
mod raw;
mod renderer;
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
    LanguageSession, ResolvedStyle, ThemeInput,
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
