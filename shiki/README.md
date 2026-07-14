# shiki

**Fast TextMate highlighting for Rust.**

`shiki` is the core crate of `shiki-rs`. It compiles TextMate grammars into a
compact runtime representation, tokenizes with Oniguruma, resolves themes, and
provides HTML and custom rendering APIs.

This crate intentionally contains no bundled language or theme catalog. Use
[`shiki-langs`](https://crates.io/crates/shiki-langs) and
[`shiki-themes`](https://crates.io/crates/shiki-themes), provide runtime
definitions, or generate an engine with
[`shiki-macros`](https://crates.io/crates/shiki-macros).

## Installation

```console
cargo add shiki
```

For the usual bundled setup:

```console
cargo add shiki shiki-langs shiki-themes
```

## Quick start

```rust,ignore
use shiki::{Highlighter, LanguageBundle};

static LANGUAGES: LanguageBundle = shiki_langs::languages![rust];

let mut highlighter = Highlighter::builder()
    .bundle(&LANGUAGES)
    .languages(["rust"])
    .theme(&shiki_themes::CATPPUCCIN_MOCHA)
    .build()?;

let html = highlighter.code_to_html("fn main() {}", "rust")?;
```

Build a shared engine when several documents need the same immutable grammar
and theme data:

```rust,ignore
let engine = Highlighter::builder()
    .bundle(&LANGUAGES)
    .languages(["rust"])
    .theme(&shiki_themes::CATPPUCCIN_MOCHA)
    .build_engine()?;

let mut first = engine.session("rust")?;
let mut second = engine.session("rust")?;
```

## Main types

- `HighlighterBuilder` selects definitions, themes, and regex limits.
- `HighlighterEngine` owns shared immutable compiled grammar and theme data.
- `Highlighter` lazily owns per-language tokenizers and rendering caches.
- `LanguageSession` owns one independent document tokenizer.
- `GrammarState` carries TextMate begin/end and begin/while state between lines.
- `HtmlRenderer` streams compact HTML without materializing owned theme tokens.
- `Renderer` is the extension point for other output formats.

## Tokenization APIs

`Highlighter` exposes representations for different integration costs:

- `tokenize_line` for incremental documents;
- `code_to_scope_tokens` for source ranges and compact scope IDs;
- `code_to_tokens` for owned tokens resolved against one theme;
- `code_to_tokens_with_themes` for every configured theme;
- `code_to_html` and `code_to_html_with_options` for streaming HTML;
- `render` for a custom `Renderer` implementation.

Scope names can be recovered from `ScopeStackId` when diagnostics or semantic
inspection needs them. `cache_stats`, `clear_language_cache`, and
`clear_all_caches` expose control over lazily accumulated runtime state.

## HTML rendering

`HtmlOptions` independently controls explicit classes and attributes, automatic
`shiki`/theme classes, `data-themes`, line wrappers, root theme variables,
foreground/background styles, default theme selection, token spans, and
variable-only output.

```rust,ignore
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

Use `HtmlOptions::clean()` for a `<pre><code>…</code></pre>` wrapper with no
automatic wrapper attributes or root styles. It retains the `line` class and
syntax-highlighted token styles.

Custom options own their `Vec<String>` and `BTreeMap<String, String>` values.
Use `LazyLock` to create one shared global configuration:

```rust,ignore
use std::sync::LazyLock;

static HTML: LazyLock<shiki::HtmlOptions> = LazyLock::new(|| {
    let mut options = shiki::HtmlOptions::clean();
    options.pre_classes.push("code-block".into());
    options
        .code_attributes
        .insert("aria-label".into(), "source".into());
    options.include_data_themes = true;
    options.include_line_wrapper = false;
    options
});
```

Classes, attributes, `line_class`, and `default_theme` are public fields. Set
`line_class` to `None` to keep a classless line wrapper, or set
`include_line_wrapper` to `false` to remove the wrapper itself.

With multiple themes, token colors are emitted as CSS custom properties. Font
and background variables are emitted only when required. Adjacent tokens with
the same final visual style are merged even when their scope stacks differ;
plain whitespace is left unwrapped when a span would have no visual effect.

## Runtime definitions

The default `json` feature enables TextMate grammar and theme JSON:

```rust,ignore
let highlighter = Highlighter::builder()
    .json_language("custom", grammar_json)?
    .json_theme("custom", theme_json)?
    .build()?;
```

Definitions may also be constructed through `RawGrammar`, `RawRule`,
`RawTheme`, and their nested raw types. `LanguageInput` adds aliases and
external injection targets to runtime grammars.

## Features

| Feature | Default | Description                                                                     |
| ------- | ------- | ------------------------------------------------------------------------------- |
| `json`  | yes     | Enables `serde`/`serde_json`, runtime JSON parsing, and JSON-backed definitions |

Compile without the JSON stack when the target only consumes snapshots generated
by `shiki-macros`:

```console
cargo add shiki --no-default-features
```

In that configuration, target-side `shiki` depends on `onig_sys`, `thiserror`,
and the small `lz4_flex` snapshot codec, but not `serde` or `serde_json`.

## Performance model

Compiled grammar IR and themes belong to the engine. Engines produced by
`shiki-macros` decode each grammar IR on first use; runtime-built engines compile
their selected grammars eagerly. Oniguruma scanners, dynamic regexes, scope
transitions, injection results, and style rows are created lazily in a
highlighter or session. `HighlighterEngine::loaded_language_count` exposes the
number of resident grammar IRs.

- Reuse engines and sessions.
- Bundle only required roots when possible.
- Use incremental line state for editors.
- Prefer scope tokens or streaming rendering when owned tokens are unnecessary.
- Expect very long minified lines to cost more than ordinary line-oriented source.
