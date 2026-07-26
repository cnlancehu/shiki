use std::time::Instant;

use shiki_core::{Highlighter, LanguageBundle};

static LANGUAGES: LanguageBundle = shiki_langs::languages![javascript];
const SOURCE: &str = r#"function render(items) {
  return items.map((item, index) => ({
    id: index,
    label: `item-${item.name}`,
    active: item.enabled && index % 2 === 0,
  })).filter(item => item.active);
}
"#;

fn main() {
    let jquery = SOURCE.repeat(1_500);
    let jquery = jquery.as_str();
    let started = Instant::now();
    let engine = Highlighter::builder()
        .bundle(&LANGUAGES)
        .languages(["javascript"])
        .theme(&shiki_themes::CATPPUCCIN_MOCHA)
        .build_engine()
        .unwrap();
    let built = started.elapsed();
    let loaded_before = engine.loaded_language_count();
    let mut highlighter = engine.highlighter();

    let started = Instant::now();
    let html = highlighter.code_to_html(jquery, "javascript").unwrap();
    let cold_html = started.elapsed();
    let cold_regexes = engine.regex_cache_stats();

    let started = Instant::now();
    let mut highlighter = engine.highlighter();
    let new_session = started.elapsed();

    let started = Instant::now();
    let tokens = highlighter
        .code_to_scope_tokens(jquery, "javascript")
        .unwrap();
    let tokenized = started.elapsed();
    let token_count: usize = tokens.iter().map(Vec::len).sum();

    let started = Instant::now();
    let cached_tokens = highlighter
        .code_to_scope_tokens(jquery, "javascript")
        .unwrap();
    let cached_tokenized = started.elapsed();
    std::hint::black_box(cached_tokens);

    let started = Instant::now();
    let themed_tokens =
        highlighter.code_to_tokens(jquery, "javascript").unwrap();
    let themed = started.elapsed();
    std::hint::black_box(themed_tokens);
    let cache = highlighter.cache_stats("javascript").unwrap().unwrap();
    let warm_regexes = engine.regex_cache_stats();

    println!(
        "build={built:?} new_session={new_session:?} cold_html={cold_html:?} tokenize={tokenized:?} cached_tokenize={cached_tokenized:?} themed={themed:?} loaded_before={loaded_before} loaded_after={} shared_regexes={} regex_compiles={} regex_hits={} cold_regex_compiles={} lines={} tokens={token_count} html_bytes={} scanners={} grammar_regex_slots={} dynamic_regexes={} dynamic_patterns={} scope_stacks={} capture_values={} style_rows={} reusable_buffer_bytes={} themed_token_bytes={} theme_style_bytes={}",
        engine.loaded_language_count(),
        warm_regexes.entries,
        warm_regexes.successful_compiles,
        warm_regexes.cache_hits,
        cold_regexes.successful_compiles,
        tokens.len(),
        html.len(),
        cache.scanners,
        cache.shared_regex_slots,
        cache.dynamic_regexes,
        cache.dynamic_patterns,
        cache.scope_stacks,
        cache.capture_values,
        cache.style_rows,
        cache.reusable_buffer_bytes,
        std::mem::size_of::<shiki_core::ThemedToken>(),
        std::mem::size_of::<shiki_core::ThemeTokenStyle>(),
    );
}
