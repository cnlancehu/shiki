use std::{collections::BTreeMap, fmt::Write};

use crate::{
    FontStyle, Highlighter, Result,
    ansi::{
        AnsiState, ResolvedAnsiStyle, parse_line as parse_ansi_line,
        resolve_style as resolve_ansi_style,
    },
    highlighter::{NamedTheme, split_lines},
    renderer::Renderer,
    theme::Style,
};

#[derive(Debug, Clone)]
pub struct HtmlOptions {
    pub pre_classes: Vec<String>,
    pub code_classes: Vec<String>,
    /// Wraps each source line in a `<span>`.
    pub include_line_wrapper: bool,
    /// Adds a class to line wrappers when configured.
    pub line_class: Option<String>,
    pub pre_attributes: BTreeMap<String, String>,
    pub code_attributes: BTreeMap<String, String>,
    pub default_theme: Option<String>,
    /// Adds the conventional `shiki` class to `<pre>`.
    pub include_shiki_class: bool,
    /// Adds the active theme name as a `<pre>` class for a single theme.
    pub include_theme_class: bool,
    /// Adds `data-themes` to `<pre>` when multiple themes are configured.
    pub include_data_themes: bool,
    /// Adds per-theme foreground/background CSS variables to `<pre style>`.
    pub include_theme_variables: bool,
    /// Enables automatically generated declarations in `<pre style>`.
    pub include_root_style: bool,
    /// Adds the active theme background to `<pre style>`.
    pub include_background: bool,
    /// Adds the active theme foreground to `<pre style>`.
    pub include_foreground: bool,
    /// Adds concrete default-theme properties alongside multi-theme variables.
    pub include_default_theme_styles: bool,
    /// Adds styled token spans. When disabled, only escaped source is emitted.
    pub include_token_styles: bool,
}

impl Default for HtmlOptions {
    fn default() -> Self {
        Self::new()
    }
}

impl HtmlOptions {
    /// Returns the Shiki-compatible default HTML configuration.
    pub const fn new() -> Self {
        Self {
            pre_classes: Vec::new(),
            code_classes: Vec::new(),
            include_line_wrapper: true,
            line_class: None,
            pre_attributes: BTreeMap::new(),
            code_attributes: BTreeMap::new(),
            default_theme: None,
            include_shiki_class: true,
            include_theme_class: true,
            include_data_themes: true,
            include_theme_variables: true,
            include_root_style: true,
            include_background: true,
            include_foreground: true,
            include_default_theme_styles: true,
            include_token_styles: true,
        }
    }

    /// Returns a minimal wrapper configuration.
    ///
    /// It adds no `<pre>`/`<code>` classes, attributes, or root theme styles.
    /// The `line` wrapper class and token styles remain enabled.
    pub fn clean() -> Self {
        Self {
            pre_classes: Vec::new(),
            code_classes: Vec::new(),
            include_line_wrapper: true,
            line_class: Some("line".to_owned()),
            pre_attributes: BTreeMap::new(),
            code_attributes: BTreeMap::new(),
            default_theme: None,
            include_shiki_class: false,
            include_theme_class: false,
            include_data_themes: false,
            include_theme_variables: false,
            include_root_style: false,
            include_background: false,
            include_foreground: false,
            include_default_theme_styles: true,
            include_token_styles: true,
        }
    }
}

/// Streaming HTML renderer used by [`Highlighter::code_to_html`].
pub struct HtmlRenderer<'a> {
    pub options: &'a HtmlOptions,
}

impl<'a> HtmlRenderer<'a> {
    pub const fn new(options: &'a HtmlOptions) -> Self {
        Self { options }
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
        render_html(highlighter, code, language, self.options)
    }
}

