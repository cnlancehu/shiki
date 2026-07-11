use std::env;
use std::fs;
use std::time::{Duration, Instant};

use shiki::{Highlighter, LanguageBundle};

static LANGUAGES: LanguageBundle = shiki_langs::languages![javascript];

fn main() -> shiki::Result<()> {
    let path = env::args().nth(1).expect("usage: diagnose <path>");
    if path == "--synthetic" {
        const STATEMENT: &str = "const value=items.map(item=>item.name).filter(Boolean).join(',');";
        let compact = STATEMENT.repeat(800);
        let formatted = format!("{STATEMENT}\n").repeat(800);
        diagnose("synthetic-compact", &compact)?;
        diagnose("synthetic-formatted", &formatted)?;
        return Ok(());
    }
    let source = fs::read_to_string(&path).expect("failed to read input");
    diagnose(&path, &source)
}

fn diagnose(label: &str, source: &str) -> shiki::Result<()> {
    let mut highlighter = Highlighter::builder()
        .bundle(&LANGUAGES)
        .languages(["javascript"])
        .theme(&shiki_themes::CATPPUCCIN_MOCHA)
        .build()?;

    let mut state = None;
    let mut total = Duration::ZERO;
    let mut token_count = 0;
    let mut lines = Vec::new();
    for (index, line) in source.lines().enumerate() {
        let started = Instant::now();
        let (tokens, next) =
            highlighter.tokenize_line(line, "javascript", state.as_ref(), index == 0)?;
        let elapsed = started.elapsed();
        state = Some(next);
        total += elapsed;
        token_count += tokens.len();
        lines.push((elapsed, index + 1, line.len(), tokens.len()));
    }
    lines.sort_unstable_by_key(|line| std::cmp::Reverse(line.0));

    println!(
        "{label}: bytes={} lines={} tokens={} tokenize={total:?}",
        source.len(),
        lines.len(),
        token_count
    );
    for (elapsed, line, bytes, tokens) in lines.into_iter().take(10) {
        println!("line={line} bytes={bytes} tokens={tokens} time={elapsed:?}");
    }
    Ok(())
}
