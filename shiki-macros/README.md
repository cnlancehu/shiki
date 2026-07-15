# shiki-macros

**Compile-time highlighters for `shiki`.**

`shiki-macros` resolves bundled language dependencies, compiles TextMate
grammar and theme IR in the proc-macro host, and embeds a compact precompiled
snapshot in the target crate. It does not emit one Rust expression per rule.

## Installation

```console
cargo add shiki shiki-macros
```

The generated code refers to `::shiki`, so the target crate should depend on
both crates directly.

## Generate a highlighter

```rust,ignore
let mut highlighter = shiki_macros::highlighter! {
    languages: ["rust", "javascript"],
    themes: [
        ("dark", "catppuccin-mocha"),
        ("light", "catppuccin-latte"),
    ],
};

let html = highlighter.code_to_html("const value = 1", "javascript")?;
```

Language and bundled theme IDs are string literals. Unknown IDs are reported as
compile errors. Language dependencies and injection grammars are included
automatically.

`text` is always available as a no-highlighting fallback; `txt` and `plain` are
aliases. It keeps HTML escaping and the theme's default styles but emits only
one token per non-empty line. It may be selected by itself or used even when it
is omitted from `languages`.

Select the complete bundled catalog with `languages: all`:

```rust,ignore
let engine = shiki_macros::highlighter_engine! {
    languages: all,
    themes: [("default", "github-dark")],
};
```

Each macro call site owns one `LazyLock<HighlighterEngine>`. `highlighter!`
returns a fresh `Highlighter` backed by that shared engine.

## Generate an engine

Use `highlighter_engine!` when the application creates independent sessions or
highlighters itself:

```rust,ignore
let engine = shiki_macros::highlighter_engine! {
    languages: ["rust"],
    themes: [("default", "github-dark")],
};

let mut first = engine.session("rust")?;
let mut second = engine.session("rust")?;
```

The returned engine is cheap to clone. Compiled grammar IR is decoded one
language at a time on first use. Native Oniguruma scanners are not embedded
because they contain process-local pointers; they also initialize and cache on
first use.

## Remove target-side JSON dependencies

Generated highlighters do not need runtime JSON parsing. Disable the default
feature of the target-side `shiki` dependency:

```console
cargo add shiki --no-default-features
cargo add shiki-macros
```

With this setup, target-side `shiki` does not depend on `serde`, `serde_json`, or
`bincode`. The proc macro still uses JSON support on the host because the
published language and theme assets are JSON inputs.

## Snapshot format

The macro serializes compiled IR with a purpose-built, versioned codec. Integer
IDs and lengths use variable-width encoding, strings are deduplicated across the
whole engine, and every grammar block is compressed independently with LZ4. The
target compiler sees one byte string and one loader call instead of a large
syntax tree of vector and enum constructors.

The first call at a macro site restores only the language index, shared string
directory, and selected themes. Grammar blocks become normal `shiki` runtime
types when that language is first highlighted. Later calls reuse both the
`LazyLock` and decoded grammar IR, so `highlighter!` only creates a cheap
per-highlighter tokenizer table and `highlighter_engine!` returns an `Arc`-backed
engine clone. `HighlighterEngine::loaded_language_count` reports how many
grammar blocks are resident. This format is internal and versioned together
with `shiki`; it is not intended as a persistent interchange format.

## What is compiled ahead of time

- language selection and dependency closure;
- grammar includes and repositories;
- injection selectors and targets;
- compact rule, pattern, scope, and capture tables;
- theme palettes, selectors, rules, and CSS names.

Token matching, dynamic end-pattern resolution, and lazy Oniguruma scanner
creation still happen in the target process.

## Trade-offs

Selecting many large grammars still increases proc-macro work, snapshot size,
and executable size. Runtime grammar IR memory grows only for languages that
are actually used. In particular, every root grammar has a separately resolved
dependency closure, so `languages: all` is substantially larger than a typical
application-specific set. Prefer the smallest stable language and theme set
when executable size and compile time matter.

Use a runtime `HighlighterBuilder` instead when definitions must be selected or
loaded dynamically.
