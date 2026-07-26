extern crate shiki_core as shiki;

use std::time::Instant;

use shiki::Highlighter;

fn make_highlighter() -> Highlighter {
    shiki_macros::highlighter! {
        languages: ["javascript"],
        themes: [("default", "catppuccin-mocha")],
    }
}

fn main() {
    let started = Instant::now();
    let first = make_highlighter();
    let cold = started.elapsed();
    let started = Instant::now();
    let second = make_highlighter();
    let warm = started.elapsed();
    std::hint::black_box((first, second));
    println!("generated_rust_cold={cold:?} generated_rust_warm={warm:?}");
}
