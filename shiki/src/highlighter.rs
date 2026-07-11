use std::collections::{BTreeMap, HashMap, HashSet};
use std::fmt::Write;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use serde::{Deserialize, Serialize};

use crate::definition::{LanguageBundle, ThemeDefinition};
use crate::error::{Error, Result};
use crate::grammar::{RawGrammar, compile};
use crate::theme::{FontStyle, RawTheme, Style, Theme};
use crate::tokenizer::{
    GrammarState, MultiThemedToken, RegexLimits, ScopeToken, ThemeId, ThemeTokenStyle, ThemedToken,
    Tokenizer, TokenizerCacheStats,
};

#[derive(Clone)]
struct NamedTheme {
    name: String,
    css_name: String,
    theme: Arc<Theme>,
}

struct CompiledLanguage {
    grammar: Arc<crate::grammar::CompiledGrammar>,
    grammar_id: u64,
}

struct EngineInner {
    languages: HashMap<String, usize>,
    compiled: Vec<CompiledLanguage>,
    themes: Vec<NamedTheme>,
    regex_limits: RegexLimits,
}

#[derive(Serialize, Deserialize)]
struct PrecompiledTheme {
    name: String,
    css_name: String,
    theme: Theme,
}

#[derive(Serialize, Deserialize)]
struct PrecompiledEngine {
    version: u32,
    languages: Vec<(String, usize)>,
    compiled: Vec<crate::grammar::CompiledGrammar>,
    themes: Vec<PrecompiledTheme>,
    regex_limits: RegexLimits,
}

const PRECOMPILED_VERSION: u32 = 1;

#[derive(Clone)]
pub struct HighlighterEngine {
    inner: Arc<EngineInner>,
}

pub struct LanguageSession {
    tokenizer: Tokenizer,
}

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

/// Streaming HTML renderer used by [`Highlighter::code_to_html`].
pub struct HtmlRenderer<'a> {
    pub options: &'a HtmlOptions,
}

#[derive(Debug, Clone, Copy)]
pub struct ResolvedStyle<'a> {
    pub color: &'a str,
    pub background: Option<&'a str>,
    pub font_style: FontStyle,
}

impl<'a> HtmlRenderer<'a> {
    pub const fn new(options: &'a HtmlOptions) -> Self {
        Self { options }
    }

    pub const fn options(&self) -> &HtmlOptions {
        self.options
    }
}

static NEXT_GRAMMAR_ID: AtomicU64 = AtomicU64::new(1);

pub struct Highlighter {
    engine: HighlighterEngine,
    tokenizers: Vec<Option<Tokenizer>>,
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

#[derive(Debug, Clone)]
pub struct HtmlOptions {
    pub pre_classes: Vec<String>,
    pub code_classes: Vec<String>,
    pub line_class: Option<String>,
    pub pre_attributes: BTreeMap<String, String>,
    pub code_attributes: BTreeMap<String, String>,
    pub default_theme: Option<String>,
    pub include_background: bool,
    pub include_foreground: bool,
    pub include_theme_class: bool,
    pub include_default_theme_styles: bool,
}

impl Default for HtmlOptions {
    fn default() -> Self {
        Self {
            pre_classes: vec!["shiki".to_owned()],
            code_classes: Vec::new(),
            line_class: Some("line".to_owned()),
            pre_attributes: BTreeMap::new(),
            code_attributes: BTreeMap::new(),
            default_theme: None,
            include_background: true,
            include_foreground: true,
            include_theme_class: true,
            include_default_theme_styles: true,
        }
    }
}

impl HtmlOptions {
    pub fn pre_class(mut self, class: impl Into<String>) -> Self {
        self.pre_classes.push(class.into());
        self
    }

    pub fn code_class(mut self, class: impl Into<String>) -> Self {
        self.code_classes.push(class.into());
        self
    }

    pub fn line_class(mut self, class: impl Into<String>) -> Self {
        self.line_class = Some(class.into());
        self
    }

