use shiki::{Highlighter, HtmlOptions, LanguageBundle, Renderer};

static LANGUAGES: LanguageBundle =
    shiki_langs::languages![astro, markdown, rust, vue];

#[test]
fn highlights_rust_to_html() {
    let mut highlighter = Highlighter::builder()
        .bundle(&LANGUAGES)
        .languages(["rust"])
        .theme(&shiki_themes::generated::GITHUB_DARK)
        .build()
        .unwrap();

    let html = highlighter
        .code_to_html("fn main() {\n    println!(\"hello\");\n}", "rust")
        .unwrap();

    assert!(html.contains("fn"));
    assert!(html.contains("main"));
    assert!(html.contains("hello"), "{html}");
    assert!(html.contains("color:"));
    assert!(html.starts_with("<pre class=\"shiki default\""), "{html}");
}

#[test]
fn plain_text_bypasses_highlighting() {
    static PLAIN_TEXT: LanguageBundle = shiki_langs::languages![text];
    let mut highlighter = Highlighter::builder()
        .bundle(&PLAIN_TEXT)
        .theme(&shiki_themes::generated::GITHUB_DARK)
        .build()
        .unwrap();
    let options = HtmlOptions {
        include_line_wrapper: false,
        ..HtmlOptions::clean()
    };
    let source = r#"<script>const message = "hello" && value;</script>"#;
    let escaped = "&lt;script&gt;const message = \"hello\" &amp;&amp; value;&lt;/script&gt;";
    let mut expected_html = None;

    for language in ["text", "txt", "plain"] {
        let html = highlighter
            .code_to_html_with_options(source, language, &options)
            .unwrap();
        assert!(html.contains(escaped), "{html}");
        assert_eq!(html.matches("<span style=").count(), 1, "{html}");
        if let Some(expected) = &expected_html {
            assert_eq!(&html, expected);
        } else {
            expected_html = Some(html);
        }

        let tokens =
            highlighter.code_to_scope_tokens(source, language).unwrap();
        assert_eq!(tokens.len(), 1);
        assert_eq!(tokens[0].len(), 1);
        assert_eq!(tokens[0][0].range, 0..source.len());
        let scopes = highlighter
            .scope_names(language, tokens[0][0].scopes)
            .unwrap();
        assert_eq!(scopes, ["text.plain"]);
    }

    let standalone = Highlighter::builder()
        .languages(["text"])
        .theme(&shiki_themes::generated::GITHUB_DARK)
        .build();
    assert!(standalone.is_ok(), "{:?}", standalone.err());
}

#[test]
fn keeps_multiline_state() {
    let mut highlighter = Highlighter::builder()
        .bundle(&LANGUAGES)
        .languages(["rust"])
        .theme(&shiki_themes::generated::GITHUB_DARK)
        .build()
        .unwrap();

    let (first, state) = highlighter
        .tokenize_line("/* comment", "rust", None, true)
        .unwrap();
    let (second, _) = highlighter
        .tokenize_line(
            "continued */ let value = 1;",
            "rust",
            Some(&state),
            false,
        )
        .unwrap();

    assert!(!first.is_empty(), "{first:#?}");
    assert!(second.len() > 1, "{second:#?}");
    assert_eq!(first[0].scopes, second[0].scopes);
    assert!(second.iter().any(|token| token.scopes != second[0].scopes));
    let scopes = highlighter.scope_names("rust", first[0].scopes).unwrap();
    assert!(
        scopes.iter().any(|scope| scope == "source.rust"),
        "{scopes:?}"
    );
    assert!(
        scopes.iter().any(|scope| scope.contains("comment")),
        "{scopes:?}"
    );
}

#[test]
fn vue_bundle_contains_eager_dependencies() {
    let highlighter = Highlighter::builder()
        .bundle(&LANGUAGES)
        .languages(["vue"])
        .theme(&shiki_themes::generated::GITHUB_DARK)
        .build();

    assert!(highlighter.is_ok(), "{:?}", highlighter.err());
}

#[test]
fn all_generated_assets_are_available() {
    for language in shiki_langs::generated::ALL_LANGUAGES {
        let _ = language.grammar();
    }
    for theme in shiki_themes::generated::ALL_THEMES {
        let _ = theme.theme();
    }
}

