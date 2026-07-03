use shiki::{Highlighter, HtmlOptions, LanguageBundle};

static LANGUAGES: LanguageBundle = shiki_langs::languages![rust];

fn main() -> shiki::Result<()> {
    let mut highlighter = Highlighter::builder()
        .bundle(&LANGUAGES)
        .languages(["rust"])
        .theme(&shiki_themes::CATPPUCCIN_MOCHA)
        .build()?;

    let code = r#"fn main() {
    println!("Hello from Rust");
}"#;
    let options = HtmlOptions::default().pre_attribute("data-language", "rust");
    let html = highlighter.code_to_html_with_options(code, "rust", &options)?;

    println!("{html}");
    Ok(())
}
