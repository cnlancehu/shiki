use std::{
    collections::{HashMap, HashSet},
    sync::{
        Arc, OnceLock,
        atomic::{AtomicU64, Ordering},
    },
};

use crate::{
    ansi::AnsiParser,
    definition::{LanguageBundle, ThemeDefinition, is_plain_text},
    error::{Error, Result},
    grammar::{RawGrammar, compile, compile_plain_text},
    renderer::{HtmlOptions, HtmlRenderer, Renderer},
    theme::{FontStyle, RawTheme, Style, Theme},
    tokenizer::{
        GrammarState, MultiThemedToken, RegexLimits, ScopeStackId, ScopeToken,
        ThemeId, ThemeTokenStyle, ThemedToken, Tokenizer, TokenizerCacheStats,
    },
};

#[derive(Clone)]
pub(crate) struct NamedTheme {
    pub(crate) name: String,
    pub(crate) css_name: String,
    pub(crate) theme: Arc<Theme>,
}

pub(crate) struct CompiledLanguage {
    grammar: OnceLock<Arc<crate::grammar::CompiledGrammar>>,
    snapshot: Option<crate::snapshot::GrammarSnapshot>,
    grammar_id: u64,
}

impl CompiledLanguage {
    fn eager(grammar: crate::grammar::CompiledGrammar) -> Self {
        Self {
            grammar: OnceLock::from(Arc::new(grammar)),
            snapshot: None,
            grammar_id: NEXT_GRAMMAR_ID.fetch_add(1, Ordering::Relaxed),
        }
    }

    fn lazy(snapshot: crate::snapshot::GrammarSnapshot) -> Self {
        Self {
            grammar: OnceLock::new(),
            snapshot: Some(snapshot),
            grammar_id: NEXT_GRAMMAR_ID.fetch_add(1, Ordering::Relaxed),
        }
    }

    pub(crate) fn grammar(&self) -> Arc<crate::grammar::CompiledGrammar> {
        self.grammar
            .get_or_init(|| {
                Arc::new(
                    self.snapshot
                        .as_ref()
                        .expect("compiled language has neither grammar nor snapshot")
                        .decode()
                        .unwrap_or_else(|error| {
                            panic!("invalid shiki grammar snapshot: {error}")
                        }),
                )
            })
            .clone()
    }

    fn is_loaded(&self) -> bool {
        self.grammar.get().is_some()
    }
}

pub(crate) struct EngineInner {
    pub(crate) languages: HashMap<String, usize>,
    pub(crate) compiled: Vec<CompiledLanguage>,
    pub(crate) themes: Vec<NamedTheme>,
    pub(crate) regex_limits: RegexLimits,
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

pub struct Highlighter {
    pub(crate) engine: HighlighterEngine,
    pub(crate) tokenizers: Vec<Option<Tokenizer>>,
}

pub struct HighlighterBuilder {
    bundle: Option<LanguageBundle>,
    languages: Vec<String>,
    runtime_languages: Vec<LanguageInput>,
    themes: Vec<(String, ThemeInput)>,
    regex_limits: RegexLimits,
}

impl Default for HighlighterBuilder {
    fn default() -> Self {
        Highlighter::builder()
    }
}

pub struct LanguageInput {
    pub id: String,
    pub aliases: Vec<String>,
    pub inject_to: Vec<String>,
    pub grammar: RawGrammar<'static>,
}

impl LanguageInput {
    pub fn new(id: impl Into<String>, grammar: RawGrammar<'static>) -> Self {
        Self {
            id: id.into(),
            aliases: Vec::new(),
            inject_to: Vec::new(),
            grammar,
        }
    }

    pub fn alias(mut self, alias: impl Into<String>) -> Self {
        self.aliases.push(alias.into());
        self
    }

    pub fn inject_to(mut self, scope: impl Into<String>) -> Self {
        self.inject_to.push(scope.into());
        self
    }
}

pub enum ThemeInput {
    Definition(&'static ThemeDefinition),
    Raw(RawTheme<'static>),
}

impl From<&'static ThemeDefinition> for ThemeInput {
    fn from(value: &'static ThemeDefinition) -> Self {
        Self::Definition(value)
    }
}

impl From<RawTheme<'static>> for ThemeInput {
    fn from(value: RawTheme<'static>) -> Self {
        Self::Raw(value)
    }
}

impl Highlighter {
    pub fn builder() -> HighlighterBuilder {
        HighlighterBuilder {
            bundle: None,
            languages: Vec::new(),
            runtime_languages: Vec::new(),
            themes: Vec::new(),
            regex_limits: RegexLimits::default(),
        }
    }

