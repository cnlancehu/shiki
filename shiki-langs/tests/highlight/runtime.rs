use shiki_core::{Highlighter, LanguageBundle};

#[test]
fn public_ansi_parser_reuses_state_across_lines() {
    let mut parser = shiki_core::ansi::AnsiParser::new();
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

    let grammar =
        shiki_core::RawGrammar::from_json("runtime", GRAMMAR).unwrap();
    let theme = shiki_core::RawTheme::from_json("runtime", THEME).unwrap();
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
