use std::{collections::HashMap, fmt, sync::Arc};

use crate::{
    grammar::{
        Capture, CompiledGrammar, Injection, Rule, RuleKind, ScopeName,
        ScopePart, ScopeTemplate,
    },
    highlighter::HighlighterEngine,
    matcher::{Expression, Priority, ScopeSelector},
    theme::{ColorId, FontStyle, Style, Theme, ThemeRule},
    tokenizer::RegexLimits,
};

const MAGIC: &[u8; 8] = b"SHIKI\0\x03\0";

pub(crate) struct SnapshotParts {
    pub languages: Vec<(String, usize)>,
    pub grammars: Vec<CompiledGrammar>,
    pub themes: Vec<(String, String, Theme)>,
    pub regex_limits: RegexLimits,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct SnapshotError(&'static str);

impl fmt::Display for SnapshotError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.0)
    }
}

pub(crate) fn encode(engine: &HighlighterEngine) -> Vec<u8> {
    let mut strings = StringTable::default();
    collect_strings(engine, &mut strings);

    let mut writer = Writer::with_capacity(strings.encoded_len());
    writer.bytes(MAGIC);
    writer.len(strings.values.len());
    for value in &strings.values {
        writer.len(value.len());
        writer.bytes(value.as_bytes());
    }

    let mut languages = engine.inner.languages.iter().collect::<Vec<_>>();
    languages.sort_unstable_by(|left, right| left.0.cmp(right.0));
    writer.len(languages.len());
    for (name, index) in languages {
        writer.string(&strings, name);
        writer.index(*index);
    }

    writer.len(engine.inner.compiled.len());
    for language in &engine.inner.compiled {
        writer.grammar(&strings, &language.grammar);
    }

    writer.len(engine.inner.themes.len());
    for named in &engine.inner.themes {
        writer.string(&strings, &named.name);
        writer.string(&strings, &named.css_name);
        writer.theme(&strings, &named.theme);
    }

    writer.u64(engine.inner.regex_limits.match_retry_limit);
    writer.u64(engine.inner.regex_limits.search_retry_limit);
    lz4_flex::block::compress_prepend_size(&writer.output)
}

pub(crate) fn decode(source: &[u8]) -> Result<SnapshotParts, SnapshotError> {
    let uncompressed = lz4_flex::block::decompress_size_prepended(source)
        .map_err(|_| SnapshotError("precompiled snapshot is not valid LZ4"))?;
    let mut reader = Reader::new(&uncompressed);
    if reader.take(MAGIC.len())? != MAGIC {
        return Err(SnapshotError("unsupported precompiled snapshot format"));
    }

    let strings = reader.vec(|reader| {
        let bytes = reader.sized_bytes()?;
        let value = std::str::from_utf8(bytes)
            .map_err(|_| SnapshotError("snapshot contains invalid UTF-8"))?;
        Ok(Arc::<str>::from(value))
    })?;

    let languages = reader.vec(|reader| {
        let name = reader.string(&strings)?.as_ref().to_owned();
        let index = reader.index()?;
        Ok((name, index))
    })?;
    let grammars = reader.vec(|reader| reader.grammar(&strings))?;
    let themes = reader.vec(|reader| {
        let name = reader.string(&strings)?.as_ref().to_owned();
        let css_name = reader.string(&strings)?.as_ref().to_owned();
        let theme = reader.theme(&strings)?;
        Ok((name, css_name, theme))
    })?;
    let regex_limits = RegexLimits {
        match_retry_limit: reader.u64()?,
        search_retry_limit: reader.u64()?,
    };
    if !reader.remaining.is_empty() {
        return Err(SnapshotError("snapshot contains trailing data"));
    }
    if languages.iter().any(|(_, index)| *index >= grammars.len()) {
        return Err(SnapshotError(
            "snapshot contains an invalid language index",
        ));
    }

    Ok(SnapshotParts {
        languages,
        grammars,
        themes,
        regex_limits,
    })
}

#[derive(Default)]
struct StringTable {
    ids: HashMap<Arc<str>, u32>,
    values: Vec<Arc<str>>,
}

