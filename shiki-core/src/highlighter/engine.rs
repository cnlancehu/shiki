use std::{
    collections::HashMap,
    sync::{
        Arc, OnceLock,
        atomic::{AtomicU64, Ordering},
    },
};

use super::instance::Highlighter;
use crate::{
    definition::LanguageDefinition,
    error::{Error, Result},
    grammar::{
        CompiledGrammar, GrammarResolver, PatternInterner, RawGrammar, compile,
        compile_plain_text,
    },
    theme::{FontStyle, Theme},
    tokenizer::{
        GrammarState, RegexCacheStats, RegexLimits, RegexPool, ScopeStackId,
        ScopeToken, ThemeId, Tokenizer, TokenizerCacheStats,
    },
};

#[derive(Clone)]
pub(crate) struct NamedTheme {
    pub(crate) name: String,
    pub(crate) css_name: String,
    pub(crate) theme: Arc<Theme>,
}

#[derive(Clone)]
pub(super) enum CatalogEntry {
    Bundled(&'static LanguageDefinition),
    Runtime(Arc<RawGrammar<'static>>),
}

pub(super) struct GrammarCatalog {
    pub(super) by_scope: HashMap<String, CatalogEntry>,
    pub(super) pattern_interner: PatternInterner,
}

impl GrammarResolver for GrammarCatalog {
    fn get(&self, scope: &str) -> Option<&RawGrammar<'static>> {
        match self.by_scope.get(scope)? {
            CatalogEntry::Bundled(definition) => Some(definition.grammar()),
            CatalogEntry::Runtime(grammar) => Some(grammar),
        }
    }
}

enum GrammarSource {
    Eager,
    Snapshot(crate::snapshot::GrammarSnapshot),
    Catalog {
        catalog: Arc<GrammarCatalog>,
        injections: Arc<HashMap<String, Vec<String>>>,
    },
    PlainText,
}

pub(crate) struct CompiledLanguage {
    scope_name: Arc<str>,
    grammar: OnceLock<std::result::Result<Arc<CompiledGrammar>, Arc<str>>>,
    source: GrammarSource,
    pub(super) grammar_id: u64,
}

impl CompiledLanguage {
    pub(super) fn eager(grammar: CompiledGrammar) -> Self {
        Self {
            scope_name: Arc::from("<eager>"),
            grammar: OnceLock::from(Ok(Arc::new(grammar))),
            source: GrammarSource::Eager,
            grammar_id: NEXT_GRAMMAR_ID.fetch_add(1, Ordering::Relaxed),
        }
    }

    pub(super) fn lazy(snapshot: crate::snapshot::GrammarSnapshot) -> Self {
        Self {
            scope_name: Arc::from("<snapshot>"),
            grammar: OnceLock::new(),
            source: GrammarSource::Snapshot(snapshot),
            grammar_id: NEXT_GRAMMAR_ID.fetch_add(1, Ordering::Relaxed),
        }
    }

    pub(super) fn catalog(
        scope_name: impl Into<Arc<str>>,
        catalog: Arc<GrammarCatalog>,
        injections: Arc<HashMap<String, Vec<String>>>,
    ) -> Self {
        Self {
            scope_name: scope_name.into(),
            grammar: OnceLock::new(),
            source: GrammarSource::Catalog {
                catalog,
                injections,
            },
            grammar_id: NEXT_GRAMMAR_ID.fetch_add(1, Ordering::Relaxed),
        }
    }

    pub(super) fn plain_text() -> Self {
        Self {
            scope_name: Arc::from("text.plain"),
            grammar: OnceLock::new(),
            source: GrammarSource::PlainText,
            grammar_id: NEXT_GRAMMAR_ID.fetch_add(1, Ordering::Relaxed),
        }
    }

