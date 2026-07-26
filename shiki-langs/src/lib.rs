#![doc = include_str!("../README.md")]

mod macros;

#[doc(hidden)]
pub mod generated;

pub use generated::*;
pub use shiki::{LanguageBundle, LanguageDefinition, LanguageGroup};

/// A bundle that enables every bundled language when no explicit roots are selected.
pub const ALL: LanguageBundle =
    LanguageBundle::from_groups(generated::ALL_LANGUAGE_GROUPS);

/// Returns a bundle containing every bundled language.
///
/// Pass this to [`shiki::HighlighterBuilder::bundle`] and omit
/// [`shiki::HighlighterBuilder::languages`] to enable all languages.
pub const fn all() -> LanguageBundle {
    ALL
}
