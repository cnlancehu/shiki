#![doc = include_str!("../README.md")]

mod macros;

#[doc(hidden)]
pub mod generated;

pub use generated::*;
pub use shiki_core::{LanguageBundle, LanguageDefinition, LanguageGroup};

/// A bundle that enables every bundled language when no explicit roots are selected.
pub const ALL: LanguageBundle =
    LanguageBundle::from_groups(generated::ALL_LANGUAGE_GROUPS);

/// Returns a bundle containing every bundled language.
///
/// Pass this to [`shiki_core::HighlighterBuilder::bundle`] and omit
/// [`shiki_core::HighlighterBuilder::languages`] to enable all languages.
pub const fn all() -> LanguageBundle {
    ALL
}
