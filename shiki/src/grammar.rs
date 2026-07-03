use std::collections::HashMap;

use serde::Deserialize;

use crate::error::{Error, Result};
use crate::matcher::{ScopeSelector, SelectorSymbols, parse_scope_selector};

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RawGrammar {
    #[serde(default)]
    pub name: Option<String>,
    pub scope_name: String,
    #[serde(default)]
    pub patterns: Vec<RawRule>,
    #[serde(default, deserialize_with = "deserialize_repository")]
    pub repository: HashMap<String, RawRule>,
    #[serde(default)]
    pub injections: HashMap<String, RawRule>,
    #[serde(default)]
    pub injection_selector: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RawRule {
    #[serde(default)]
    pub include: Option<String>,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub content_name: Option<String>,
    #[serde(default, rename = "match")]
    pub match_pattern: Option<String>,
    #[serde(default, deserialize_with = "deserialize_captures")]
    pub captures: HashMap<String, RawRule>,
    #[serde(default)]
    pub begin: Option<String>,
    #[serde(default, deserialize_with = "deserialize_captures")]
    pub begin_captures: HashMap<String, RawRule>,
    #[serde(default)]
    pub end: Option<String>,
    #[serde(default, deserialize_with = "deserialize_captures")]
    pub end_captures: HashMap<String, RawRule>,
    #[serde(default, rename = "while")]
    pub while_pattern: Option<String>,
    #[serde(default, deserialize_with = "deserialize_captures")]
    pub while_captures: HashMap<String, RawRule>,
    #[serde(default)]
    pub patterns: Vec<RawRule>,
    #[serde(default, deserialize_with = "deserialize_repository")]
    pub repository: HashMap<String, RawRule>,
    #[serde(default, deserialize_with = "deserialize_boolish")]
    pub apply_end_pattern_last: bool,
}

impl RawGrammar {
    pub fn from_json(name: &str, source: &str) -> Result<Self> {
        serde_json::from_str(source).map_err(|source| Error::InvalidGrammar {
            name: name.to_owned(),
            source,
        })
    }
}

pub struct StaticRawMapEntry<T: 'static> {
    pub key: &'static str,
    pub value: T,
}

impl<T> StaticRawMapEntry<T> {
    pub const fn new(key: &'static str, value: T) -> Self {
        Self { key, value }
    }
}

pub struct StaticRawGrammar {
    pub name: Option<&'static str>,
    pub scope_name: &'static str,
    pub patterns: &'static [StaticRawRule],
    pub repository: &'static [StaticRawMapEntry<StaticRawRule>],
    pub injections: &'static [StaticRawMapEntry<StaticRawRule>],
    pub injection_selector: Option<&'static str>,
}

pub struct StaticRawRule {
    pub include: Option<&'static str>,
    pub name: Option<&'static str>,
    pub content_name: Option<&'static str>,
    pub match_pattern: Option<&'static str>,
    pub captures: &'static [StaticRawMapEntry<StaticRawRule>],
    pub begin: Option<&'static str>,
    pub begin_captures: &'static [StaticRawMapEntry<StaticRawRule>],
    pub end: Option<&'static str>,
    pub end_captures: &'static [StaticRawMapEntry<StaticRawRule>],
    pub while_pattern: Option<&'static str>,
    pub while_captures: &'static [StaticRawMapEntry<StaticRawRule>],
    pub patterns: &'static [StaticRawRule],
    pub repository: &'static [StaticRawMapEntry<StaticRawRule>],
    pub apply_end_pattern_last: bool,
}

impl StaticRawRule {
    pub const EMPTY: Self = Self {
        include: None,
        name: None,
        content_name: None,
        match_pattern: None,
        captures: &[],
        begin: None,
        begin_captures: &[],
        end: None,
        end_captures: &[],
        while_pattern: None,
        while_captures: &[],
        patterns: &[],
        repository: &[],
        apply_end_pattern_last: false,
    };

    fn to_owned(&self) -> RawRule {
        RawRule {
            include: self.include.map(str::to_owned),
            name: self.name.map(str::to_owned),
            content_name: self.content_name.map(str::to_owned),
            match_pattern: self.match_pattern.map(str::to_owned),
            captures: static_map_to_owned(self.captures),
            begin: self.begin.map(str::to_owned),
            begin_captures: static_map_to_owned(self.begin_captures),
            end: self.end.map(str::to_owned),
            end_captures: static_map_to_owned(self.end_captures),
            while_pattern: self.while_pattern.map(str::to_owned),
            while_captures: static_map_to_owned(self.while_captures),
            patterns: self.patterns.iter().map(Self::to_owned).collect(),
            repository: static_map_to_owned(self.repository),
            apply_end_pattern_last: self.apply_end_pattern_last,
        }
    }
}