#[test]
fn all_bundle_contains_every_generated_language() {
    let ids = shiki_langs::all()
        .definitions()
        .map(|language| language.id)
        .collect::<std::collections::HashSet<_>>();
    assert_eq!(ids.len(), shiki_langs::generated::ALL_LANGUAGES.len());
}

#[test]
fn rust_function_and_string_use_distinct_theme_rules() {
    let mut highlighter = Highlighter::builder()
        .bundle(&LANGUAGES)
        .languages(["rust"])
        .theme(&shiki_themes::generated::CATPPUCCIN_MOCHA)
        .build()
        .unwrap();
    let line = r#"object.get("name")"#;
    let lines = highlighter.code_to_tokens(line, "rust").unwrap();
    let function = lines[0]
        .iter()
        .find(|token| token.content == "get")
        .unwrap();
    let string = lines[0]
        .iter()
        .find(|token| token.content == "name")
        .unwrap();
    assert_ne!(function.color, string.color);
}

#[test]
fn markdown_fenced_rust_uses_embedded_grammar() {
    let mut highlighter = Highlighter::builder()
        .bundle(&LANGUAGES)
        .languages(["markdown"])
        .themes([
            ("light", &shiki_themes::CATPPUCCIN_LATTE),
            ("dark", &shiki_themes::CATPPUCCIN_MOCHA),
        ])
        .build()
        .unwrap();
    let source =
        "```rust\nfn main() {\n    println!(\"Hello, world!\");\n}\n```";
    let lines = highlighter
        .code_to_scope_tokens(source, "markdown")
        .unwrap();
    let rust_line = "fn main() {";
    let scoped = lines[1]
        .iter()
        .map(|token| {
            (
                &rust_line[token.range.clone()],
                highlighter.scope_names("markdown", token.scopes).unwrap(),
            )
        })
        .collect::<Vec<_>>();

    assert!(
        scoped.iter().any(|(content, scopes)| {
            *content == "fn"
                && scopes.iter().any(|scope| scope == "keyword.other.fn.rust")
        }),
        "{scoped:#?}"
    );
    assert!(
        scoped.iter().any(|(content, scopes)| {
            *content == "main"
                && scopes
                    .iter()
                    .any(|scope| scope == "entity.name.function.rust")
        }),
        "{scoped:#?}"
    );

    let html = highlighter.code_to_html(source, "markdown").unwrap();
    assert!(html.contains("--light:#8839ef;--dark:#cba6f7"), "{html}");
    assert!(html.contains("--light:#1e66f5;--light-font-style:italic;--dark:#89b4fa;--dark-font-style:italic"), "{html}");
}

#[test]
fn javascript_tokens_are_ordered_and_non_overlapping() {
    static JAVASCRIPT: LanguageBundle =
        shiki_langs::languages![javascript, markdown];
    let mut highlighter = Highlighter::builder()
        .bundle(&JAVASCRIPT)
        .languages(["javascript"])
        .themes([
            ("light", &shiki_themes::CATPPUCCIN_LATTE),
            ("dark", &shiki_themes::CATPPUCCIN_MOCHA),
        ])
        .build()
        .unwrap();
    let source = "export const text_encoder = new TextEncoder();";

    let lines = highlighter
        .code_to_scope_tokens(source, "javascript")
        .unwrap();
    assert_token_partition(source, &lines[0]);

    let html = highlighter.code_to_html(source, "javascript").unwrap();
    assert_eq!(visible_text(&html), source);
}

#[test]
fn markdown_embedded_javascript_tokens_are_ordered_and_non_overlapping() {
    static JAVASCRIPT: LanguageBundle =
        shiki_langs::languages![javascript, markdown];
    let mut highlighter = Highlighter::builder()
        .bundle(&JAVASCRIPT)
        .languages(["markdown"])
        .themes([
            ("light", &shiki_themes::CATPPUCCIN_LATTE),
            ("dark", &shiki_themes::CATPPUCCIN_MOCHA),
        ])
        .build()
        .unwrap();
    let line = "export export const text_encoder = new *TextEncoder*();";
    let source = format!("```js\n{line}\n```");

    let lines = highlighter
        .code_to_scope_tokens(&source, "markdown")
        .unwrap();
    assert_token_partition(line, &lines[1]);

    let html = highlighter.code_to_html(&source, "markdown").unwrap();
    assert_eq!(visible_text(&html), source);
}