fn render_html(
    highlighter: &mut Highlighter,
    code: &str,
    language: &str,
    options: &HtmlOptions,
) -> Result<String> {
    let language = if crate::ansi::is_ansi(language) {
        None
    } else {
        let language = highlighter.language_index(language)?;
        highlighter.ensure_tokenizer(language);
        Some(language)
    };
    let default_index = options
        .default_theme
        .as_deref()
        .and_then(|name| {
            highlighter
                .engine
                .inner
                .themes
                .iter()
                .position(|theme| theme.name == name)
        })
        .unwrap_or(0);
    let default = &highlighter.engine.inner.themes[default_index];
    let multiple = highlighter.engine.inner.themes.len() > 1;

    let theme_class = (options.include_theme_class && !multiple)
        .then_some(default.css_name.as_str());
    let shiki_class = options.include_shiki_class.then_some("shiki");
    let data_themes = (options.include_data_themes && multiple).then(|| {
        highlighter
            .engine
            .inner
            .themes
            .iter()
            .map(|theme| theme.name.as_str())
            .collect::<Vec<_>>()
            .join(" ")
    });
    let automatic_pre_attributes =
        data_themes.as_deref().map(|value| [("data-themes", value)]);
    let automatic_pre_attributes = automatic_pre_attributes
        .as_ref()
        .map_or(&[][..], <[_; 1]>::as_slice);

    let mut root_style = String::new();
    if options.include_root_style && multiple {
        if options.include_theme_variables {
            for theme in &highlighter.engine.inner.themes {
                write!(
                    root_style,
                    "--{}:{};--{}-bg:{};",
                    theme.css_name,
                    theme.theme.foreground,
                    theme.css_name,
                    theme.theme.background
                )
                .expect("write to String");
            }
        }
        if options.include_background {
            if options.include_theme_variables {
                write!(
                    root_style,
                    "background-color:var(--{}-bg);",
                    default.css_name
                )
                .expect("write to String");
            } else {
                write!(
                    root_style,
                    "background-color:{};",
                    default.theme.background
                )
                .expect("write to String");
            }
        }
        if options.include_foreground {
            if options.include_theme_variables {
                write!(root_style, "color:var(--{});", default.css_name)
                    .expect("write to String");
            } else {
                write!(root_style, "color:{};", default.theme.foreground)
                    .expect("write to String");
            }
        }
    } else if options.include_root_style {
        if options.include_background {
            write!(
                root_style,
                "background-color:{};",
                default.theme.background
            )
            .expect("write to String");
        }
        if options.include_foreground {
            write!(root_style, "color:{};", default.theme.foreground)
                .expect("write to String");
        }
    }

    let mut output = String::with_capacity(code.len().saturating_mul(12));
    open_tag(
        &mut output,
        "pre",
        shiki_class
            .into_iter()
            .chain(options.pre_classes.iter().map(String::as_str))
            .chain(theme_class),
        &options.pre_attributes,
        automatic_pre_attributes,
        Some(&root_style),
    );
    open_tag(
        &mut output,
        "code",
        options.code_classes.iter().map(String::as_str),
        &options.code_attributes,
        &[],
        None,
    );
    let themes = &highlighter.engine.inner.themes;
    let Some(language) = language else {
        write_ansi_lines(
            &mut output,
            code,
            themes,
            default_index,
            multiple,
            options,
        );
        output.push_str("</code></pre>");
        return Ok(output);
    };
    let tokenizer = highlighter.tokenizers[language]
        .as_mut()
        .expect("initialized tokenizer");
    let mut state = None;
    for (line_index, source) in split_lines(code).enumerate() {
        let (line, next) = tokenizer.tokenize_line_owned(
            source,
            state.take(),
            line_index == 0,
        )?;
        state = Some(next);
        if line_index > 0 {
            output.push('\n');
        }
        if options.include_line_wrapper {
            open_tag(
                &mut output,
                "span",
                options.line_class.as_deref(),
                &BTreeMap::new(),
                &[],
                None,
            );
        }
        let mut run_start = 0;
        let mut run_end = 0;
        let mut run_styles = Vec::with_capacity(themes.len());
        for token in &line {
            let styles = tokenizer.styles(token.scopes);
            if !run_styles.is_empty()
                && run_end == token.range.start
                && styles_equivalent(&run_styles, styles, themes)
            {
                run_end = token.range.end;
                continue;
            }
            if !run_styles.is_empty() {
                write_token_run(
                    &mut output,
                    &source[run_start..run_end],
                    &run_styles,
                    themes,
                    default_index,
                    multiple,
                    options,
                );
            }
            run_start = token.range.start;
            run_end = token.range.end;
            run_styles.clear();
            run_styles.extend_from_slice(styles);
        }
        if !run_styles.is_empty() {
            write_token_run(
                &mut output,
                &source[run_start..run_end],
                &run_styles,
                themes,
                default_index,
                multiple,
                options,
            );
        }
        if options.include_line_wrapper {
            output.push_str("</span>");
        }
    }
    output.push_str("</code></pre>");
    Ok(output)
}