impl StringTable {
    fn intern(&mut self, value: &str) {
        if self.ids.contains_key(value) {
            return;
        }
        let id = checked_u32(self.values.len());
        let value: Arc<str> = Arc::from(value);
        self.values.push(value.clone());
        self.ids.insert(value, id);
    }

    fn id(&self, value: &str) -> u32 {
        *self
            .ids
            .get(value)
            .expect("all snapshot strings must be collected before encoding")
    }

    fn encoded_len(&self) -> usize {
        MAGIC.len()
            + 4
            + self
                .values
                .iter()
                .map(|value| 4 + value.len())
                .sum::<usize>()
    }
}

fn collect_strings(engine: &HighlighterEngine, strings: &mut StringTable) {
    let mut language_names = engine.inner.languages.keys().collect::<Vec<_>>();
    language_names.sort_unstable();
    for name in language_names {
        strings.intern(name);
    }
    for language in &engine.inner.compiled {
        collect_grammar_strings(&language.grammar, strings);
    }
    for named in &engine.inner.themes {
        strings.intern(&named.name);
        strings.intern(&named.css_name);
        collect_theme_strings(&named.theme, strings);
    }
}

fn collect_grammar_strings(
    grammar: &CompiledGrammar,
    strings: &mut StringTable,
) {
    for pattern in &grammar.patterns {
        strings.intern(pattern);
    }
    for template in &grammar.scope_templates {
        for part in &template.parts {
            if let ScopePart::Literal(value) = part {
                strings.intern(value);
            }
        }
    }
    for selector in &grammar.injection_selectors {
        strings.intern(selector);
    }
}

fn collect_theme_strings(theme: &Theme, strings: &mut StringTable) {
    strings.intern(&theme.name);
    strings.intern(&theme.foreground);
    strings.intern(&theme.background);
    for color in &theme.colors {
        strings.intern(color);
    }
    for selector in &theme.selectors {
        strings.intern(selector);
    }
}

struct Writer {
    output: Vec<u8>,
}

impl Writer {
    fn with_capacity(capacity: usize) -> Self {
        Self {
            output: Vec::with_capacity(capacity),
        }
    }

    fn bytes(&mut self, value: &[u8]) {
        self.output.extend_from_slice(value);
    }

    fn u8(&mut self, value: u8) {
        self.output.push(value);
    }

    fn u32(&mut self, value: u32) {
        let mut value = value;
        while value >= 0x80 {
            self.u8((value as u8) | 0x80);
            value >>= 7;
        }
        self.u8(value as u8);
    }

    fn u64(&mut self, value: u64) {
        let mut value = value;
        while value >= 0x80 {
            self.u8((value as u8) | 0x80);
            value >>= 7;
        }
        self.u8(value as u8);
    }

    fn len(&mut self, value: usize) {
        self.u32(checked_u32(value));
    }

    fn index(&mut self, value: usize) {
        self.u32(checked_u32(value));
    }

    fn string(&mut self, strings: &StringTable, value: &str) {
        self.u32(strings.id(value));
    }

    fn option_u32(&mut self, value: Option<u32>) {
        self.u32(match value {
            Some(value) => value
                .checked_add(1)
                .expect("snapshot ID exceeds 32-bit limits"),
            None => 0,
        });
    }

    fn u32_slice(&mut self, values: &[u32]) {
        self.len(values.len());
        for value in values {
            self.u32(*value);
        }
    }

    fn captures(&mut self, captures: &[Capture]) {
        self.len(captures.len());
        for capture in captures {
            self.index(capture.index);
            self.option_u32(capture.name);
            self.option_u32(capture.content_name);
            self.option_u32(capture.retokenize);
        }
    }

