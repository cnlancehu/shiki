use std::time::Instant;

fn main() {
    let started = Instant::now();
    let engine = shiki_macros::highlighter_engine! {
        languages: all,
        themes: [
            ("dark", "catppuccin-mocha"),
            ("light", "catppuccin-latte"),
        ],
    };
    let initialized = started.elapsed();
    let loaded_before_highlight = engine.loaded_language_count();

    let started = Instant::now();
    let mut highlighter = engine.highlighter();
    let html = highlighter
        .code_to_html("fn main() { println!(\"hello\"); }", "rust")
        .unwrap();
    let first_highlight = started.elapsed();

    println!(
        "languages={} loaded_before_highlight={loaded_before_highlight} loaded_after_highlight={} initialized={initialized:?} first_highlight={first_highlight:?} html_bytes={}",
        engine.language_count(),
        engine.loaded_language_count(),
        html.len(),
    );
}
