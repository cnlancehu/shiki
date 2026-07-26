use std::{borrow::Cow, collections::HashMap, ops::Range, sync::Arc};

use super::{
    PatternId, ScopeStackId, ScopeToken,
    scanner::CompiledRegex,
    scope::{ScopeArena, push_scope_name, push_scope_names},
};
use crate::grammar::{Capture, RuleId, ScopeName, ScopeNameId, ScopeTemplate};

pub(super) struct Retokenize {
    pub(super) range: Range<usize>,
    pub(super) rule: RuleId,
    pub(super) scopes: ScopeStackId,
}

pub(super) fn clamp_captures(
    captures: &mut [Option<Range<usize>>],
    limit: usize,
) {
    for range in captures.iter_mut().flatten() {
        range.start = range.start.min(limit);
        range.end = range.end.min(limit);
    }
}

pub(super) fn intern_pattern(
    pattern_ids: &mut HashMap<Arc<str>, PatternId>,
    patterns: &mut Vec<Arc<str>>,
    regexes: &mut Vec<Option<Arc<CompiledRegex>>>,
    pattern: &str,
) -> PatternId {
    debug_assert_eq!(patterns.len(), regexes.len());
    if let Some(id) = pattern_ids.get(pattern) {
        return *id;
    }
    let id = patterns.len() as PatternId;
    let pattern: Arc<str> = Arc::from(pattern);
    patterns.push(pattern.clone());
    regexes.push(None);
    pattern_ids.insert(pattern, id);
    id
}

pub(super) fn emit(
    tokens: &mut Vec<ScopeToken>,
    start: usize,
    end: usize,
    scopes: ScopeStackId,
) {
    if start >= end {
        return;
    }
    if let Some(last) = tokens.last_mut()
        && last.range.end == start
        && last.scopes == scopes
    {
        last.range.end = end;
        return;
    }
    tokens.push(ScopeToken {
        range: start..end,
        scopes,
    });
}

#[allow(clippy::too_many_arguments)]
pub(super) fn emit_captures(
    arena: &mut ScopeArena,
    scope_names: &[ScopeName],
    scope_templates: &[ScopeTemplate],
    tokens: &mut Vec<ScopeToken>,
    matches: &[Option<Range<usize>>],
    parent_scopes: ScopeStackId,
    name: Option<ScopeNameId>,
    captures: &[Capture],
    limit: usize,
    text: &str,
) -> Vec<Retokenize> {
    let Some(full) = matches.first().and_then(Clone::clone) else {
        return Vec::new();
    };
    let base = push_scope_name(
        arena,
        scope_names,
        scope_templates,
        parent_scopes,
        name,
        matches,
        text,
    );
    if captures.is_empty() {
        emit(tokens, full.start.min(limit), full.end.min(limit), base);
        return Vec::new();
    }
    let mut boundaries = vec![full.start, full.end];
    for capture in captures {
        if let Some(range) = matches
            .get(capture.index)
            .and_then(Clone::clone)
            .and_then(|range| intersect_ranges(range, &full))
        {
            boundaries.push(range.start);
            boundaries.push(range.end);
        }
    }
    boundaries.sort_unstable();
    boundaries.dedup();
    for pair in boundaries.windows(2) {
        let start = pair[0];
        let end = pair[1];
        let mut scopes = base;
        for capture in captures {
            if let Some(range) =
                matches.get(capture.index).and_then(Clone::clone)
                && range.start <= start
                && range.end >= end
            {
                push_scope_names(
                    arena,
                    scope_names,
                    scope_templates,
                    &mut scopes,
                    capture.name,
                    matches,
                    text,
                );
                push_scope_names(
                    arena,
                    scope_names,
                    scope_templates,
                    &mut scopes,
                    capture.content_name,
                    matches,
                    text,
                );
            }
        }
        emit(tokens, start.min(limit), end.min(limit), scopes);
    }
    captures
        .iter()
        .filter_map(|capture| {
            let rule = capture.retokenize?;
            let range = matches
                .get(capture.index)
                .and_then(Clone::clone)
                .and_then(|range| intersect_ranges(range, &full))?;
            let mut scopes = base;
            push_scope_names(
                arena,
                scope_names,
                scope_templates,
                &mut scopes,
                capture.name,
                matches,
                text,
            );
            push_scope_names(
                arena,
                scope_names,
                scope_templates,
                &mut scopes,
                capture.content_name,
                matches,
                text,
            );
            Some(Retokenize {
                range: range.start.min(limit)..range.end.min(limit),
                rule,
                scopes,
            })
        })
        .collect()
}