fn assert_token_partition(source: &str, tokens: &[shiki::ScopeToken]) {
    let mut position = 0;
    for token in tokens {
        assert_eq!(token.range.start, position, "{tokens:#?}");
        assert!(token.range.end > token.range.start, "{tokens:#?}");
        assert!(token.range.end <= source.len(), "{tokens:#?}");
        assert!(source.is_char_boundary(token.range.start), "{tokens:#?}");
        assert!(source.is_char_boundary(token.range.end), "{tokens:#?}");
        position = token.range.end;
    }
    assert_eq!(position, source.len(), "{tokens:#?}");
}

fn visible_text(html: &str) -> String {
    let mut output = String::new();
    let mut in_tag = false;
    for ch in html.chars() {
        match ch {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => output.push(ch),
            _ => {}
        }
    }
    output
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&amp;", "&")
}

#[test]
fn renders_ansi_with_theme_palette_and_decorations() {
    let mut highlighter = Highlighter::builder()
        .bundle(&LANGUAGES)
        .languages(["ansi"])
        .themes([
            ("light", &shiki_themes::CATPPUCCIN_LATTE),
            ("dark", &shiki_themes::CATPPUCCIN_MOCHA),
        ])
        .build()
        .unwrap();
    let source = concat!(
        "plain ",
        "\x1b[1;3;4;9;31;43mstyled\x1b[0m ",
        "\x1b[38;2;1;2;3;48;5;24mtruecolor\x1b[0m"
    );
    let html = highlighter.code_to_html(source, "ansi").unwrap();

    assert!(!html.contains('\x1b'), "{html}");
    assert!(html.contains("--light:#d20f39"), "{html}");
    assert!(html.contains("--dark:#f38ba8"), "{html}");
    assert!(html.contains("--light-bg:#df8e1d"), "{html}");
    assert!(html.contains("--dark-bg:#f9e2af"), "{html}");
    assert!(html.contains("--light:#010203"), "{html}");
    assert!(html.contains("--dark:#010203"), "{html}");
    assert!(html.contains("#005f87"), "{html}");
    assert!(html.contains("font-style:italic"), "{html}");
    assert!(html.contains("font-weight:bold"), "{html}");
    assert!(html.contains("underline line-through"), "{html}");

    let tokens = highlighter.code_to_tokens(source, "ansi").unwrap();
    assert_eq!(
        tokens[0]
            .iter()
            .map(|token| token.content.as_str())
            .collect::<String>(),
        "plain styled truecolor"
    );
    let styled = tokens[0]
        .iter()
        .find(|token| token.content == "styled")
        .unwrap();
    assert_eq!(&*styled.color, "#d20f39");
    assert_eq!(styled.background.as_deref(), Some("#df8e1d"));
    assert!(styled.font_style.contains(shiki::FontStyle::BOLD));
    assert!(styled.font_style.contains(shiki::FontStyle::ITALIC));
    assert!(styled.font_style.contains(shiki::FontStyle::UNDERLINE));
    assert!(styled.font_style.contains(shiki::FontStyle::STRIKETHROUGH));
}

#[test]
fn ansi_state_crosses_lines_and_supports_reverse_and_dim() {
    let mut highlighter = Highlighter::builder()
        .languages(["ansi"])
        .theme(&shiki_themes::CATPPUCCIN_MOCHA)
        .build()
        .unwrap();
    let source =
        "\x1b[32mfirst\nsecond\x1b[0m \x1b[2;31mdim\x1b[0m \x1b[7;34mreverse";
    let lines = highlighter.code_to_tokens(source, "ansi").unwrap();

    assert_eq!(&*lines[0][0].color, "#a6e3a1");
    assert_eq!(&*lines[1][0].color, "#a6e3a1");
    let dim = lines[1]
        .iter()
        .find(|token| token.content == "dim")
        .unwrap();
    assert_eq!(&*dim.color, "#f38ba880");
    let reverse = lines[1]
        .iter()
        .find(|token| token.content == "reverse")
        .unwrap();
    assert_eq!(&*reverse.color, "#1e1e2e");
    assert_eq!(reverse.background.as_deref(), Some("#89b4fa"));
}

