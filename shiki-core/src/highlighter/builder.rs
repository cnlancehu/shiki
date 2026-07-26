use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

use super::{
    engine::{
        CatalogEntry, CompiledLanguage, EngineInner, GrammarCatalog,
        HighlighterEngine, NamedTheme,
    },
    instance::Highlighter,
};
use crate::{
    definition::{LanguageBundle, ThemeDefinition, is_plain_text},
    error::{Error, Result},
    grammar::{PatternInterner, RawGrammar},
    theme::{RawTheme, Theme},
    tokenizer::{RegexLimits, RegexPool},
};

pub struct HighlighterBuilder {
    bundle: Option<LanguageBundle>,
    languages: Vec<String>,
    runtime_languages: Vec<LanguageInput>,
    themes: Vec<(String, ThemeInput)>,
    regex_limits: RegexLimits,
}

impl Default for HighlighterBuilder {
    fn default() -> Self {
        Highlighter::builder()
    }
}

pub struct LanguageInput {
    pub id: String,
    pub aliases: Vec<String>,
    pub inject_to: Vec<String>,
    pub grammar: RawGrammar<'static>,
}

impl LanguageInput {
    pub fn new(id: impl Into<String>, grammar: RawGrammar<'static>) -> Self {
        Self {
            id: id.into(),
            aliases: Vec::new(),
            inject_to: Vec::new(),
            grammar,
        }
    }

    pub fn alias(mut self, alias: impl Into<String>) -> Self {
        self.aliases.push(alias.into());
        self
    }

    pub fn inject_to(mut self, scope: impl Into<String>) -> Self {
        self.inject_to.push(scope.into());
        self
    }
}

pub enum ThemeInput {
    Definition(&'static ThemeDefinition),
    Raw(RawTheme<'static>),
}

impl From<&'static ThemeDefinition> for ThemeInput {
    fn from(value: &'static ThemeDefinition) -> Self {
        Self::Definition(value)
    }
}

impl From<RawTheme<'static>> for ThemeInput {
    fn from(value: RawTheme<'static>) -> Self {
        Self::Raw(value)
    }
}

impl HighlighterBuilder {
    pub(super) fn new() -> Self {
        Self {
            bundle: None,
            languages: Vec::new(),
            runtime_languages: Vec::new(),
            themes: Vec::new(),
            regex_limits: RegexLimits::default(),
        }
    }

    pub fn bundle(mut self, bundle: &LanguageBundle) -> Self {
        self.bundle = Some(*bundle);
        self
    }

    pub fn languages<I, S>(mut self, languages: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.languages = languages.into_iter().map(Into::into).collect();
        self
    }

    pub fn theme(mut self, theme: impl Into<ThemeInput>) -> Self {
        self.themes = vec![("default".to_owned(), theme.into())];
        self
    }

    pub fn themes<I, S, T>(mut self, themes: I) -> Self
    where
        I: IntoIterator<Item = (S, T)>,
        S: Into<String>,
        T: Into<ThemeInput>,
    {
        self.themes = themes
            .into_iter()
            .map(|(name, theme)| (name.into(), theme.into()))
            .collect();
        self
    }

    pub fn regex_limits(mut self, limits: RegexLimits) -> Self {
        self.regex_limits = limits;
        self
    }

    pub fn language(
        mut self,
        id: impl Into<String>,
        grammar: RawGrammar<'static>,
    ) -> Self {
        self.runtime_languages.push(LanguageInput::new(id, grammar));
        self
    }

    pub fn language_definition(mut self, language: LanguageInput) -> Self {
        self.runtime_languages.push(language);
        self
    }

    #[cfg(feature = "json")]
    pub fn json_language(
        self,
        id: impl Into<String>,
        source: &str,
    ) -> Result<Self> {
        let id = id.into();
        let grammar = RawGrammar::from_json(&id, source)?;
        Ok(self.language(id, grammar))
    }

    #[cfg(feature = "json")]
    pub fn json_theme(
        mut self,
        name: impl Into<String>,
        source: &str,
    ) -> Result<Self> {
        let name = name.into();
        let theme = RawTheme::from_json(&name, source)?;
        self.themes.push((name, ThemeInput::Raw(theme)));
        Ok(self)
    }

    pub fn build(self) -> Result<Highlighter> {
        Ok(self.build_engine()?.highlighter())
    }

