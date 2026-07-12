use std::{
    collections::{HashMap, HashSet},
    sync::{Arc, OnceLock},
};

use crate::{
    error::{Error, Result},
    grammar::RawGrammar,
    theme::{RawTheme, Theme},
};

pub struct LanguageDefinition {
    pub id: &'static str,
    pub display_name: &'static str,
    pub scope_name: &'static str,
    pub aliases: &'static [&'static str],
    pub dependencies: &'static [&'static str],
    pub inject_to: &'static [&'static str],
    grammar: Option<&'static RawGrammar<'static>>,
    #[cfg(feature = "json")]
    source: Option<&'static str>,
    #[cfg(feature = "json")]
    parsed: OnceLock<RawGrammar<'static>>,
}

impl LanguageDefinition {
    pub const fn new(
        id: &'static str,
        display_name: &'static str,
        scope_name: &'static str,
        aliases: &'static [&'static str],
        dependencies: &'static [&'static str],
        inject_to: &'static [&'static str],
        grammar: &'static RawGrammar<'static>,
    ) -> Self {
        Self {
            id,
            display_name,
            scope_name,
            aliases,
            dependencies,
            inject_to,
            grammar: Some(grammar),
            #[cfg(feature = "json")]
            source: None,
            #[cfg(feature = "json")]
            parsed: OnceLock::new(),
        }
    }

    #[cfg(feature = "json")]
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
            grammar: None,
            source: Some(source),
            parsed: OnceLock::new(),
        }
    }

    pub fn grammar(&'static self) -> &'static RawGrammar<'static> {
        if let Some(grammar) = self.grammar {
            return grammar;
        }
        #[cfg(feature = "json")]
        {
            self.parsed.get_or_init(|| {
                RawGrammar::from_json(
                    self.id,
                    self.source
                        .expect("language definition has no grammar source"),
                )
                .expect("bundled grammar JSON is invalid")
            })
        }
        #[cfg(not(feature = "json"))]
        unreachable!("a non-JSON language definition must contain a grammar")
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

    pub fn definitions(
        self,
    ) -> impl Iterator<Item = &'static LanguageDefinition> {
        self.groups.iter().flat_map(|group| group.iter().copied())
    }

    pub(crate) fn resolve(
        self,
        selected: &[String],
    ) -> Result<(
        Vec<&'static LanguageDefinition>,
        Vec<&'static LanguageDefinition>,
    )> {
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
                    by_name.get(dependency).copied().ok_or_else(|| {
                        Error::MissingDependency {
                            language: definition.id.to_owned(),
                            dependency: (*dependency).to_owned(),
                        }
                    })?;
                visit(child, by_name, seen, output)?;
            }
            output.push(definition);
            Ok(())
        }

        let mut seen = HashSet::new();
        let mut output = Vec::new();
        for root in &roots {
            visit(root, &by_name, &mut seen, &mut output)?;
        }
        Ok((output, roots))
    }
}

pub struct ThemeDefinition {
    pub id: &'static str,
    pub display_name: &'static str,
    raw: Option<&'static RawTheme<'static>>,
    #[cfg(feature = "json")]
    source: Option<&'static str>,
    #[cfg(feature = "json")]
    parsed_raw: OnceLock<RawTheme<'static>>,
    parsed_theme: OnceLock<Arc<Theme>>,
}

impl ThemeDefinition {
    pub const fn new(
        id: &'static str,
        display_name: &'static str,
        raw: &'static RawTheme<'static>,
    ) -> Self {
        Self {
            id,
            display_name,
            raw: Some(raw),
            #[cfg(feature = "json")]
            source: None,
            #[cfg(feature = "json")]
            parsed_raw: OnceLock::new(),
            parsed_theme: OnceLock::new(),
        }
    }

    #[cfg(feature = "json")]
    pub const fn from_json(
        id: &'static str,
        display_name: &'static str,
        source: &'static str,
    ) -> Self {
        Self {
            id,
            display_name,
            raw: None,
            source: Some(source),
            parsed_raw: OnceLock::new(),
            parsed_theme: OnceLock::new(),
        }
    }

    pub fn raw(&'static self) -> &'static RawTheme<'static> {
        if let Some(raw) = self.raw {
            return raw;
        }
        #[cfg(feature = "json")]
        {
            self.parsed_raw.get_or_init(|| {
                RawTheme::from_json(
                    self.id,
                    self.source.expect("theme definition has no source"),
                )
                .expect("bundled theme JSON is invalid")
            })
        }
        #[cfg(not(feature = "json"))]
        unreachable!("a non-JSON theme definition must contain raw theme data")
    }

    pub fn theme(&'static self) -> Arc<Theme> {
        self.parsed_theme
            .get_or_init(|| Arc::new(Theme::from_raw(self.id, self.raw())))
            .clone()
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