impl StaticRawGrammar {
    pub fn to_owned(&self) -> RawGrammar {
        RawGrammar {
            name: self.name.map(str::to_owned),
            scope_name: self.scope_name.to_owned(),
            patterns: self.patterns.iter().map(StaticRawRule::to_owned).collect(),
            repository: static_map_to_owned(self.repository),
            injections: static_map_to_owned(self.injections),
            injection_selector: self.injection_selector.map(str::to_owned),
        }
    }
}

fn static_map_to_owned(
    values: &'static [StaticRawMapEntry<StaticRawRule>],
) -> HashMap<String, RawRule> {
    values
        .iter()
        .map(|entry| (entry.key.to_owned(), entry.value.to_owned()))
        .collect()
}

pub(crate) type RuleId = u32;
pub(crate) type PatternSourceId = u32;
pub(crate) type ScopeNameId = u32;
pub(crate) type ScopeTemplateId = u32;

pub(crate) struct ScopeName {
    pub scopes: Box<[ScopeTemplateId]>,
}

pub(crate) struct ScopeTemplate {
    pub parts: Box<[ScopePart]>,
}

pub(crate) enum ScopePart {
    Literal(String),
    Capture(usize),
}

#[derive(Debug, Clone)]
pub(crate) struct Capture {
    pub index: usize,
    pub name: Option<ScopeNameId>,
    pub content_name: Option<ScopeNameId>,
    pub retokenize: Option<RuleId>,
}

