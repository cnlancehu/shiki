use shiki::{Highlighter, HtmlOptions, LanguageBundle};

use super::LANGUAGES;

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
