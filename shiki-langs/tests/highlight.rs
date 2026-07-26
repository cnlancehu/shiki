use shiki::LanguageBundle;

static LANGUAGES: LanguageBundle =
    shiki_langs::languages![astro, markdown, rust, vue];

#[path = "highlight/ansi.rs"]
mod ansi;
#[path = "highlight/basic.rs"]
mod basic;
#[path = "highlight/engine.rs"]
mod engine;
#[path = "highlight/html.rs"]
mod html;
#[path = "highlight/injections.rs"]
mod injections;
#[path = "highlight/runtime.rs"]
mod runtime;