    pub fn without_line_wrapper(mut self) -> Self {
        self.line_class = None;
        self
    }

    pub fn pre_attribute(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.pre_attributes.insert(name.into(), value.into());
        self
    }

    pub fn code_attribute(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.code_attributes.insert(name.into(), value.into());
        self
    }

    pub fn default_theme(mut self, name: impl Into<String>) -> Self {
        self.default_theme = Some(name.into());
        self
    }

    pub fn variables_only(mut self) -> Self {
        self.include_default_theme_styles = false;
        self
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

    pub fn theme_name(&self, theme: crate::tokenizer::ThemeId) -> Option<&str> {
        self.engine
            .inner
            .themes
            .get(theme as usize)
            .map(|theme| theme.name.as_str())
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
        let language = self.language_index(language)?;
        if theme_id as usize >= self.engine.inner.themes.len() {
            return Err(Error::InvalidThemeId(theme_id));
        }
        self.ensure_tokenizer(language);
        let theme = self
            .engine
            .inner
            .themes
            .get(theme_id as usize)
            .ok_or(Error::InvalidThemeId(theme_id))?;
        let tokenizer = self.tokenizers[language]
            .as_mut()
            .expect("initialized tokenizer");
        if !tokenizer.contains_scope_stack(scopes) {
            return Err(Error::InvalidScopeStack(scopes));
        }
        let style = tokenizer.styles(scopes)[theme_id as usize];
        Ok(ResolvedStyle {
            color: theme.theme.color(
                style
                    .foreground
                    .unwrap_or_else(|| theme.theme.foreground_id()),
            ),
            background: style.background.map(|color| theme.theme.color(color)),
            font_style: style.font_style.unwrap_or_default(),
        })
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

    pub fn cache_stats(&self, language: &str) -> Result<Option<TokenizerCacheStats>> {
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
            let (tokens, next) = tokenizer.tokenize_line_owned(line, state.take(), index == 0)?;
            output.push(tokens);
            state = Some(next);
        }
        Ok(output)
    }

    pub fn code_to_tokens(&mut self, code: &str, language: &str) -> Result<Vec<Vec<ThemedToken>>> {
        let tokens = self.code_to_scope_tokens(code, language)?;
        let language = self.language_index(language)?;
        let theme = &self.engine.inner.themes[0].theme;
        let tokenizer = self.tokenizers[language]
            .as_mut()
            .expect("initialized tokenizer");
        let mut output = Vec::with_capacity(tokens.len());
        for (tokens, line) in tokens.into_iter().zip(split_lines(code)) {
            let mut output_line = Vec::with_capacity(tokens.len());
            for token in tokens {
                let style = tokenizer.styles(token.scopes)[0];
                output_line.push(ThemedToken {
                    content: line[token.range].to_owned(),
                    color: theme
                        .color_arc(style.foreground.unwrap_or_else(|| theme.foreground_id())),
                    background: style.background.map(|color| theme.color_arc(color)),
                    font_style: style.font_style.unwrap_or_default(),
                    scopes: token.scopes,
                });
            }
            output.push(output_line);
        }
        Ok(output)
    }

    pub fn code_to_tokens_with_themes(
        &mut self,
        code: &str,
        language: &str,
    ) -> Result<Vec<Vec<MultiThemedToken>>> {
        let tokens = self.code_to_scope_tokens(code, language)?;
        let language = self.language_index(language)?;
        let themes = &self.engine.inner.themes;
        let tokenizer = self.tokenizers[language]
            .as_mut()
            .expect("initialized tokenizer");
        let mut output = Vec::with_capacity(tokens.len());
        for (tokens, line) in tokens.into_iter().zip(split_lines(code)) {
            let mut output_line = Vec::with_capacity(tokens.len());
            for token in tokens {
                let styles = themes
                    .iter()
                    .enumerate()
                    .zip(tokenizer.styles(token.scopes))
                    .map(|((theme_id, theme), style)| ThemeTokenStyle {
                        theme: theme_id.try_into().expect("too many themes"),
                        color: theme.theme.color_arc(
                            style
                                .foreground
                                .unwrap_or_else(|| theme.theme.foreground_id()),
                        ),
                        background: style.background.map(|color| theme.theme.color_arc(color)),
                        font_style: style.font_style.unwrap_or_default(),
                    })
                    .collect();
                output_line.push(MultiThemedToken {
                    content: line[token.range].to_owned(),
                    styles,
                    scopes: token.scopes,
                });
            }
            output.push(output_line);
        }
        Ok(output)
    }

    pub fn code_to_html(&mut self, code: &str, language: &str) -> Result<String> {
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

    fn render_html(&mut self, code: &str, language: &str, options: &HtmlOptions) -> Result<String> {
        let language = self.language_index(language)?;
        self.ensure_tokenizer(language);
        let default_index = options
            .default_theme
            .as_deref()
            .and_then(|name| {
                self.engine
                    .inner
                    .themes
                    .iter()
                    .position(|theme| theme.name == name)
            })
            .unwrap_or(0);
        let default = &self.engine.inner.themes[default_index];
        let multiple = self.engine.inner.themes.len() > 1;

        let mut pre_classes = options.pre_classes.clone();
        if options.include_theme_class && !multiple {
            pre_classes.push(default.theme.name.to_string());
        }
        let mut pre_attributes = options.pre_attributes.clone();
        if multiple {
            pre_attributes.insert(
                "data-themes".to_owned(),
                self.engine
                    .inner
                    .themes
                    .iter()
                    .map(|theme| theme.name.as_str())
                    .collect::<Vec<_>>()
                    .join(" "),
            );
        }

        let mut root_style = String::new();
        if multiple {
            for theme in &self.engine.inner.themes {
                write!(
                    root_style,
                    "--{}:{};--{}-bg:{};",
                    theme.css_name, theme.theme.foreground, theme.css_name, theme.theme.background
                )
                .expect("write to String");
            }
            if options.include_background {
                write!(
                    root_style,
                    "background-color:var(--{}-bg);",
                    default.css_name
                )
                .expect("write to String");
            }
            if options.include_foreground {
                write!(root_style, "color:var(--{});", default.css_name).expect("write to String");
            }
        } else {
            if options.include_background {
                write!(root_style, "background-color:{};", default.theme.background)
                    .expect("write to String");
            }
            if options.include_foreground {
                write!(root_style, "color:{};", default.theme.foreground).expect("write to String");
            }
        }

        let mut output = String::with_capacity(code.len().saturating_mul(12));
        open_tag(
            &mut output,
            "pre",
            &pre_classes,
            &pre_attributes,
            Some(&root_style),
        );
        open_tag(
            &mut output,
            "code",
            &options.code_classes,
            &options.code_attributes,
            None,
        );
        let themes = &self.engine.inner.themes;
        let tokenizer = self.tokenizers[language]
            .as_mut()
            .expect("initialized tokenizer");
        let mut state = None;
        for (line_index, source) in split_lines(code).enumerate() {
            let (line, next) =
                tokenizer.tokenize_line_owned(source, state.take(), line_index == 0)?;
            state = Some(next);
            if line_index > 0 {
                output.push('\n');
            }
            if let Some(line_class) = &options.line_class {
                open_tag(
                    &mut output,
                    "span",
                    std::slice::from_ref(line_class),
                    &BTreeMap::new(),
                    None,
                );
            }
            for token in &line {
                output.push_str("<span style=\"");
                write_token_style(
                    &mut output,
                    tokenizer.styles(token.scopes),
                    themes,
                    default_index,
                    multiple,
                    options.include_default_theme_styles,
                );
                output.push_str("\">");
                push_escaped_html(&mut output, &source[token.range.clone()]);
                output.push_str("</span>");
            }
            if options.line_class.is_some() {
                output.push_str("</span>");
            }
        }
        output.push_str("</code></pre>");
        Ok(output)
    }

    fn language_index(&self, language: &str) -> Result<usize> {
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

    fn ensure_tokenizer(&mut self, index: usize) {
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
            compiled.grammar.clone(),
            compiled.grammar_id,
            themes,
            self.engine.inner.regex_limits,
        ));
    }
}

impl Renderer for HtmlRenderer<'_> {
    type Output = String;