    pub(crate) fn grammar(&self) -> Result<Arc<CompiledGrammar>> {
        match self.grammar.get_or_init(|| {
            let grammar = match &self.source {
                GrammarSource::Eager => {
                    unreachable!("eager grammars initialize their cache")
                }
                GrammarSource::Snapshot(snapshot) => {
                    snapshot.decode().map_err(|error| error.to_string())
                }
                GrammarSource::Catalog {
                    catalog,
                    injections,
                } => compile(
                    &self.scope_name,
                    catalog.as_ref(),
                    injections,
                    &catalog.pattern_interner,
                )
                .map_err(|error| error.to_string()),
                GrammarSource::PlainText => Ok(compile_plain_text()),
            };
            grammar.map(Arc::new).map_err(Arc::from)
        }) {
            Ok(grammar) => Ok(grammar.clone()),
            Err(message) => Err(Error::GrammarInitialization {
                grammar: self.scope_name.to_string(),
                message: message.to_string(),
            }),
        }
    }

    pub(crate) fn grammar_for_snapshot(&self) -> Arc<CompiledGrammar> {
        self.grammar().unwrap_or_else(|error| {
            panic!("cannot encode an invalid shiki grammar: {error}")
        })
    }

    fn is_loaded(&self) -> bool {
        matches!(self.grammar.get(), Some(Ok(_)))
    }
}

pub(crate) struct EngineInner {
    pub(crate) languages: HashMap<String, usize>,
    pub(crate) compiled: Vec<CompiledLanguage>,
    pub(crate) themes: Vec<NamedTheme>,
    pub(crate) regex_limits: RegexLimits,
    pub(crate) regex_pool: Arc<RegexPool>,
}

#[derive(Clone)]
pub struct HighlighterEngine {
    pub(crate) inner: Arc<EngineInner>,
}

pub struct LanguageSession {
    tokenizer: Tokenizer,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResolvedStyle<'a> {
    pub color: &'a str,
    pub background: Option<&'a str>,
    pub font_style: FontStyle,
}

/// Read-only theme metadata exposed to renderers.
#[derive(Debug, Clone, Copy)]
pub struct ThemeInfo<'a> {
    pub id: ThemeId,
    pub name: &'a str,
    pub css_name: &'a str,
    pub theme: &'a Theme,
}

static NEXT_GRAMMAR_ID: AtomicU64 = AtomicU64::new(1);

impl HighlighterEngine {
    pub fn language_keys(&self) -> impl Iterator<Item = &str> {
        self.inner.languages.keys().map(String::as_str)
    }

    pub fn language_count(&self) -> usize {
        self.inner.compiled.len()
    }

    /// Returns the number of grammar IRs currently resident in memory.
    ///
    /// Both bundled runtime catalogs and precompiled engines load grammar IR
    /// lazily on first use.
    pub fn loaded_language_count(&self) -> usize {
        self.inner
            .compiled
            .iter()
            .filter(|language| language.is_loaded())
            .count()
    }

    /// Reports the engine-wide cache of immutable Oniguruma programs.
    ///
    /// Static grammar regexes are compiled once per engine and shared by all
    /// highlighters and sessions. Dynamic back-reference patterns remain
    /// session-local and are intentionally excluded.
    pub fn regex_cache_stats(&self) -> RegexCacheStats {
        self.inner.regex_pool.stats()
    }

    /// Drops engine-owned regex cache entries.
    ///
    /// Existing sessions keep their programs alive and remain valid. A future
    /// session may compile a removed pattern again.
    pub fn clear_regex_cache(&self) -> usize {
        self.inner.regex_pool.clear()
    }

    pub fn theme_names(&self) -> impl Iterator<Item = &str> {
        self.inner.themes.iter().map(|theme| theme.name.as_str())
    }

