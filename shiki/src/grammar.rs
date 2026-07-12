use std::{collections::HashMap, sync::Arc};

use serde::{Deserialize, Serialize};

use crate::{
    error::{Error, Result},
    matcher::{ScopeSelector, SelectorSymbols, parse_scope_selector},
    raw::{RawList, RawMap, RawString},
};

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RawGrammar<'a> {
    #[serde(default)]
    pub name: Option<RawString<'a>>,
    pub scope_name: RawString<'a>,
    #[serde(default)]
    pub patterns: RawList<'a, RawRule<'a>>,
    #[serde(default, deserialize_with = "deserialize_repository")]
    pub repository: RawMap<'a, RawRule<'a>>,
    #[serde(default)]
    pub injections: RawMap<'a, RawRule<'a>>,
    #[serde(default)]
    pub injection_selector: Option<RawString<'a>>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RawRule<'a> {
    #[serde(default)]
    pub include: Option<RawString<'a>>,
    #[serde(default)]
    pub name: Option<RawString<'a>>,
    #[serde(default)]
    pub content_name: Option<RawString<'a>>,
    #[serde(default, rename = "match")]
    pub match_pattern: Option<RawString<'a>>,
    #[serde(default, deserialize_with = "deserialize_captures")]
    pub captures: RawMap<'a, RawRule<'a>>,
    #[serde(default)]
    pub begin: Option<RawString<'a>>,
    #[serde(default, deserialize_with = "deserialize_captures")]
    pub begin_captures: RawMap<'a, RawRule<'a>>,
    #[serde(default)]
    pub end: Option<RawString<'a>>,
    #[serde(default, deserialize_with = "deserialize_captures")]
    pub end_captures: RawMap<'a, RawRule<'a>>,
    #[serde(default, rename = "while")]
    pub while_pattern: Option<RawString<'a>>,
    #[serde(default, deserialize_with = "deserialize_captures")]
    pub while_captures: RawMap<'a, RawRule<'a>>,
    #[serde(default)]
    pub patterns: RawList<'a, RawRule<'a>>,
    #[serde(default, deserialize_with = "deserialize_repository")]
    pub repository: RawMap<'a, RawRule<'a>>,
    #[serde(default, deserialize_with = "deserialize_boolish")]
    pub apply_end_pattern_last: bool,
}

impl RawGrammar<'static> {
    pub fn from_json(name: &str, source: &str) -> Result<Self> {
        serde_json::from_str(source).map_err(|source| Error::InvalidGrammar {
            name: name.to_owned(),
            source,
        })
    }
}

impl<'a> RawRule<'a> {
    pub const EMPTY: Self = Self {
        include: None,
        name: None,
        content_name: None,
        match_pattern: None,
        captures: RawMap::borrowed(&[]),
        begin: None,
        begin_captures: RawMap::borrowed(&[]),
        end: None,
        end_captures: RawMap::borrowed(&[]),
        while_pattern: None,
        while_captures: RawMap::borrowed(&[]),
        patterns: RawList::borrowed(&[]),
        repository: RawMap::borrowed(&[]),
        apply_end_pattern_last: false,
    };
}

pub type RuleId = u32;
pub type PatternSourceId = u32;
pub type ScopeNameId = u32;
pub type ScopeTemplateId = u32;

