use std::collections::{BTreeMap, HashMap, HashSet};
use std::fmt::Write;
use std::sync::Arc;

use crate::definition::{LanguageBundle, ThemeDefinition};
use crate::error::{Error, Result};
use crate::grammar::{RawGrammar, compile};
use crate::theme::{FontStyle, RawTheme, Style, Theme};
use crate::tokenizer::{
    GrammarState, MultiThemedToken, ScopeToken, ThemeTokenStyle, ThemedToken, Tokenizer,
};

struct NamedTheme {
    name: String,
    css_name: String,
    theme: Arc<Theme>,
}

pub struct Highlighter {
    languages: HashMap<String, usize>,
    tokenizers: Vec<Tokenizer>,
    themes: Vec<NamedTheme>,
}

pub struct HighlighterBuilder {
    bundle: Option<LanguageBundle>,
    languages: Vec<String>,
    runtime_languages: Vec<RawLanguage>,
    themes: Vec<(String, ThemeInput)>,
    max_line_length: Option<usize>,
}

pub struct RawLanguage {
    pub id: String,
    pub aliases: Vec<String>,
    pub inject_to: Vec<String>,
    pub grammar: RawGrammar,
}

impl RawLanguage {
    pub fn new(id: impl Into<String>, grammar: RawGrammar) -> Self {
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

enum ThemeInput {
    Definition(&'static ThemeDefinition),
    Raw(RawTheme),
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
            max_line_length: None,
        }
    }

    pub fn theme_names(&self) -> impl Iterator<Item = &str> {
        self.themes.iter().map(|theme| theme.name.as_str())
    }

    pub fn theme_name(&self, theme: crate::tokenizer::ThemeId) -> Option<&str> {
        self.themes
            .get(theme as usize)
            .map(|theme| theme.name.as_str())
    }

    pub fn initial_state(&mut self, language: &str) -> Result<GrammarState> {
        Ok(self.tokenizer(language)?.initial_state())
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
        let tokenizer = &mut self.tokenizers[language];
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
        let theme = &self.themes[0].theme;
        let tokenizer = &mut self.tokenizers[language];
        let mut output = Vec::with_capacity(tokens.len());
        for (tokens, line) in tokens.into_iter().zip(split_lines(code)) {
            let mut output_line = Vec::with_capacity(tokens.len());
            for token in tokens {
                let style = tokenizer.styles(token.scopes)[0];
                output_line.push(ThemedToken {
                    content: line[token.range].to_owned(),
                    color: theme
                        .color(style.foreground.unwrap_or_else(|| theme.foreground_id()))
                        .to_owned(),
                    background: style.background.map(|color| theme.color(color).to_owned()),
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
        let themes = &self.themes;
        let tokenizer = &mut self.tokenizers[language];
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
                        color: theme
                            .theme
                            .color(
                                style
                                    .foreground
                                    .unwrap_or_else(|| theme.theme.foreground_id()),
                            )
                            .to_owned(),
                        background: style
                            .background
                            .map(|color| theme.theme.color(color).to_owned()),
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
        self.code_to_html_with_options(code, language, &HtmlOptions::default())
    }

    pub fn code_to_html_with_options(
        &mut self,
        code: &str,
        language: &str,
        options: &HtmlOptions,
    ) -> Result<String> {
        let language = self.language_index(language)?;
        let default_index = options
            .default_theme
            .as_deref()
            .and_then(|name| self.themes.iter().position(|theme| theme.name == name))
            .unwrap_or(0);
        let default = &self.themes[default_index];
        let multiple = self.themes.len() > 1;

        let mut pre_classes = options.pre_classes.clone();
        if options.include_theme_class && !multiple {
            pre_classes.push(default.theme.name.clone());
        }
        let mut pre_attributes = options.pre_attributes.clone();
        if multiple {
            pre_attributes.insert(
                "data-themes".to_owned(),
                self.themes
                    .iter()
                    .map(|theme| theme.name.as_str())
                    .collect::<Vec<_>>()
                    .join(" "),
            );
        }

        let mut root_style = String::new();
        if multiple {
            for theme in &self.themes {
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
        let themes = &self.themes;
        let tokenizer = &mut self.tokenizers[language];
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
        self.languages
            .get(language)
            .copied()
            .ok_or_else(|| Error::GrammarNotLoaded(language.to_owned()))
    }

    fn tokenizer(&mut self, language: &str) -> Result<&mut Tokenizer> {
        let index = self.language_index(language)?;
        Ok(&mut self.tokenizers[index])
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

    pub fn max_tokenization_line_length(mut self, length: usize) -> Self {
        self.max_line_length = Some(length);
        self
    }

    pub fn unlimited_tokenization_line_length(mut self) -> Self {
        self.max_line_length = None;
        self
    }

    pub fn theme(mut self, theme: &'static ThemeDefinition) -> Self {
        self.themes = vec![("default".to_owned(), ThemeInput::Definition(theme))];
        self
    }

    pub fn themes<I, S>(mut self, themes: I) -> Self
    where
        I: IntoIterator<Item = (S, &'static ThemeDefinition)>,
        S: Into<String>,
    {
        self.themes = themes
            .into_iter()
            .map(|(name, theme)| (name.into(), ThemeInput::Definition(theme)))
            .collect();
        self
    }

    pub fn raw_language(mut self, id: impl Into<String>, grammar: RawGrammar) -> Self {
        self.runtime_languages.push(RawLanguage::new(id, grammar));
        self
    }

    pub fn raw_language_definition(mut self, language: RawLanguage) -> Self {
        self.runtime_languages.push(language);
        self
    }

    pub fn json_language(self, id: impl Into<String>, source: &str) -> Result<Self> {
        let id = id.into();
        let grammar = RawGrammar::from_json(&id, source)?;
        Ok(self.raw_language(id, grammar))
    }

    pub fn raw_theme(mut self, name: impl Into<String>, theme: RawTheme) -> Self {
        self.themes.push((name.into(), ThemeInput::Raw(theme)));
        self
    }

    pub fn json_theme(self, name: impl Into<String>, source: &str) -> Result<Self> {
        let name = name.into();
        let theme = RawTheme::from_json(&name, source)?;
        Ok(self.raw_theme(name, theme))
    }

    pub fn build(self) -> Result<Highlighter> {
        let definitions = match self.bundle {
            Some(bundle) => bundle.resolve(&self.languages)?,
            None if self.runtime_languages.is_empty() => return Err(Error::NoLanguage),
            None => Vec::new(),
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
                ThemeInput::Definition(definition) => definition.theme()?,
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
                definition.grammar().map(|grammar| {
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
                        grammar,
                    )
                })
            })
            .collect::<Result<Vec<_>>>()?;
        let runtime_languages = self.runtime_languages.into_iter().map(|language| {
            let scope_name = language.grammar.scope_name.clone();
            (
                language.id,
                scope_name,
                language.aliases,
                language.inject_to,
                language.grammar,
            )
        });
        let mut grammars: HashMap<String, &RawGrammar> = HashMap::new();
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
        let theme_refs = themes
            .iter()
            .map(|theme| theme.theme.clone())
            .collect::<Vec<_>>();
        let mut languages = HashMap::new();
        let mut tokenizers = Vec::with_capacity(static_languages.len() + runtime_languages.len());
        for (id, scope_name, aliases, _, _) in static_languages
            .iter()
            .map(|(id, scope, aliases, inject, grammar)| {
                (id, scope, aliases, inject, grammar.as_ref())
            })
            .chain(
                runtime_languages
                    .iter()
                    .map(|(id, scope, aliases, inject, grammar)| {
                        (id, scope, aliases, inject, grammar)
                    }),
            )
        {
            let injection_scopes = injections
                .get(scope_name)
                .map(Vec::as_slice)
                .unwrap_or_default();
            let grammar = compile(scope_name, &grammars, injection_scopes)?;
            let index = tokenizers.len();
            tokenizers.push(Tokenizer::new(
                grammar,
                theme_refs.clone(),
                self.max_line_length,
            ));
            languages.insert(id.clone(), index);
            languages.insert(scope_name.clone(), index);
            for alias in aliases {
                languages.insert(alias.clone(), index);
            }
        }
        Ok(Highlighter {
            languages,
            tokenizers,
            themes,
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
        write!(output, " class=\"{}\"", escape_attr(&classes.join(" "))).expect("write to String");
    }
    for (name, value) in attributes {
        if valid_attribute_name(name) {
            write!(output, " {name}=\"{}\"", escape_attr(value)).expect("write to String");
        }
    }
    if let Some(style) = style.filter(|style| !style.is_empty()) {
        write!(output, " style=\"{}\"", escape_attr(style)).expect("write to String");
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

fn escape_html(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
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

fn escape_attr(value: &str) -> String {
    escape_html(value).replace('"', "&quot;")
}