#[test]
fn astro_injection_enters_javascript_grammar() {
    let mut highlighter = Highlighter::builder()
        .bundle(&LANGUAGES)
        .languages(["astro"])
        .theme(&shiki_themes::generated::CATPPUCCIN_MOCHA)
        .build()
        .unwrap();
    let (_, state) = highlighter
        .tokenize_line("<script>", "astro", None, true)
        .unwrap();
    let (tokens, state) = highlighter
        .tokenize_line("const answer = \"yes\"", "astro", Some(&state), false)
        .unwrap();
    highlighter
        .tokenize_line("</script>", "astro", Some(&state), false)
        .unwrap();

    let keyword = tokens
        .iter()
        .find(|token| &"const answer = \"yes\""[token.range.clone()] == "const")
        .expect("javascript keyword token");
    let string = tokens
        .iter()
        .find(|token| &"const answer = \"yes\""[token.range.clone()] == "yes")
        .expect("javascript string token");
    assert_ne!(keyword.scopes, string.scopes, "{tokens:#?}");
}

#[test]
fn astro_dynamic_scope_capture_uses_numeric_identity() {
    let mut highlighter = Highlighter::builder()
        .bundle(&LANGUAGES)
        .languages(["astro"])
        .theme(&shiki_themes::generated::CATPPUCCIN_MOCHA)
        .build()
        .unwrap();

    let tokens = highlighter
        .code_to_scope_tokens("<script lang=\"foobar\">value</script>", "astro")
        .unwrap();

    assert!(
        tokens[0]
            .iter()
            .map(|token| token.scopes)
            .collect::<std::collections::HashSet<_>>()
            .len()
            > 1
    );
}

