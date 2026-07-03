# Shiki for Rust

**_Highly Experimental !!_**

A native Rust implementation of Shiki's TextMate highlighting model. It uses
Oniguruma directly, supports embedded grammars and injection selectors, and
keeps runtime scope, grammar, theme, and color data behind compact numeric IDs.

## Quick Start

The root example bundles only Rust and renders highlighted HTML:

```console
cargo run --release --example basic
```

Its essential setup is:

```rust
use shiki::{Highlighter, LanguageBundle};

static LANGUAGES: LanguageBundle = shiki_langs::languages![rust];

let mut highlighter = Highlighter::builder()
    .bundle(&LANGUAGES)
    .languages(["rust"])
    .theme(&shiki_themes::CATPPUCCIN_MOCHA)
    .build()?;

let html = highlighter.code_to_html("let answer = 91;", "rust")?;
# Ok::<(), shiki::Error>(())
```

`languages!` resolves and embeds transitive dependencies at compile time. For
example, bundling Vue also includes the grammars needed by Vue. The
`languages(...)` builder call chooses which bundled roots are enabled in that
highlighter.

## Runtime Definitions

Runtime TextMate JSON remains supported:

```rust
let highlighter = Highlighter::builder()
    .json_language("custom", grammar_json)?
    .json_theme("custom", theme_json)?
    .build()?;
# Ok::<(), shiki::Error>(())
```

To parse or construct definitions separately, use `RawGrammar::from_json` and
`RawTheme::from_json`, then pass them with `raw_language` and `raw_theme`.
`RawGrammar`, `RawRule`, `RawTheme`, and their nested theme types are public and
can also be constructed directly. Use `RawLanguage` with
`raw_language_definition` when runtime grammars need aliases or external
`inject_to` targets.

## Multiple Themes

Theme names become CSS variable prefixes:

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
# Ok::<(), shiki::Error>(())
```

Each token contains variables such as `--dark` and `--light`, plus corresponding
font and optional background variables. `HtmlOptions` controls classes,
attributes, line wrappers, the default theme, root colors, and whether default
theme declarations are emitted.

## Token APIs

- `code_to_html` streams tokenization directly into HTML.
- `code_to_scope_tokens` returns compact ranges and `ScopeStackId` values.
- `code_to_tokens` resolves one theme into owned token values.
- `code_to_tokens_with_themes` resolves all configured themes.
- `tokenize_line` and `GrammarState` support editors and incremental documents.

Reuse a `Highlighter` whenever possible. Compiled scanners, regexes, scope
transitions, injection results, and style rows are cached on first use.
