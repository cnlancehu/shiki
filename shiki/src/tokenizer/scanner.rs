mod regex;

use std::{collections::HashSet, ops::Range, ptr, sync::Arc};

pub(crate) use regex::RegexPool;
use regex::onig_error;
pub(super) use regex::{CompiledRegex, StaticRegexSlots};

use super::{
    Frame, InjectionSetId, PatternId, RegexLimits, ScannerId, ScopeId,
    ScopeStackId, Tokenizer, scope::scope_chunks,
};
use crate::{
    error::{Error, Result},
    grammar::{CompiledGrammar, PatternSourceId, RuleId, RuleKind},
    matcher::{Priority, scope_matches as selector_matches},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(super) enum Action {
    Rule(RuleId),
    End,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(super) enum ScannerPatternRef {
    Grammar(PatternSourceId),
    Dynamic(PatternId),
    Empty,
}

type ScannerPattern = (Action, ScannerPatternRef);

pub(super) struct Scanner {
    actions: Vec<Action>,
    leading_literals: Vec<Box<[u8]>>,
    set: *mut onig_sys::OnigRegSet,
    match_params: Vec<*mut onig_sys::OnigMatchParam>,
    _regexes: Vec<Arc<CompiledRegex>>,
}

impl Drop for Scanner {
    fn drop(&mut self) {
        for param in self.match_params.drain(..) {
            unsafe { onig_sys::onig_free_match_param(param) };
        }
        free_scanner_set(self.set);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(super) enum ScannerKey {
    State {
        rule: RuleId,
        end_pattern: Option<ScannerPatternRef>,
        injections: InjectionSetId,
    },
    While {
        rule: RuleId,
        pattern: ScannerPatternRef,
    },
}

impl Tokenizer {
    pub(super) fn candidates(
        &mut self,
        frame: &mut Frame,
        _is_first_line: bool,
        _position: usize,
    ) -> Result<ScannerId> {
        if frame.scanner != super::NO_SCANNER {
            return Ok(frame.scanner);
        }
        let injection_set = self.injection_set(frame.content_scopes);
        let key = ScannerKey::State {
            rule: frame.rule,
            end_pattern: frame.end_pattern,
            injections: injection_set,
        };
        if let Some(scanner) = self.scanner_ids.get(&key).copied() {
            frame.scanner = scanner;
            return Ok(scanner);
        }

        let injections = self.injection_sets[injection_set as usize].clone();
        let mut raw = Vec::new();
        let rule = &self.grammar.rules[frame.rule as usize];
        match &rule.kind {
            RuleKind::IncludeOnly { patterns } => {
                collect_patterns(
                    &self.grammar,
                    patterns,
                    &mut HashSet::new(),
                    &mut raw,
                );
            }
            RuleKind::BeginEnd {
                patterns,
                apply_end_last,
                ..
            } => {
                if !apply_end_last {
                    raw.push((
                        Action::End,
                        frame.end_pattern.unwrap_or(ScannerPatternRef::Empty),
                    ));
                }
                collect_patterns(
                    &self.grammar,
                    patterns,
                    &mut HashSet::new(),
                    &mut raw,
                );
                if *apply_end_last {
                    raw.push((
                        Action::End,
                        frame.end_pattern.unwrap_or(ScannerPatternRef::Empty),
                    ));
                }
            }
            RuleKind::BeginWhile { patterns, .. } => {
                collect_patterns(
                    &self.grammar,
                    patterns,
                    &mut HashSet::new(),
                    &mut raw,
                );
            }
            _ => {}
        }
        let mut left_injections = Vec::new();
        let mut other_injections = Vec::new();
        for &(is_left, rule) in injections.iter() {
            let target = if is_left {
                &mut left_injections
            } else {
                &mut other_injections
            };
            collect_patterns(
                &self.grammar,
                &[rule],
                &mut HashSet::new(),
                target,
            );
        }
        left_injections.extend(raw);
        left_injections.extend(other_injections);
        raw = left_injections;
        let mut seen_patterns = HashSet::with_capacity(raw.len());
        raw.retain(|(_, pattern)| seen_patterns.insert(*pattern));
        let scanner = self.compile_scanner(key, &raw)?;
        frame.scanner = scanner;
        Ok(scanner)
    }

    fn injection_set(&mut self, scopes: ScopeStackId) -> InjectionSetId {
        let index = scopes as usize;
        if let Some(id) = self.scopes.nodes[index].injections {
            return id;
        }
        for scope in
            self.injection_scope_matches.len()..self.scopes.values.len()
        {
            let chunks =
                scope_chunks(&self.grammar, &self.scopes, scope as ScopeId);
            self.injection_scope_matches.push(
                self.grammar
                    .injection_selectors
                    .iter()
                    .map(|selector| selector_matches(&chunks, selector))
                    .collect(),
            );
        }
        let path = self.scopes.path(scopes);
        let injections: Vec<_> = self
            .grammar
            .injections
            .iter()
            .filter(|injection| {
                injection
                    .selector
                    .matches(&path, &self.injection_scope_matches)
            })
            .map(|injection| {
                (
                    injection.selector.priority == Priority::Left,
                    injection.rule,
                )
            })
            .collect();
        let id = if let Some(id) =
            self.injection_set_ids.get(injections.as_slice())
        {
            *id
        } else {
            let id = self.injection_sets.len() as InjectionSetId;
            let injections: Arc<[(bool, RuleId)]> = Arc::from(injections);
            self.injection_sets.push(injections.clone());
            self.injection_set_ids.insert(injections, id);
            id
        };
        self.scopes.nodes[index].injections = Some(id);
        id
    }

    pub(super) fn compile_scanner(
        &mut self,
        key: ScannerKey,
        patterns: &[ScannerPattern],
    ) -> Result<ScannerId> {
        if let Some(scanner) = self.scanner_ids.get(&key) {
            return Ok(*scanner);
        }
        if patterns.is_empty() {
            let id = self.scanners.len() as ScannerId;
            self.scanners.push(Scanner {
                actions: Vec::new(),
                leading_literals: Vec::new(),
                set: ptr::null_mut(),
                match_params: Vec::new(),
                _regexes: Vec::new(),
            });
            self.scanner_ids.insert(key, id);
            return Ok(id);
        }
        let mut regexes = Vec::with_capacity(patterns.len());
        let mut leading_literals = Vec::new();
        let mut collect_leading_literals = true;
        for (_, pattern_ref) in patterns {
            let (pattern, grammar_pattern) = match pattern_ref {
                ScannerPatternRef::Grammar(id) => {
                    let pattern =
                        self.grammar.patterns.get(*id as usize).ok_or_else(
                            || Error::RegexSearch {
                                message: format!(
                                    "grammar pattern ID {id} is out of bounds"
                                ),
                            },
                        )?;
                    (pattern.as_ref(), Some(pattern))
                }
                ScannerPatternRef::Dynamic(id) => {
                    let pattern =
                        self.patterns.get(*id as usize).ok_or_else(|| {
                            Error::RegexSearch {
                                message: format!(
                                    "dynamic pattern ID {id} is out of bounds"
                                ),
                            }
                        })?;
                    (pattern.as_ref(), None)
                }
                ScannerPatternRef::Empty => ("", None),
            };
            if collect_leading_literals {
                // A matching literal in this priority prefix wins over every later rule.
                if let Some(literal) = exact_regex_literal(pattern) {
                    leading_literals.push(literal);
                } else {
                    collect_leading_literals = false;
                }
            }
            let regex = match pattern_ref {
                ScannerPatternRef::Grammar(id) => {
                    let pattern = grammar_pattern
                        .expect("grammar patterns keep their interned source");
                    self.static_regexes.get(*id, pattern, &self.regex_pool)?
                }
                ScannerPatternRef::Empty => {
                    self.static_regexes.get_empty(&self.regex_pool)?
                }
                ScannerPatternRef::Dynamic(id) => {
                    let index = *id as usize;
                    let cached =
                        self.dynamic_regexes.get(index).ok_or_else(|| {
                            Error::RegexSearch {
                                message: format!(
                                    "dynamic pattern ID {id} is out of bounds"
                                ),
                            }
                        })?;
                    if let Some(regex) = cached {
                        regex.clone()
                    } else {
                        let regex =
                            Arc::new(CompiledRegex::compile(pattern).map_err(
                                |message| Error::InvalidRegex {
                                    pattern: pattern.to_owned(),
                                    message,
                                },
                            )?);
                        self.dynamic_regexes[index] = Some(regex.clone());
                        regex
                    }
                }
            };
            regexes.push(regex);
        }
        let set = create_scanner_set(&regexes)?;
        let match_params =
            match create_match_params(regexes.len(), self.regex_limits) {
                Ok(params) => params,
                Err(error) => {
                    free_scanner_set(set);
                    return Err(error);
                }
            };
        let id = self.scanners.len() as ScannerId;
        self.scanners.push(Scanner {
            actions: patterns.iter().map(|(action, _)| *action).collect(),
            leading_literals,
            set,
            match_params,
            _regexes: regexes,
        });
        self.scanner_ids.insert(key, id);
        Ok(id)
    }

    pub(super) fn find_next(
        &mut self,
        scanner_id: ScannerId,
        text: &str,
        start: usize,
        allow_a: bool,
        allow_g: bool,
        captures: &mut Vec<Option<Range<usize>>>,
    ) -> Result<Option<Action>> {
        find_next_regset(
            &mut self.scanners[scanner_id as usize],
            text,
            start,
            allow_a,
            allow_g,
            captures,
        )
    }
}

fn create_match_params(
    count: usize,
    limits: RegexLimits,
) -> Result<Vec<*mut onig_sys::OnigMatchParam>> {
    let mut params = Vec::with_capacity(count);
    for _ in 0..count {
        let param = unsafe { onig_sys::onig_new_match_param() };
        if param.is_null() {
            for param in params {
                unsafe { onig_sys::onig_free_match_param(param) };
            }
            return Err(Error::RegexSearch {
                message: "failed to allocate Oniguruma match parameters"
                    .to_owned(),
            });
        }
        unsafe {
            onig_sys::onig_set_retry_limit_in_match_of_match_param(
                param,
                std::os::raw::c_ulong::from(limits.match_retry_limit),
            );
            onig_sys::onig_set_retry_limit_in_search_of_match_param(
                param,
                std::os::raw::c_ulong::from(limits.search_retry_limit),
            );
        }
        params.push(param);
    }
    Ok(params)
}

fn free_scanner_set(set: *mut onig_sys::OnigRegSet) {
    if set.is_null() {
        return;
    }
    unsafe {
        while onig_sys::onig_regset_number_of_regex(set) > 0 {
            onig_sys::onig_regset_replace(set, 0, ptr::null_mut());
        }
        onig_sys::onig_regset_free(set);
    }
}

fn create_scanner_set(
    regexes: &[Arc<CompiledRegex>],
) -> Result<*mut onig_sys::OnigRegSet> {
    if regexes.is_empty() {
        return Ok(ptr::null_mut());
    }
    let mut regexes = regexes
        .iter()
        .map(|regex| regex.as_raw())
        .collect::<Vec<_>>();
    let mut set = ptr::null_mut();
    let code = unsafe {
        onig_sys::onig_regset_new(
            &mut set,
            regexes.len().try_into().expect("too many scanner patterns"),
            regexes.as_mut_ptr(),
        )
    };
    if code != onig_sys::ONIG_NORMAL as i32 {
        return Err(Error::InvalidRegex {
            pattern: "<scanner>".to_owned(),
            message: onig_error(code, ptr::null_mut()),
        });
    }
    Ok(set)
}

fn scanner_options(allow_a: bool, allow_g: bool) -> onig_sys::OnigOptionType {
    let mut options = onig_sys::ONIG_OPTION_NONE;
    if !allow_a {
        options |= onig_sys::ONIG_OPTION_NOT_BEGIN_STRING;
    }
    if !allow_g {
        options |= onig_sys::ONIG_OPTION_NOT_BEGIN_POSITION;
    }
    options
}

fn exact_regex_literal(pattern: &str) -> Option<Box<[u8]>> {
    if pattern.is_empty() {
        return None;
    }
    let mut literal = Vec::with_capacity(pattern.len());
    let mut chars = pattern.chars();
    while let Some(ch) = chars.next() {
        match ch {
            '.' | '^' | '$' | '*' | '+' | '?' | '(' | ')' | '[' | ']' | '{'
            | '}' | '|' => {
                return None;
            }
            '\\' => {
                let escaped = chars.next()?;
                if !matches!(
                    escaped,
                    '\\' | '.'
                        | '^'
                        | '$'
                        | '*'
                        | '+'
                        | '?'
                        | '('
                        | ')'
                        | '['
                        | ']'
                        | '{'
                        | '}'
                        | '|'
                        | '/'
                        | '-'
                ) {
                    return None;
                }
                let mut buffer = [0; 4];
                literal.extend_from_slice(
                    escaped.encode_utf8(&mut buffer).as_bytes(),
                );
            }
            literal_char => {
                let mut buffer = [0; 4];
                literal.extend_from_slice(
                    literal_char.encode_utf8(&mut buffer).as_bytes(),
                );
            }
        }
    }
    Some(literal.into_boxed_slice())
}

fn find_next_regset(
    scanner: &mut Scanner,
    text: &str,
    start: usize,
    allow_a: bool,
    allow_g: bool,
    captures: &mut Vec<Option<Range<usize>>>,
) -> Result<Option<Action>> {
    if scanner.actions.is_empty() {
        return Ok(None);
    }
    // Adjacent delimiters and punctuation can bypass the multi-regex engine entirely.
    for (index, literal) in scanner.leading_literals.iter().enumerate() {
        if text.as_bytes()[start..].starts_with(literal) {
            captures.clear();
            captures.push(Some(start..start + literal.len()));
            return Ok(Some(scanner.actions[index]));
        }
    }
    let initial_options = scanner_options(allow_a, allow_g);
    if text.len() >= 1_000
        && let Some(action) = match_at(
            scanner,
            text.as_bytes(),
            start,
            initial_options,
            captures,
        )?
    {
        return Ok(Some(action));
    }
    if let Some((index, region)) = search_regset(
        scanner,
        text.as_bytes(),
        start,
        text.len(),
        initial_options,
    )? {
        copy_captures(region, captures);
        return Ok(Some(scanner.actions[index]));
    }
    Ok(None)
}

fn match_at(
    scanner: &mut Scanner,
    text: &[u8],
    start: usize,
    options: onig_sys::OnigOptionType,
    captures: &mut Vec<Option<Range<usize>>>,
) -> Result<Option<Action>> {
    for index in 0..scanner.actions.len() {
        let regex =
            unsafe { onig_sys::onig_regset_get_regex(scanner.set, index as _) };
        let region = unsafe {
            onig_sys::onig_regset_get_region(scanner.set, index as _)
        };
        let code = unsafe {
            onig_sys::onig_match_with_param(
                regex,
                text.as_ptr(),
                text.as_ptr().add(text.len()),
                text.as_ptr().add(start),
                region,
                options,
                scanner.match_params[index],
            )
        };
        if code >= 0 {
            copy_captures(region, captures);
            return Ok(Some(scanner.actions[index]));
        }
        if code != onig_sys::ONIG_MISMATCH {
            return Err(Error::RegexSearch {
                message: onig_error(code, ptr::null_mut()),
            });
        }
    }
    Ok(None)
}

fn search_regset(
    scanner: &mut Scanner,
    text: &[u8],
    start: usize,
    range: usize,
    options: onig_sys::OnigOptionType,
) -> Result<Option<(usize, *mut onig_sys::OnigRegion)>> {
    if scanner.set.is_null() {
        return Ok(None);
    }
    let mut match_position = 0;
    let index = unsafe {
        onig_sys::onig_regset_search_with_param(
            scanner.set,
            text.as_ptr(),
            text.as_ptr().add(text.len()),
            text.as_ptr().add(start),
            text.as_ptr().add(range),
            onig_sys::OnigRegSetLead_ONIG_REGSET_POSITION_LEAD,
            options,
            scanner.match_params.as_mut_ptr(),
            &mut match_position,
        )
    };
    if index == onig_sys::ONIG_MISMATCH {
        return Ok(None);
    }
    if index < 0 {
        return Err(Error::RegexSearch {
            message: onig_error(index, ptr::null_mut()),
        });
    }
    let region =
        unsafe { onig_sys::onig_regset_get_region(scanner.set, index) };
    if region.is_null() {
        return Err(Error::RegexSearch {
            message: "Oniguruma returned a match without a capture region"
                .to_owned(),
        });
    }
    Ok(Some((index as usize, region)))
}

fn copy_captures(
    region: *const onig_sys::OnigRegion,
    captures: &mut Vec<Option<Range<usize>>>,
) {
    let region = unsafe { &*region };
    captures.clear();
    captures.reserve(region.num_regs as usize);
    for capture in 0..region.num_regs as usize {
        let begin = unsafe { *region.beg.add(capture) };
        let end = unsafe { *region.end.add(capture) };
        captures.push(
            (begin >= 0 && end >= 0).then_some(begin as usize..end as usize),
        );
    }
}

fn collect_patterns(
    grammar: &CompiledGrammar,
    ids: &[RuleId],
    seen: &mut HashSet<RuleId>,
    output: &mut Vec<ScannerPattern>,
) {
    for id in ids {
        if !seen.insert(*id) {
            continue;
        }
        let rule = &grammar.rules[*id as usize];
        match &rule.kind {
            RuleKind::Match { pattern, .. } => {
                output.push((
                    Action::Rule(*id),
                    ScannerPatternRef::Grammar(*pattern),
                ));
            }
            RuleKind::BeginEnd { begin, .. }
            | RuleKind::BeginWhile { begin, .. } => {
                output.push((
                    Action::Rule(*id),
                    ScannerPatternRef::Grammar(*begin),
                ));
            }
            RuleKind::IncludeOnly { patterns } => {
                collect_patterns(grammar, patterns, seen, output);
            }
            RuleKind::Placeholder => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::exact_regex_literal;

    #[test]
    fn recognizes_only_exact_regex_literals() {
        assert_eq!(
            exact_regex_literal("=>").as_deref(),
            Some(b"=>".as_slice())
        );
        assert_eq!(
            exact_regex_literal(r"\}\[\\").as_deref(),
            Some(b"}[\\".as_slice())
        );
        assert_eq!(
            exact_regex_literal("你好").as_deref(),
            Some("你好".as_bytes())
        );

        for pattern in ["", r"\d", ".", "a+", "^value", "(value)", "[ab]"] {
            assert!(exact_regex_literal(pattern).is_none(), "{pattern}");
        }
    }
}
