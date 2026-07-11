#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("language `{0}` is not present in this bundle")]
    LanguageNotBundled(String),
    #[error("language dependency `{dependency}` required by `{language}` is not bundled")]
    MissingDependency {
        language: String,
        dependency: String,
    },
    #[error("grammar `{0}` was not loaded")]
    GrammarNotLoaded(String),
    #[error("grammar state belongs to a different language session")]
    GrammarStateMismatch,
    #[error("scope stack ID {0} does not belong to this language session")]
    InvalidScopeStack(u32),
    #[error("theme `{0}` is not present in this bundle")]
    ThemeNotBundled(String),
    #[error("theme ID {0} does not belong to this highlighter")]
    InvalidThemeId(u16),
    #[error("invalid grammar JSON for `{name}`: {source}")]
    InvalidGrammar {
        name: String,
        source: serde_json::Error,
    },
    #[error("invalid theme JSON for `{name}`: {source}")]
    InvalidTheme {
        name: String,
        source: serde_json::Error,
    },
    #[error("invalid regular expression `{pattern}`: {message}")]
    InvalidRegex { pattern: String, message: String },
    #[error("regular expression search failed: {message}")]
    RegexSearch { message: String },
    #[error("invalid precompiled highlighter: {0}")]
    InvalidPrecompiled(String),
    #[error("grammar include `{include}` could not be resolved in `{grammar}`")]
    UnresolvedInclude { grammar: String, include: String },
    #[error("no language has been selected")]
    NoLanguage,
    #[error("no theme has been selected")]
    NoTheme,
    #[error("theme name `{0}` is duplicated after CSS normalization")]
    DuplicateTheme(String),
}

pub type Result<T, E = Error> = std::result::Result<T, E>;