#[test]
fn renders_multiple_themes_as_css_variables() {
    let mut highlighter = Highlighter::builder()
        .bundle(&LANGUAGES)
        .languages(["rust"])
        .themes([
            ("dark", &shiki_themes::CATPPUCCIN_MOCHA),
            ("light", &shiki_themes::CATPPUCCIN_LATTE),
        ])
        .build()
        .unwrap();

    let tokens = highlighter
        .code_to_tokens_with_themes(r#"object.get("name")"#, "rust")
        .unwrap();
    assert_eq!(tokens[0][0].styles.len(), 2);

    let mut options = HtmlOptions {
        default_theme: Some("light".into()),
        ..HtmlOptions::default()
    };
    options.pre_classes.push("code-block".into());
    options.code_classes.push("language-rust".into());
    options
        .pre_attributes
        .insert("data-language".into(), "rust".into());
    options.include_line_wrapper = false;
    let html = highlighter
        .code_to_html_with_options(r#"object.get("name")"#, "rust", &options)
        .unwrap();

    assert!(html.contains("data-themes=\"dark light\""), "{html}");
    assert!(html.contains("--dark:#a6e3a1"), "{html}");
    assert!(html.contains("--light:#40a02b"), "{html}");
    assert!(html.contains("color:var(--light)"), "{html}");
    assert!(html.contains("class=\"shiki code-block\""), "{html}");
    assert!(html.contains("class=\"language-rust\""), "{html}");
    assert!(html.contains("data-language=\"rust\""), "{html}");
    assert!(!html.contains("class=\"line\""), "{html}");
}

#[test]
fn html_options_support_clean_and_lazy_static_configurations() {
    let clean_options = HtmlOptions::clean();

    let mut plain_options = HtmlOptions::new();
    plain_options.include_shiki_class = false;
    plain_options.include_theme_class = false;
    plain_options.include_data_themes = false;
    plain_options.include_root_style = false;
    plain_options.include_token_styles = false;
    plain_options.include_line_wrapper = false;

    let mut classless_line_options = HtmlOptions::clean();
    classless_line_options.line_class = None;
    classless_line_options.include_token_styles = false;

    static STATIC: std::sync::LazyLock<HtmlOptions> =
        std::sync::LazyLock::new(|| {
            let mut options = HtmlOptions::clean();
            options.pre_classes.push("static-pre".into());
            options.code_classes.push("static-code".into());
            options
                .pre_attributes
                .insert("data-owner".into(), "static".into());
            options
                .pre_attributes
                .insert("style".into(), "border:0".into());
            options
                .code_attributes
                .insert("aria-label".into(), "source".into());
            options.default_theme = Some("light".into());
            options.include_data_themes = true;
            options.include_line_wrapper = false;
            options
        });

    let mut highlighter = Highlighter::builder()
        .bundle(&LANGUAGES)
        .languages(["rust"])
        .themes([
            ("dark", &shiki_themes::CATPPUCCIN_MOCHA),
            ("light", &shiki_themes::CATPPUCCIN_LATTE),
        ])
        .build()
        .unwrap();

    let clean = highlighter
        .code_to_html_with_options("let value = 1;", "rust", &clean_options)
        .unwrap();
    assert!(
        clean.starts_with("<pre><code><span class=\"line\">"),
        "{clean}"
    );
    assert!(!clean.contains("data-themes="), "{clean}");
    assert!(!clean.starts_with("<pre class="), "{clean}");
    assert!(!clean.starts_with("<pre style="), "{clean}");
    assert!(clean.contains("<span style="), "{clean}");

    let plain = highlighter
        .code_to_html_with_options("let value = 1;", "rust", &plain_options)
        .unwrap();
    assert_eq!(plain, "<pre><code>let value = 1;</code></pre>");

    let classless_line = highlighter
        .code_to_html_with_options(
            "let value = 1;",
            "rust",
            &classless_line_options,
        )
        .unwrap();
    assert_eq!(
        classless_line,
        "<pre><code><span>let value = 1;</span></code></pre>"
    );

    let static_html = highlighter
        .code_to_html_with_options("let value = 1;", "rust", &STATIC)
        .unwrap();
    assert!(
        static_html.starts_with(
            "<pre class=\"static-pre\" data-owner=\"static\" data-themes=\"dark light\" style=\"border:0\"><code class=\"static-code\" aria-label=\"source\">"
        ),
        "{static_html}"
    );
    assert!(!static_html.contains("class=\"line\""), "{static_html}");
    assert!(static_html.contains("color:var(--light)"), "{static_html}");
}

#[test]
fn html_rejects_an_unknown_default_theme() {
    let mut highlighter = Highlighter::builder()
        .bundle(&LANGUAGES)
        .languages(["rust"])
        .theme(&shiki_themes::CATPPUCCIN_MOCHA)
        .build()
        .unwrap();
    let options = HtmlOptions {
        default_theme: Some("missing".into()),
        ..HtmlOptions::default()
    };

    let error = highlighter
        .code_to_html_with_options("let value = 1;", "rust", &options)
        .unwrap_err();
    assert!(
        matches!(error, shiki::Error::ThemeNotBundled(name) if name == "missing")
    );
}

#[test]
fn html_root_style_switches_are_independent() {
    let mut options = HtmlOptions::clean();
    options.include_root_style = true;
    options.include_theme_variables = false;
    options.include_background = true;
    options.include_foreground = true;
    options.include_line_wrapper = false;

    let mut highlighter = Highlighter::builder()
        .bundle(&LANGUAGES)
        .languages(["rust"])
        .themes([
            ("dark", &shiki_themes::CATPPUCCIN_MOCHA),
            ("light", &shiki_themes::CATPPUCCIN_LATTE),
        ])
        .build()
        .unwrap();
    let html = highlighter
        .code_to_html_with_options("let value = 1;", "rust", &options)
        .unwrap();

    assert!(
        html.starts_with(
            "<pre style=\"background-color:#1e1e2e;color:#cdd6f4;\"><code>"
        ),
        "{html}"
    );
    assert!(!html.starts_with("<pre style=\"--dark:"), "{html}");
}

#[test]
fn renders_compact_multi_theme_html() {
    const GRAMMAR: &str = r#"{
        "scopeName": "source.runtime",
        "patterns": [
            { "match": "\\bhello\\b", "name": "keyword.runtime" },
            { "match": " comment", "name": "comment.content.runtime" },
            { "match": " +", "name": "whitespace.runtime" },
            { "match": "//", "name": "comment.punctuation.runtime" },
            { "match": "\"", "name": "string.punctuation.runtime" },
            { "match": "\\bobject\\b", "name": "string.content.runtime" }
        ]
    }"#;
    const LIGHT: &str = r##"{
        "name": "light",
        "settings": [
            { "settings": { "foreground": "#7C7F93", "background": "#EFF1F5" } },
            { "scope": "keyword.runtime", "settings": { "foreground": "#DF8E1D", "fontStyle": "italic" } },
            { "scope": "comment.punctuation.runtime, comment.content.runtime", "settings": { "foreground": "#7C7F93", "fontStyle": "italic" } },
            { "scope": "string.punctuation.runtime, string.content.runtime", "settings": { "foreground": "#40A02B" } }
        ]
    }"##;
    const DARK: &str = r##"{
        "name": "dark",
        "settings": [
            { "settings": { "foreground": "#9399B2", "background": "#1E1E2E" } },
            { "scope": "keyword.runtime", "settings": { "foreground": "#F9E2AF", "fontStyle": "italic" } },
            { "scope": "comment.punctuation.runtime, comment.content.runtime", "settings": { "foreground": "#9399B2", "fontStyle": "italic" } },
            { "scope": "string.punctuation.runtime, string.content.runtime", "settings": { "foreground": "#A6E3A1" } }
        ]
    }"##;

    let mut highlighter = Highlighter::builder()
        .json_language("runtime", GRAMMAR)
        .unwrap()
        .json_theme("light", LIGHT)
        .unwrap()
        .json_theme("dark", DARK)
        .unwrap()
        .build()
        .unwrap();
    let options = HtmlOptions {
        include_default_theme_styles: false,
        include_line_wrapper: false,
        ..HtmlOptions::default()
    };
    let html = highlighter
        .code_to_html_with_options("hello world", "runtime", &options)
        .unwrap();

    assert!(
        html.contains(
            "--light:#DF8E1D;--light-font-style:italic;--dark:#F9E2AF;--dark-font-style:italic;"
        ),
        "{html}"
    );
    assert!(html.contains("--light:#7C7F93;--dark:#9399B2;"), "{html}");
    assert!(!html.contains("-font-style:normal"), "{html}");
    assert!(!html.contains("-font-weight:normal"), "{html}");
    assert!(!html.contains("-text-decoration:none"), "{html}");

    let default_options = HtmlOptions {
        include_line_wrapper: false,
        ..HtmlOptions::default()
    };
    let italic = highlighter
        .code_to_html_with_options("hello", "runtime", &default_options)
        .unwrap();
    assert!(
        italic.contains("font-style:var(--light-font-style);"),
        "{italic}"
    );
    let plain = highlighter
        .code_to_html_with_options("world", "runtime", &default_options)
        .unwrap();
    assert!(!plain.contains("-font-style:"), "{plain}");
    assert!(!plain.contains("font-style:var("), "{plain}");

    let comment = highlighter
        .code_to_html_with_options("        // comment", "runtime", &options)
        .unwrap();
    assert_eq!(comment.matches("<span style=").count(), 1, "{comment}");
    assert!(
        comment.contains(
            "<code>        <span style=\"--light:#7C7F93;--light-font-style:italic;--dark:#9399B2;--dark-font-style:italic;\">// comment</span></code>"
        ),
        "{comment}"
    );

    let string = highlighter
        .code_to_html_with_options("\"object\"", "runtime", &options)
        .unwrap();
    assert_eq!(string.matches("<span style=").count(), 1, "{string}");
    assert!(
        string.contains(
            "<span style=\"--light:#40A02B;--dark:#A6E3A1;\">\"object\"</span>"
        ),
        "{string}"
    );
}

