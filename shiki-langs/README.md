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

The core engine always provides plain text under `text`, `txt`, and `plain`.
These names are also accepted by `languages!`, although they do not add a
grammar asset:

```rust,ignore
static PLAIN_TEXT: LanguageBundle = shiki_langs::languages![text];
```

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

Building an engine with all languages records only catalog metadata. Grammar
JSON, compiled grammar IR, and native regex programs initialize when a language
is first used. Static regex programs are then shared by every session created
from that engine, so keeping one engine alive avoids both repeated compilation
and duplicate native regex memory.

## Generated definitions

Each generated language exports an uppercase `LanguageDefinition` constant,
such as `RUST`, `JAVASCRIPT`, or `VUE`. Definitions contain:

- canonical ID and display name;
- TextMate root scope name;
- aliases;
- dependency roots;
- external injection targets;
- the generated or packaged raw grammar.

Reading IDs, aliases, scopes, and dependency metadata does not parse grammar
JSON. Calling `LanguageDefinition::grammar()` explicitly does initialize that
raw asset; normal engine construction defers it until the language is used.

`languages!` uses dependency groups rather than blindly bundling every grammar,
so embedded languages and injections continue to work without unrelated assets.