fn intersect_ranges(
    range: Range<usize>,
    bounds: &Range<usize>,
) -> Option<Range<usize>> {
    let range = range.start.max(bounds.start)..range.end.min(bounds.end);
    (!range.is_empty()).then_some(range)
}

#[cfg(debug_assertions)]
pub(super) fn assert_token_partition(tokens: &[ScopeToken], line_len: usize) {
    let mut position = 0;
    for token in tokens {
        debug_assert_eq!(
            token.range.start, position,
            "overlapping token ranges"
        );
        debug_assert!(token.range.end > token.range.start, "empty token range");
        debug_assert!(token.range.end <= line_len, "token exceeds source line");
        position = token.range.end;
    }
    debug_assert_eq!(position, line_len, "tokens do not cover source line");
}

pub(super) fn replace_range(
    tokens: &mut Vec<ScopeToken>,
    range: Range<usize>,
    replacement: Vec<ScopeToken>,
) {
    let mut output = Vec::with_capacity(tokens.len() + replacement.len());
    let mut inserted = false;
    for token in tokens.drain(..) {
        if token.range.end <= range.start || token.range.start >= range.end {
            if !inserted && token.range.start >= range.end {
                output.extend(replacement.iter().cloned());
                inserted = true;
            }
            output.push(token);
            continue;
        }
        if token.range.start < range.start {
            output.push(ScopeToken {
                range: token.range.start..range.start,
                scopes: token.scopes,
            });
        }
        if !inserted {
            output.extend(replacement.iter().cloned());
            inserted = true;
        }
        if token.range.end > range.end {
            output.push(ScopeToken {
                range: range.end..token.range.end,
                scopes: token.scopes,
            });
        }
    }
    if !inserted {
        output.extend(replacement);
    }
    *tokens = output;
}

pub(super) fn resolve_backrefs<'a>(
    pattern: &'a str,
    captures: &[Option<Range<usize>>],
    text: &str,
) -> Cow<'a, str> {
    let mut output = String::with_capacity(pattern.len());
    let bytes = pattern.as_bytes();
    let mut index = 0;
    let mut last = 0;
    let mut replaced = false;
    while index < bytes.len() {
        if bytes[index] != b'\\' {
            index += 1;
            continue;
        }
        let slash_start = index;
        while bytes.get(index) == Some(&b'\\') {
            index += 1;
        }
        if bytes.get(index).is_some_and(u8::is_ascii_digit)
            && (index - slash_start) % 2 == 1
        {
            output.push_str(&pattern[last..index - 1]);
            let capture = (bytes[index] - b'0') as usize;
            if let Some(range) = captures.get(capture).and_then(Clone::clone) {
                output.push_str(&regex_escape(&text[range]));
            }
            index += 1;
            last = index;
            replaced = true;
        } else if index < bytes.len() {
            index += 1;
        }
    }
    if !replaced {
        return Cow::Borrowed(pattern);
    }
    output.push_str(&pattern[last..]);
    Cow::Owned(output)
}

fn regex_escape(value: &str) -> String {
    let mut output = String::with_capacity(value.len());
    for ch in value.chars() {
        if r"\.^$|?*+()[]{}".contains(ch) {
            output.push('\\');
        }
        output.push(ch);
    }
    output
}

#[cfg(test)]
mod tests {
    use std::borrow::Cow;

    use super::resolve_backrefs;

    #[test]
    fn resolves_only_unescaped_backrefs_without_corrupting_utf8() {
        let text = "终.";
        let captures = [Some(0..text.len()), Some(0.."终".len())];
        assert_eq!(
            resolve_backrefs(r"前\1后\\1", &captures, text),
            r"前终后\\1"
        );
        assert_eq!(resolve_backrefs(r"\1", &[None, Some(3..4)], "abc."), r"\.");
        assert!(matches!(
            resolve_backrefs(r"前\\1后", &captures, text),
            Cow::Borrowed(_)
        ));
    }
}
