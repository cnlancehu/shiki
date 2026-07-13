use shiki::{Highlighter, HtmlOptions, LanguageBundle, Renderer};

static LANGUAGES: LanguageBundle = shiki_langs::languages![astro, rust, vue];

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

    let html = highlighter
        .code_to_html_with_options(
            r#"object.get("name")"#,
            "rust",
            &HtmlOptions::default()
                .default_theme("light")
                .pre_class("code-block")
                .code_class("language-rust")
                .pre_attribute("data-language", "rust")
                .without_line_wrapper(),
        )
        .unwrap();

    assert!(!html.contains("data-themes="), "{html}");
    assert!(html.contains("--dark:#a6e3a1"), "{html}");
    assert!(html.contains("--light:#40a02b"), "{html}");
    assert!(html.contains("color:var(--light)"), "{html}");
    assert!(html.contains("class=\"code-block\""), "{html}");
    assert!(html.contains("class=\"language-rust\""), "{html}");
    assert!(html.contains("data-language=\"rust\""), "{html}");
    assert!(!html.contains("class=\"line\""), "{html}");
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
    let html = highlighter
        .code_to_html_with_options(
            "hello world",
            "runtime",
            &HtmlOptions::default()
                .variables_only()
                .without_line_wrapper(),
        )
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

    let italic = highlighter
        .code_to_html_with_options(
            "hello",
            "runtime",
            &HtmlOptions::default().without_line_wrapper(),
        )
        .unwrap();
    assert!(
        italic.contains("font-style:var(--light-font-style);"),
        "{italic}"
    );
    let plain = highlighter
        .code_to_html_with_options(
            "world",
            "runtime",
            &HtmlOptions::default().without_line_wrapper(),
        )
        .unwrap();
    assert!(!plain.contains("-font-style:"), "{plain}");
    assert!(!plain.contains("font-style:var("), "{plain}");

    let options = HtmlOptions::default()
        .variables_only()
        .without_line_wrapper();
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
            Ok(highlighter
                .code_to_scope_tokens(code, language)?
                .iter()
                .map(Vec::len)
                .sum())
        }
    }

    let mut highlighter = Highlighter::builder()
        .bundle(&LANGUAGES)
        .languages(["rust"])
        .theme(&shiki_themes::generated::GITHUB_DARK)
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
fn generated_items_are_reexported() {
    assert_eq!(shiki_langs::RUST.id, "rust");
    assert_eq!(shiki_themes::CATPPUCCIN_MOCHA.id, "catppuccin-mocha");
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
