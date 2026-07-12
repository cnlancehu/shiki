//! Compile bundled TextMate grammars and themes into an embedded highlighter snapshot.
//!
//! `highlighter!` returns a fresh [`shiki::Highlighter`] backed by one engine shared at
//! the macro call site. `highlighter_engine!` returns a clone of that shared engine.
//!
//! ```
//! let mut highlighter = shiki_macros::highlighter! {
//!     languages: ["rust"],
//!     themes: [("dark", "catppuccin-mocha")],
//! };
//! let html = highlighter.code_to_html("let value = 1;", "rust")?;
//! # Ok::<(), shiki::Error>(())
//! ```

use proc_macro::TokenStream;
use proc_macro2::{Literal, Span};
use quote::quote;
use syn::{
    Ident, LitStr, Result, Token, bracketed, parenthesized,
    parse::{Parse, ParseStream},
    punctuated::Punctuated,
};

struct ThemeSelection {
    name: LitStr,
    definition: LitStr,
}

struct HighlighterInput {
    languages: Vec<LitStr>,
    themes: Vec<ThemeSelection>,
}

impl Parse for HighlighterInput {
    fn parse(input: ParseStream<'_>) -> Result<Self> {
        let mut languages: Option<Vec<LitStr>> = None;
        let mut themes: Option<Vec<ThemeSelection>> = None;
        while !input.is_empty() {
            let key: Ident = input.parse()?;
            input.parse::<Token![:]>()?;
            match key.to_string().as_str() {
                "languages" => {
                    if languages.is_some() {
                        return Err(syn::Error::new(
                            key.span(),
                            "duplicate `languages` field",
                        ));
                    }
                    let content;
                    bracketed!(content in input);
                    languages = Some(
                        Punctuated::<LitStr, Token![,]>::parse_terminated(
                            &content,
                        )?
                        .into_iter()
                        .collect(),
                    );
                }
                "themes" => {
                    if themes.is_some() {
                        return Err(syn::Error::new(
                            key.span(),
                            "duplicate `themes` field",
                        ));
                    }
                    let content;
                    bracketed!(content in input);
                    let selections =
                        Punctuated::<ThemeSelection, Token![,]>::parse_terminated(&content)?;
                    themes = Some(selections.into_iter().collect());
                }
                _ => {
                    return Err(syn::Error::new(
                        key.span(),
                        "expected `languages` or `themes`",
                    ));
                }
            }
            if input.peek(Token![,]) {
                input.parse::<Token![,]>()?;
            }
        }
        let languages = languages.ok_or_else(|| {
            syn::Error::new(
                Span::call_site(),
                "missing `languages: [...]` field",
            )
        })?;
        let themes = themes.ok_or_else(|| {
            syn::Error::new(Span::call_site(), "missing `themes: [...]` field")
        })?;
        if languages.is_empty() {
            return Err(syn::Error::new(
                Span::call_site(),
                "at least one language is required",
            ));
        }
        if themes.is_empty() {
            return Err(syn::Error::new(
                Span::call_site(),
                "at least one theme is required",
            ));
        }
        Ok(Self { languages, themes })
    }
}

impl Parse for ThemeSelection {
    fn parse(input: ParseStream<'_>) -> Result<Self> {
        let content;
        parenthesized!(content in input);
        let name = content.parse()?;
        content.parse::<Token![,]>()?;
        let definition = content.parse()?;
        if !content.is_empty() {
            return Err(
                content.error("expected `(output_name, bundled_theme_id)`")
            );
        }
        Ok(Self { name, definition })
    }
}

#[proc_macro]
pub fn highlighter(input: TokenStream) -> TokenStream {
    expand(input, false)
}

#[proc_macro]
pub fn highlighter_engine(input: TokenStream) -> TokenStream {
    expand(input, true)
}

fn expand(input: TokenStream, engine_only: bool) -> TokenStream {
    match syn::parse::<HighlighterInput>(input).and_then(compile) {
        Ok(bytes) => {
            let bytes = Literal::byte_string(&bytes);
            let result = if engine_only {
                quote!(ENGINE.clone())
            } else {
                quote!(ENGINE.highlighter())
            };
            quote!({
                static ENGINE: ::std::sync::LazyLock<::shiki::HighlighterEngine> =
                    ::std::sync::LazyLock::new(|| {
                        ::shiki::HighlighterEngine::__from_precompiled_bytes(#bytes)
                            .expect("shiki-macros generated an incompatible highlighter snapshot")
                    });
                #result
            })
            .into()
        }
        Err(error) => error.into_compile_error().into(),
    }
}

fn compile(input: HighlighterInput) -> Result<Vec<u8>> {
    let language_ids = input
        .languages
        .iter()
        .map(LitStr::value)
        .collect::<Vec<_>>();
    for (id, literal) in language_ids.iter().zip(&input.languages) {
        let available =
            shiki_langs::generated::ALL_LANGUAGES
                .iter()
                .any(|language| {
                    language.id == id
                        || language.aliases.iter().any(|alias| alias == id)
                });
        if !available {
            return Err(syn::Error::new(
                literal.span(),
                format!("bundled language `{id}` does not exist"),
            ));
        }
    }

    let selected_themes = input
        .themes
        .iter()
        .map(|selection| {
            let id = selection.definition.value();
            let definition = shiki_themes::generated::ALL_THEMES
                .iter()
                .copied()
                .find(|theme| theme.id == id)
                .ok_or_else(|| {
                    syn::Error::new(
                        selection.definition.span(),
                        format!("bundled theme `{id}` does not exist"),
                    )
                })?;
            Ok((selection.name.value(), definition))
        })
        .collect::<Result<Vec<_>>>()?;

    let groups: &'static [shiki::LanguageGroup] = Box::leak(
        vec![shiki_langs::generated::ALL_LANGUAGES].into_boxed_slice(),
    );
    let bundle = shiki::LanguageBundle::from_groups(groups);
    let engine = shiki::Highlighter::builder()
        .bundle(&bundle)
        .languages(language_ids)
        .themes(selected_themes)
        .build_engine()
        .map_err(|error| syn::Error::new(Span::call_site(), error))?;
    engine
        .__to_precompiled_bytes()
        .map_err(|error| syn::Error::new(Span::call_site(), error))
}
