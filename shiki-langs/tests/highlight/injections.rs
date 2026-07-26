use shiki_core::Highlighter;

use super::LANGUAGES;

#[test]
fn astro_injection_enters_javascript_grammar() {
    let mut highlighter = Highlighter::builder()
        .bundle(&LANGUAGES)
        .languages(["astro"])
        .theme(&shiki_themes::generated::CATPPUCCIN_MOCHA)
        .build()
        .unwrap();
    let (_, state) = highlighter
        .tokenize_line("<script>", "astro", None, true)
        .unwrap();
    let (tokens, state) = highlighter
        .tokenize_line("const answer = \"yes\"", "astro", Some(&state), false)
        .unwrap();
    highlighter
        .tokenize_line("</script>", "astro", Some(&state), false)
        .unwrap();

    let keyword = tokens
        .iter()
        .find(|token| &"const answer = \"yes\""[token.range.clone()] == "const")
        .expect("javascript keyword token");
    let string = tokens
        .iter()
        .find(|token| &"const answer = \"yes\""[token.range.clone()] == "yes")
        .expect("javascript string token");
    assert_ne!(keyword.scopes, string.scopes, "{tokens:#?}");
}

#[test]
fn astro_dynamic_scope_capture_uses_numeric_identity() {
    let mut highlighter = Highlighter::builder()
        .bundle(&LANGUAGES)
        .languages(["astro"])
        .theme(&shiki_themes::generated::CATPPUCCIN_MOCHA)
        .build()
        .unwrap();

    let tokens = highlighter
        .code_to_scope_tokens("<script lang=\"foobar\">value</script>", "astro")
        .unwrap();

    assert!(
        tokens[0]
            .iter()
            .map(|token| token.scopes)
            .collect::<std::collections::HashSet<_>>()
            .len()
            > 1
    );
}
