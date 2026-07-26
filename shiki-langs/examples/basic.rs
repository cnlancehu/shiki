use shiki_core::{Highlighter, HtmlOptions, LanguageBundle};

static LANGUAGES: LanguageBundle = shiki_langs::languages![rust];

fn main() -> shiki_core::Result<()> {
    let mut highlighter = Highlighter::builder()
        .bundle(&LANGUAGES)
        .languages(["rust"])
        .theme(&shiki_themes::CATPPUCCIN_MOCHA)
        .build()?;

    let code = r#"fn main() {
    println!("Hello from Rust");
}"#;
    let mut options = HtmlOptions::default();
    options
        .pre_attributes
        .insert("data-language".into(), "rust".into());
    let html = highlighter.code_to_html_with_options(code, "rust", &options)?;

    println!("{html}");
    Ok(())
}
