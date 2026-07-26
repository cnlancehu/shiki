#![doc = include_str!("../README.md")]

mod macros;

#[doc(hidden)]
pub mod generated;

pub use generated::*;
pub use shiki_core::{ThemeBundle, ThemeDefinition};