    pub const fn engine(&self) -> &HighlighterEngine {
        &self.engine
    }

    pub fn theme_names(&self) -> impl Iterator<Item = &str> {
        self.engine.theme_names()
    }

    pub fn themes(&self) -> impl ExactSizeIterator<Item = ThemeInfo<'_>> {
        self.engine.themes()
    }

    pub fn theme_name(&self, theme: ThemeId) -> Option<&str> {
        self.engine.theme_name(theme)
    }

    pub fn theme_css_name(&self, theme: ThemeId) -> Option<&str> {
        self.engine.theme_css_name(theme)
    }

    pub fn theme(&self, theme: ThemeId) -> Option<&Theme> {
        self.engine.theme(theme)
    }

    pub fn token_style(
        &mut self,
        language: &str,
        scopes: crate::tokenizer::ScopeStackId,
        theme_id: ThemeId,
    ) -> Result<ResolvedStyle<'_>> {
        self.token_styles(language, scopes)?
            .nth(theme_id as usize)
            .map(|(_, style)| style)
            .ok_or(Error::InvalidThemeId(theme_id))
    }

    /// Resolves a scope stack for every configured theme without allocating.
    ///
    /// This is the preferred styling primitive for custom streaming renderers.
    pub fn token_styles<'a>(
        &'a mut self,
        language: &str,
        scopes: ScopeStackId,
    ) -> Result<impl ExactSizeIterator<Item = (ThemeId, ResolvedStyle<'a>)> + 'a>
    {
        let language = self.language_index(language)?;
        self.ensure_tokenizer(language);
        let themes = &self.engine.inner.themes;
        let tokenizer = self.tokenizers[language]
            .as_mut()
            .expect("initialized tokenizer");
        if !tokenizer.contains_scope_stack(scopes) {
            return Err(Error::InvalidScopeStack(scopes));
        }
        let styles = tokenizer.styles(scopes);
        Ok(themes.iter().zip(styles).enumerate().map(
            |(theme_id, (theme, style))| {
                (
                    theme_id.try_into().expect("too many themes"),
                    ResolvedStyle {
                        color: theme.theme.color(
                            style
                                .foreground
                                .unwrap_or_else(|| theme.theme.foreground_id()),
                        ),
                        background: style
                            .background
                            .map(|color| theme.theme.color(color)),
                        font_style: style.font_style.unwrap_or_default(),
                    },
                )
            },
        ))
    }

    pub fn clear_language_cache(&mut self, language: &str) -> Result<()> {
        let index = self.language_index(language)?;
        self.tokenizers[index] = None;
        Ok(())
    }

    pub fn clear_all_caches(&mut self) {
        *self = self.engine.highlighter();
    }

    pub fn is_language_initialized(&self, language: &str) -> Result<bool> {
        Ok(self.tokenizers[self.language_index(language)?].is_some())
    }

    pub fn initialized_language_count(&self) -> usize {
        self.tokenizers
            .iter()
            .filter(|tokenizer| tokenizer.is_some())
            .count()
    }

    pub fn cache_stats(
        &self,
        language: &str,
    ) -> Result<Option<TokenizerCacheStats>> {
        let index = self.language_index(language)?;
        Ok(self.tokenizers[index].as_ref().map(Tokenizer::cache_stats))
    }

    pub fn initial_state(&mut self, language: &str) -> Result<GrammarState> {
        Ok(self.tokenizer(language)?.initial_state())
    }

    pub fn scope_names(
        &self,
        language: &str,
        stack: crate::tokenizer::ScopeStackId,
    ) -> Result<Vec<String>> {
        let index = self.language_index(language)?;
        self.tokenizers[index]
            .as_ref()
            .ok_or(Error::InvalidScopeStack(stack))?
            .scope_names(stack)
    }

    pub fn tokenize_line(
        &mut self,
        line: &str,
        language: &str,
        previous: Option<&GrammarState>,
        is_first_line: bool,
    ) -> Result<(Vec<ScopeToken>, GrammarState)> {
        self.tokenizer(language)?
            .tokenize_line(line, previous, is_first_line)
    }

    /// Tokenizes one line into a caller-owned buffer.
    ///
    /// The buffer is cleared and reused. `state` is advanced in place, which
    /// avoids cloning a grammar stack between lines.
    pub fn tokenize_line_into(
        &mut self,
        line: &str,
        language: &str,
        state: &mut GrammarState,
        is_first_line: bool,
        output: &mut Vec<ScopeToken>,
    ) -> Result<()> {
        let tokenizer = self.tokenizer(language)?;
        tokenizer.validate_state(state)?;
        let previous = std::mem::take(state);
        *state = tokenizer.tokenize_line_into_owned(
            line,
            Some(previous),
            is_first_line,
            output,
        )?;
        Ok(())
    }

    pub fn code_to_scope_tokens(
        &mut self,
        code: &str,
        language: &str,
    ) -> Result<Vec<Vec<ScopeToken>>> {
        let language = self.language_index(language)?;
        self.ensure_tokenizer(language);
        let tokenizer = self.tokenizers[language]
            .as_mut()
            .expect("initialized tokenizer");
        let mut state = None;
        let mut output = Vec::new();
        for (index, line) in split_lines(code).enumerate() {
            let (tokens, next) = tokenizer.tokenize_line_owned(
                line,
                state.take(),
                index == 0,
            )?;
            output.push(tokens);
            state = Some(next);
        }
        Ok(output)
    }

    pub fn code_to_tokens(
        &mut self,
        code: &str,
        language: &str,
    ) -> Result<Vec<Vec<ThemedToken>>> {
        if crate::ansi::is_ansi(language) {
            return Ok(self.code_to_ansi_tokens(code));
        }
        let language = self.language_index(language)?;
        self.ensure_tokenizer(language);
        let themes = &self.engine.inner.themes;
        let tokenizer = self.tokenizers[language]
            .as_mut()
            .expect("initialized tokenizer");
        map_grammar_tokens(
            tokenizer,
            themes,
            code,
            |line, token, styles, themes| {
                let theme = &themes[0].theme;
                let style = styles[0];
                ThemedToken {
                    content: line[token.range.clone()].to_owned(),
                    color: theme.color_arc(
                        style
                            .foreground
                            .unwrap_or_else(|| theme.foreground_id()),
                    ),
                    background: style
                        .background
                        .map(|color| theme.color_arc(color)),
                    font_style: style.font_style.unwrap_or_default(),
                    scopes: token.scopes,
                }
            },
        )
    }

    pub fn code_to_tokens_with_themes(
        &mut self,
        code: &str,
        language: &str,
    ) -> Result<Vec<Vec<MultiThemedToken>>> {
        if crate::ansi::is_ansi(language) {
            return Ok(self.code_to_ansi_tokens_with_themes(code));
        }
        let language = self.language_index(language)?;
        self.ensure_tokenizer(language);
        let themes = &self.engine.inner.themes;
        let tokenizer = self.tokenizers[language]
            .as_mut()
            .expect("initialized tokenizer");
        map_grammar_tokens(
            tokenizer,
            themes,
            code,
            |line, token, styles, themes| {
                let styles = themes
                    .iter()
                    .enumerate()
                    .zip(styles)
                    .map(|((theme_id, theme), style)| ThemeTokenStyle {
                        theme: theme_id.try_into().expect("too many themes"),
                        color: theme.theme.color_arc(
                            style
                                .foreground
                                .unwrap_or_else(|| theme.theme.foreground_id()),
                        ),
                        background: style
                            .background
                            .map(|color| theme.theme.color_arc(color)),
                        font_style: style.font_style.unwrap_or_default(),
                    })
                    .collect();
                MultiThemedToken {
                    content: line[token.range.clone()].to_owned(),
                    styles,
                    scopes: token.scopes,
                }
            },
        )
    }

    fn code_to_ansi_tokens(&self, code: &str) -> Vec<Vec<ThemedToken>> {
        let theme = &self.engine.inner.themes[0].theme;
        let mut parser = AnsiParser::new();
        split_lines(code)
            .map(|line| {
                let spans = parser.parse_line(line);
                let mut output: Vec<ThemedToken> =
                    Vec::with_capacity(spans.len());
                for span in spans {
                    let style = span.state.resolve(theme);
                    let content = &line[span.range.clone()];
                    if let Some(previous) = output.last_mut()
                        && previous.color == style.color
                        && previous.background == style.background
                        && previous.font_style == style.font_style
                    {
                        previous.content.push_str(content);
                    } else {
                        output.push(ThemedToken {
                            content: content.to_owned(),
                            color: style.color,
                            background: style.background,
                            font_style: style.font_style,
                            scopes: 0,
                        });
                    }
                }
                output
            })
            .collect()
    }

    fn code_to_ansi_tokens_with_themes(
        &self,
        code: &str,
    ) -> Vec<Vec<MultiThemedToken>> {
        let themes = &self.engine.inner.themes;
        let mut parser = AnsiParser::new();
        split_lines(code)
            .map(|line| {
                let spans = parser.parse_line(line);
                let mut output: Vec<MultiThemedToken> =
                    Vec::with_capacity(spans.len());
                for span in spans {
                    let styles = themes
                        .iter()
                        .enumerate()
                        .map(|(theme_id, theme)| {
                            let style = span.state.resolve(&theme.theme);
                            ThemeTokenStyle {
                                theme: theme_id
                                    .try_into()
                                    .expect("too many themes"),
                                color: style.color,
                                background: style.background,
                                font_style: style.font_style,
                            }
                        })
                        .collect::<Vec<_>>();
                    let content = &line[span.range.clone()];
                    if let Some(previous) = output.last_mut()
                        && previous.styles == styles
                    {
                        previous.content.push_str(content);
                    } else {
                        output.push(MultiThemedToken {
                            content: content.to_owned(),
                            styles,
                            scopes: 0,
                        });
                    }
                }
                output
            })
            .collect()
    }

    pub fn code_to_html(
        &mut self,
        code: &str,
        language: &str,
    ) -> Result<String> {
        let options = HtmlOptions::default();
        self.code_to_html_with_options(code, language, &options)
    }

    pub fn code_to_html_with_options(
        &mut self,
        code: &str,
        language: &str,
        options: &HtmlOptions,
    ) -> Result<String> {
        let mut renderer = HtmlRenderer::new(options);
        self.render(code, language, &mut renderer)
    }

    pub fn render<R: Renderer + ?Sized>(
        &mut self,
        code: &str,
        language: &str,
        renderer: &mut R,
    ) -> Result<R::Output> {
        renderer.render(self, code, language)
    }

    pub(crate) fn language_index(&self, language: &str) -> Result<usize> {
        self.engine
            .inner
            .languages
            .get(language)
            .copied()
            .ok_or_else(|| Error::GrammarNotLoaded(language.to_owned()))
    }

    fn tokenizer(&mut self, language: &str) -> Result<&mut Tokenizer> {
        let index = self.language_index(language)?;
        self.ensure_tokenizer(index);
        Ok(self.tokenizers[index]
            .as_mut()
            .expect("initialized tokenizer"))
    }

    pub(crate) fn ensure_tokenizer(&mut self, index: usize) {
        if self.tokenizers[index].is_some() {
            return;
        }
        let compiled = &self.engine.inner.compiled[index];
        let themes = self
            .engine
            .inner
            .themes
            .iter()
            .map(|theme| theme.theme.clone())
            .collect();
        self.tokenizers[index] = Some(Tokenizer::new(
            compiled.grammar(),
            compiled.grammar_id,
            themes,
            self.engine.inner.regex_limits,
        ));
    }
}