fn write_ansi_lines(
    output: &mut String,
    code: &str,
    themes: &[NamedTheme],
    default_index: usize,
    multiple: bool,
    options: &HtmlOptions,
) {
    let mut state = AnsiState::default();
    let mut spans = Vec::new();
    let mut styles = Vec::with_capacity(themes.len());
    for (line_index, source) in split_lines(code).enumerate() {
        if line_index > 0 {
            output.push('\n');
        }
        if options.include_line_wrapper {
            open_tag(
                output,
                "span",
                options.line_class.as_deref(),
                &BTreeMap::new(),
                &[],
                None,
            );
        }
        parse_ansi_line(source, &mut state, &mut spans);
        for span in &spans {
            styles.clear();
            styles.extend(
                themes
                    .iter()
                    .map(|theme| resolve_ansi_style(&theme.theme, span.state)),
            );
            write_token_run(
                output,
                &source[span.range.clone()],
                &styles,
                themes,
                default_index,
                multiple,
                options,
            );
        }
        if options.include_line_wrapper {
            output.push_str("</span>");
        }
    }
}

fn styles_equivalent(
    left: &[Style],
    right: &[Style],
    themes: &[NamedTheme],
) -> bool {
    left.len() == right.len()
        && left.len() == themes.len()
        && left
            .iter()
            .zip(right)
            .zip(themes)
            .all(|((left, right), theme)| {
                left.foreground
                    .unwrap_or_else(|| theme.theme.foreground_id())
                    == right
                        .foreground
                        .unwrap_or_else(|| theme.theme.foreground_id())
                    && left.background == right.background
                    && left.font_style.unwrap_or_default()
                        == right.font_style.unwrap_or_default()
            })
}

trait VisualStyle {
    fn color<'a>(&'a self, theme: &'a NamedTheme) -> &'a str;
    fn background<'a>(&'a self, theme: &'a NamedTheme) -> Option<&'a str>;
    fn font_style(&self) -> FontStyle;
    fn explicit(&self) -> bool {
        false
    }
}

impl VisualStyle for Style {
    fn color<'a>(&'a self, theme: &'a NamedTheme) -> &'a str {
        theme.theme.color(
            self.foreground
                .unwrap_or_else(|| theme.theme.foreground_id()),
        )
    }

    fn background<'a>(&'a self, theme: &'a NamedTheme) -> Option<&'a str> {
        self.background.map(|color| theme.theme.color(color))
    }

    fn font_style(&self) -> FontStyle {
        self.font_style.unwrap_or_default()
    }
}

impl VisualStyle for ResolvedAnsiStyle {
    fn color<'a>(&'a self, _theme: &'a NamedTheme) -> &'a str {
        &self.color
    }

    fn background<'a>(&'a self, _theme: &'a NamedTheme) -> Option<&'a str> {
        self.background.as_deref()
    }

    fn font_style(&self) -> FontStyle {
        self.font_style
    }

    fn explicit(&self) -> bool {
        self.explicit
    }
}

fn write_token_run<S: VisualStyle>(
    output: &mut String,
    content: &str,
    styles: &[S],
    themes: &[NamedTheme],
    default_index: usize,
    multiple: bool,
    options: &HtmlOptions,
) {
    if !options.include_token_styles {
        push_escaped_html(output, content);
        return;
    }
    let unstyled_whitespace = content.chars().all(char::is_whitespace)
        && styles.iter().zip(themes).all(|(style, theme)| {
            !style.explicit()
                && style.background(theme).is_none()
                && style.font_style() == FontStyle::default()
        });
    if unstyled_whitespace {
        push_escaped_html(output, content);
        return;
    }
    output.push_str("<span style=\"");
    write_token_style(
        output,
        styles,
        themes,
        default_index,
        multiple,
        options.include_default_theme_styles,
    );
    output.push_str("\">");
    push_escaped_html(output, content);
    output.push_str("</span>");
}

