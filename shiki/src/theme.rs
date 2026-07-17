use std::{collections::HashMap, sync::Arc};

#[cfg(feature = "json")]
use serde::Deserialize;

#[cfg(feature = "json")]
use crate::error::{Error, Result};
use crate::raw::{RawList, RawMap, RawString};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct FontStyle(pub u8);

impl FontStyle {
    pub const ITALIC: Self = Self(1);
    pub const BOLD: Self = Self(2);
    pub const UNDERLINE: Self = Self(4);
    pub const STRIKETHROUGH: Self = Self(8);

    pub const fn contains(self, other: Self) -> bool {
        self.0 & other.0 != 0
    }

    pub const fn bits(self) -> u8 {
        self.0
    }

    pub const fn from_bits(bits: u8) -> Self {
        Self(bits & 0b1111)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ColorId(pub u32);

impl ColorId {
    const fn index(self) -> usize {
        self.0 as usize
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Style {
    pub foreground: Option<ColorId>,
    pub background: Option<ColorId>,
    pub font_style: Option<FontStyle>,
}

#[derive(Debug, Clone)]
pub struct ThemeRule {
    pub target: SelectorId,
    pub parents: Vec<SelectorId>,
    pub target_depth: usize,
    pub style: Style,
    pub order: usize,
}

type SelectorId = u32;

#[derive(Debug, Clone)]
pub struct Theme {
    pub name: Arc<str>,
    pub foreground: Arc<str>,
    pub background: Arc<str>,
    pub colors: Vec<Arc<str>>,
    pub foreground_id: ColorId,
    pub ansi_colors: [ColorId; 16],
    pub selectors: Vec<Arc<str>>,
    pub rules: Vec<ThemeRule>,
}

pub struct ThemeMatcher {
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
            .map(Arc::from)
            .unwrap_or_else(|| Arc::from("#000000"));
        let mut background = raw
            .bg
            .as_deref()
            .or_else(|| {
                raw.colors
                    .get("editor.background")
                    .map(|value| value.as_ref())
            })
            .map(Arc::from)
            .unwrap_or_else(|| Arc::from("#ffffff"));
        let mut colors = Vec::new();
        let mut color_ids = HashMap::new();
        let mut selectors = Vec::new();
        let mut selector_ids = HashMap::new();
        let mut rules = Vec::new();

        for (order, entry) in settings.iter().enumerate() {
            let style = Style {
                foreground: entry.settings.foreground.as_deref().map(|color| {
                    intern_color(&mut colors, &mut color_ids, color)
                }),
                background: entry.settings.background.as_deref().map(|color| {
                    intern_color(&mut colors, &mut color_ids, color)
                }),
                font_style: entry
                    .settings
                    .font_style
                    .as_deref()
                    .map(parse_font_style),
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
                for selector in
                    selector.split(',').map(str::trim).filter(|s| !s.is_empty())
                {
                    let mut parts: Vec<_> =
                        selector.split_whitespace().collect();
                    let Some(target) = parts.pop() else {
                        continue;
                    };
                    parts.reverse();
                    let target_depth = target.split('.').count();
                    rules.push(ThemeRule {
                        target: intern_selector(
                            &mut selectors,
                            &mut selector_ids,
                            target,
                        ),
                        parents: parts
                            .into_iter()
                            .filter(|parent| *parent != ">")
                            .map(|parent| {
                                intern_selector(
                                    &mut selectors,
                                    &mut selector_ids,
                                    parent,
                                )
                            })
                            .collect(),
                        target_depth,
                        style,
                        order,
                    });
                }
            }
        }
        let foreground_id =
            intern_color(&mut colors, &mut color_ids, &foreground);
        let background_id =
            intern_color(&mut colors, &mut color_ids, &background);
        let ansi_colors = std::array::from_fn(|index| {
            let color = raw
                .colors
                .get(ANSI_COLOR_KEYS[index])
                .map(AsRef::as_ref)
                .unwrap_or(DEFAULT_ANSI_COLORS[index]);
            intern_color(&mut colors, &mut color_ids, color)
        });
        Self {
            name: Arc::from(raw.name.as_deref().unwrap_or(name)),
            foreground: colors[foreground_id.index()].clone(),
            background: colors[background_id.index()].clone(),
            colors,
            foreground_id,
            ansi_colors,
            selectors,
            rules,
        }
    }

    pub fn color(&self, color: ColorId) -> &str {
        &self.colors[color.index()]
    }

    pub fn palette(&self) -> impl ExactSizeIterator<Item = &str> {
        self.colors.iter().map(AsRef::as_ref)
    }

    pub fn selectors(&self) -> impl ExactSizeIterator<Item = &str> {
        self.selectors.iter().map(AsRef::as_ref)
    }

    pub fn color_arc(&self, color: ColorId) -> Arc<str> {
        self.colors[color.index()].clone()
    }

    pub const fn foreground_id(&self) -> ColorId {
        self.foreground_id
    }

    pub fn ansi_color(&self, index: u8) -> &str {
        self.color(self.ansi_colors[usize::from(index.min(15))])
    }

    pub fn matcher(self: &Arc<Self>) -> ThemeMatcher {
        ThemeMatcher {
            theme: self.clone(),
            scope_matches: Vec::new(),
            candidates: Vec::new(),
        }
    }
}

const ANSI_COLOR_KEYS: [&str; 16] = [
    "terminal.ansiBlack",
    "terminal.ansiRed",
    "terminal.ansiGreen",
    "terminal.ansiYellow",
    "terminal.ansiBlue",
    "terminal.ansiMagenta",
    "terminal.ansiCyan",
    "terminal.ansiWhite",
    "terminal.ansiBrightBlack",
    "terminal.ansiBrightRed",
    "terminal.ansiBrightGreen",
    "terminal.ansiBrightYellow",
    "terminal.ansiBrightBlue",
    "terminal.ansiBrightMagenta",
    "terminal.ansiBrightCyan",
    "terminal.ansiBrightWhite",
];

const DEFAULT_ANSI_COLORS: [&str; 16] = [
    "#000000", "#cd3131", "#0DBC79", "#E5E510", "#2472C8", "#BC3FBC",
    "#11A8CD", "#E5E5E5", "#666666", "#F14C4C", "#23D18B", "#F5F543",
    "#3B8EEA", "#D670D6", "#29B8DB", "#FFFFFF",
];

impl ThemeMatcher {
    pub fn scope_count(&self) -> usize {
        self.scope_matches.len()
    }

    pub fn register_scope(&mut self, chunks: &[&str]) {
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
            .filter_map(|(index, rule)| {
                matches[rule.target as usize].then_some(index)
            })
            .collect();
        candidates.sort_by_key(|index| {
            let rule = &self.theme.rules[*index];
            (rule.target_depth, rule.parents.len(), rule.order)
        });
        self.scope_matches.push(matches);
        self.candidates.push(candidates.into_boxed_slice());
    }

    pub fn resolve_scope(&self, path: &[u32], mut result: Style) -> Style {
        let Some((&scope, parents)) = path.split_last() else {
            return result;
        };
        for rule_index in self.candidates[scope as usize].iter().copied() {
            let rule = &self.theme.rules[rule_index];
            let mut cursor = parents.len();
            let parents_match = rule.parents.iter().all(|selector| {
                let Some(index) = parents[..cursor].iter().rposition(|scope| {
                    self.scope_matches[*scope as usize][*selector as usize]
                }) else {
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
    colors: &mut Vec<Arc<str>>,
    color_ids: &mut HashMap<Arc<str>, ColorId>,
    color: &str,
) -> ColorId {
    if let Some(id) = color_ids.get(color) {
        return *id;
    }
    let id = ColorId(colors.len() as u32);
    let color: Arc<str> = Arc::from(color);
    colors.push(color.clone());
    color_ids.insert(color, id);
    id
}

fn intern_selector(
    selectors: &mut Vec<Arc<str>>,
    selector_ids: &mut HashMap<Arc<str>, SelectorId>,
    selector: &str,
) -> SelectorId {
    if let Some(id) = selector_ids.get(selector) {
        return *id;
    }
    let id = selectors.len() as SelectorId;
    let selector: Arc<str> = Arc::from(selector);
    selectors.push(selector.clone());
    selector_ids.insert(selector, id);
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

#[derive(Debug, Clone)]
#[cfg_attr(feature = "json", derive(Deserialize))]
#[cfg_attr(feature = "json", serde(rename_all = "camelCase"))]
pub struct RawTheme<'a> {
    #[cfg_attr(feature = "json", serde(default))]
    pub name: Option<RawString<'a>>,
    #[cfg_attr(feature = "json", serde(default))]
    pub fg: Option<RawString<'a>>,
    #[cfg_attr(feature = "json", serde(default))]
    pub bg: Option<RawString<'a>>,
    #[cfg_attr(
        feature = "json",
        serde(default, deserialize_with = "deserialize_string_map")
    )]
    pub colors: RawMap<'a, RawString<'a>>,
    #[cfg_attr(feature = "json", serde(default))]
    pub settings: RawList<'a, RawThemeRule<'a>>,
    #[cfg_attr(feature = "json", serde(default))]
    pub token_colors: RawList<'a, RawThemeRule<'a>>,
}

impl RawTheme<'static> {
    #[cfg(feature = "json")]
    pub fn from_json(name: &str, source: &str) -> Result<Self> {
        serde_json::from_str(source).map_err(|source| Error::InvalidTheme {
            name: name.to_owned(),
            source,
        })
    }
}

#[derive(Debug, Clone, Default)]
#[cfg_attr(feature = "json", derive(Deserialize))]
pub struct RawThemeRule<'a> {
    #[cfg_attr(feature = "json", serde(default))]
    pub scope: RawThemeScope<'a>,
    #[cfg_attr(feature = "json", serde(default))]
    pub settings: RawThemeSettings<'a>,
}

#[derive(Debug, Clone, Default)]
#[cfg_attr(feature = "json", derive(Deserialize))]
#[cfg_attr(feature = "json", serde(untagged))]
pub enum RawThemeScope<'a> {
    String(RawString<'a>),
    Array(RawList<'a, RawString<'a>>),
    #[default]
    Missing,
}

impl RawThemeScope<'_> {
    pub fn is_empty(&self) -> bool {
        match self {
            Self::String(_) => false,
            Self::Array(values) => values.is_empty(),
            Self::Missing => true,
        }
    }

    pub fn iter(&self) -> impl Iterator<Item = &str> {
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

#[derive(Debug, Clone, Default)]
#[cfg_attr(feature = "json", derive(Deserialize))]
#[cfg_attr(feature = "json", serde(rename_all = "camelCase"))]
pub struct RawThemeSettings<'a> {
    #[cfg_attr(feature = "json", serde(default))]
    pub foreground: Option<RawString<'a>>,
    #[cfg_attr(feature = "json", serde(default))]
    pub background: Option<RawString<'a>>,
    #[cfg_attr(feature = "json", serde(default))]
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

#[cfg(feature = "json")]
fn deserialize_string_map<'de, D>(
    deserializer: D,
) -> std::result::Result<RawMap<'static, RawString<'static>>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let values =
        std::collections::HashMap::<String, serde_json::Value>::deserialize(
            deserializer,
        )?;
    Ok(RawMap::Owned(
        values
            .into_iter()
            .filter_map(|(key, value)| {
                value
                    .as_str()
                    .map(|value| (key, RawString::from(value.to_owned())))
            })
            .collect::<std::collections::BTreeMap<_, _>>(),
    ))
}