#[derive(Debug, Clone)]
pub(crate) enum RuleKind {
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

#[derive(Debug, Clone)]
pub(crate) struct Rule {
    pub name: Option<ScopeNameId>,
    pub content_name: Option<ScopeNameId>,
    pub kind: RuleKind,
}

pub(crate) struct CompiledGrammar {
    pub root_scope_name: ScopeNameId,
    pub root: RuleId,
    pub rules: Vec<Rule>,
    pub patterns: Vec<String>,
    pub scope_names: Vec<ScopeName>,
    pub scope_templates: Vec<ScopeTemplate>,
    pub injection_selectors: Vec<String>,
    pub injections: Vec<Injection>,
}

#[derive(Debug, Clone)]
pub(crate) struct Injection {
    pub selector: ScopeSelector,
    pub rule: RuleId,
}

type RepoChain<'a> = Vec<&'a HashMap<String, RawRule>>;

pub(crate) fn compile(
    scope_name: &str,
    grammars: &HashMap<String, &RawGrammar>,
    external_injections: &[String],
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
    for (selector, raw) in &base.injections {
        let repo = vec![&base.repository];
        let id = compiler.compile_rule(raw, &repo, base, base)?;
        compiler.add_injection(selector, id);
    }
    for injection_scope in external_injections {
        if let Some(injection) = grammars.get(injection_scope) {
            let id = compiler.compile_root(injection, base)?;
            let selector = injection
                .injection_selector
                .clone()
                .unwrap_or_else(|| format!("L:{scope_name}"));
            compiler.add_injection(&selector, id);
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
    grammars: &'a HashMap<String, &'a RawGrammar>,
    rules: Vec<Rule>,
    roots: HashMap<(String, String), RuleId>,
    raw_cache: HashMap<(usize, String, String), RuleId>,
    injections: Vec<Injection>,
    patterns: Vec<String>,
    pattern_ids: HashMap<String, PatternSourceId>,
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

    fn push_placeholder(&mut self, name: Option<&str>, content_name: Option<&str>) -> RuleId {
        let id = self.rules.len() as RuleId;
        let name = name.map(|name| self.intern_scope_name(name));
        let content_name = content_name.map(|name| self.intern_scope_name(name));
        self.rules.push(Rule {
            name,
            content_name,
            kind: RuleKind::Placeholder,
        });
        id
    }

    fn compile_root(&mut self, grammar: &'a RawGrammar, base: &'a RawGrammar) -> Result<RuleId> {
        let key = (grammar.scope_name.clone(), base.scope_name.clone());
        if let Some(id) = self.roots.get(&key) {
            return Ok(*id);
        }
        let id = self.push_placeholder(Some(&grammar.scope_name), None);
        self.roots.insert(key, id);
        let repo = vec![&grammar.repository];
        let patterns = self.compile_patterns(&grammar.patterns, &repo, grammar, base)?;
        self.rules[id as usize].kind = RuleKind::IncludeOnly { patterns };
        Ok(id)
    }

    fn compile_patterns(
        &mut self,
        patterns: &'a [RawRule],
        repo: &RepoChain<'a>,
        grammar: &'a RawGrammar,
        base: &'a RawGrammar,
    ) -> Result<Vec<RuleId>> {
        patterns
            .iter()
            .map(|rule| self.compile_rule(rule, repo, grammar, base))
            .collect()
    }

    fn compile_rule(
        &mut self,
        raw: &'a RawRule,
        repo: &RepoChain<'a>,
        grammar: &'a RawGrammar,
        base: &'a RawGrammar,
    ) -> Result<RuleId> {
        if let Some(include) = &raw.include {
            return self.compile_include(include, repo, grammar, base);
        }

        let key = (
            raw as *const RawRule as usize,
            grammar.scope_name.clone(),
            base.scope_name.clone(),
        );
        if let Some(id) = self.raw_cache.get(&key) {
            return Ok(*id);
        }
        let id = self.push_placeholder(raw.name.as_deref(), raw.content_name.as_deref());
        self.raw_cache.insert(key, id);

        let mut nested_repo = repo.clone();
        if !raw.repository.is_empty() {
            nested_repo.insert(0, &raw.repository);
        }
        let kind = if let Some(pattern) = &raw.match_pattern {
            let pattern = self.intern_pattern(normalize_pattern(pattern));
            let captures = self.compile_captures(&raw.captures, &nested_repo, grammar, base)?;
            RuleKind::Match { pattern, captures }
        } else if let Some(begin) = &raw.begin {
            let patterns = self.compile_patterns(&raw.patterns, &nested_repo, grammar, base)?;
            let begin = self.intern_pattern(normalize_pattern(begin));
            if let Some(while_pattern) = &raw.while_pattern {
                let begin_captures = self.compile_captures(
                    captures_or(&raw.begin_captures, &raw.captures),
                    &nested_repo,
                    grammar,
                    base,
                )?;
                let while_pattern = self.intern_pattern(normalize_pattern(while_pattern));
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
                let end = self.intern_pattern(normalize_pattern(raw.end.as_deref().unwrap_or("")));
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
            let patterns = self.compile_patterns(&raw.patterns, &nested_repo, grammar, base)?;
            RuleKind::IncludeOnly { patterns }
        };
        self.rules[id as usize].kind = kind;
        Ok(id)
    }

    fn compile_captures(
        &mut self,
        captures: &'a HashMap<String, RawRule>,
        repo: &RepoChain<'a>,
        grammar: &'a RawGrammar,
        base: &'a RawGrammar,
    ) -> Result<Vec<Capture>> {
        let mut output = Vec::new();
        for (key, capture) in captures {
            let Ok(index) = key.parse::<usize>() else {
                continue;
            };
            let retokenize = if capture.patterns.is_empty() && capture.include.is_none() {
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
        grammar: &'a RawGrammar,
        base: &'a RawGrammar,
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
                        return self.compile_rule(raw, &external_repo, external, base);
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
        if let Some(id) = self.pattern_ids.get(&pattern) {
            return *id;
        }
        let id = self.patterns.len() as PatternSourceId;
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
        if bytes[index] == b'$' && index + 1 < bytes.len() && bytes[index + 1].is_ascii_digit() {
            if start < index {
                parts.push(ScopePart::Literal(scope[start..index].to_owned()));
            }
            parts.push(ScopePart::Capture((bytes[index + 1] - b'0') as usize));
            index += 2;
            start = index;
        } else {
            index += 1;
        }
    }
    if start < scope.len() {
        parts.push(ScopePart::Literal(scope[start..].to_owned()));
    }
    parts
}

fn captures_or<'a>(
    specific: &'a HashMap<String, RawRule>,
    fallback: &'a HashMap<String, RawRule>,
) -> &'a HashMap<String, RawRule> {
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
) -> std::result::Result<HashMap<String, RawRule>, D::Error>
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
        value => return Err(D::Error::custom(format!("invalid captures value: {value}"))),
    };
    let mut captures = HashMap::new();
    for (key, value) in values {
        if key.parse::<usize>().is_err() {
            continue;
        }
        let rule = match value {
            serde_json::Value::String(name) => RawRule {
                name: Some(name),
                ..RawRule::default()
            },
            value @ serde_json::Value::Object(_) => {
                serde_json::from_value(value).map_err(D::Error::custom)?
            }
            _ => continue,
        };
        captures.insert(key, rule);
    }
    Ok(captures)
}

fn deserialize_boolish<'de, D>(deserializer: D) -> std::result::Result<bool, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = serde_json::Value::deserialize(deserializer)?;
    Ok(match value {
        serde_json::Value::Bool(value) => value,
        serde_json::Value::Number(value) => value.as_i64().is_some_and(|value| value != 0),
        serde_json::Value::String(value) => value == "1" || value.eq_ignore_ascii_case("true"),
        _ => false,
    })
}

fn deserialize_repository<'de, D>(
    deserializer: D,
) -> std::result::Result<HashMap<String, RawRule>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::de::Error as _;

    let values = HashMap::<String, serde_json::Value>::deserialize(deserializer)?;
    values
        .into_iter()
        .map(|(name, value)| {
            let rule = match value {
                serde_json::Value::Array(values) => RawRule {
                    patterns: values
                        .into_iter()
                        .map(serde_json::from_value)
                        .collect::<std::result::Result<Vec<RawRule>, _>>()
                        .map_err(D::Error::custom)?,
                    ..RawRule::default()
                },
                value => serde_json::from_value(value).map_err(D::Error::custom)?,
            };
            Ok((name, rule))
        })
        .collect()
}