#[test]
fn renderer_trait_supports_custom_outputs() {
    struct ScopeCount;

    impl Renderer for ScopeCount {
        type Output = usize;

        fn render(
            &mut self,
            highlighter: &mut Highlighter,
            code: &str,
            language: &str,
        ) -> shiki::Result<Self::Output> {
            let mut state = highlighter.initial_state(language)?;
            let mut tokens = Vec::new();
            let mut count = 0;
            let theme_count = highlighter.themes().len();
            for (line_index, line) in shiki::split_lines(code).enumerate() {
                highlighter.tokenize_line_into(
                    line,
                    language,
                    &mut state,
                    line_index == 0,
                    &mut tokens,
                )?;
                for token in &tokens {
                    assert_eq!(
                        highlighter
                            .token_styles(language, token.scopes)?
                            .count(),
                        theme_count
                    );
                    count += 1;
                }
            }
            Ok(count)
        }
    }

    let mut highlighter = Highlighter::builder()
        .bundle(&LANGUAGES)
        .languages(["rust"])
        .themes([
            ("light", &shiki_themes::generated::GITHUB_LIGHT),
            ("dark", &shiki_themes::generated::GITHUB_DARK),
        ])
        .build()
        .unwrap();
    let count = highlighter
        .render("let value = 1;", "rust", &mut ScopeCount)
        .unwrap();
    assert!(count > 1);

    let expected = highlighter.code_to_html("let value = 1;", "rust").unwrap();
    let options = HtmlOptions::default();
    let mut html = shiki::HtmlRenderer::new(&options);
    let actual = highlighter
        .render("let value = 1;", "rust", &mut html)
        .unwrap();
    assert_eq!(actual, expected);
}

