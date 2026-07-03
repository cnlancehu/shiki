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
    let themed_tokens = highlighter.code_to_tokens(jquery, "javascript").unwrap();
    let themed = started.elapsed();
    std::hint::black_box(themed_tokens);

    println!(
        "build={built:?} cached_build={cached_build:?} cold_html={cold_html:?} tokenize={tokenized:?} cached_tokenize={cached_tokenized:?} themed={themed:?} lines={} tokens={token_count} html_bytes={}",
        tokens.len(),
        html.len(),
    );
}
