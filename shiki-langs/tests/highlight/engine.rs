use shiki_core::Highlighter;

use super::LANGUAGES;

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
    assert!(matches!(error, shiki_core::Error::GrammarStateMismatch));
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
    assert!(matches!(error, shiki_core::Error::GrammarStateMismatch));
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
    assert!(matches!(error, shiki_core::Error::GrammarStateMismatch));
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
    assert_eq!(engine.loaded_language_count(), 0);
    assert_eq!(engine.regex_cache_stats().entries, 0);
    assert_eq!(highlighter.initialized_language_count(), 0);
    highlighter
        .tokenize_line("let value = 1;", "rust", None, true)
        .unwrap();
    assert_eq!(highlighter.initialized_language_count(), 1);
    assert!(highlighter.is_language_initialized("rust").unwrap());
    assert!(!highlighter.is_language_initialized("astro").unwrap());
    let stats = highlighter.cache_stats("rust").unwrap().unwrap();
    assert!(stats.scanners > 0);
    assert!(stats.shared_regex_slots > 0);
    assert!(stats.reusable_buffer_bytes > 0);
    assert_eq!(engine.loaded_language_count(), 1);
    assert!(engine.regex_cache_stats().successful_compiles > 0);
}

#[test]
fn runtime_all_catalog_is_lazy_and_sessions_share_static_regexes() {
    let engine = Highlighter::builder()
        .bundle(&shiki_langs::all())
        .theme(&shiki_themes::generated::GITHUB_DARK)
        .build_engine()
        .unwrap();
    assert_eq!(engine.loaded_language_count(), 0);
    assert_eq!(engine.regex_cache_stats().entries, 0);

    let mut first = engine.session("rust").unwrap();
    let mut first_state = first.initial_state();
    first
        .tokenize_line(
            "fn main() { println!(\"hello\"); }",
            &mut first_state,
            true,
        )
        .unwrap();
    let warmed = engine.regex_cache_stats();
    assert_eq!(engine.loaded_language_count(), 1);
    assert!(warmed.successful_compiles > 0);

    drop(first);
    let mut second = engine.session("rs").unwrap();
    let mut second_state = second.initial_state();
    second
        .tokenize_line(
            "fn main() { println!(\"hello\"); }",
            &mut second_state,
            true,
        )
        .unwrap();
    let reused = engine.regex_cache_stats();
    assert_eq!(reused.successful_compiles, warmed.successful_compiles);
    assert!(reused.cache_hits > warmed.cache_hits);
}

#[test]
fn a_session_keeps_shared_regexes_alive_after_its_engine_is_dropped() {
    let engine = Highlighter::builder()
        .bundle(&LANGUAGES)
        .languages(["rust"])
        .theme(&shiki_themes::generated::GITHUB_DARK)
        .build_engine()
        .unwrap();
    let mut session = engine.session("rust").unwrap();
    drop(engine);

    let mut state = session.initial_state();
    let tokens = session
        .tokenize_line("let answer = 42;", &mut state, true)
        .unwrap();
    assert!(!tokens.is_empty());
}

#[test]
fn long_matches_are_not_truncated_at_the_old_scanner_window() {
    let grammar = shiki_core::RawGrammar::from_json(
        "long-match",
        r#"{
            "scopeName": "source.long-match",
            "patterns": [
                { "match": "a{128}b", "name": "constant.long-match" },
                { "match": "a+", "name": "constant.short-match" }
            ]
        }"#,
    )
    .unwrap();
    let mut highlighter = Highlighter::builder()
        .language("long-match", grammar)
        .theme(&shiki_themes::generated::GITHUB_DARK)
        .build()
        .unwrap();
    let source = format!("{}{}b", "界".repeat(30), "a".repeat(128));
    let tokens = highlighter
        .code_to_scope_tokens(&source, "long-match")
        .unwrap();
    let long = tokens[0]
        .iter()
        .find(|token| token.range.start == "界".len() * 30)
        .expect("long match token");
    assert_eq!(long.range.end, source.len());
    let scopes = highlighter.scope_names("long-match", long.scopes).unwrap();
    assert_eq!(
        scopes.last().map(String::as_str),
        Some("constant.long-match")
    );
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