#[test]
fn public_ansi_parser_reuses_state_across_lines() {
    let mut parser = shiki::ansi::AnsiParser::new();
    let first = "\x1b[31mred";
    let spans = parser.parse_line(first);
    assert_eq!(&first[spans[0].range.clone()], "red");
    assert!(spans[0].state.foreground().is_some());

    let second = "still red\x1b[0m plain";
    let spans = parser.parse_line(second);
    assert_eq!(&second[spans[0].range.clone()], "still red");
    assert!(spans[0].state.foreground().is_some());
    assert_eq!(&second[spans[1].range.clone()], " plain");
    assert!(!spans[1].state.has_explicit_style());
}

#[test]
fn generated_items_are_reexported() {
    assert_eq!(shiki_langs::RUST.id, "rust");
    assert_eq!(shiki_themes::CATPPUCCIN_MOCHA.id, "catppuccin-mocha");
}

#[test]
fn ansi_is_available_as_a_special_language_bundle() {
    static ANSI: LanguageBundle = shiki_langs::languages![ansi];
    let mut highlighter = Highlighter::builder()
        .bundle(&ANSI)
        .theme(&shiki_themes::MIN_DARK)
        .build()
        .unwrap();
    let tokens = highlighter
        .code_to_tokens("\x1b[31mred\x1b[0m", "ansi")
        .unwrap();

    assert_eq!(tokens[0][0].content, "red");
    assert_eq!(&*tokens[0][0].color, "#cd3131");
}

#[test]
fn loads_runtime_json_and_raw_definitions() {
    const GRAMMAR: &str = r#"{
        "scopeName": "source.runtime",
        "patterns": [{ "match": "\\bhello\\b", "name": "keyword.runtime" }]
    }"#;
    const THEME: &str = r##"{
        "name": "runtime",
        "settings": [
            { "settings": { "foreground": "#ffffff", "background": "#000000" } },
            { "scope": "keyword.runtime", "settings": { "foreground": "#ff0000" } }
        ]
    }"##;

    let mut from_json = Highlighter::builder()
        .json_language("runtime", GRAMMAR)
        .unwrap()
        .json_theme("runtime", THEME)
        .unwrap()
        .build()
        .unwrap();
    assert!(
        from_json
            .code_to_html("hello world", "runtime")
            .unwrap()
            .contains("#ff0000")
    );

    let grammar = shiki::RawGrammar::from_json("runtime", GRAMMAR).unwrap();
    let theme = shiki::RawTheme::from_json("runtime", THEME).unwrap();
    let mut from_raw = Highlighter::builder()
        .language("runtime", grammar)
        .theme(theme)
        .build()
        .unwrap();
    assert!(
        from_raw
            .code_to_html("hello world", "source.runtime")
            .unwrap()
            .contains("#ff0000")
    );
}

