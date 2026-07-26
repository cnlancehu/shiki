# shiki-themes

**Bundled themes for `shiki`.**

`shiki-themes` publishes 65 generated Shiki/TextMate theme definitions. Themes
can be used directly, selected dynamically from metadata, or combined into
multi-theme CSS-variable output.

## Installation

```console
cargo add shiki shiki-langs shiki-themes
```

## One theme

Generated definitions are exported as uppercase constants:

```rust,ignore
let mut highlighter = shiki::Highlighter::builder()
    .bundle(&LANGUAGES)
    .theme(&shiki_themes::CATPPUCCIN_MOCHA)
    .build()?;
```

Other examples include `CATPPUCCIN_LATTE`, `GITHUB_DARK`, `DRACULA`, `NORD`,
and `VESPER`. Use `generated::ALL_THEMES` to enumerate the complete current
catalog instead of relying on a hard-coded list.

## Multiple themes

The name paired with each definition becomes its CSS custom-property prefix:

```rust,ignore
let mut highlighter = shiki::Highlighter::builder()
    .bundle(&LANGUAGES)
    .themes([
        ("dark", &shiki_themes::CATPPUCCIN_MOCHA),
        ("light", &shiki_themes::CATPPUCCIN_LATTE),
    ])
    .build()?;

let html = highlighter.code_to_html("let value = 1;", "rust")?;
```

The HTML renderer emits colors such as `--dark` and `--light`. Font-style,
font-weight, text-decoration, and background variables are emitted only when
the resolved token needs them.

Set `HtmlOptions::default_theme` for an inline fallback theme. When application
CSS manages theme switching, keep `include_theme_variables` enabled and set
`include_default_theme_styles` to `false` to omit concrete fallback
declarations.

## Metadata and raw themes

Every constant is a `ThemeDefinition`. It exposes the canonical ID, display
name, raw theme data, and a lazily compiled `Arc<Theme>`.

`ALL_THEMES` contains the complete catalog. The `themes!` macro can create a
small `ThemeBundle` for metadata lookup:

```rust,ignore
let themes = shiki_themes::themes![dracula, nord, vesper];
let nord = themes.get("nord").expect("bundled theme");
```

Applications may also construct `RawTheme` values directly or use runtime JSON
through the default `shiki/json` feature.
