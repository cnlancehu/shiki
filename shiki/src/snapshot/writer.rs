use super::{StringTable, checked_u32};
use crate::{
    grammar::{Capture, CompiledGrammar, RuleKind, ScopePart},
    matcher::{Expression, Priority, ScopeSelector},
    theme::{Style, Theme},
};

pub(super) struct Writer {
    pub(super) output: Vec<u8>,
}

impl Writer {
    pub(super) fn with_capacity(capacity: usize) -> Self {
        Self {
            output: Vec::with_capacity(capacity),
        }
    }

    pub(super) fn bytes(&mut self, value: &[u8]) {
        self.output.extend_from_slice(value);
    }

    pub(super) fn sized_bytes(&mut self, value: &[u8]) {
        self.len(value.len());
        self.bytes(value);
    }

    pub(super) fn text(&mut self, value: &str) {
        self.sized_bytes(value.as_bytes());
    }

    pub(super) fn snapshot_strings(&mut self, strings: &StringTable) {
        self.len(strings.values.len());
        for value in &strings.values {
            self.len(value.len());
        }
        for value in &strings.values {
            self.bytes(value.as_bytes());
        }
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

    pub(super) fn u64(&mut self, value: u64) {
        let mut value = value;
        while value >= 0x80 {
            self.u8((value as u8) | 0x80);
            value >>= 7;
        }
        self.u8(value as u8);
    }

    pub(super) fn len(&mut self, value: usize) {
        self.u32(checked_u32(value));
    }

    pub(super) fn index(&mut self, value: usize) {
        self.u32(checked_u32(value));
    }

    pub(super) fn string(&mut self, strings: &StringTable, value: &str) {
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

    pub(super) fn grammar(
        &mut self,
        strings: &StringTable,
        grammar: &CompiledGrammar,
    ) {
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

    pub(super) fn theme(&mut self, strings: &StringTable, theme: &Theme) {
        self.string(strings, &theme.name);
        self.string(strings, &theme.foreground);
        self.string(strings, &theme.background);
        self.len(theme.colors.len());
        for color in &theme.colors {
            self.string(strings, color);
        }
        self.u32(theme.foreground_id.0);
        self.u32_slice(&theme.ansi_colors.map(|color| color.0));
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
