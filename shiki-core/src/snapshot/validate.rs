use std::fmt;

use crate::{
    grammar::{Capture, CompiledGrammar, RuleKind},
    matcher::Expression,
    theme::Theme,
};

// These are per compressed block, not limits for the complete snapshot.  The
// largest bundled grammar/theme blocks are far below these ceilings, while a
// corrupt four-byte LZ4 size prefix can otherwise request almost 4 GiB.
pub(super) const MAX_GRAMMAR_BLOCK_BYTES: usize = 64 * 1024 * 1024;
pub(super) const MAX_THEME_BLOCK_BYTES: usize = 64 * 1024 * 1024;
// Every collection element consumes at least one encoded byte.  Keep a second
// decoded-item budget because a small encoded element can expand to a much
// larger Rust value.
pub(super) const MAX_COLLECTION_ITEMS: usize = 1_000_000;
pub(super) const MAX_SELECTOR_EXPRESSION_DEPTH: usize = 128;

#[derive(Debug, Clone, Copy)]
pub(crate) struct SnapshotError(pub(super) &'static str);

impl fmt::Display for SnapshotError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.0)
    }
}

pub(super) fn decompress_block(
    source: &[u8],
    max_len: usize,
    invalid_message: &'static str,
    too_large_message: &'static str,
) -> Result<Vec<u8>, SnapshotError> {
    let size = source
        .get(..4)
        .and_then(|bytes| <[u8; 4]>::try_from(bytes).ok())
        .map(u32::from_le_bytes)
        .ok_or(SnapshotError(invalid_message))? as usize;
    if size > max_len {
        return Err(SnapshotError(too_large_message));
    }
    lz4_flex::block::decompress_size_prepended(source)
        .map_err(|_| SnapshotError(invalid_message))
}

pub(super) fn validate_grammar(
    grammar: &CompiledGrammar,
) -> Result<(), SnapshotError> {
    let rule_count = grammar.rules.len();
    let pattern_count = grammar.patterns.len();
    let scope_name_count = grammar.scope_names.len();
    let scope_template_count = grammar.scope_templates.len();
    let selector_count = grammar.injection_selectors.len();

    if !valid_id(grammar.root, rule_count) {
        return Err(SnapshotError(
            "grammar snapshot contains an invalid root rule ID",
        ));
    }
    if !valid_id(grammar.root_scope_name, scope_name_count) {
        return Err(SnapshotError(
            "grammar snapshot contains an invalid root scope ID",
        ));
    }
    if !matches!(
        grammar.rules[grammar.root as usize].kind,
        RuleKind::IncludeOnly { .. }
    ) {
        return Err(SnapshotError(
            "grammar snapshot has an invalid root rule kind",
        ));
    }

    for rule in &grammar.rules {
        if !valid_optional_id(rule.name, scope_name_count)
            || !valid_optional_id(rule.content_name, scope_name_count)
        {
            return Err(SnapshotError(
                "grammar snapshot contains an invalid scope ID",
            ));
        }
        match &rule.kind {
            RuleKind::Match { pattern, captures } => {
                validate_pattern_id(*pattern, pattern_count)?;
                validate_captures(captures, rule_count, scope_name_count)?;
            }
            RuleKind::IncludeOnly { patterns } => {
                validate_rule_ids(patterns, rule_count)?;
            }
            RuleKind::BeginEnd {
                begin,
                begin_captures,
                end,
                end_captures,
                patterns,
                ..
            } => {
                validate_pattern_id(*begin, pattern_count)?;
                validate_captures(
                    begin_captures,
                    rule_count,
                    scope_name_count,
                )?;
                validate_pattern_id(*end, pattern_count)?;
                validate_captures(end_captures, rule_count, scope_name_count)?;
                validate_rule_ids(patterns, rule_count)?;
            }
            RuleKind::BeginWhile {
                begin,
                begin_captures,
                while_pattern,
                while_captures,
                patterns,
            } => {
                validate_pattern_id(*begin, pattern_count)?;
                validate_captures(
                    begin_captures,
                    rule_count,
                    scope_name_count,
                )?;
                validate_pattern_id(*while_pattern, pattern_count)?;
                validate_captures(
                    while_captures,
                    rule_count,
                    scope_name_count,
                )?;
                validate_rule_ids(patterns, rule_count)?;
            }
            RuleKind::Placeholder => {}
        }
    }

    if grammar.scope_names.iter().any(|scope| {
        scope
            .scopes
            .iter()
            .any(|id| !valid_id(*id, scope_template_count))
    }) {
        return Err(SnapshotError(
            "grammar snapshot contains an invalid scope template ID",
        ));
    }
    for injection in &grammar.injections {
        if !valid_id(injection.rule, rule_count) {
            return Err(SnapshotError(
                "grammar snapshot contains an invalid injection rule ID",
            ));
        }
        if !validate_expression_ids(
            &injection.selector.expression,
            selector_count,
        ) {
            return Err(SnapshotError(
                "grammar snapshot contains an invalid selector ID",
            ));
        }
    }
    Ok(())
}

