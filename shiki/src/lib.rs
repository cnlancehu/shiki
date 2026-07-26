#![doc = include_str!("../README.md")]

pub use shiki_core::*;
#[cfg(feature = "langs")]
pub use shiki_langs as langs;
#[cfg(feature = "macros")]
pub use shiki_macros as macros;
#[cfg(feature = "macros")]
pub use shiki_macros::{highlighter, highlighter_engine};
#[cfg(feature = "themes")]
pub use shiki_themes as themes;
