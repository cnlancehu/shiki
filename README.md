# Shiki for Rust

<p align="center">
  <a href="https://crates.io/crates/shiki"><img alt="Crates.io" src="https://img.shields.io/crates/v/shiki"></a>
</p>

**_Highly Experimental !!_**

A native Rust implementation of Shiki's TextMate highlighting model. It uses
Oniguruma directly, supports embedded grammars and injection selectors, and
keeps runtime scope, grammar, theme, and color data behind compact numeric IDs.

## Quick Start

The root example bundles only Rust and renders highlighted HTML:

```console
cargo add shiki shiki-langs shiki-themes
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
```

To parse or construct definitions separately, use `RawGrammar::from_json` and
`RawTheme::from_json`, then pass them directly with `language` and `theme`.
`RawGrammar`, `RawRule`, `RawTheme`, and their nested theme types are public and
can also be constructed directly. Use `LanguageInput` with
`language_definition` when runtime grammars need aliases or external
`inject_to` targets.

Bundled definitions use the same raw types. Generated modules initialize one
static `RawGrammar` or `RawTheme` backed by borrowed strings and slices;
runtime JSON uses the owned variants of those same containers.
`LanguageDefinition` only stores bundle metadata and a reference to the static
grammar. No JSON source or parallel static raw model is retained.

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

## Shared Engines and Document Sessions

Use a shared engine when multiple documents or threads need independent
tokenization state. Grammar IR and themes stay shared while dynamic scanners,
scope transitions, and styles are owned by each session.

```rust
let engine = Highlighter::builder()
    .bundle(&LANGUAGES)
    .languages(["rust"])
    .theme(&shiki_themes::CATPPUCCIN_MOCHA)
    .build_engine()?;

let mut session = engine.session("rust")?;
let mut state = session.initial_state();
let tokens = session.tokenize_line("let value = 1;", &mut state, true)?;
```

Legacy `Highlighter` values can release their accumulated dynamic caches with
`clear_language_cache` or `clear_all_caches`.

## Compile-time Highlighters

`shiki-macros` compiles bundled TextMate grammars and themes while the proc
macro runs. The expansion embeds a versioned grammar/theme snapshot and keeps
one shared engine per macro call site.

```rust
let mut highlighter = shiki_macros::highlighter! {
    languages: ["rust", "javascript"],
    themes: [
        ("dark", "catppuccin-mocha"),
        ("light", "catppuccin-latte"),
    ],
};
```

Use `highlighter_engine!` with the same input to obtain a cloneable
`HighlighterEngine`. Native Oniguruma objects contain process-local pointers,
so they are initialized and cached on first use; JSON parsing, grammar
expansion, injection resolution, scope interning, and theme compilation have
already happened at Rust compile time.
