//! Compile bundled TextMate grammars and themes into generated Rust data structures.
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
use proc_macro2::{Span, TokenStream as TokenStream2};
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
        Ok(engine) => {
            let result = if engine_only {
                quote!(ENGINE.clone())
            } else {
                quote!(ENGINE.highlighter())
            };
            quote!({
                static ENGINE: ::std::sync::LazyLock<::shiki::HighlighterEngine> =
                    ::std::sync::LazyLock::new(|| {
                        #engine
                    });
                #result
            })
            .into()
        }
        Err(error) => error.into_compile_error().into(),
    }
}

fn compile(input: HighlighterInput) -> Result<TokenStream2> {
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
    Ok(quote_engine(&engine))
}

fn quote_engine(engine: &shiki::HighlighterEngine) -> TokenStream2 {
    let mut languages = engine.inner.languages.iter().collect::<Vec<_>>();
    languages.sort_unstable_by(|left, right| left.0.cmp(right.0));
    let languages = languages.into_iter().map(|(name, index)| {
        let name = string_literal(name);
        quote!((#name, #index))
    });
    let grammars = engine
        .inner
        .compiled
        .iter()
        .map(|language| quote_grammar(&language.grammar));
    let themes = engine.inner.themes.iter().map(|theme| {
        let name = string_literal(&theme.name);
        let css_name = string_literal(&theme.css_name);
        let theme = quote_theme(&theme.theme);
        quote!((#name, #css_name, #theme))
    });
    let limits = engine.inner.regex_limits;
    let match_retry_limit = limits.match_retry_limit;
    let search_retry_limit = limits.search_retry_limit;
    quote! {
        ::shiki::HighlighterEngine::__from_rust_parts(
            ::std::vec![#(#languages),*],
            ::std::vec![#(#grammars),*],
            ::std::vec![#(#themes),*],
            ::shiki::RegexLimits {
                match_retry_limit: #match_retry_limit,
                search_retry_limit: #search_retry_limit,
            },
        )
    }
}

fn quote_grammar(grammar: &shiki::__private::CompiledGrammar) -> TokenStream2 {
    let root_scope_name = grammar.root_scope_name;
    let root = grammar.root;
    let rules = grammar.rules.iter().map(quote_rule);
    let patterns = grammar.patterns.iter().map(|value| quote_arc_str(value));
    let scope_names = grammar.scope_names.iter().map(|scope| {
        let scopes = scope.scopes.iter();
        quote! {
            ::shiki::__private::ScopeName {
                scopes: ::std::vec![#(#scopes),*].into_boxed_slice(),
            }
        }
    });
    let scope_templates = grammar.scope_templates.iter().map(|template| {
        let parts = template.parts.iter().map(quote_scope_part);
        quote! {
            ::shiki::__private::ScopeTemplate {
                parts: ::std::vec![#(#parts),*].into_boxed_slice(),
            }
        }
    });
    let injection_selectors = grammar
        .injection_selectors
        .iter()
        .map(|value| quote_arc_str(value));
    let injections = grammar.injections.iter().map(|injection| {
        let selector = quote_scope_selector(&injection.selector);
        let rule = injection.rule;
        quote! {
            ::shiki::__private::Injection {
                selector: #selector,
                rule: #rule,
            }
        }
    });
    quote! {
        ::shiki::__private::CompiledGrammar {
            root_scope_name: #root_scope_name,
            root: #root,
            rules: ::std::vec![#(#rules),*],
            patterns: ::std::vec![#(#patterns),*],
            scope_names: ::std::vec![#(#scope_names),*],
            scope_templates: ::std::vec![#(#scope_templates),*],
            injection_selectors: ::std::vec![#(#injection_selectors),*],
            injections: ::std::vec![#(#injections),*],
        }
    }
}

fn quote_rule(rule: &shiki::__private::Rule) -> TokenStream2 {
    let name = quote_option_u32(rule.name);
    let content_name = quote_option_u32(rule.content_name);
    let kind = match &rule.kind {
        shiki::__private::RuleKind::Match { pattern, captures } => {
            let captures = captures.iter().map(quote_capture);
            quote! {
                ::shiki::__private::RuleKind::Match {
                    pattern: #pattern,
                    captures: ::std::vec![#(#captures),*],
                }
            }
        }
        shiki::__private::RuleKind::IncludeOnly { patterns } => quote! {
            ::shiki::__private::RuleKind::IncludeOnly {
                patterns: ::std::vec![#(#patterns),*],
            }
        },
        shiki::__private::RuleKind::BeginEnd {
            begin,
            begin_captures,
            end,
            end_captures,
            patterns,
            apply_end_last,
        } => {
            let begin_captures = begin_captures.iter().map(quote_capture);
            let end_captures = end_captures.iter().map(quote_capture);
            quote! {
                ::shiki::__private::RuleKind::BeginEnd {
                    begin: #begin,
                    begin_captures: ::std::vec![#(#begin_captures),*],
                    end: #end,
                    end_captures: ::std::vec![#(#end_captures),*],
                    patterns: ::std::vec![#(#patterns),*],
                    apply_end_last: #apply_end_last,
                }
            }
        }
        shiki::__private::RuleKind::BeginWhile {
            begin,
            begin_captures,
            while_pattern,
            while_captures,
            patterns,
        } => {
            let begin_captures = begin_captures.iter().map(quote_capture);
            let while_captures = while_captures.iter().map(quote_capture);
            quote! {
                ::shiki::__private::RuleKind::BeginWhile {
                    begin: #begin,
                    begin_captures: ::std::vec![#(#begin_captures),*],
                    while_pattern: #while_pattern,
                    while_captures: ::std::vec![#(#while_captures),*],
                    patterns: ::std::vec![#(#patterns),*],
                }
            }
        }
        shiki::__private::RuleKind::Placeholder => {
            quote!(::shiki::__private::RuleKind::Placeholder)
        }
    };
    quote! {
        ::shiki::__private::Rule {
            name: #name,
            content_name: #content_name,
            kind: #kind,
        }
    }
}

fn quote_capture(capture: &shiki::__private::Capture) -> TokenStream2 {
    let index = capture.index;
    let name = quote_option_u32(capture.name);
    let content_name = quote_option_u32(capture.content_name);
    let retokenize = quote_option_u32(capture.retokenize);
    quote! {
        ::shiki::__private::Capture {
            index: #index,
            name: #name,
            content_name: #content_name,
            retokenize: #retokenize,
        }
    }
}

fn quote_scope_part(part: &shiki::__private::ScopePart) -> TokenStream2 {
    match part {
        shiki::__private::ScopePart::Literal(value) => {
            let value = quote_arc_str(value);
            quote!(::shiki::__private::ScopePart::Literal(#value))
        }
        shiki::__private::ScopePart::Capture(index) => {
            quote!(::shiki::__private::ScopePart::Capture(#index))
        }
    }
}

fn quote_scope_selector(
    selector: &shiki::__private::ScopeSelector,
) -> TokenStream2 {
    let priority = match selector.priority {
        shiki::__private::Priority::Left => {
            quote!(::shiki::__private::Priority::Left)
        }
        shiki::__private::Priority::Normal => {
            quote!(::shiki::__private::Priority::Normal)
        }
        shiki::__private::Priority::Right => {
            quote!(::shiki::__private::Priority::Right)
        }
    };
    let expression = quote_expression(&selector.expression);
    quote! {
        ::shiki::__private::ScopeSelector {
            priority: #priority,
            expression: #expression,
        }
    }
}

fn quote_expression(expression: &shiki::__private::Expression) -> TokenStream2 {
    match expression {
        shiki::__private::Expression::Path(path) => quote! {
            ::shiki::__private::Expression::Path(::std::vec![#(#path),*])
        },
        shiki::__private::Expression::And(expressions) => {
            let expressions = expressions.iter().map(quote_expression);
            quote! {
                ::shiki::__private::Expression::And(::std::vec![#(#expressions),*])
            }
        }
        shiki::__private::Expression::Or(expressions) => {
            let expressions = expressions.iter().map(quote_expression);
            quote! {
                ::shiki::__private::Expression::Or(::std::vec![#(#expressions),*])
            }
        }
        shiki::__private::Expression::Not(expression) => {
            let expression = quote_expression(expression);
            quote! {
                ::shiki::__private::Expression::Not(::std::boxed::Box::new(#expression))
            }
        }
    }
}

fn quote_theme(theme: &shiki::__private::Theme) -> TokenStream2 {
    let name = quote_arc_str(&theme.name);
    let foreground = quote_arc_str(&theme.foreground);
    let background = quote_arc_str(&theme.background);
    let colors = theme.colors.iter().map(|value| quote_arc_str(value));
    let foreground_id = theme.foreground_id.0;
    let selectors = theme.selectors.iter().map(|value| quote_arc_str(value));
    let rules = theme.rules.iter().map(quote_theme_rule);
    quote! {
        ::shiki::__private::Theme {
            name: #name,
            foreground: #foreground,
            background: #background,
            colors: ::std::vec![#(#colors),*],
            foreground_id: ::shiki::__private::ColorId(#foreground_id),
            selectors: ::std::vec![#(#selectors),*],
            rules: ::std::vec![#(#rules),*],
        }
    }
}

fn quote_theme_rule(rule: &shiki::__private::ThemeRule) -> TokenStream2 {
    let target = rule.target;
    let parents = &rule.parents;
    let target_depth = rule.target_depth;
    let style = quote_style(rule.style);
    let order = rule.order;
    quote! {
        ::shiki::__private::ThemeRule {
            target: #target,
            parents: ::std::vec![#(#parents),*],
            target_depth: #target_depth,
            style: #style,
            order: #order,
        }
    }
}

fn quote_style(style: shiki::__private::Style) -> TokenStream2 {
    let foreground = quote_color_id(style.foreground);
    let background = quote_color_id(style.background);
    let font_style = match style.font_style {
        Some(value) => {
            let bits = value.bits();
            quote!(::core::option::Option::Some(::shiki::FontStyle::from_bits(#bits)))
        }
        None => quote!(::core::option::Option::None),
    };
    quote! {
        ::shiki::__private::Style {
            foreground: #foreground,
            background: #background,
            font_style: #font_style,
        }
    }
}

fn quote_color_id(value: Option<shiki::__private::ColorId>) -> TokenStream2 {
    match value {
        Some(value) => {
            let value = value.0;
            quote!(::core::option::Option::Some(::shiki::__private::ColorId(#value)))
        }
        None => quote!(::core::option::Option::None),
    }
}

fn quote_option_u32(value: Option<u32>) -> TokenStream2 {
    match value {
        Some(value) => quote!(::core::option::Option::Some(#value)),
        None => quote!(::core::option::Option::None),
    }
}

fn quote_arc_str(value: &str) -> TokenStream2 {
    let value = string_literal(value);
    quote!(::std::sync::Arc::<str>::from(#value))
}

fn string_literal(value: &str) -> LitStr {
    LitStr::new(value, Span::call_site())
}
