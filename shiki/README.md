# shiki

**The batteries-optional facade for shiki-rs.**

`shiki` always re-exports the complete
[`shiki_core`](https://crates.io/crates/shiki-rs-core) API. Bundled languages,
themes, and compile-time macros are available behind
independent features, so applications can opt into only the catalogs they use.

## Features

| Feature  | Default | Re-export |
|----------|---------|-----------|
| `json`   | yes     | Enables runtime JSON support in `shiki-core` |
| `langs`  | no      | `shiki::langs` |
| `themes` | no      | `shiki::themes` |
| `macros` | no      | `shiki::macros` and the macros at the crate root |
| `full`   | no      | Enables all features above |

```toml
[dependencies]
shiki = { version = "0.0.7", features = ["langs", "themes"] }
```

```rust,ignore
use shiki::{Highlighter, LanguageBundle};

static LANGUAGES: LanguageBundle = shiki::langs::languages![rust];

let mut highlighter = Highlighter::builder()
    .bundle(&LANGUAGES)
    .languages(["rust"])
    .theme(&shiki::themes::CATPPUCCIN_MOCHA)
    .build()?;
```
