use super::{
    builder::HighlighterBuilder,
    engine::{HighlighterEngine, NamedTheme, ResolvedStyle, ThemeInfo},
};
use crate::{
    ansi::AnsiParser,
    error::{Error, Result},
    renderer::{HtmlOptions, HtmlRenderer, Renderer},
    theme::{Style, Theme},
    tokenizer::{
        GrammarState, MultiThemedToken, ScopeStackId, ScopeToken, ThemeId,
        ThemeTokenStyle, ThemedToken, Tokenizer, TokenizerCacheStats,
    },
};

pub struct Highlighter {
    pub(crate) engine: HighlighterEngine,
    pub(crate) tokenizers: Vec<Option<Tokenizer>>,
}

impl Highlighter {
    pub fn builder() -> HighlighterBuilder {
        HighlighterBuilder::new()
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
        scopes: ScopeStackId,
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
        self.ensure_tokenizer(language)?;
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
        stack: ScopeStackId,
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
        self.ensure_tokenizer(language)?;
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
        self.ensure_tokenizer(language)?;
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
        self.ensure_tokenizer(language)?;
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
        self.ensure_tokenizer(index)?;
        Ok(self.tokenizers[index]
            .as_mut()
            .expect("initialized tokenizer"))
    }

    pub(crate) fn ensure_tokenizer(&mut self, index: usize) -> Result<()> {
        if self.tokenizers[index].is_some() {
            return Ok(());
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
            compiled.grammar()?,
            self.engine.inner.regex_pool.clone(),
            compiled.grammar_id,
            themes,
            self.engine.inner.regex_limits,
        ));
        Ok(())
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

/// Splits source exactly as the highlighter does, preserving an empty source
/// line while omitting the synthetic line after a trailing newline.
pub fn split_lines(code: &str) -> impl Iterator<Item = &str> {
    let without_trailing_newline = code.strip_suffix('\n').unwrap_or(code);
    without_trailing_newline
        .split('\n')
        .map(|line| line.strip_suffix('\r').unwrap_or(line))
}
