use shiki_core::{Highlighter, HtmlOptions, Renderer};

use super::LANGUAGES;

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
    assert!(static_html.contains("--light:"), "{static_html}");
    assert!(!static_html.contains("color:var(--light)"), "{static_html}");
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
        matches!(error, shiki_core::Error::ThemeNotBundled(name) if name == "missing")
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
        ) -> shiki_core::Result<Self::Output> {
            let mut state = highlighter.initial_state(language)?;
            let mut tokens = Vec::new();
            let mut count = 0;
            let theme_count = highlighter.themes().len();
            for (line_index, line) in shiki_core::split_lines(code).enumerate()
            {
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
    let mut html = shiki_core::HtmlRenderer::new(&options);
    let actual = highlighter
        .render("let value = 1;", "rust", &mut html)
        .unwrap();
    assert_eq!(actual, expected);
}