    pub fn themes(&self) -> impl ExactSizeIterator<Item = ThemeInfo<'_>> {
        self.inner
            .themes
            .iter()
            .enumerate()
            .map(|(id, named)| ThemeInfo {
                id: id.try_into().expect("too many themes"),
                name: &named.name,
                css_name: &named.css_name,
                theme: &named.theme,
            })
    }

    pub fn theme_name(&self, theme: ThemeId) -> Option<&str> {
        self.inner
            .themes
            .get(theme as usize)
            .map(|theme| theme.name.as_str())
    }

    pub fn theme_css_name(&self, theme: ThemeId) -> Option<&str> {
        self.inner
            .themes
            .get(theme as usize)
            .map(|theme| theme.css_name.as_str())
    }

    pub fn theme(&self, theme: ThemeId) -> Option<&Theme> {
        self.inner
            .themes
            .get(theme as usize)
            .map(|theme| theme.theme.as_ref())
    }

    pub fn highlighter(&self) -> Highlighter {
        Highlighter {
            engine: self.clone(),
            tokenizers: (0..self.inner.compiled.len()).map(|_| None).collect(),
        }
    }

    pub fn session(&self, language: &str) -> Result<LanguageSession> {
        let index = self
            .inner
            .languages
            .get(language)
            .copied()
            .ok_or_else(|| Error::GrammarNotLoaded(language.to_owned()))?;
        let compiled = &self.inner.compiled[index];
        let themes = self
            .inner
            .themes
            .iter()
            .map(|theme| theme.theme.clone())
            .collect();
        Ok(LanguageSession {
            tokenizer: Tokenizer::new(
                compiled.grammar()?,
                self.inner.regex_pool.clone(),
                compiled.grammar_id,
                themes,
                self.inner.regex_limits,
            ),
        })
    }

    #[doc(hidden)]
    pub fn __to_snapshot(&self) -> Vec<u8> {
        crate::snapshot::encode(self)
    }

    #[doc(hidden)]
    pub fn __from_snapshot(source: &[u8]) -> Self {
        let parts =
            crate::snapshot::decode_owned(source).unwrap_or_else(|error| {
                panic!("invalid shiki precompiled snapshot: {error}")
            });
        Self::from_snapshot_parts(
            parts.languages,
            parts.grammars,
            parts.themes,
            parts.regex_limits,
        )
    }

    #[doc(hidden)]
    pub fn __from_static_snapshot(source: &'static [u8]) -> Self {
        let parts =
            crate::snapshot::decode_static(source).unwrap_or_else(|error| {
                panic!("invalid shiki precompiled snapshot: {error}")
            });
        Self::from_snapshot_parts(
            parts.languages,
            parts.grammars,
            parts.themes,
            parts.regex_limits,
        )
    }

    #[doc(hidden)]
    pub fn __from_rust_parts(
        languages: Vec<(&'static str, usize)>,
        grammars: Vec<CompiledGrammar>,
        themes: Vec<(&'static str, &'static str, Theme)>,
        regex_limits: RegexLimits,
    ) -> Self {
        Self::from_owned_parts(
            languages
                .into_iter()
                .map(|(name, index)| (name.to_owned(), index))
                .collect(),
            grammars,
            themes
                .into_iter()
                .map(|(name, css_name, theme)| {
                    (name.to_owned(), css_name.to_owned(), theme)
                })
                .collect(),
            regex_limits,
        )
    }

    fn from_owned_parts(
        languages: Vec<(String, usize)>,
        grammars: Vec<CompiledGrammar>,
        themes: Vec<(String, String, Theme)>,
        regex_limits: RegexLimits,
    ) -> Self {
        crate::snapshot::validate_owned_parts(&languages, &grammars, &themes)
            .unwrap_or_else(|error| {
                panic!("invalid shiki Rust parts: {error}")
            });
        let compiled =
            grammars.into_iter().map(CompiledLanguage::eager).collect();
        let themes = themes
            .into_iter()
            .map(|(name, css_name, theme)| NamedTheme {
                name,
                css_name,
                theme: Arc::new(theme),
            })
            .collect();
        Self {
            inner: Arc::new(EngineInner {
                languages: languages.into_iter().collect(),
                compiled,
                themes,
                regex_limits,
                regex_pool: Arc::new(RegexPool::default()),
            }),
        }
    }

    fn from_snapshot_parts(
        languages: Vec<(String, usize)>,
        grammars: Vec<crate::snapshot::GrammarSnapshot>,
        themes: Vec<(String, String, Theme)>,
        regex_limits: RegexLimits,
    ) -> Self {
        let compiled =
            grammars.into_iter().map(CompiledLanguage::lazy).collect();
        let themes = themes
            .into_iter()
            .map(|(name, css_name, theme)| NamedTheme {
                name,
                css_name,
                theme: Arc::new(theme),
            })
            .collect();
        Self {
            inner: Arc::new(EngineInner {
                languages: languages.into_iter().collect(),
                compiled,
                themes,
                regex_limits,
                regex_pool: Arc::new(RegexPool::default()),
            }),
        }
    }
}

impl LanguageSession {
    pub fn initial_state(&self) -> GrammarState {
        self.tokenizer.initial_state()
    }

