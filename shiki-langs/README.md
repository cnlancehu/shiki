# shiki-langs

**Bundled TextMate grammars for `shiki`.**

`shiki-langs` publishes 253 generated language definitions together with their
TextMate grammar assets, aliases, dependency metadata, and injection targets.

## Installation

```console
cargo add shiki shiki-langs shiki-themes
```

## Select languages at compile time

`languages!` builds a static `LanguageBundle` and includes transitive grammar
dependencies automatically:

```rust,ignore
use shiki::{Highlighter, LanguageBundle};

static LANGUAGES: LanguageBundle =
    shiki_langs::languages![rust, javascript, typescript, vue];

let mut highlighter = Highlighter::builder()
    .bundle(&LANGUAGES)
    .languages(["rust", "javascript", "typescript", "vue"])
    .theme(&shiki_themes::CATPPUCCIN_MOCHA)
    .build()?;
```

Macro arguments are generated identifiers. Common aliases are accepted when
they can be represented as Rust identifiers, for example `js`, `ts`, `yml`, or
`adoc`.

The builder's `.languages(...)` call enables selected roots from the bundle.
Omit it to enable every root contained in that bundle.

## Enable every language

```rust,ignore
let languages = shiki_langs::all();
let engine = shiki::Highlighter::builder()
    .bundle(&languages)
    .theme(&shiki_themes::CATPPUCCIN_MOCHA)
    .build_engine()?;
```

`ALL` is the equivalent constant bundle. `ALL_LANGUAGES` and
`ALL_LANGUAGE_GROUPS` expose generated metadata for discovery and tooling.

Building all languages has a larger startup and memory cost. Tokenizer caches
are still initialized lazily per language, so sharing one engine is recommended.

## Generated definitions

Each generated language exports an uppercase `LanguageDefinition` constant,
such as `RUST`, `JAVASCRIPT`, or `VUE`. Definitions contain:

- canonical ID and display name;
- TextMate root scope name;
- aliases;
- dependency roots;
- external injection targets;
- the generated or packaged raw grammar.

`languages!` uses dependency groups rather than blindly bundling every grammar,
so embedded languages and injections continue to work without unrelated assets.