fn write_token_style<S: VisualStyle>(
    output: &mut String,
    styles: &[S],
    themes: &[NamedTheme],
    default_index: usize,
    multiple: bool,
    include_default_theme_styles: bool,
) {
    if !multiple {
        let style = &styles[0];
        let color = style.color(&themes[0]);
        output.push_str("color:");
        push_escaped_attr(output, color);
        output.push(';');
        if let Some(background) = style.background(&themes[0]) {
            output.push_str("background-color:");
            push_escaped_attr(output, background);
            output.push(';');
        }
        write_font_style(output, style.font_style(), "");
        return;
    }

    let mut has_background = false;
    for (theme, style) in themes.iter().zip(styles) {
        let color = style.color(theme);
        write!(output, "--{}:", theme.css_name).expect("write to String");
        push_escaped_attr(output, color);
        output.push(';');
        if let Some(background) = style.background(theme) {
            has_background = true;
            write!(output, "--{}-bg:", theme.css_name)
                .expect("write to String");
            push_escaped_attr(output, background);
            output.push(';');
        }
        write_font_variables(output, &theme.css_name, style.font_style());
    }
    let default = &themes[default_index].css_name;
    if include_default_theme_styles {
        write!(output, "color:var(--{default});").expect("write to String");
        let default_font_style = styles[default_index].font_style();
        if default_font_style.contains(FontStyle::ITALIC) {
            write!(output, "font-style:var(--{default}-font-style);")
                .expect("write to String");
        }
        if default_font_style.contains(FontStyle::BOLD) {
            write!(output, "font-weight:var(--{default}-font-weight);")
                .expect("write to String");
        }
        if decoration(default_font_style) != "none" {
            write!(output, "text-decoration:var(--{default}-text-decoration);")
                .expect("write to String");
        }
        if has_background {
            write!(output, "background-color:var(--{default}-bg,transparent);")
                .expect("write to String");
        }
    }
}

fn write_font_variables(output: &mut String, name: &str, style: FontStyle) {
    if style.contains(FontStyle::ITALIC) {
        write!(output, "--{name}-font-style:italic;").expect("write to String");
    }
    if style.contains(FontStyle::BOLD) {
        write!(output, "--{name}-font-weight:bold;").expect("write to String");
    }
    let decoration = decoration(style);
    if decoration != "none" {
        write!(output, "--{name}-text-decoration:{decoration};")
            .expect("write to String");
    }
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
        write!(output, "{prefix}text-decoration:{decoration};")
            .expect("write to String");
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

fn open_tag<'a>(
    output: &mut String,
    tag: &str,
    classes: impl IntoIterator<Item = &'a str>,
    attributes: &BTreeMap<String, String>,
    automatic_attributes: &[(&str, &str)],
    style: Option<&str>,
) {
    write!(output, "<{tag}").expect("write to String");
    let mut classes = classes.into_iter().filter(|class| !class.is_empty());
    if let Some(first) = classes.next() {
        output.push_str(" class=\"");
        push_escaped_attr(output, first);
        for class in classes {
            output.push(' ');
            push_escaped_attr(output, class);
        }
        output.push('"');
    }
    for (name, value) in attributes.iter() {
        if name != "style"
            && !automatic_attributes
                .iter()
                .any(|(automatic, _)| *automatic == name)
            && valid_attribute_name(name)
        {
            write!(output, " {name}=\"").expect("write to String");
            push_escaped_attr(output, value);
            output.push('"');
        }
    }
    for (name, value) in automatic_attributes {
        if valid_attribute_name(name) {
            write!(output, " {name}=\"").expect("write to String");
            push_escaped_attr(output, value);
            output.push('"');
        }
    }
    let custom_style =
        attributes.get("style").filter(|style| !style.is_empty());
    let automatic_style = style.filter(|style| !style.is_empty());
    if custom_style.is_some() || automatic_style.is_some() {
        output.push_str(" style=\"");
        if let Some(style) = custom_style {
            push_escaped_attr(output, style);
            if automatic_style.is_some() && !style.ends_with(';') {
                output.push(';');
            }
        }
        if let Some(style) = automatic_style {
            push_escaped_attr(output, style);
        }
        output.push('"');
    }
    output.push('>');
}

fn valid_attribute_name(name: &str) -> bool {
    !name.is_empty()
        && name.bytes().all(|byte| {
            byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b':')
        })
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