#[derive(Clone, Serialize, Deserialize)]
pub struct ScopeName {
    pub scopes: Box<[ScopeTemplateId]>,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct ScopeTemplate {
    pub parts: Box<[ScopePart]>,
}

#[derive(Clone, Serialize, Deserialize)]
pub enum ScopePart {
    Literal(Arc<str>),
    Capture(usize),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Capture {
    pub index: usize,
    pub name: Option<ScopeNameId>,
    pub content_name: Option<ScopeNameId>,
    pub retokenize: Option<RuleId>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum RuleKind {
    Match {
        pattern: PatternSourceId,
        captures: Vec<Capture>,
    },
    IncludeOnly {
        patterns: Vec<RuleId>,
    },
    BeginEnd {
        begin: PatternSourceId,
        begin_captures: Vec<Capture>,
        end: PatternSourceId,
        end_captures: Vec<Capture>,
        patterns: Vec<RuleId>,
        apply_end_last: bool,
    },
    BeginWhile {
        begin: PatternSourceId,
        begin_captures: Vec<Capture>,
        while_pattern: PatternSourceId,
        while_captures: Vec<Capture>,
        patterns: Vec<RuleId>,
    },
    Placeholder,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Rule {
    pub name: Option<ScopeNameId>,
    pub content_name: Option<ScopeNameId>,
    pub kind: RuleKind,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct CompiledGrammar {
    pub root_scope_name: ScopeNameId,
    pub root: RuleId,
    pub rules: Vec<Rule>,
    pub patterns: Vec<Arc<str>>,
    pub scope_names: Vec<ScopeName>,
    pub scope_templates: Vec<ScopeTemplate>,
    pub injection_selectors: Vec<Arc<str>>,
    pub injections: Vec<Injection>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Injection {
    pub selector: ScopeSelector,
    pub rule: RuleId,
}

type RepoChain<'a> = Vec<&'a RawMap<'static, RawRule<'static>>>;

pub fn compile(
    scope_name: &str,
    grammars: &HashMap<String, &RawGrammar<'static>>,
    external_injections: &HashMap<String, Vec<String>>,
) -> Result<CompiledGrammar> {
    let base = grammars
        .get(scope_name)
        .ok_or_else(|| Error::GrammarNotLoaded(scope_name.to_owned()))?;
    let mut compiler = Compiler {
        grammars,
        rules: Vec::new(),
        roots: HashMap::new(),
        raw_cache: HashMap::new(),
        injections: Vec::new(),
        patterns: Vec::new(),
        pattern_ids: HashMap::new(),
        scope_names: Vec::new(),
        scope_name_ids: HashMap::new(),
        scope_templates: Vec::new(),
        scope_template_ids: HashMap::new(),
        injection_selectors: SelectorSymbols::default(),
    };
    let root_scope_name = compiler.intern_scope_name(&base.scope_name);
    let root = compiler.compile_root(base, base)?;
    for (selector, raw) in base.injections.iter() {
        let repo = vec![&base.repository];
        let id = compiler.compile_rule(raw, &repo, base, base)?;
        compiler.add_injection(selector, id);
    }
    let mut external_injections =
        external_injections.iter().collect::<Vec<_>>();
    external_injections.sort_by_key(|(target_scope, _)| target_scope.as_str());
    for (target_scope, injection_scopes) in external_injections {
        for injection_scope in injection_scopes {
            if let Some(injection) = grammars.get(injection_scope) {
                let id = compiler.compile_root(injection, base)?;
                let selector = injection
                    .injection_selector
                    .as_deref()
                    .map(str::to_owned)
                    .unwrap_or_else(|| format!("L:{target_scope}"));
                compiler.add_injection(&selector, id);
            }
        }
    }
    compiler
        .injections
        .sort_by_key(|injection| injection.selector.priority);
    Ok(CompiledGrammar {
        root_scope_name,
        root,
        rules: compiler.rules,
        patterns: compiler.patterns,
        scope_names: compiler.scope_names,
        scope_templates: compiler.scope_templates,
        injection_selectors: compiler.injection_selectors.values,
        injections: compiler.injections,
    })
}

struct Compiler<'a> {
    grammars: &'a HashMap<String, &'a RawGrammar<'static>>,
    rules: Vec<Rule>,
    roots: HashMap<(String, String), RuleId>,
    raw_cache: HashMap<(usize, String, String), RuleId>,
    injections: Vec<Injection>,
    patterns: Vec<Arc<str>>,
    pattern_ids: HashMap<Arc<str>, PatternSourceId>,
    scope_names: Vec<ScopeName>,
    scope_name_ids: HashMap<String, ScopeNameId>,
    scope_templates: Vec<ScopeTemplate>,
    scope_template_ids: HashMap<String, ScopeTemplateId>,
    injection_selectors: SelectorSymbols,
}

impl<'a> Compiler<'a> {
    fn add_injection(&mut self, selector: &str, rule: RuleId) {
        self.injections.extend(
            parse_scope_selector(selector, &mut self.injection_selectors)
                .into_iter()
                .map(|selector| Injection { selector, rule }),
        );
    }

    fn push_placeholder(
        &mut self,
        name: Option<&str>,
        content_name: Option<&str>,
    ) -> RuleId {
        let id = self.rules.len() as RuleId;
        let name = name.map(|name| self.intern_scope_name(name));
        let content_name =
            content_name.map(|name| self.intern_scope_name(name));
        self.rules.push(Rule {
            name,
            content_name,
            kind: RuleKind::Placeholder,
        });
        id
    }

    fn compile_root(
        &mut self,
        grammar: &'a RawGrammar<'static>,
        base: &'a RawGrammar<'static>,
    ) -> Result<RuleId> {
        let key = (
            grammar.scope_name.as_ref().to_owned(),
            base.scope_name.as_ref().to_owned(),
        );
        if let Some(id) = self.roots.get(&key) {
            return Ok(*id);
        }
        let id = self.push_placeholder(Some(&grammar.scope_name), None);
        self.roots.insert(key, id);
        let repo = vec![&grammar.repository];
        let patterns =
            self.compile_patterns(&grammar.patterns, &repo, grammar, base)?;
        self.rules[id as usize].kind = RuleKind::IncludeOnly { patterns };
        Ok(id)
    }

    fn compile_patterns(
        &mut self,
        patterns: &'a [RawRule<'static>],
        repo: &RepoChain<'a>,
        grammar: &'a RawGrammar<'static>,
        base: &'a RawGrammar<'static>,
    ) -> Result<Vec<RuleId>> {
        patterns
            .iter()
            .map(|rule| self.compile_rule(rule, repo, grammar, base))
            .collect()
    }

    fn compile_rule(
        &mut self,
        raw: &'a RawRule<'static>,
        repo: &RepoChain<'a>,
        grammar: &'a RawGrammar<'static>,
        base: &'a RawGrammar<'static>,
    ) -> Result<RuleId> {
        if let Some(include) = &raw.include {
            return self.compile_include(include, repo, grammar, base);
        }

        let key = (
            raw as *const RawRule as usize,
            grammar.scope_name.as_ref().to_owned(),
            base.scope_name.as_ref().to_owned(),
        );
        if let Some(id) = self.raw_cache.get(&key) {
            return Ok(*id);
        }
        let id = self
            .push_placeholder(raw.name.as_deref(), raw.content_name.as_deref());
        self.raw_cache.insert(key, id);

        let mut nested_repo = repo.clone();
        if !raw.repository.is_empty() {
            nested_repo.insert(0, &raw.repository);
        }
        let kind = if let Some(pattern) = &raw.match_pattern {
            let pattern = self.intern_pattern(normalize_pattern(pattern));
            let captures = self.compile_captures(
                &raw.captures,
                &nested_repo,
                grammar,
                base,
            )?;
            RuleKind::Match { pattern, captures }
        } else if let Some(begin) = &raw.begin {
            let patterns = self.compile_patterns(
                &raw.patterns,
                &nested_repo,
                grammar,
                base,
            )?;
            let begin = self.intern_pattern(normalize_pattern(begin));
            if let Some(while_pattern) = &raw.while_pattern {
                let begin_captures = self.compile_captures(
                    captures_or(&raw.begin_captures, &raw.captures),
                    &nested_repo,
                    grammar,
                    base,
                )?;
                let while_pattern =
                    self.intern_pattern(normalize_pattern(while_pattern));
                let while_captures = self.compile_captures(
                    captures_or(&raw.while_captures, &raw.captures),
                    &nested_repo,
                    grammar,
                    base,
                )?;
                RuleKind::BeginWhile {
                    begin,
                    begin_captures,
                    while_pattern,
                    while_captures,
                    patterns,
                }
            } else {
                let begin_captures = self.compile_captures(
                    captures_or(&raw.begin_captures, &raw.captures),
                    &nested_repo,
                    grammar,
                    base,
                )?;
                let end = self.intern_pattern(normalize_pattern(
                    raw.end.as_deref().unwrap_or(""),
                ));
                let end_captures = self.compile_captures(
                    captures_or(&raw.end_captures, &raw.captures),
                    &nested_repo,
                    grammar,
                    base,
                )?;
                RuleKind::BeginEnd {
                    begin,
                    begin_captures,
                    end,
                    end_captures,
                    patterns,
                    apply_end_last: raw.apply_end_pattern_last,
                }
            }
        } else {
            let patterns = self.compile_patterns(
                &raw.patterns,
                &nested_repo,
                grammar,
                base,
            )?;
            RuleKind::IncludeOnly { patterns }
        };
        self.rules[id as usize].kind = kind;
        Ok(id)
    }

    fn compile_captures(
        &mut self,
        captures: &'a RawMap<'static, RawRule<'static>>,
        repo: &RepoChain<'a>,
        grammar: &'a RawGrammar<'static>,
        base: &'a RawGrammar<'static>,
    ) -> Result<Vec<Capture>> {
        let mut output = Vec::new();
        for (key, capture) in captures.iter() {
            let Ok(index) = key.parse::<usize>() else {
                continue;
            };
            let retokenize =
                if capture.patterns.is_empty() && capture.include.is_none() {
                    None
                } else {
                    Some(self.compile_rule(capture, repo, grammar, base)?)
                };
            output.push(Capture {
                index,
                name: capture
                    .name
                    .as_deref()
                    .map(|name| self.intern_scope_name(name)),
                content_name: capture
                    .content_name
                    .as_deref()
                    .map(|name| self.intern_scope_name(name)),
                retokenize,
            });
        }
        output.sort_by_key(|capture| capture.index);
        Ok(output)
    }

    fn compile_include(
        &mut self,
        include: &str,
        repo: &RepoChain<'a>,
        grammar: &'a RawGrammar<'static>,
        base: &'a RawGrammar<'static>,
    ) -> Result<RuleId> {
        match include {
            "$self" => return self.compile_root(grammar, base),
            "$base" => return self.compile_root(base, base),
            _ => {}
        }
        if let Some(name) = include.strip_prefix('#') {
            for repository in repo {
                if let Some(raw) = repository.get(name) {
                    return self.compile_rule(raw, repo, grammar, base);
                }
            }
        } else {
            let (scope, name) = include
                .split_once('#')
                .map_or((include, None), |(scope, name)| (scope, Some(name)));
            if let Some(external) = self.grammars.get(scope) {
                if let Some(name) = name {
                    if let Some(raw) = external.repository.get(name) {
                        let external_repo = vec![&external.repository];
                        return self.compile_rule(
                            raw,
                            &external_repo,
                            external,
                            base,
                        );
                    }
                } else {
                    return self.compile_root(external, base);
                }
            }
        }
        let id = self.push_placeholder(None, None);
        self.rules[id as usize].kind = RuleKind::IncludeOnly {
            patterns: Vec::new(),
        };
        Ok(id)
    }

    fn intern_pattern(&mut self, pattern: String) -> PatternSourceId {
        if let Some(id) = self.pattern_ids.get(pattern.as_str()) {
            return *id;
        }
        let id = self.patterns.len() as PatternSourceId;
        let pattern: Arc<str> = Arc::from(pattern);
        self.patterns.push(pattern.clone());
        self.pattern_ids.insert(pattern, id);
        id
    }

    fn intern_scope_name(&mut self, name: &str) -> ScopeNameId {
        if let Some(id) = self.scope_name_ids.get(name) {
            return *id;
        }
        let id = self.scope_names.len() as ScopeNameId;
        let scopes = name
            .split_whitespace()
            .map(|scope| self.intern_scope_template(scope))
            .collect();
        self.scope_names.push(ScopeName { scopes });
        self.scope_name_ids.insert(name.to_owned(), id);
        id
    }

    fn intern_scope_template(&mut self, scope: &str) -> ScopeTemplateId {
        if let Some(id) = self.scope_template_ids.get(scope) {
            return *id;
        }
        let id = self.scope_templates.len() as ScopeTemplateId;
        self.scope_templates.push(ScopeTemplate {
            parts: parse_scope_template(scope).into_boxed_slice(),
        });
        self.scope_template_ids.insert(scope.to_owned(), id);
        id
    }
}

fn parse_scope_template(scope: &str) -> Vec<ScopePart> {
    let mut parts = Vec::new();
    let bytes = scope.as_bytes();
    let mut start = 0;
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'$'
            && index + 1 < bytes.len()
            && bytes[index + 1].is_ascii_digit()
        {
            if start < index {
                parts.push(ScopePart::Literal(Arc::from(&scope[start..index])));
            }
            parts.push(ScopePart::Capture((bytes[index + 1] - b'0') as usize));
            index += 2;
            start = index;
        } else {
            index += 1;
        }
    }
    if start < scope.len() {
        parts.push(ScopePart::Literal(Arc::from(&scope[start..])));
    }
    parts
}

fn captures_or<'a>(
    specific: &'a RawMap<'static, RawRule<'static>>,
    fallback: &'a RawMap<'static, RawRule<'static>>,
) -> &'a RawMap<'static, RawRule<'static>> {
    if specific.is_empty() {
        fallback
    } else {
        specific
    }
}

fn normalize_pattern(pattern: &str) -> String {
    let mut output = String::with_capacity(pattern.len());
    let mut chars = pattern.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '\\' && chars.peek() == Some(&'z') {
            chars.next();
            output.push_str(r"$(?!\n)(?<!\n)");
        } else {
            output.push(ch);
        }
    }
    output
}