    fn grammar(&mut self, strings: &StringTable, grammar: &CompiledGrammar) {
        self.u32(grammar.root_scope_name);
        self.u32(grammar.root);
        self.len(grammar.rules.len());
        for rule in &grammar.rules {
            self.option_u32(rule.name);
            self.option_u32(rule.content_name);
            match &rule.kind {
                RuleKind::Match { pattern, captures } => {
                    self.u8(0);
                    self.u32(*pattern);
                    self.captures(captures);
                }
                RuleKind::IncludeOnly { patterns } => {
                    self.u8(1);
                    self.u32_slice(patterns);
                }
                RuleKind::BeginEnd {
                    begin,
                    begin_captures,
                    end,
                    end_captures,
                    patterns,
                    apply_end_last,
                } => {
                    self.u8(2);
                    self.u32(*begin);
                    self.captures(begin_captures);
                    self.u32(*end);
                    self.captures(end_captures);
                    self.u32_slice(patterns);
                    self.u8(u8::from(*apply_end_last));
                }
                RuleKind::BeginWhile {
                    begin,
                    begin_captures,
                    while_pattern,
                    while_captures,
                    patterns,
                } => {
                    self.u8(3);
                    self.u32(*begin);
                    self.captures(begin_captures);
                    self.u32(*while_pattern);
                    self.captures(while_captures);
                    self.u32_slice(patterns);
                }
                RuleKind::Placeholder => self.u8(4),
            }
        }

        self.len(grammar.patterns.len());
        for pattern in &grammar.patterns {
            self.string(strings, pattern);
        }
        self.len(grammar.scope_names.len());
        for scope in &grammar.scope_names {
            self.u32_slice(&scope.scopes);
        }
        self.len(grammar.scope_templates.len());
        for template in &grammar.scope_templates {
            self.len(template.parts.len());
            for part in &template.parts {
                match part {
                    ScopePart::Literal(value) => {
                        self.u8(0);
                        self.string(strings, value);
                    }
                    ScopePart::Capture(index) => {
                        self.u8(1);
                        self.index(*index);
                    }
                }
            }
        }
        self.len(grammar.injection_selectors.len());
        for selector in &grammar.injection_selectors {
            self.string(strings, selector);
        }
        self.len(grammar.injections.len());
        for injection in &grammar.injections {
            self.scope_selector(&injection.selector);
            self.u32(injection.rule);
        }
    }

    fn scope_selector(&mut self, selector: &ScopeSelector) {
        self.u8(match selector.priority {
            Priority::Left => 0,
            Priority::Normal => 1,
            Priority::Right => 2,
        });
        self.expression(&selector.expression);
    }

    fn expression(&mut self, expression: &Expression) {
        match expression {
            Expression::Path(path) => {
                self.u8(0);
                self.u32_slice(path);
            }
            Expression::And(expressions) => {
                self.u8(1);
                self.len(expressions.len());
                for expression in expressions {
                    self.expression(expression);
                }
            }
            Expression::Or(expressions) => {
                self.u8(2);
                self.len(expressions.len());
                for expression in expressions {
                    self.expression(expression);
                }
            }
            Expression::Not(expression) => {
                self.u8(3);
                self.expression(expression);
            }
        }
    }

    fn theme(&mut self, strings: &StringTable, theme: &Theme) {
        self.string(strings, &theme.name);
        self.string(strings, &theme.foreground);
        self.string(strings, &theme.background);
        self.len(theme.colors.len());
        for color in &theme.colors {
            self.string(strings, color);
        }
        self.u32(theme.foreground_id.0);
        self.len(theme.selectors.len());
        for selector in &theme.selectors {
            self.string(strings, selector);
        }
        self.len(theme.rules.len());
        for rule in &theme.rules {
            self.u32(rule.target);
            self.u32_slice(&rule.parents);
            self.index(rule.target_depth);
            self.style(rule.style);
            self.index(rule.order);
        }
    }

    fn style(&mut self, style: Style) {
        let mut flags = 0;
        if style.foreground.is_some() {
            flags |= 1;
        }
        if style.background.is_some() {
            flags |= 2;
        }
        if style.font_style.is_some() {
            flags |= 4;
        }
        self.u8(flags);
        if let Some(color) = style.foreground {
            self.u32(color.0);
        }
        if let Some(color) = style.background {
            self.u32(color.0);
        }
        if let Some(font_style) = style.font_style {
            self.u8(font_style.bits());
        }
    }
}

struct Reader<'a> {
    remaining: &'a [u8],
}

impl<'a> Reader<'a> {
    fn new(source: &'a [u8]) -> Self {
        Self { remaining: source }
    }