fn validate_pattern_id(
    id: u32,
    pattern_count: usize,
) -> Result<(), SnapshotError> {
    valid_id(id, pattern_count)
        .then_some(())
        .ok_or(SnapshotError(
            "grammar snapshot contains an invalid regex pattern ID",
        ))
}

fn validate_rule_ids(
    ids: &[u32],
    rule_count: usize,
) -> Result<(), SnapshotError> {
    if ids.iter().all(|id| valid_id(*id, rule_count)) {
        Ok(())
    } else {
        Err(SnapshotError(
            "grammar snapshot contains an invalid rule ID",
        ))
    }
}

fn validate_captures(
    captures: &[Capture],
    rule_count: usize,
    scope_name_count: usize,
) -> Result<(), SnapshotError> {
    if captures.iter().all(|capture| {
        valid_optional_id(capture.name, scope_name_count)
            && valid_optional_id(capture.content_name, scope_name_count)
            && valid_optional_id(capture.retokenize, rule_count)
    }) {
        Ok(())
    } else {
        Err(SnapshotError(
            "grammar snapshot contains an invalid capture ID",
        ))
    }
}

fn validate_expression_ids(
    expression: &Expression,
    selector_count: usize,
) -> bool {
    match expression {
        Expression::Path(path) => {
            path.iter().all(|id| valid_id(*id, selector_count))
        }
        Expression::And(expressions) | Expression::Or(expressions) => {
            expressions.iter().all(|expression| {
                validate_expression_ids(expression, selector_count)
            })
        }
        Expression::Not(expression) => {
            validate_expression_ids(expression, selector_count)
        }
    }
}

pub(super) fn validate_theme(theme: &Theme) -> Result<(), SnapshotError> {
    let color_count = theme.colors.len();
    let selector_count = theme.selectors.len();
    if !valid_id(theme.foreground_id.0, color_count)
        || theme
            .ansi_colors
            .iter()
            .any(|color| !valid_id(color.0, color_count))
    {
        return Err(SnapshotError(
            "snapshot contains an invalid theme color ID",
        ));
    }
    for rule in &theme.rules {
        if !valid_id(rule.target, selector_count)
            || rule
                .parents
                .iter()
                .any(|selector| !valid_id(*selector, selector_count))
        {
            return Err(SnapshotError(
                "snapshot contains an invalid theme selector ID",
            ));
        }
        if rule
            .style
            .foreground
            .is_some_and(|color| !valid_id(color.0, color_count))
            || rule
                .style
                .background
                .is_some_and(|color| !valid_id(color.0, color_count))
        {
            return Err(SnapshotError(
                "snapshot contains an invalid theme style color ID",
            ));
        }
    }
    Ok(())
}

fn valid_optional_id(id: Option<u32>, len: usize) -> bool {
    id.is_none_or(|id| valid_id(id, len))
}

fn valid_id(id: u32, len: usize) -> bool {
    usize::try_from(id).is_ok_and(|id| id < len)
}