fn deserialize_captures<'de, D>(
    deserializer: D,
) -> std::result::Result<RawMap<'static, RawRule<'static>>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::de::Error as _;

    let value = serde_json::Value::deserialize(deserializer)?;
    let values: Vec<(String, serde_json::Value)> = match value {
        serde_json::Value::Object(values) => values.into_iter().collect(),
        serde_json::Value::Array(values) => values
            .into_iter()
            .enumerate()
            .map(|(index, value)| (index.to_string(), value))
            .collect(),
        serde_json::Value::Null => Vec::new(),
        value => {
            return Err(D::Error::custom(format!(
                "invalid captures value: {value}"
            )));
        }
    };
    let mut captures = HashMap::new();
    for (key, value) in values {
        if key.parse::<usize>().is_err() {
            continue;
        }
        let rule = match value {
            serde_json::Value::String(name) => RawRule {
                name: Some(name.into()),
                ..RawRule::default()
            },
            value @ serde_json::Value::Object(_) => {
                serde_json::from_value(value).map_err(D::Error::custom)?
            }
            _ => continue,
        };
        captures.insert(key, rule);
    }
    Ok(RawMap::Owned(captures.into_iter().collect()))
}

fn deserialize_boolish<'de, D>(
    deserializer: D,
) -> std::result::Result<bool, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = serde_json::Value::deserialize(deserializer)?;
    Ok(match value {
        serde_json::Value::Bool(value) => value,
        serde_json::Value::Number(value) => {
            value.as_i64().is_some_and(|value| value != 0)
        }
        serde_json::Value::String(value) => {
            value == "1" || value.eq_ignore_ascii_case("true")
        }
        _ => false,
    })
}

fn deserialize_repository<'de, D>(
    deserializer: D,
) -> std::result::Result<RawMap<'static, RawRule<'static>>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::de::Error as _;

    let values =
        HashMap::<String, serde_json::Value>::deserialize(deserializer)?;
    let values = values
        .into_iter()
        .map(|(name, value)| {
            let rule =
                match value {
                    serde_json::Value::Array(values) => RawRule {
                        patterns:
                            RawList::Owned(
                                values
                                    .into_iter()
                                    .map(serde_json::from_value)
                                    .collect::<std::result::Result<
                                        Vec<RawRule<'static>>,
                                        _,
                                    >>()
                                    .map_err(D::Error::custom)?,
                            ),
                        ..RawRule::default()
                    },
                    value => serde_json::from_value(value)
                        .map_err(D::Error::custom)?,
                };
            Ok((name, rule))
        })
        .collect::<std::result::Result<std::collections::BTreeMap<_, _>, _>>(
        )?;
    Ok(RawMap::Owned(values))
}