    fn take(&mut self, len: usize) -> Result<&'a [u8], SnapshotError> {
        if len > self.remaining.len() {
            return Err(SnapshotError("precompiled snapshot is truncated"));
        }
        let (value, remaining) = self.remaining.split_at(len);
        self.remaining = remaining;
        Ok(value)
    }

    fn u8(&mut self) -> Result<u8, SnapshotError> {
        Ok(self.take(1)?[0])
    }

    fn u32(&mut self) -> Result<u32, SnapshotError> {
        let mut value = 0_u32;
        for shift in (0..35).step_by(7) {
            let byte = self.u8()?;
            if shift == 28 && byte > 0x0f {
                return Err(SnapshotError(
                    "snapshot contains an invalid integer",
                ));
            }
            value |= u32::from(byte & 0x7f) << shift;
            if byte & 0x80 == 0 {
                return Ok(value);
            }
        }
        Err(SnapshotError("snapshot contains an invalid integer"))
    }

    fn u64(&mut self) -> Result<u64, SnapshotError> {
        let mut value = 0_u64;
        for shift in (0..70).step_by(7) {
            let byte = self.u8()?;
            if shift == 63 && byte > 1 {
                return Err(SnapshotError(
                    "snapshot contains an invalid integer",
                ));
            }
            value |= u64::from(byte & 0x7f) << shift;
            if byte & 0x80 == 0 {
                return Ok(value);
            }
        }
        Err(SnapshotError("snapshot contains an invalid integer"))
    }

    fn index(&mut self) -> Result<usize, SnapshotError> {
        Ok(self.u32()? as usize)
    }

    fn sized_bytes(&mut self) -> Result<&'a [u8], SnapshotError> {
        let len = self.index()?;
        self.take(len)
    }

    fn vec<T>(
        &mut self,
        mut read: impl FnMut(&mut Self) -> Result<T, SnapshotError>,
    ) -> Result<Vec<T>, SnapshotError> {
        let len = self.index()?;
        let mut values = Vec::new();
        values
            .try_reserve_exact(len)
            .map_err(|_| SnapshotError("snapshot collection is too large"))?;
        for _ in 0..len {
            values.push(read(self)?);
        }
        Ok(values)
    }

    fn string(
        &mut self,
        strings: &[Arc<str>],
    ) -> Result<Arc<str>, SnapshotError> {
        strings
            .get(self.index()?)
            .cloned()
            .ok_or(SnapshotError("snapshot contains an invalid string ID"))
    }

    fn option_u32(&mut self) -> Result<Option<u32>, SnapshotError> {
        let value = self.u32()?;
        Ok((value != 0).then_some(value.saturating_sub(1)))
    }

    fn u32_vec(&mut self) -> Result<Vec<u32>, SnapshotError> {
        self.vec(Self::u32)
    }

    fn captures(&mut self) -> Result<Vec<Capture>, SnapshotError> {
        self.vec(|reader| {
            Ok(Capture {
                index: reader.index()?,
                name: reader.option_u32()?,
                content_name: reader.option_u32()?,
                retokenize: reader.option_u32()?,
            })
        })
    }

    fn grammar(
        &mut self,
        strings: &[Arc<str>],
    ) -> Result<CompiledGrammar, SnapshotError> {
        let root_scope_name = self.u32()?;
        let root = self.u32()?;
        let rules = self.vec(|reader| {
            let name = reader.option_u32()?;
            let content_name = reader.option_u32()?;
            let kind = match reader.u8()? {
                0 => RuleKind::Match {
                    pattern: reader.u32()?,
                    captures: reader.captures()?,
                },
                1 => RuleKind::IncludeOnly {
                    patterns: reader.u32_vec()?,
                },
                2 => RuleKind::BeginEnd {
                    begin: reader.u32()?,
                    begin_captures: reader.captures()?,
                    end: reader.u32()?,
                    end_captures: reader.captures()?,
                    patterns: reader.u32_vec()?,
                    apply_end_last: match reader.u8()? {
                        0 => false,
                        1 => true,
                        _ => {
                            return Err(SnapshotError(
                                "snapshot contains an invalid boolean",
                            ));
                        }
                    },
                },
                3 => RuleKind::BeginWhile {
                    begin: reader.u32()?,
                    begin_captures: reader.captures()?,
                    while_pattern: reader.u32()?,
                    while_captures: reader.captures()?,
                    patterns: reader.u32_vec()?,
                },
                4 => RuleKind::Placeholder,
                _ => {
                    return Err(SnapshotError(
                        "snapshot contains an invalid rule tag",
                    ));
                }
            };
            Ok(Rule {
                name,
                content_name,
                kind,
            })
        })?;
        let patterns = self.vec(|reader| reader.string(strings))?;
        let scope_names = self.vec(|reader| {
            Ok(ScopeName {
                scopes: reader.u32_vec()?.into_boxed_slice(),
            })
        })?;
        let scope_templates = self.vec(|reader| {
            Ok(ScopeTemplate {
                parts: reader
                    .vec(|reader| match reader.u8()? {
                        0 => Ok(ScopePart::Literal(reader.string(strings)?)),
                        1 => Ok(ScopePart::Capture(reader.index()?)),
                        _ => Err(SnapshotError(
                            "snapshot contains an invalid scope part tag",
                        )),
                    })?
                    .into_boxed_slice(),
            })
        })?;
        let injection_selectors = self.vec(|reader| reader.string(strings))?;
        let injections = self.vec(|reader| {
            Ok(Injection {
                selector: reader.scope_selector()?,
                rule: reader.u32()?,
            })
        })?;
        Ok(CompiledGrammar {
            root_scope_name,
            root,
            rules,
            patterns,
            scope_names,
            scope_templates,
            injection_selectors,
            injections,
        })
    }

    fn scope_selector(&mut self) -> Result<ScopeSelector, SnapshotError> {
        let priority = match self.u8()? {
            0 => Priority::Left,
            1 => Priority::Normal,
            2 => Priority::Right,
            _ => {
                return Err(SnapshotError(
                    "snapshot contains an invalid selector priority",
                ));
            }
        };
        Ok(ScopeSelector {
            priority,
            expression: self.expression()?,
        })
    }

    fn expression(&mut self) -> Result<Expression, SnapshotError> {
        match self.u8()? {
            0 => Ok(Expression::Path(self.u32_vec()?)),
            1 => Ok(Expression::And(self.vec(Self::expression)?)),
            2 => Ok(Expression::Or(self.vec(Self::expression)?)),
            3 => Ok(Expression::Not(Box::new(self.expression()?))),
            _ => Err(SnapshotError(
                "snapshot contains an invalid selector expression tag",
            )),
        }
    }

    fn theme(&mut self, strings: &[Arc<str>]) -> Result<Theme, SnapshotError> {
        let name = self.string(strings)?;
        let foreground = self.string(strings)?;
        let background = self.string(strings)?;
        let colors = self.vec(|reader| reader.string(strings))?;
        let foreground_id = ColorId(self.u32()?);
        let selectors = self.vec(|reader| reader.string(strings))?;
        let rules = self.vec(|reader| {
            Ok(ThemeRule {
                target: reader.u32()?,
                parents: reader.u32_vec()?,
                target_depth: reader.index()?,
                style: reader.style()?,
                order: reader.index()?,
            })
        })?;
        if foreground_id.0 as usize >= colors.len() {
            return Err(SnapshotError(
                "snapshot contains an invalid foreground color ID",
            ));
        }
        Ok(Theme {
            name,
            foreground,
            background,
            colors,
            foreground_id,
            selectors,
            rules,
        })
    }

    fn style(&mut self) -> Result<Style, SnapshotError> {
        let flags = self.u8()?;
        if flags & !0b111 != 0 {
            return Err(SnapshotError("snapshot contains invalid style flags"));
        }
        Ok(Style {
            foreground: if flags & 1 != 0 {
                Some(ColorId(self.u32()?))
            } else {
                None
            },
            background: if flags & 2 != 0 {
                Some(ColorId(self.u32()?))
            } else {
                None
            },
            font_style: if flags & 4 != 0 {
                Some(FontStyle::from_bits(self.u8()?))
            } else {
                None
            },
        })
    }
}

fn checked_u32(value: usize) -> u32 {
    u32::try_from(value).expect("precompiled snapshot exceeds 32-bit limits")
}
