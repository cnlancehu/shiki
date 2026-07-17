# shiki-rs

<p align="center">
  <a href="https://crates.io/crates/shiki"><img alt="shiki on crates.io" src="https://img.shields.io/crates/v/shiki"></a>
  <a href="https://docs.rs/shiki"><img alt="shiki documentation" src="https://img.shields.io/docsrs/shiki"></a>
</p>

**Fast TextMate highlighting for Rust.**

`shiki-rs` is a native Rust TextMate tokenizer and Shiki-compatible syntax
highlighter. It uses Oniguruma directly, supports embedded grammars and
injection selectors, provides bundled languages and themes, and can generate a
precompiled highlighter as a compact binary snapshot.

> The project is published, but the API is still experimental while
> compatibility and performance work continues.

## Crates

| Crate                                                   | Purpose                                                              |
| ------------------------------------------------------- | -------------------------------------------------------------------- |
| [`shiki`](https://crates.io/crates/shiki)               | Core tokenizer, themes, engines, sessions, token APIs, and renderers |
| [`shiki-langs`](https://crates.io/crates/shiki-langs)   | 253 bundled TextMate language grammars                               |
| [`shiki-themes`](https://crates.io/crates/shiki-themes) | 65 bundled Shiki/TextMate themes                                     |
| [`shiki-macros`](https://crates.io/crates/shiki-macros) | Compile-time highlighter generation                                  |

## Installation

For normal runtime construction with bundled languages and themes:

```console
cargo add shiki shiki-langs shiki-themes
```

For compile-time generated highlighters:

```console
cargo add shiki shiki-macros
```

## Quick start

Bundle only the languages the application needs:

```rust
use shiki::{Highlighter, LanguageBundle};

static LANGUAGES: LanguageBundle = shiki_langs::languages![rust];

fn main() -> shiki::Result<()> {
    let mut highlighter = Highlighter::builder()
        .bundle(&LANGUAGES)
        .languages(["rust"])
        .theme(&shiki_themes::CATPPUCCIN_MOCHA)
        .build()?;

    let html = highlighter.code_to_html("let answer = 42;", "rust")?;
    println!("{html}");
    Ok(())
}
```

### Plain-text fallback

Every engine reserves `text`, `txt`, and `plain` for unhighlighted input. Plain
text still receives HTML escaping and the selected theme's default styles, but
each non-empty line remains a single token:

```rust
let html = highlighter.code_to_html(user_source, "text")?;
```

This fallback is always available, including in engines generated without an
explicit plain-text entry.

### ANSI terminal output

`ansi` is also a built-in special language and does not require a grammar
bundle. ANSI control sequences are removed and their visual state is rendered
with the selected theme's `terminal.ansi*` palette (or VS Code-compatible
fallbacks):

```rust
let html = highlighter.code_to_html(
    "\x1b[1;32mcompleted\x1b[0m in 12ms",
    "ansi",
)?;
```

Standard and bright colors, 256-color and TrueColor sequences, foreground and
background colors, reverse video, dim text, bold, italic, underline, and
strikethrough are supported. ANSI state is retained across source lines and
works with both single-theme and multi-theme HTML. `code_to_tokens` and
`code_to_tokens_with_themes` also accept `ansi`; those tokens have no TextMate
scope stack and use scope ID `0`.

`languages!` accepts generated identifiers and aliases. It also includes every
transitive grammar dependency required by the selected roots. For example,
selecting Vue includes the grammars injected into Vue documents.

```rust
static WEB_LANGUAGES: shiki::LanguageBundle =
    shiki_langs::languages![html, css, javascript, typescript, vue];
```

The `.languages(...)` builder call chooses which bundled roots are enabled in a
particular engine. Omitting it enables every root in that bundle.

## Every bundled language

Use `shiki_langs::all()` when dynamic language selection matters more than
startup cost and memory:

```rust
use shiki::Highlighter;

fn build_all() -> shiki::Result<shiki::HighlighterEngine> {
    let languages = shiki_langs::all();
    Highlighter::builder()
        .bundle(&languages)
        .theme(&shiki_themes::CATPPUCCIN_MOCHA)
        .build_engine()
}
```

An engine initializes each language tokenizer lazily. The immutable compiled
grammar IR is shared, while scanners, regex caches, scope stacks, and style rows
are created only for languages that are used.

## Multiple themes

Theme output names become CSS variable prefixes:

```rust
use shiki::{Highlighter, LanguageBundle};

static LANGUAGES: LanguageBundle = shiki_langs::languages![rust];

let mut highlighter = Highlighter::builder()
    .bundle(&LANGUAGES)
    .languages(["rust"])
    .themes([
        ("dark", &shiki_themes::CATPPUCCIN_MOCHA),
        ("light", &shiki_themes::CATPPUCCIN_LATTE),
    ])
    .build()?;

let html = highlighter.code_to_html("let themed = true;", "rust")?;
```

Tokens contain variables such as `--dark`, `--light`, and only the font or
background variables that are actually needed. The HTML renderer merges
adjacent tokens with the same resolved visual style and avoids wrapping plain
whitespace when doing so has no visual effect.

`HtmlOptions` controls:

- explicit `<pre>` and `<code>` classes and attributes;
- the automatic `shiki` class, single-theme class, and multi-theme
  `data-themes` attribute independently;
- line wrappers and their class;
- the default theme used for inline fallback declarations;
- the root `style` attribute, theme variables, foreground, and background
  independently;
- whether styled token spans are emitted;
- variable-only token styles for application-managed theme switching.

```rust
let mut options = shiki::HtmlOptions::default();
options.default_theme = Some("light".into());
options.pre_classes.push("code-block".into());
options.code_classes.push("language-rust".into());
options
    .pre_attributes
    .insert("data-language".into(), "rust".into());
options.include_line_wrapper = false;
options.include_default_theme_styles = false;

let html = highlighter.code_to_html_with_options(
    "let value = 1;",
    "rust",
    &options,
)?;
```

`HtmlOptions::clean()` produces the minimal wrapper configuration: no automatic
`<pre>`/`<code>` classes, attributes, or root styles, while retaining the
`line` wrapper and syntax-highlighted token styles.

The options deliberately use standard owned collections. Use `LazyLock` when a
custom configuration should be initialized once and shared globally:

```rust
use std::sync::LazyLock;

static HTML: LazyLock<shiki::HtmlOptions> = LazyLock::new(|| {
    let mut options = shiki::HtmlOptions::clean();
    options.pre_classes.push("code-block".into());
    options.code_classes.push("language-rust".into());
    options
        .pre_attributes
        .insert("data-language".into(), "rust".into());
    options.include_data_themes = true;
    options.include_line_wrapper = false;
    options
});
```

The automatic-output switches are `include_shiki_class`,
`include_theme_class`, `include_data_themes`, `include_line_wrapper`,
`include_root_style`, `include_theme_variables`, `include_background`,
`include_foreground`, `include_default_theme_styles`, and
`include_token_styles`. Set `line_class` to `None` to retain the line wrapper
without its class, or set `include_line_wrapper` to `false` to remove it.

## Token APIs

Choose the least expensive representation that fits the consumer:

- `code_to_html` streams tokenization directly into compact HTML.
- `code_to_html_with_options` customizes the built-in HTML renderer.
- `code_to_scope_tokens` returns source ranges and compact `ScopeStackId`s.
- `code_to_tokens` returns owned tokens resolved against one theme.
- `code_to_tokens_with_themes` returns owned tokens for every configured theme.
- `tokenize_line` and `GrammarState` support incremental documents and editors.
- `tokenize_line_into` reuses caller-owned storage in streaming renderers.
- `token_styles` resolves all configured themes without allocating.
- `ansi::AnsiParser` exposes the stateful ANSI path to custom renderers.

## Shared engines and sessions

Reuse a `Highlighter` for one long-lived workflow, or share a
`HighlighterEngine` across independent documents. Engines share immutable
grammar and theme data. Each `LanguageSession` owns its mutable scanner and
scope caches, so sessions may advance independently without sharing document
state.

```rust
let engine = shiki::Highlighter::builder()
    .bundle(&LANGUAGES)
    .languages(["rust"])
    .theme(&shiki_themes::CATPPUCCIN_MOCHA)
    .build_engine()?;

let mut session = engine.session("rust")?;
let mut state = session.initial_state();

let first = session.tokenize_line("/* open", &mut state, true)?;
let second = session.tokenize_line("close */", &mut state, false)?;
```

Use `cache_stats` to inspect initialized caches. A reusable `Highlighter` can
release them with `clear_language_cache` or `clear_all_caches`.

## Custom renderers

`Renderer` separates tokenization from output. `HtmlRenderer` is the built-in
streaming implementation; structured data or application-specific renderers
can implement the same trait.

```rust
use shiki::{Highlighter, Renderer};

struct TokenCount;

impl Renderer for TokenCount {
    type Output = usize;

    fn render(
        &mut self,
        highlighter: &mut Highlighter,
        code: &str,
        language: &str,
    ) -> shiki::Result<Self::Output> {
        let mut state = highlighter.initial_state(language)?;
        let mut tokens = Vec::new();
        let mut count = 0;
        for (line_index, line) in shiki::split_lines(code).enumerate() {
            highlighter.tokenize_line_into(
                line,
                language,
                &mut state,
                line_index == 0,
                &mut tokens,
            )?;
            for token in &tokens {
                // Iterates borrowed styles; no per-token style Vec is built.
                count += highlighter
                    .token_styles(language, token.scopes)?
                    .count();
            }
        }
        Ok(count)
    }
}
```

The public `ThemeInfo` iterator provides theme names, CSS-safe names, and raw
theme palettes. ANSI renderers can use `ansi::AnsiParser::parse_line`; it
retains SGR state between lines and reuses its span buffer.

## Runtime grammars and themes

The default `json` feature supports runtime TextMate grammar and theme JSON:

```rust
let highlighter = shiki::Highlighter::builder()
    .json_language("custom", grammar_json)?
    .json_theme("custom", theme_json)?
    .build()?;
```

Definitions may also be parsed or constructed separately with `RawGrammar`,
`RawRule`, `RawTheme`, `RawThemeRule`, and their nested raw container types.
`LanguageInput` adds aliases and external injection targets to runtime
grammars.

Disable JSON support when only a generated snapshot is needed:

```console
cargo add shiki --no-default-features
```

Without default features, target-side `shiki` does not depend on `serde` or
`serde_json`. Snapshot loading uses the small, pure-Rust `lz4_flex` block
decoder.

## Compile-time highlighters

`shiki-macros` resolves and compiles bundled grammars and themes while the proc
macro runs, serializes the resulting IR into a versioned binary snapshot with
independently LZ4-compressed grammar blocks, and emits that snapshot as one byte
string. The target compiler no longer has to type-check and generate code for
every grammar rule. A `LazyLock` restores the small language index and selected
themes once; each grammar IR is decoded only when that language is first used.
Runtime grammar JSON parsing is not used.

```rust
let mut highlighter = shiki_macros::highlighter! {
    languages: ["rust", "javascript"],
    themes: [
        ("dark", "catppuccin-mocha"),
        ("light", "catppuccin-latte"),
    ],
};

let html = highlighter.code_to_html("const value = 1", "javascript")?;
```

Use `highlighter_engine!` to obtain a cloneable shared engine instead. Grammar
IR and native Oniguruma scanners both initialize lazily per language. Scanners
cannot be embedded because they contain process-local pointers.

Use `languages: all` to select every bundled language:

```rust
let engine = shiki_macros::highlighter_engine! {
    languages: all,
    themes: [("dark", "catppuccin-mocha")],
};
```

For a macro-only target without target-side JSON dependencies:

```console
cargo add shiki --no-default-features
cargo add shiki-macros
```

The proc macro still parses bundled JSON assets on the host. The snapshot codec
is purpose-built and does not use `serde`, `serde_json`, or `bincode` on the
target. Very large language sets increase compile time and executable size, but
runtime rule-table memory is paid only for languages that are actually used.

## Performance guidance

- Reuse engines, highlighters, and sessions rather than rebuilding them per request.
- Bundle only required roots unless languages are selected dynamically.
- Keep document state and use `tokenize_line` for incremental updates.
- Prefer scope tokens or streaming renderers when owned themed tokens are unnecessary.
- TextMate tokenization is line-oriented; extremely long minified lines are a harder workload than normal source files.
- Proc macros remove grammar/theme compilation from runtime, but token matching still happens at runtime.

## License

This project is licensed under either of:

- Apache License, Version 2.0
- MIT License

at your option.
