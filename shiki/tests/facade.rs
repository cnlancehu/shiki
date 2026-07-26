#[test]
fn reexports_core_api_types() {
    let builder: shiki_core::HighlighterBuilder = shiki::Highlighter::builder();
    let limits: shiki_core::RegexLimits = shiki::RegexLimits::default();
    let parser: shiki_core::ansi::AnsiParser = shiki::ansi::AnsiParser::new();

    std::hint::black_box((builder, limits, parser));
}

#[cfg(feature = "langs")]
#[test]
fn langs_feature_exposes_the_language_catalog() {
    static LANGUAGES: shiki::LanguageBundle = shiki::langs::languages![rust];

    let rust: &'static shiki::LanguageDefinition = &shiki::langs::RUST;
    assert_eq!(rust.id, "rust");
    assert!(
        LANGUAGES
            .definitions()
            .any(|language| language.id == "rust")
    );
}

#[cfg(feature = "themes")]
#[test]
fn themes_feature_exposes_the_theme_catalog() {
    let theme: &'static shiki::ThemeDefinition =
        &shiki::themes::CATPPUCCIN_MOCHA;

    assert_eq!(theme.id, "catppuccin-mocha");
}

#[cfg(feature = "macros")]
#[test]
fn macros_feature_exposes_root_and_module_macros() {
    let mut highlighter = shiki::highlighter! {
        languages: ["text"],
        themes: [("dark", "github-dark")],
    };
    let html = highlighter.code_to_html("<plain>", "text").unwrap();
    assert!(html.contains("&lt;plain&gt;"), "{html}");

    let engine = shiki::macros::highlighter_engine! {
        languages: ["text"],
        themes: [("dark", "github-dark")],
    };
    let _session = engine.session("text").unwrap();

    let root_engine = shiki::highlighter_engine! {
        languages: ["text"],
        themes: [("dark", "github-dark")],
    };
    let _root_session = root_engine.session("text").unwrap();
}