fn map_grammar_tokens<T>(
    tokenizer: &mut Tokenizer,
    themes: &[NamedTheme],
    code: &str,
    mut map: impl FnMut(&str, &ScopeToken, &[Style], &[NamedTheme]) -> T,
) -> Result<Vec<Vec<T>>> {
    let mut state = None;
    let mut scope_tokens = Vec::new();
    let mut output = Vec::new();
    for (line_index, line) in split_lines(code).enumerate() {
        let next = tokenizer.tokenize_line_into_owned(
            line,
            state.take(),
            line_index == 0,
            &mut scope_tokens,
        )?;
        state = Some(next);
        let mut output_line = Vec::with_capacity(scope_tokens.len());
        for token in &scope_tokens {
            let styles = tokenizer.styles(token.scopes);
            output_line.push(map(line, token, styles, themes));
        }
        output.push(output_line);
    }
    Ok(output)
}

impl HighlighterEngine {
    pub fn language_keys(&self) -> impl Iterator<Item = &str> {
        self.inner.languages.keys().map(String::as_str)
    }

    pub fn language_count(&self) -> usize {
        self.inner.compiled.len()
    }

    /// Returns the number of grammar IRs currently resident in memory.
    ///
    /// Precompiled engines load grammar IR lazily on first use. Engines built
    /// at runtime already contain all compiled grammar IRs.
    pub fn loaded_language_count(&self) -> usize {
        self.inner
            .compiled
            .iter()
            .filter(|language| language.is_loaded())
            .count()
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
                compiled.grammar(),
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
        grammars: Vec<crate::grammar::CompiledGrammar>,
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
        grammars: Vec<crate::grammar::CompiledGrammar>,
        themes: Vec<(String, String, Theme)>,
        regex_limits: RegexLimits,
    ) -> Self {
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

    pub fn scope_names(
        &self,
        stack: crate::tokenizer::ScopeStackId,
    ) -> Result<Vec<String>> {
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

impl HighlighterBuilder {
    pub fn bundle(mut self, bundle: &LanguageBundle) -> Self {
        self.bundle = Some(*bundle);
        self
    }

    pub fn languages<I, S>(mut self, languages: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.languages = languages.into_iter().map(Into::into).collect();
        self
    }

    pub fn theme(mut self, theme: impl Into<ThemeInput>) -> Self {
        self.themes = vec![("default".to_owned(), theme.into())];
        self
    }

    pub fn themes<I, S, T>(mut self, themes: I) -> Self
    where
        I: IntoIterator<Item = (S, T)>,
        S: Into<String>,
        T: Into<ThemeInput>,
    {
        self.themes = themes
            .into_iter()
            .map(|(name, theme)| (name.into(), theme.into()))
            .collect();
        self
    }

    pub fn regex_limits(mut self, limits: RegexLimits) -> Self {
        self.regex_limits = limits;
        self
    }

    pub fn language(
        mut self,
        id: impl Into<String>,
        grammar: RawGrammar<'static>,
    ) -> Self {
        self.runtime_languages.push(LanguageInput::new(id, grammar));
        self
    }

    pub fn language_definition(mut self, language: LanguageInput) -> Self {
        self.runtime_languages.push(language);
        self
    }

    #[cfg(feature = "json")]
    pub fn json_language(
        self,
        id: impl Into<String>,
        source: &str,
    ) -> Result<Self> {
        let id = id.into();
        let grammar = RawGrammar::from_json(&id, source)?;
        Ok(self.language(id, grammar))
    }

    #[cfg(feature = "json")]
    pub fn json_theme(
        mut self,
        name: impl Into<String>,
        source: &str,
    ) -> Result<Self> {
        let name = name.into();
        let theme = RawTheme::from_json(&name, source)?;
        self.themes.push((name, ThemeInput::Raw(theme)));
        Ok(self)
    }

    pub fn build(self) -> Result<Highlighter> {
        Ok(self.build_engine()?.highlighter())
    }

    pub fn build_engine(self) -> Result<HighlighterEngine> {
        let selected_special = !self.languages.is_empty()
            && self
                .languages
                .iter()
                .all(|id| is_plain_text(id) || crate::ansi::is_ansi(id));
        let (definitions, root_definitions) = match self.bundle {
            Some(bundle) => bundle.resolve(&self.languages)?,
            None if self.runtime_languages.is_empty() && !selected_special => {
                return Err(Error::NoLanguage);
            }
            None => (Vec::new(), Vec::new()),
        };
        if self.themes.is_empty() {
            return Err(Error::NoTheme);
        }

        let mut seen_themes = HashSet::new();
        let mut themes = Vec::with_capacity(self.themes.len());
        for (name, definition) in self.themes {
            let css_name = css_name(&name);
            if !seen_themes.insert(css_name.clone()) {
                return Err(Error::DuplicateTheme(name));
            }
            let theme = match definition {
                ThemeInput::Definition(definition) => definition.theme(),
                ThemeInput::Raw(raw) => Arc::new(Theme::from_raw(&name, &raw)),
            };
            themes.push(NamedTheme {
                name,
                css_name,
                theme,
            });
        }

        let static_languages = definitions
            .iter()
            .map(|definition| {
                (
                    definition.id.to_owned(),
                    definition.scope_name.to_owned(),
                    definition
                        .aliases
                        .iter()
                        .map(|alias| (*alias).to_owned())
                        .collect::<Vec<_>>(),
                    definition
                        .inject_to
                        .iter()
                        .map(|scope| (*scope).to_owned())
                        .collect::<Vec<_>>(),
                    definition.grammar(),
                )
            })
            .collect::<Vec<_>>();
        let runtime_languages =
            self.runtime_languages.into_iter().map(|language| {
                let scope_name =
                    language.grammar.scope_name.as_ref().to_owned();
                (
                    language.id,
                    scope_name,
                    language.aliases,
                    language.inject_to,
                    language.grammar,
                )
            });
        let mut grammars: HashMap<String, &RawGrammar<'static>> =
            HashMap::new();
        let mut injections: HashMap<String, Vec<String>> = HashMap::new();
        for (_, scope_name, _, inject_to, grammar) in &static_languages {
            grammars.insert(scope_name.clone(), grammar);
            for target in inject_to {
                injections
                    .entry(target.clone())
                    .or_default()
                    .push(scope_name.clone());
            }
        }
        let runtime_languages = runtime_languages.collect::<Vec<_>>();
        for (_, scope_name, _, inject_to, grammar) in &runtime_languages {
            grammars.insert(scope_name.clone(), grammar);
            for target in inject_to {
                injections
                    .entry(target.clone())
                    .or_default()
                    .push(scope_name.clone());
            }
        }
        let mut languages = HashMap::new();
        let root_ids = root_definitions
            .iter()
            .map(|definition| definition.id)
            .collect::<HashSet<_>>();
        let mut compiled = Vec::with_capacity(
            root_definitions.len() + runtime_languages.len(),
        );
        for (id, scope_name, aliases, _, _) in static_languages
            .iter()
            .filter(|(id, ..)| root_ids.contains(id.as_str()))
            .map(|(id, scope, aliases, inject, grammar)| {
                (id, scope, aliases, inject, *grammar)
            })
            .chain(runtime_languages.iter().map(
                |(id, scope, aliases, inject, grammar)| {
                    (id, scope, aliases, inject, grammar)
                },
            ))
        {
            let grammar = compile(scope_name, &grammars, &injections)?;
            let index = compiled.len();
            compiled.push(CompiledLanguage::eager(grammar));
            languages.insert(id.clone(), index);
            languages.insert(scope_name.clone(), index);
            for alias in aliases {
                languages.insert(alias.clone(), index);
            }
        }
        let plain_text = compiled.len();
        compiled.push(CompiledLanguage::eager(compile_plain_text()));
        for name in ["text", "txt", "plain", "text.plain"] {
            languages.insert(name.to_owned(), plain_text);
        }
        Ok(HighlighterEngine {
            inner: Arc::new(EngineInner {
                languages,
                compiled,
                themes,
                regex_limits: self.regex_limits,
            }),
        })
    }
}

pub fn css_name(name: &str) -> String {
    let output: String = name
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_') {
                ch.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect();
    if output.is_empty() {
        "theme".to_owned()
    } else {
        output
    }
}

/// Splits source exactly as the highlighter does, preserving an empty source
/// line while omitting the synthetic line after a trailing newline.
pub fn split_lines(code: &str) -> impl Iterator<Item = &str> {
    let without_trailing_newline = code.strip_suffix('\n').unwrap_or(code);
    without_trailing_newline
        .split('\n')
        .map(|line| line.strip_suffix('\r').unwrap_or(line))
}
