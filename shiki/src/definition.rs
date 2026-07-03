use std::collections::{HashMap, HashSet};
use std::sync::{Arc, OnceLock};

use crate::error::{Error, Result};
use crate::grammar::{RawGrammar, StaticRawGrammar};
use crate::theme::{StaticRawTheme, Theme};

enum LanguageSource {
    Static(&'static StaticRawGrammar),
    Json(&'static str),
}

pub struct LanguageDefinition {
    pub id: &'static str,
    pub display_name: &'static str,
    pub scope_name: &'static str,
    pub aliases: &'static [&'static str],
    pub dependencies: &'static [&'static str],
    pub inject_to: &'static [&'static str],
    source: LanguageSource,
    parsed: OnceLock<Arc<RawGrammar>>,
}

impl LanguageDefinition {
    pub const fn new(
        id: &'static str,
        display_name: &'static str,
        scope_name: &'static str,
        aliases: &'static [&'static str],
        dependencies: &'static [&'static str],
        inject_to: &'static [&'static str],
        grammar: &'static StaticRawGrammar,
    ) -> Self {
        Self {
            id,
            display_name,
            scope_name,
            aliases,
            dependencies,
            inject_to,
            source: LanguageSource::Static(grammar),
            parsed: OnceLock::new(),
        }
    }

    pub const fn from_json(
        id: &'static str,
        display_name: &'static str,
        scope_name: &'static str,
        aliases: &'static [&'static str],
        dependencies: &'static [&'static str],
        inject_to: &'static [&'static str],
        source: &'static str,
    ) -> Self {
        Self {
            id,
            display_name,
            scope_name,
            aliases,
            dependencies,
            inject_to,
            source: LanguageSource::Json(source),
            parsed: OnceLock::new(),
        }
    }

    pub fn grammar(&'static self) -> Result<Arc<RawGrammar>> {
        if let Some(grammar) = self.parsed.get() {
            return Ok(grammar.clone());
        }
        let grammar = match self.source {
            LanguageSource::Static(grammar) => grammar.to_owned(),
            LanguageSource::Json(source) => RawGrammar::from_json(self.id, source)?,
        };
        let grammar = Arc::new(grammar);
        let _ = self.parsed.set(grammar.clone());
        Ok(self.parsed.get().cloned().unwrap_or(grammar))
    }
}

pub type LanguageGroup = &'static [&'static LanguageDefinition];

#[derive(Clone, Copy)]
pub struct LanguageBundle {
    groups: &'static [LanguageGroup],
}

impl LanguageBundle {
    pub const fn from_groups(groups: &'static [LanguageGroup]) -> Self {
        Self { groups }
    }

    pub fn definitions(self) -> impl Iterator<Item = &'static LanguageDefinition> {
        self.groups.iter().flat_map(|group| group.iter().copied())
    }

    pub(crate) fn resolve(self, selected: &[String]) -> Result<Vec<&'static LanguageDefinition>> {
        let mut by_name = HashMap::new();
        for definition in self.definitions() {
            by_name.insert(definition.id, definition);
            for alias in definition.aliases {
                by_name.insert(*alias, definition);
            }
        }

        let roots: Vec<_> = if selected.is_empty() {
            self.groups
                .iter()
                .filter_map(|group| group.last().copied())
                .collect()
        } else {
            selected
                .iter()
                .map(|id| {
                    by_name
                        .get(id.as_str())
                        .copied()
                        .ok_or_else(|| Error::LanguageNotBundled(id.clone()))
                })
                .collect::<Result<_>>()?
        };

        fn visit(
            definition: &'static LanguageDefinition,
            by_name: &HashMap<&str, &'static LanguageDefinition>,
            seen: &mut HashSet<&'static str>,
            output: &mut Vec<&'static LanguageDefinition>,
        ) -> Result<()> {
            if !seen.insert(definition.id) {
                return Ok(());
            }
            for dependency in definition.dependencies {
                let child =
                    by_name
                        .get(dependency)
                        .copied()
                        .ok_or_else(|| Error::MissingDependency {
                            language: definition.id.to_owned(),
                            dependency: (*dependency).to_owned(),
                        })?;
                visit(child, by_name, seen, output)?;
            }
            output.push(definition);
            Ok(())
        }

        let mut seen = HashSet::new();
        let mut output = Vec::new();
        for root in roots {
            visit(root, &by_name, &mut seen, &mut output)?;
        }
        Ok(output)
    }
}

pub struct ThemeDefinition {
    pub id: &'static str,
    pub display_name: &'static str,
    source: ThemeSource,
    parsed: OnceLock<Arc<Theme>>,
}

enum ThemeSource {
    Static(&'static StaticRawTheme),
    Json(&'static str),
}

impl ThemeDefinition {
    pub const fn new(
        id: &'static str,
        display_name: &'static str,
        theme: &'static StaticRawTheme,
    ) -> Self {
        Self {
            id,
            display_name,
            source: ThemeSource::Static(theme),
            parsed: OnceLock::new(),
        }
    }

    pub const fn from_json(
        id: &'static str,
        display_name: &'static str,
        source: &'static str,
    ) -> Self {
        Self {
            id,
            display_name,
            source: ThemeSource::Json(source),
            parsed: OnceLock::new(),
        }
    }

    pub fn theme(&'static self) -> Result<Arc<Theme>> {
        if let Some(theme) = self.parsed.get() {
            return Ok(theme.clone());
        }
        let theme = match self.source {
            ThemeSource::Static(raw) => Theme::from_static(self.id, raw),
            ThemeSource::Json(source) => Theme::from_json(self.id, source)?,
        };
        let theme = Arc::new(theme);
        let _ = self.parsed.set(theme.clone());
        Ok(self.parsed.get().cloned().unwrap_or(theme))
    }
}

#[derive(Clone, Copy)]
pub struct ThemeBundle {
    themes: &'static [&'static ThemeDefinition],
}

impl ThemeBundle {
    pub const fn new(themes: &'static [&'static ThemeDefinition]) -> Self {
        Self { themes }
    }

    pub fn get(self, name: &str) -> Option<&'static ThemeDefinition> {
        self.themes.iter().copied().find(|theme| theme.id == name)
    }
}