    pub fn cache_stats(&self) -> TokenizerCacheStats {
        self.tokenizer.cache_stats()
    }

    pub fn scope_names(&self, stack: ScopeStackId) -> Result<Vec<String>> {
        self.tokenizer.scope_names(stack)
    }

    pub fn tokenize_line(
        &mut self,
        line: &str,
        state: &mut GrammarState,
        is_first_line: bool,
    ) -> Result<Vec<ScopeToken>> {
        let mut output = Vec::new();
        self.tokenize_line_into(line, state, is_first_line, &mut output)?;
        Ok(output)
    }

    /// Tokenizes one line into a reusable caller-owned buffer.
    pub fn tokenize_line_into(
        &mut self,
        line: &str,
        state: &mut GrammarState,
        is_first_line: bool,
        output: &mut Vec<ScopeToken>,
    ) -> Result<()> {
        self.tokenizer.validate_state(state)?;
        let previous = std::mem::take(state);
        *state = self.tokenizer.tokenize_line_into_owned(
            line,
            Some(previous),
            is_first_line,
            output,
        )?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::HighlighterEngine;
    use crate::{
        Highlighter, RawGrammar, RawList, RawMap, RawRule, RawString, RawTheme,
        RegexLimits,
        grammar::compile_plain_text,
        theme::{ColorId, Theme},
    };

    fn grammar(scope_name: &'static str) -> RawGrammar<'static> {
        RawGrammar {
            name: None,
            scope_name: RawString::borrowed(scope_name),
            patterns: RawList::from(vec![RawRule {
                match_pattern: Some(RawString::borrowed(r"\bshared-pattern\b")),
                ..RawRule::EMPTY
            }]),
            repository: RawMap::EMPTY,
            injections: RawMap::EMPTY,
            injection_selector: None,
        }
    }

    fn theme() -> RawTheme<'static> {
        RawTheme {
            name: None,
            fg: None,
            bg: None,
            colors: RawMap::EMPTY,
            settings: RawList::EMPTY,
            token_colors: RawList::EMPTY,
        }
    }

    fn from_rust_parts(
        grammar: crate::grammar::CompiledGrammar,
        theme: Theme,
        language_index: usize,
    ) -> HighlighterEngine {
        HighlighterEngine::__from_rust_parts(
            vec![("test", language_index)],
            vec![grammar],
            vec![("test", "test", theme)],
            RegexLimits::default(),
        )
    }

    #[test]
    fn runtime_catalog_shares_pattern_allocations_between_roots() {
        let engine = Highlighter::builder()
            .language("first", grammar("source.first"))
            .language("second", grammar("source.second"))
            .theme(theme())
            .build_engine()
            .unwrap();
        let first = engine.inner.languages["first"];
        let second = engine.inner.languages["second"];
        let first = engine.inner.compiled[first].grammar().unwrap();
        let second = engine.inner.compiled[second].grammar().unwrap();

        assert_eq!(first.patterns.len(), 1);
        assert_eq!(second.patterns.len(), 1);
        assert!(Arc::ptr_eq(&first.patterns[0], &second.patterns[0]));
    }

    #[test]
    #[should_panic(
        expected = "invalid shiki Rust parts: snapshot contains an invalid language index"
    )]
    fn rust_parts_reject_an_invalid_language_index() {
        from_rust_parts(
            compile_plain_text(),
            Theme::from_raw("test", &theme()),
            1,
        );
    }

    #[test]
    #[should_panic(
        expected = "invalid shiki Rust parts: grammar snapshot contains an invalid root rule ID"
    )]
    fn rust_parts_reject_invalid_grammar_cross_references() {
        let mut grammar = compile_plain_text();
        grammar.root = u32::MAX;
        from_rust_parts(grammar, Theme::from_raw("test", &theme()), 0);
    }

    #[test]
    #[should_panic(
        expected = "invalid shiki Rust parts: snapshot contains an invalid theme color ID"
    )]
    fn rust_parts_reject_invalid_theme_cross_references() {
        let mut compiled_theme = Theme::from_raw("test", &theme());
        compiled_theme.foreground_id = ColorId(u32::MAX);
        from_rust_parts(compile_plain_text(), compiled_theme, 0);
    }
}