    pub fn build_engine(self) -> Result<HighlighterEngine> {
        let selected_special = !self.languages.is_empty()
            && self
                .languages
                .iter()
                .all(|id| is_plain_text(id) || crate::ansi::is_ansi(id));
        let (definitions, root_definitions) = match self.bundle {
            Some(bundle) => bundle.resolve(&self.languages)?,
            None if self.runtime_languages.is_empty() && !selected_special => {
                return Err(Error::NoLanguage);
            }
            None => (Vec::new(), Vec::new()),
        };
        if self.themes.is_empty() {
            return Err(Error::NoTheme);
        }

        let mut seen_themes = HashSet::new();
        let mut themes = Vec::with_capacity(self.themes.len());
        for (name, definition) in self.themes {
            let css_name = css_name(&name);
            if !seen_themes.insert(css_name.clone()) {
                return Err(Error::DuplicateTheme(name));
            }
            let theme = match definition {
                ThemeInput::Definition(definition) => definition.theme(),
                ThemeInput::Raw(raw) => Arc::new(Theme::from_raw(&name, &raw)),
            };
            themes.push(NamedTheme {
                name,
                css_name,
                theme,
            });
        }

        let runtime_languages = self
            .runtime_languages
            .into_iter()
            .map(|language| {
                let scope_name =
                    language.grammar.scope_name.as_ref().to_owned();
                (
                    language.id,
                    scope_name,
                    language.aliases,
                    language.inject_to,
                    Arc::new(language.grammar),
                )
            })
            .collect::<Vec<_>>();

        let mut catalog =
            HashMap::with_capacity(definitions.len() + runtime_languages.len());
        let mut injections: HashMap<String, Vec<String>> = HashMap::new();
        for definition in &definitions {
            catalog.insert(
                definition.scope_name.to_owned(),
                CatalogEntry::Bundled(definition),
            );
            for target in definition.inject_to {
                injections
                    .entry((*target).to_owned())
                    .or_default()
                    .push(definition.scope_name.to_owned());
            }
        }
        for (_, scope_name, _, inject_to, grammar) in &runtime_languages {
            catalog.insert(
                scope_name.clone(),
                CatalogEntry::Runtime(grammar.clone()),
            );
            for target in inject_to {
                injections
                    .entry(target.clone())
                    .or_default()
                    .push(scope_name.clone());
            }
        }
        let catalog = Arc::new(GrammarCatalog {
            by_scope: catalog,
            pattern_interner: PatternInterner::default(),
        });
        let injections = Arc::new(injections);

        let mut languages = HashMap::new();
        let mut compiled = Vec::with_capacity(
            root_definitions.len() + runtime_languages.len(),
        );
        let mut seen_roots = HashSet::new();
        for definition in root_definitions {
            if !seen_roots.insert(definition.id) {
                continue;
            }
            let index = compiled.len();
            compiled.push(CompiledLanguage::catalog(
                definition.scope_name,
                catalog.clone(),
                injections.clone(),
            ));
            languages.insert(definition.id.to_owned(), index);
            languages.insert(definition.scope_name.to_owned(), index);
            for alias in definition.aliases {
                languages.insert((*alias).to_owned(), index);
            }
        }
        for (id, scope_name, aliases, _, _) in &runtime_languages {
            let index = compiled.len();
            compiled.push(CompiledLanguage::catalog(
                scope_name.as_str(),
                catalog.clone(),
                injections.clone(),
            ));
            languages.insert(id.clone(), index);
            languages.insert(scope_name.clone(), index);
            for alias in aliases {
                languages.insert(alias.clone(), index);
            }
        }
        let plain_text = compiled.len();
        compiled.push(CompiledLanguage::plain_text());
        for name in ["text", "txt", "plain", "text.plain"] {
            languages.insert(name.to_owned(), plain_text);
        }
        Ok(HighlighterEngine {
            inner: Arc::new(EngineInner {
                languages,
                compiled,
                themes,
                regex_limits: self.regex_limits,
                regex_pool: Arc::new(RegexPool::default()),
            }),
        })
    }
}

pub fn css_name(name: &str) -> String {
    let output: String = name
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_') {
                ch.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect();
    if output.is_empty() {
        "theme".to_owned()
    } else {
        output
    }
}
