use shiki_macros::{highlighter, highlighter_engine};

#[test]
fn creates_a_highlighter_from_generated_rust() {
    let mut highlighter = highlighter! {
        languages: ["rust"],
        themes: [("dark", "catppuccin-mocha")],
    };
    let html = highlighter
        .code_to_html("fn main() { println!(\"hello\"); }", "rust")
        .unwrap();
    assert!(html.contains("hello"));
    assert!(html.contains("#"));
}

#[test]
fn creates_independent_sessions_from_a_shared_engine() {
    let engine = highlighter_engine! {
        languages: ["rust"],
        themes: [("dark", "github-dark")],
    };
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
fn loads_precompiled_grammars_on_first_use() {
    let engine = highlighter_engine! {
        languages: ["rust", "javascript"],
        themes: [("dark", "github-dark")],
    };

    assert_eq!(engine.loaded_language_count(), 0);
    let _rust = engine.session("rust").unwrap();
    assert_eq!(engine.loaded_language_count(), 1);
    let _javascript = engine.session("javascript").unwrap();
    assert_eq!(engine.loaded_language_count(), 2);
}

#[test]
fn snapshot_roundtrip_is_deterministic() {
    let engine = highlighter_engine! {
        languages: ["javascript"],
        themes: [("dark", "catppuccin-mocha")],
    };
    let snapshot = engine.__to_snapshot();
    let restored = shiki::HighlighterEngine::__from_snapshot(&snapshot);

    assert_eq!(restored.language_count(), engine.language_count());
    assert_eq!(restored.__to_snapshot(), snapshot);
}
