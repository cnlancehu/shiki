use shiki_core::Highlighter;

use super::LANGUAGES;

#[test]
fn renders_ansi_with_theme_palette_and_decorations() {
    let mut highlighter = Highlighter::builder()
        .bundle(&LANGUAGES)
        .languages(["ansi"])
        .themes([
            ("light", &shiki_themes::CATPPUCCIN_LATTE),
            ("dark", &shiki_themes::CATPPUCCIN_MOCHA),
        ])
        .build()
        .unwrap();
    let source = concat!(
        "plain ",
        "\x1b[1;3;4;9;31;43mstyled\x1b[0m ",
        "\x1b[38;2;1;2;3;48;5;24mtruecolor\x1b[0m"
    );
    let html = highlighter.code_to_html(source, "ansi").unwrap();

    assert!(!html.contains('\x1b'), "{html}");
    assert!(html.contains("--light:#d20f39"), "{html}");
    assert!(html.contains("--dark:#f38ba8"), "{html}");
    assert!(html.contains("--light-bg:#df8e1d"), "{html}");
    assert!(html.contains("--dark-bg:#f9e2af"), "{html}");
    assert!(html.contains("--light:#010203"), "{html}");
    assert!(html.contains("--dark:#010203"), "{html}");
    assert!(html.contains("#005f87"), "{html}");
    assert!(html.contains("font-style:italic"), "{html}");
    assert!(html.contains("font-weight:bold"), "{html}");
    assert!(html.contains("underline line-through"), "{html}");

    let tokens = highlighter.code_to_tokens(source, "ansi").unwrap();
    assert_eq!(
        tokens[0]
            .iter()
            .map(|token| token.content.as_str())
            .collect::<String>(),
        "plain styled truecolor"
    );
    let styled = tokens[0]
        .iter()
        .find(|token| token.content == "styled")
        .unwrap();
    assert_eq!(&*styled.color, "#d20f39");
    assert_eq!(styled.background.as_deref(), Some("#df8e1d"));
    assert!(styled.font_style.contains(shiki_core::FontStyle::BOLD));
    assert!(styled.font_style.contains(shiki_core::FontStyle::ITALIC));
    assert!(styled.font_style.contains(shiki_core::FontStyle::UNDERLINE));
    assert!(
        styled
            .font_style
            .contains(shiki_core::FontStyle::STRIKETHROUGH)
    );
}

#[test]
fn ansi_state_crosses_lines_and_supports_reverse_and_dim() {
    let mut highlighter = Highlighter::builder()
        .languages(["ansi"])
        .theme(&shiki_themes::CATPPUCCIN_MOCHA)
        .build()
        .unwrap();
    let source =
        "\x1b[32mfirst\nsecond\x1b[0m \x1b[2;31mdim\x1b[0m \x1b[7;34mreverse";
    let lines = highlighter.code_to_tokens(source, "ansi").unwrap();

    assert_eq!(&*lines[0][0].color, "#a6e3a1");
    assert_eq!(&*lines[1][0].color, "#a6e3a1");
    let dim = lines[1]
        .iter()
        .find(|token| token.content == "dim")
        .unwrap();
    assert_eq!(&*dim.color, "#f38ba880");
    let reverse = lines[1]
        .iter()
        .find(|token| token.content == "reverse")
        .unwrap();
    assert_eq!(&*reverse.color, "#1e1e2e");
    assert_eq!(reverse.background.as_deref(), Some("#89b4fa"));
}