#[test]
fn rejects_state_from_another_language() {
    let mut highlighter = Highlighter::builder()
        .bundle(&LANGUAGES)
        .languages(["rust", "astro"])
        .theme(&shiki_themes::generated::GITHUB_DARK)
        .build()
        .unwrap();
    let (_, rust_state) = highlighter
        .tokenize_line("/* open", "rust", None, true)
        .unwrap();
    let error = highlighter
        .tokenize_line("const value = 1", "astro", Some(&rust_state), true)
        .unwrap_err();
    assert!(matches!(error, shiki::Error::GrammarStateMismatch));
}

#[test]
fn rejects_state_from_another_same_language_session() {
    let engine = Highlighter::builder()
        .bundle(&LANGUAGES)
        .languages(["rust"])
        .theme(&shiki_themes::generated::GITHUB_DARK)
        .build_engine()
        .unwrap();
    let mut first = engine.session("rust").unwrap();
    let mut second = engine.session("rust").unwrap();
    let mut foreign_state = first.initial_state();

    first
        .tokenize_line("/* open", &mut foreign_state, true)
        .unwrap();

    let error = second
        .tokenize_line("let value = 1;", &mut foreign_state, true)
        .unwrap_err();
    assert!(matches!(error, shiki::Error::GrammarStateMismatch));
    let continued = first
        .tokenize_line("close */ let value = 1;", &mut foreign_state, false)
        .unwrap();
    assert!(continued.len() > 1, "{continued:#?}");
}

#[test]
fn rejects_state_after_clearing_language_cache() {
    let mut highlighter = Highlighter::builder()
        .bundle(&LANGUAGES)
        .languages(["rust"])
        .theme(&shiki_themes::generated::GITHUB_DARK)
        .build()
        .unwrap();
    let (_, stale_state) = highlighter
        .tokenize_line("/* open", "rust", None, true)
        .unwrap();
    highlighter.clear_language_cache("rust").unwrap();

    let error = highlighter
        .tokenize_line("close */", "rust", Some(&stale_state), false)
        .unwrap_err();
    assert!(matches!(error, shiki::Error::GrammarStateMismatch));
}

#[test]
fn shared_engine_creates_isolated_sessions() {
    let engine = Highlighter::builder()
        .bundle(&LANGUAGES)
        .languages(["rust"])
        .theme(&shiki_themes::generated::GITHUB_DARK)
        .build_engine()
        .unwrap();
    let mut first = engine.session("rust").unwrap();
    let mut second = engine.session("rust").unwrap();
    let mut first_state = first.initial_state();
    let mut second_state = second.initial_state();
    let first_tokens = first
        .tokenize_line("/* open", &mut first_state, true)
        .unwrap();
    let second_tokens = second
        .tokenize_line("let value = 1;", &mut second_state, true)
        .unwrap();
    assert!(!first_tokens.is_empty());
    assert!(second_tokens.len() > 1);
}

#[test]
fn highlighter_initializes_language_caches_lazily() {
    let engine = Highlighter::builder()
        .bundle(&LANGUAGES)
        .languages(["rust", "astro"])
        .theme(&shiki_themes::generated::GITHUB_DARK)
        .build_engine()
        .unwrap();
    let mut highlighter = engine.highlighter();
    assert_eq!(highlighter.initialized_language_count(), 0);
    highlighter
        .tokenize_line("let value = 1;", "rust", None, true)
        .unwrap();
    assert_eq!(highlighter.initialized_language_count(), 1);
    assert!(highlighter.is_language_initialized("rust").unwrap());
    assert!(!highlighter.is_language_initialized("astro").unwrap());
    let stats = highlighter.cache_stats("rust").unwrap().unwrap();
    assert!(stats.scanners > 0);
    assert!(stats.regexes > 0);
    assert!(stats.reusable_buffer_bytes > 0);
}

#[test]
#[ignore = "expensive compatibility sweep; run explicitly before releases"]
fn all_generated_grammars_compile_root_scanner() {
    let ids: Vec<_> = shiki_langs::generated::ALL_LANGUAGES
        .iter()
        .map(|language| language.id)
        .collect();
    let all = shiki_langs::all();
    let mut highlighter = Highlighter::builder()
        .bundle(&all)
        .theme(&shiki_themes::generated::GITHUB_DARK)
        .build()
        .unwrap();

    for id in ids {
        highlighter
            .tokenize_line("", id, None, true)
            .unwrap_or_else(|error| panic!("{id}: {error}"));
    }
}
