# shiki-macros

**Compile-time highlighters for `shiki`.**

`shiki-macros` resolves bundled language dependencies, compiles TextMate
grammar and theme IR in the proc-macro host, and emits Rust data structures
directly into the target crate. Runtime snapshot deserialization is not used.

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

The returned engine is cheap to clone. Native Oniguruma scanners are not
embedded because they contain process-local pointers; they initialize and cache
on first use.

## Remove target-side JSON dependencies

Generated highlighters do not need runtime JSON parsing. Disable the default
feature of the target-side `shiki` dependency:

```console
cargo add shiki --no-default-features
cargo add shiki-macros
```

With this setup, target-side `shiki` does not depend on `serde` or `serde_json`.
The proc macro still uses them on the host because the published language and
theme assets are JSON inputs.

## What is compiled ahead of time

- language selection and dependency closure;
- grammar includes and repositories;
- injection selectors and targets;
- compact rule, pattern, scope, and capture tables;
- theme palettes, selectors, rules, and CSS names.

Token matching, dynamic end-pattern resolution, and lazy Oniguruma scanner
creation still happen in the target process.

## Trade-offs

Compile-time generation reduces runtime construction work but emits more Rust
code. Selecting many large grammars increases proc-macro work, target compile
time, and binary data size. Prefer the smallest stable language and theme set
that fits the application.

Use a runtime `HighlighterBuilder` instead when definitions must be selected or
loaded dynamically.