    fn render(
        &mut self,
        highlighter: &mut Highlighter,
        code: &str,
        language: &str,
    ) -> Result<Self::Output> {
        highlighter.render_html(code, language, self.options)
    }
}

impl HighlighterEngine {
    pub fn language_keys(&self) -> impl Iterator<Item = &str> {
        self.inner.languages.keys().map(String::as_str)
    }

    pub fn language_count(&self) -> usize {
        self.inner.compiled.len()
    }

    pub fn theme_names(&self) -> impl Iterator<Item = &str> {
        self.inner.themes.iter().map(|theme| theme.name.as_str())
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
                compiled.grammar.clone(),
                compiled.grammar_id,
                themes,
                self.inner.regex_limits,
            ),
        })
    }

    #[doc(hidden)]
    pub fn __to_precompiled_bytes(&self) -> Result<Vec<u8>> {
        let mut languages = self
            .inner
            .languages
            .iter()
            .map(|(name, index)| (name.clone(), *index))
            .collect::<Vec<_>>();
        languages.sort_by(|left, right| left.0.cmp(&right.0));
        let compiled = self
            .inner
            .compiled
            .iter()
            .map(|language| language.grammar.as_ref().clone())
            .collect();
        let themes = self
            .inner
            .themes
            .iter()
            .map(|theme| PrecompiledTheme {
                name: theme.name.clone(),
                css_name: theme.css_name.clone(),
                theme: theme.theme.as_ref().clone(),
            })
            .collect();
        serde_json::to_vec(&PrecompiledEngine {
            version: PRECOMPILED_VERSION,
            languages,
            compiled,
            themes,
            regex_limits: self.inner.regex_limits,
        })
        .map_err(|error| Error::InvalidPrecompiled(error.to_string()))
    }

    #[doc(hidden)]
    pub fn __from_precompiled_bytes(bytes: &[u8]) -> Result<Self> {
        let data: PrecompiledEngine = serde_json::from_slice(bytes)
            .map_err(|error| Error::InvalidPrecompiled(error.to_string()))?;
        if data.version != PRECOMPILED_VERSION {
            return Err(Error::InvalidPrecompiled(format!(
                "snapshot version {} is not supported (expected {PRECOMPILED_VERSION})",
                data.version
            )));
        }
        let compiled = data
            .compiled
            .into_iter()
            .map(|grammar| CompiledLanguage {
                grammar: Arc::new(grammar),
                grammar_id: NEXT_GRAMMAR_ID.fetch_add(1, Ordering::Relaxed),
            })
            .collect();
        let themes = data
            .themes
            .into_iter()
            .map(|theme| NamedTheme {
                name: theme.name,
                css_name: theme.css_name,
                theme: Arc::new(theme.theme),
            })
            .collect();
        Ok(Self {
            inner: Arc::new(EngineInner {
                languages: data.languages.into_iter().collect(),
                compiled,
                themes,
                regex_limits: data.regex_limits,
            }),
        })
    }
}

