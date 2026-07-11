use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;

use crate::error::{Error, Result};
use crate::raw::{RawList, RawMap, RawString};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct FontStyle(u8);

impl FontStyle {
    pub const ITALIC: Self = Self(1);
    pub const BOLD: Self = Self(2);
    pub const UNDERLINE: Self = Self(4);
    pub const STRIKETHROUGH: Self = Self(8);

    pub const fn contains(self, other: Self) -> bool {
        self.0 & other.0 != 0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub(crate) struct ColorId(u32);

impl ColorId {
    const fn index(self) -> usize {
        self.0 as usize
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Style {
    pub foreground: Option<ColorId>,
    pub background: Option<ColorId>,
    pub font_style: Option<FontStyle>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ThemeRule {
    target: SelectorId,
    parents: Vec<SelectorId>,
    target_depth: usize,
    style: Style,
    order: usize,
}

type SelectorId = u32;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Theme {
    pub name: String,
    pub foreground: String,
    pub background: String,
    colors: Vec<String>,
    foreground_id: ColorId,
    selectors: Vec<String>,
    rules: Vec<ThemeRule>,
}

pub(crate) struct ThemeMatcher {
    theme: Arc<Theme>,
    scope_matches: Vec<Box<[bool]>>,
    candidates: Vec<Box<[usize]>>,
}

impl Theme {
    pub fn from_raw(name: &str, raw: &RawTheme<'_>) -> Self {
        let settings = if raw.settings.is_empty() {
            &raw.token_colors
        } else {
            &raw.settings
        };
        let mut foreground = raw
            .fg
            .as_deref()
            .or_else(|| {
                raw.colors
                    .get("editor.foreground")
                    .map(|value| value.as_ref())
            })
            .map(str::to_owned)
            .unwrap_or_else(|| "#000000".to_owned());
        let mut background = raw
            .bg
            .as_deref()
            .or_else(|| {
                raw.colors
                    .get("editor.background")
                    .map(|value| value.as_ref())
            })
            .map(str::to_owned)
            .unwrap_or_else(|| "#ffffff".to_owned());
        let mut colors = Vec::new();
        let mut color_ids = HashMap::new();
        let mut selectors = Vec::new();
        let mut selector_ids = HashMap::new();
        let mut rules = Vec::new();

        for (order, entry) in settings.iter().enumerate() {
            let style = Style {
                foreground: entry
                    .settings
                    .foreground
                    .as_deref()
                    .map(|color| intern_color(&mut colors, &mut color_ids, color)),
                background: entry
                    .settings
                    .background
                    .as_deref()
                    .map(|color| intern_color(&mut colors, &mut color_ids, color)),
                font_style: entry.settings.font_style.as_deref().map(parse_font_style),
            };
            if entry.scope.is_empty() {
                if let Some(value) = style.foreground {
                    foreground = colors[value.index()].clone();
                }
                if let Some(value) = style.background {
                    background = colors[value.index()].clone();
                }
                continue;
            }
            for selector in entry.scope.iter() {
                for selector in selector.split(',').map(str::trim).filter(|s| !s.is_empty()) {
                    let mut parts: Vec<_> = selector.split_whitespace().collect();
                    let Some(target) = parts.pop() else {
                        continue;
                    };
                    parts.reverse();
                    let target_depth = target.split('.').count();
                    rules.push(ThemeRule {
                        target: intern_selector(&mut selectors, &mut selector_ids, target),
                        parents: parts
                            .into_iter()
                            .filter(|parent| *parent != ">")
                            .map(|parent| {
                                intern_selector(&mut selectors, &mut selector_ids, parent)
                            })
                            .collect(),
                        target_depth,
                        style,
                        order,
                    });
                }
            }
        }
        let foreground_id = intern_color(&mut colors, &mut color_ids, &foreground);
        intern_color(&mut colors, &mut color_ids, &background);
        Self {
            name: raw.name.as_deref().unwrap_or(name).to_owned(),
            foreground,
            background,
            colors,
            foreground_id,
            selectors,
            rules,
        }
    }

    pub(crate) fn color(&self, color: ColorId) -> &str {
        &self.colors[color.index()]
    }

    pub(crate) const fn foreground_id(&self) -> ColorId {
        self.foreground_id
    }

    pub(crate) fn matcher(self: &Arc<Self>) -> ThemeMatcher {
        ThemeMatcher {
            theme: self.clone(),
            scope_matches: Vec::new(),
            candidates: Vec::new(),
        }
    }
}

impl ThemeMatcher {
    pub(crate) fn scope_count(&self) -> usize {
        self.scope_matches.len()
    }

    pub(crate) fn register_scope(&mut self, chunks: &[&str]) {
        let matches: Box<[_]> = self
            .theme
            .selectors
            .iter()
            .map(|selector| scope_matches(chunks, selector))
            .collect();
        let mut candidates: Vec<_> = self
            .theme
            .rules
            .iter()
            .enumerate()
            .filter_map(|(index, rule)| matches[rule.target as usize].then_some(index))
            .collect();
        candidates.sort_by_key(|index| {
            let rule = &self.theme.rules[*index];
            (rule.target_depth, rule.parents.len(), rule.order)
        });
        self.scope_matches.push(matches);
        self.candidates.push(candidates.into_boxed_slice());
    }

    pub(crate) fn resolve_scope(&self, path: &[u32], mut result: Style) -> Style {
        let Some((&scope, parents)) = path.split_last() else {
            return result;
        };
        for rule_index in self.candidates[scope as usize].iter().copied() {
            let rule = &self.theme.rules[rule_index];
            let mut cursor = parents.len();
            let parents_match = rule.parents.iter().all(|selector| {
                let Some(index) = parents[..cursor]
                    .iter()
                    .rposition(|scope| self.scope_matches[*scope as usize][*selector as usize])
                else {
                    return false;
                };
                cursor = index;
                true
            });
            if parents_match {
                apply_style(&mut result, rule.style);
            }
        }
        result
    }
}

fn apply_style(result: &mut Style, style: Style) {
    if style.foreground.is_some() {
        result.foreground = style.foreground;
    }
    if style.background.is_some() {
        result.background = style.background;
    }
    if style.font_style.is_some() {
        result.font_style = style.font_style;
    }
}

fn intern_color(
    colors: &mut Vec<String>,
    color_ids: &mut HashMap<String, ColorId>,
    color: &str,
) -> ColorId {
    if let Some(id) = color_ids.get(color) {
        return *id;
    }
    let id = ColorId(colors.len() as u32);
    colors.push(color.to_owned());
    color_ids.insert(color.to_owned(), id);
    id
}

fn intern_selector(
    selectors: &mut Vec<String>,
    selector_ids: &mut HashMap<String, SelectorId>,
    selector: &str,
) -> SelectorId {
    if let Some(id) = selector_ids.get(selector) {
        return *id;
    }
    let id = selectors.len() as SelectorId;
    selectors.push(selector.to_owned());
    selector_ids.insert(selector.to_owned(), id);
    id
}

fn scope_matches(chunks: &[&str], selector: &str) -> bool {
    crate::matcher::scope_matches(chunks, selector)
}

fn parse_font_style(value: &str) -> FontStyle {
    let mut style = FontStyle::default();
    for item in value.split_whitespace() {
        style.0 |= match item {
            "italic" => FontStyle::ITALIC.0,
            "bold" => FontStyle::BOLD.0,
            "underline" => FontStyle::UNDERLINE.0,
            "strikethrough" => FontStyle::STRIKETHROUGH.0,
            _ => 0,
        };
    }
    style
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RawTheme<'a> {
    #[serde(default)]
    pub name: Option<RawString<'a>>,
    #[serde(default)]
    pub fg: Option<RawString<'a>>,
    #[serde(default)]
    pub bg: Option<RawString<'a>>,
    #[serde(default, deserialize_with = "deserialize_string_map")]
    pub colors: RawMap<'a, RawString<'a>>,
    #[serde(default)]
    pub settings: RawList<'a, RawThemeRule<'a>>,
    #[serde(default)]
    pub token_colors: RawList<'a, RawThemeRule<'a>>,
}

impl RawTheme<'static> {
    pub fn from_json(name: &str, source: &str) -> Result<Self> {
        serde_json::from_str(source).map_err(|source| Error::InvalidTheme {
            name: name.to_owned(),
            source,
        })
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct RawThemeRule<'a> {
    #[serde(default)]
    pub scope: RawThemeScope<'a>,
    #[serde(default)]
    pub settings: RawThemeSettings<'a>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(untagged)]
pub enum RawThemeScope<'a> {
    String(RawString<'a>),
    Array(RawList<'a, RawString<'a>>),
    #[default]
    Missing,
}

impl RawThemeScope<'_> {
    fn is_empty(&self) -> bool {
        match self {
            Self::String(_) => false,
            Self::Array(values) => values.is_empty(),
            Self::Missing => true,
        }
    }

    fn iter(&self) -> impl Iterator<Item = &str> {
        let string = match self {
            Self::String(value) => Some(value.as_ref()),
            _ => None,
        };
        let array = match self {
            Self::Array(values) => values.iter(),
            _ => [].iter(),
        };
        string.into_iter().chain(array.map(AsRef::as_ref))
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RawThemeSettings<'a> {
    #[serde(default)]
    pub foreground: Option<RawString<'a>>,
    #[serde(default)]
    pub background: Option<RawString<'a>>,
    #[serde(default)]
    pub font_style: Option<RawString<'a>>,
}

impl<'a> RawThemeRule<'a> {
    pub const EMPTY: Self = Self {
        scope: RawThemeScope::Missing,
        settings: RawThemeSettings::EMPTY,
    };
}

impl<'a> RawThemeSettings<'a> {
    pub const EMPTY: Self = Self {
        foreground: None,
        background: None,
        font_style: None,
    };
}

fn deserialize_string_map<'de, D>(
    deserializer: D,
) -> std::result::Result<RawMap<'static, RawString<'static>>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let values = std::collections::HashMap::<String, serde_json::Value>::deserialize(deserializer)?;
    Ok(RawMap::Owned(
        values
            .into_iter()
            .filter_map(|(key, value)| {
                value
                    .as_str()
                    .map(|value| (key, RawString::Owned(value.to_owned())))
            })
            .collect::<std::collections::BTreeMap<_, _>>(),
    ))
}
