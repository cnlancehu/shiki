use std::time::Instant;

use shiki::{Highlighter, LanguageBundle};

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
    let mut highlighter = Highlighter::builder()
        .bundle(&LANGUAGES)
        .languages(["javascript"])
        .theme(&shiki_themes::CATPPUCCIN_MOCHA)
        .build()
        .unwrap();
    let built = started.elapsed();

    let started = Instant::now();
    let html = highlighter.code_to_html(jquery, "javascript").unwrap();
    let cold_html = started.elapsed();

    let started = Instant::now();
    let mut highlighter = Highlighter::builder()
        .bundle(&LANGUAGES)
        .languages(["javascript"])
        .theme(&shiki_themes::CATPPUCCIN_MOCHA)
        .build()
        .unwrap();
    let cached_build = started.elapsed();

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

    println!(
        "build={built:?} cached_build={cached_build:?} cold_html={cold_html:?} tokenize={tokenized:?} cached_tokenize={cached_tokenized:?} themed={themed:?} lines={} tokens={token_count} html_bytes={} scanners={} regexes={} dynamic_patterns={} scope_stacks={} capture_values={} style_rows={} reusable_buffer_bytes={} themed_token_bytes={} theme_style_bytes={}",
        tokens.len(),
        html.len(),
        cache.scanners,
        cache.regexes,
        cache.dynamic_patterns,
        cache.scope_stacks,
        cache.capture_values,
        cache.style_rows,
        cache.reusable_buffer_bytes,
        std::mem::size_of::<shiki::ThemedToken>(),
        std::mem::size_of::<shiki::ThemeTokenStyle>(),
    );
}