impl LanguageSession {
    pub fn initial_state(&self) -> GrammarState {
        self.tokenizer.initial_state()
    }

    pub fn cache_stats(&self) -> TokenizerCacheStats {
        self.tokenizer.cache_stats()
    }

    pub fn scope_names(&self, stack: crate::tokenizer::ScopeStackId) -> Result<Vec<String>> {
        self.tokenizer.scope_names(stack)
    }

    pub fn tokenize_line(
        &mut self,
        line: &str,
        state: &mut GrammarState,
        is_first_line: bool,
    ) -> Result<Vec<ScopeToken>> {
        let previous = std::mem::take(state);
        let (tokens, next) =
            self.tokenizer
                .tokenize_line_owned(line, Some(previous), is_first_line)?;
        *state = next;
        Ok(tokens)
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

    pub fn language(mut self, id: impl Into<String>, grammar: RawGrammar<'static>) -> Self {
        self.runtime_languages.push(LanguageInput::new(id, grammar));
        self
    }

    pub fn language_definition(mut self, language: LanguageInput) -> Self {
        self.runtime_languages.push(language);
        self
    }

    pub fn json_language(self, id: impl Into<String>, source: &str) -> Result<Self> {
        let id = id.into();
        let grammar = RawGrammar::from_json(&id, source)?;
        Ok(self.language(id, grammar))
    }

    pub fn json_theme(mut self, name: impl Into<String>, source: &str) -> Result<Self> {
        let name = name.into();
        let theme = RawTheme::from_json(&name, source)?;
        self.themes.push((name, ThemeInput::Raw(theme)));
        Ok(self)
    }

    pub fn build(self) -> Result<Highlighter> {
        Ok(self.build_engine()?.highlighter())
    }

    pub fn build_engine(self) -> Result<HighlighterEngine> {
        let (definitions, root_definitions) = match self.bundle {
            Some(bundle) => bundle.resolve(&self.languages)?,
            None if self.runtime_languages.is_empty() => return Err(Error::NoLanguage),
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
        let runtime_languages = self.runtime_languages.into_iter().map(|language| {
            let scope_name = language.grammar.scope_name.as_ref().to_owned();
            (
                language.id,
                scope_name,
                language.aliases,
                language.inject_to,
                language.grammar,
            )
        });
        let mut grammars: HashMap<String, &RawGrammar<'static>> = HashMap::new();
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
        let mut compiled = Vec::with_capacity(root_definitions.len() + runtime_languages.len());
        for (id, scope_name, aliases, _, _) in static_languages
            .iter()
            .filter(|(id, ..)| root_ids.contains(id.as_str()))
            .map(|(id, scope, aliases, inject, grammar)| (id, scope, aliases, inject, *grammar))
            .chain(
                runtime_languages
                    .iter()
                    .map(|(id, scope, aliases, inject, grammar)| {
                        (id, scope, aliases, inject, grammar)
                    }),
            )
        {
            let grammar = Arc::new(compile(scope_name, &grammars, &injections)?);
            let index = compiled.len();
            compiled.push(CompiledLanguage {
                grammar,
                grammar_id: NEXT_GRAMMAR_ID.fetch_add(1, Ordering::Relaxed),
            });
            languages.insert(id.clone(), index);
            languages.insert(scope_name.clone(), index);
            for alias in aliases {
                languages.insert(alias.clone(), index);
            }
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

fn write_token_style(
    output: &mut String,
    styles: &[Style],
    themes: &[NamedTheme],
    default_index: usize,
    multiple: bool,
    include_default_theme_styles: bool,
) {
    if !multiple {
        let style = styles[0];
        let color = themes[0].theme.color(
            style
                .foreground
                .unwrap_or_else(|| themes[0].theme.foreground_id()),
        );
        output.push_str("color:");
        push_escaped_attr(output, color);
        output.push(';');
        if let Some(background) = style.background {
            output.push_str("background-color:");
            push_escaped_attr(output, themes[0].theme.color(background));
            output.push(';');
        }
        write_font_style(output, style.font_style.unwrap_or_default(), "");
        return;
    }

    let mut has_background = false;
    for (theme, style) in themes.iter().zip(styles) {
        let color = theme.theme.color(
            style
                .foreground
                .unwrap_or_else(|| theme.theme.foreground_id()),
        );
        write!(output, "--{}:", theme.css_name).expect("write to String");
        push_escaped_attr(output, color);
        output.push(';');
        if let Some(background) = style.background {
            has_background = true;
            write!(output, "--{}-bg:", theme.css_name).expect("write to String");
            push_escaped_attr(output, theme.theme.color(background));
            output.push(';');
        }
        write_font_variables(
            output,
            &theme.css_name,
            style.font_style.unwrap_or_default(),
        );
    }
    let default = &themes[default_index].css_name;
    if include_default_theme_styles {
        write!(
            output,
            "color:var(--{default});font-style:var(--{default}-font-style);font-weight:var(--{default}-font-weight);text-decoration:var(--{default}-text-decoration);"
        )
        .expect("write to String");
        if has_background {
            write!(output, "background-color:var(--{default}-bg,transparent);")
                .expect("write to String");
        }
    }
}

fn write_font_variables(output: &mut String, name: &str, style: FontStyle) {
    let font_style = if style.contains(FontStyle::ITALIC) {
        "italic"
    } else {
        "normal"
    };
    let font_weight = if style.contains(FontStyle::BOLD) {
        "bold"
    } else {
        "normal"
    };
    let decoration = decoration(style);
    write!(
        output,
        "--{name}-font-style:{font_style};--{name}-font-weight:{font_weight};--{name}-text-decoration:{decoration};"
    )
    .expect("write to String");
}

fn write_font_style(output: &mut String, style: FontStyle, prefix: &str) {
    if style.contains(FontStyle::ITALIC) {
        write!(output, "{prefix}font-style:italic;").expect("write to String");
    }
    if style.contains(FontStyle::BOLD) {
        write!(output, "{prefix}font-weight:bold;").expect("write to String");
    }
    let decoration = decoration(style);
    if decoration != "none" {
        write!(output, "{prefix}text-decoration:{decoration};").expect("write to String");
    }
}

fn decoration(style: FontStyle) -> &'static str {
    match (
        style.contains(FontStyle::UNDERLINE),
        style.contains(FontStyle::STRIKETHROUGH),
    ) {
        (true, true) => "underline line-through",
        (true, false) => "underline",
        (false, true) => "line-through",
        (false, false) => "none",
    }
}

fn open_tag(
    output: &mut String,
    tag: &str,
    classes: &[String],
    attributes: &BTreeMap<String, String>,
    style: Option<&str>,
) {
    write!(output, "<{tag}").expect("write to String");
    if !classes.is_empty() {
        output.push_str(" class=\"");
        for (index, class) in classes.iter().enumerate() {
            if index > 0 {
                output.push(' ');
            }
            push_escaped_attr(output, class);
        }
        output.push('"');
    }
    for (name, value) in attributes {
        if valid_attribute_name(name) {
            write!(output, " {name}=\"").expect("write to String");
            push_escaped_attr(output, value);
            output.push('"');
        }
    }
    if let Some(style) = style.filter(|style| !style.is_empty()) {
        output.push_str(" style=\"");
        push_escaped_attr(output, style);
        output.push('"');
    }
    output.push('>');
}

fn valid_attribute_name(name: &str) -> bool {
    !name.is_empty()
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b':'))
}

fn css_name(name: &str) -> String {
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

fn split_lines(code: &str) -> impl Iterator<Item = &str> {
    let without_trailing_newline = code.strip_suffix('\n').unwrap_or(code);
    without_trailing_newline
        .split('\n')
        .map(|line| line.strip_suffix('\r').unwrap_or(line))
}

fn push_escaped_html(output: &mut String, value: &str) {
    let mut last = 0;
    for (index, ch) in value.char_indices() {
        let replacement = match ch {
            '&' => "&amp;",
            '<' => "&lt;",
            '>' => "&gt;",
            _ => continue,
        };
        output.push_str(&value[last..index]);
        output.push_str(replacement);
        last = index + ch.len_utf8();
    }
    output.push_str(&value[last..]);
}

fn push_escaped_attr(output: &mut String, value: &str) {
    let mut last = 0;
    for (index, ch) in value.char_indices() {
        let replacement = match ch {
            '&' => "&amp;",
            '<' => "&lt;",
            '>' => "&gt;",
            '"' => "&quot;",
            _ => continue,
        };
        output.push_str(&value[last..index]);
        output.push_str(replacement);
        last = index + ch.len_utf8();
    }
    output.push_str(&value[last..]);
}
