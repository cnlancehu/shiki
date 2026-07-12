use std::{collections::BTreeMap, fmt::Write};

use crate::{
    FontStyle, Highlighter, Result,
    highlighter::{NamedTheme, split_lines},
    renderer::Renderer,
    theme::Style,
};

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

    pub fn pre_attribute(
        mut self,
        name: impl Into<String>,
        value: impl Into<String>,
    ) -> Self {
        self.pre_attributes.insert(name.into(), value.into());
        self
    }

    pub fn code_attribute(
        mut self,
        name: impl Into<String>,
        value: impl Into<String>,
    ) -> Self {
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
    let language = highlighter.language_index(language)?;
    highlighter.ensure_tokenizer(language);
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

    let mut pre_classes = options.pre_classes.clone();
    if options.include_theme_class && !multiple {
        pre_classes.push(default.theme.name.to_string());
    }
    let mut pre_attributes = options.pre_attributes.clone();
    if multiple {
        pre_attributes.insert(
            "data-themes".to_owned(),
            highlighter
                .engine
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
        if options.include_background {
            write!(
                root_style,
                "background-color:var(--{}-bg);",
                default.css_name
            )
            .expect("write to String");
        }
        if options.include_foreground {
            write!(root_style, "color:var(--{});", default.css_name)
                .expect("write to String");
        }
    } else {
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
    let themes = &highlighter.engine.inner.themes;
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
            write!(output, "--{}-bg:", theme.css_name)
                .expect("write to String");
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
