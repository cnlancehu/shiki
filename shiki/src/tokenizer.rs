use std::borrow::Cow;
use std::collections::{HashMap, HashSet};
use std::ffi::CStr;
use std::ops::Range;
use std::ptr;
use std::sync::{Arc, OnceLock};

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::grammar::{
    Capture, CompiledGrammar, RuleId, RuleKind, ScopeName, ScopeNameId, ScopePart, ScopeTemplate,
    ScopeTemplateId,
};
use crate::matcher::{Priority, scope_matches as selector_matches};
use crate::theme::{FontStyle, Style, Theme, ThemeMatcher};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScopeToken {
    pub range: Range<usize>,
    pub scopes: ScopeStackId,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ThemedToken {
    pub content: String,
    pub color: Arc<str>,
    pub background: Option<Arc<str>>,
    pub font_style: FontStyle,
    pub scopes: ScopeStackId,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ThemeTokenStyle {
    pub theme: ThemeId,
    pub color: Arc<str>,
    pub background: Option<Arc<str>>,
    pub font_style: FontStyle,
}

pub type ThemeId = u16;

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct RegexLimits {
    pub match_retry_limit: u64,
    pub search_retry_limit: u64,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TokenizerCacheStats {
    pub scanners: usize,
    pub regexes: usize,
    pub dynamic_patterns: usize,
    pub scope_values: usize,
    pub scope_stacks: usize,
    pub capture_values: usize,
    pub injection_sets: usize,
    pub style_rows: usize,
    pub reusable_buffer_bytes: usize,
}

impl Default for RegexLimits {
    fn default() -> Self {
        Self {
            match_retry_limit: 10_000_000,
            search_retry_limit: 10_000_000,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MultiThemedToken {
    pub content: String,
    pub styles: Vec<ThemeTokenStyle>,
    pub scopes: ScopeStackId,
}

type ScopeId = u32;
pub type ScopeStackId = u32;
type PatternId = u32;
type InjectionSetId = u32;
type CaptureValueId = u32;
type ScannerId = u32;
const NO_SCANNER: ScannerId = ScannerId::MAX;

#[derive(Clone, PartialEq, Eq, Hash)]
struct ScopeValue {
    template: ScopeTemplateId,
    captures: Arc<[CaptureValueId]>,
}

struct ScopeNode {
    scope: Option<ScopeId>,
    styles_ready: bool,
    injections: Option<InjectionSetId>,
}

struct ScopeArena {
    scope_ids: HashMap<ScopeValue, ScopeId>,
    values: Vec<ScopeValue>,
    capture_ids: HashMap<Arc<str>, CaptureValueId>,
    captures: Vec<Arc<str>>,
    nodes: Vec<ScopeNode>,
    parents: Vec<ScopeStackId>,
    transitions: HashMap<(ScopeStackId, ScopeId), ScopeStackId>,
}

impl ScopeArena {
    fn new() -> Self {
        Self {
            scope_ids: HashMap::new(),
            values: Vec::new(),
            capture_ids: HashMap::new(),
            captures: Vec::new(),
            nodes: vec![ScopeNode {
                scope: None,
                styles_ready: true,
                injections: None,
            }],
            parents: vec![0],
            transitions: HashMap::new(),
        }
    }

    fn push(
        &mut self,
        parent: ScopeStackId,
        template: ScopeTemplateId,
        captures: Arc<[CaptureValueId]>,
    ) -> ScopeStackId {
        let value = ScopeValue { template, captures };
        let scope_id = if let Some(id) = self.scope_ids.get(&value) {
            *id
        } else {
            let id = self.values.len() as ScopeId;
            self.values.push(value.clone());
            self.scope_ids.insert(value, id);
            id
        };
        if let Some(child) = self.transitions.get(&(parent, scope_id)) {
            return *child;
        }
        let child = self.nodes.len() as ScopeStackId;
        self.nodes.push(ScopeNode {
            scope: Some(scope_id),
            styles_ready: false,
            injections: None,
        });
        self.parents.push(parent);
        self.transitions.insert((parent, scope_id), child);
        child
    }

    fn path(&self, mut stack: ScopeStackId) -> Vec<ScopeId> {
        let mut path = Vec::new();
        while stack != 0 {
            let node = &self.nodes[stack as usize];
            path.push(node.scope.expect("non-root scope node"));
            stack = self.parents[stack as usize];
        }
        path.reverse();
        path
    }

    fn intern_capture(&mut self, value: &str) -> CaptureValueId {
        if let Some(id) = self.capture_ids.get(value) {
            return *id;
        }
        let id = self.captures.len() as CaptureValueId;
        let value: Arc<str> = Arc::from(value);
        self.captures.push(value.clone());
        self.capture_ids.insert(value, id);
        id
    }
}

#[derive(Debug, Clone, Copy)]
struct Frame {
    rule: RuleId,
    scopes: ScopeStackId,
    content_scopes: ScopeStackId,
    end_pattern: Option<PatternId>,
    while_pattern: Option<PatternId>,
    anchor_position: usize,
    scanner: ScannerId,
    while_scanner: ScannerId,
}

#[derive(Debug, Clone, Default)]
pub struct GrammarState {
    grammar: u64,
    stack: Vec<Frame>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum Action {
    Rule(RuleId),
    End,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum ScannerPatternRef {
    Grammar(u32),
    Dynamic(PatternId),
    Empty,
}

type ScannerPattern = (Action, ScannerPatternRef);

struct Scanner {
    actions: Vec<Action>,
    leading_literals: Vec<Box<[u8]>>,
    set: *mut onig_sys::OnigRegSet,
    match_params: Vec<*mut onig_sys::OnigMatchParam>,
}

impl Drop for Scanner {
    fn drop(&mut self) {
        for param in self.match_params.drain(..) {
            unsafe { onig_sys::onig_free_match_param(param) };
        }
        free_scanner_set(self.set);
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
                message: "failed to allocate Oniguruma match parameters".to_owned(),
            });
        }
        unsafe {
            onig_sys::onig_set_retry_limit_in_match_of_match_param(
                param,
                limits.match_retry_limit as _,
            );
            onig_sys::onig_set_retry_limit_in_search_of_match_param(
                param,
                limits.search_retry_limit as _,
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

fn create_scanner_set(regexes: &[onig_sys::OnigRegex]) -> Result<*mut onig_sys::OnigRegSet> {
    if regexes.is_empty() {
        return Ok(ptr::null_mut());
    }
    let mut regexes = regexes.to_vec();
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum ScannerKey {
    State {
        rule: RuleId,
        end_pattern: Option<PatternId>,
        injections: InjectionSetId,
    },
    Dynamic {
        rule: RuleId,
        pattern: PatternId,
    },
}

struct Retokenize {
    range: Range<usize>,
    rule: RuleId,
    scopes: ScopeStackId,
}

pub(crate) struct Tokenizer {
    grammar: Arc<CompiledGrammar>,
    grammar_id: u64,
    regex_limits: RegexLimits,
    scanner_ids: HashMap<ScannerKey, ScannerId>,
    scanners: Vec<Scanner>,
    regex_ids: HashMap<ScannerPatternRef, u32>,
    regexes: Vec<onig_sys::OnigRegex>,
    pattern_ids: HashMap<Arc<str>, PatternId>,
    patterns: Vec<Arc<str>>,
    injection_set_ids: HashMap<Arc<[(bool, RuleId)]>, InjectionSetId>,
    injection_sets: Vec<Arc<[(bool, RuleId)]>>,
    injection_scope_matches: Vec<Box<[bool]>>,
    scopes: ScopeArena,
    themes: Vec<ThemeMatcher>,
    style_rows: Vec<Style>,
    root_scope: ScopeStackId,
    line_buffers: Vec<String>,
    capture_buffers: Vec<Vec<Option<Range<usize>>>>,
}

impl Tokenizer {
    pub fn new(
        grammar: Arc<CompiledGrammar>,
        grammar_id: u64,
        themes: Vec<Arc<Theme>>,
        regex_limits: RegexLimits,
    ) -> Self {
        let mut scopes = ScopeArena::new();
        let root_scope = push_scope_name(
            &mut scopes,
            &grammar.scope_names,
            &grammar.scope_templates,
            0,
            Some(grammar.root_scope_name),
            &[],
            "",
        );
        let mut injection_set_ids = HashMap::new();
        injection_set_ids.insert(Arc::from([]), 0);
        let theme_count = themes.len();
        let default_styles = vec![Style::default(); theme_count];
        Self {
            grammar,
            grammar_id,
            regex_limits,
            scanner_ids: HashMap::new(),
            scanners: Vec::new(),
            regex_ids: HashMap::new(),
            regexes: Vec::new(),
            pattern_ids: HashMap::new(),
            patterns: Vec::new(),
            injection_set_ids,
            injection_sets: vec![Arc::from([])],
            injection_scope_matches: Vec::new(),
            scopes,
            themes: themes.iter().map(Theme::matcher).collect(),
            style_rows: default_styles,
            root_scope,
            line_buffers: Vec::new(),
            capture_buffers: Vec::new(),
        }
    }

    pub fn initial_state(&self) -> GrammarState {
        GrammarState {
            grammar: self.grammar_id,
            stack: vec![Frame {
                rule: self.grammar.root,
                scopes: self.root_scope,
                content_scopes: self.root_scope,
                end_pattern: None,
                while_pattern: None,
                anchor_position: 0,
                scanner: NO_SCANNER,
                while_scanner: NO_SCANNER,
            }],
        }
    }

    pub fn cache_stats(&self) -> TokenizerCacheStats {
        let line_bytes = self
            .line_buffers
            .iter()
            .map(String::capacity)
            .sum::<usize>();
        let capture_bytes = self
            .capture_buffers
            .iter()
            .map(|buffer| buffer.capacity() * std::mem::size_of::<Option<Range<usize>>>())
            .sum::<usize>();
        TokenizerCacheStats {
            scanners: self.scanners.len(),
            regexes: self.regexes.len(),
            dynamic_patterns: self.patterns.len(),
            scope_values: self.scopes.values.len(),
            scope_stacks: self.scopes.nodes.len(),
            capture_values: self.scopes.captures.len(),
            injection_sets: self.injection_sets.len(),
            style_rows: self.style_rows.len(),
            reusable_buffer_bytes: line_bytes + capture_bytes,
        }
    }

    pub fn tokenize_line(
        &mut self,
        line: &str,
        previous: Option<&GrammarState>,
        is_first_line: bool,
    ) -> Result<(Vec<ScopeToken>, GrammarState)> {
        self.tokenize_line_owned(line, previous.cloned(), is_first_line)
    }

    pub(crate) fn tokenize_line_owned(
        &mut self,
        line: &str,
        previous: Option<GrammarState>,
        is_first_line: bool,
    ) -> Result<(Vec<ScopeToken>, GrammarState)> {
        let mut tokens = Vec::new();
        let mut state = previous.unwrap_or_else(|| self.initial_state());
        if !state.stack.is_empty() && state.grammar != self.grammar_id {
            return Err(Error::GrammarStateMismatch);
        }
        if state.stack.is_empty() {
            state = self.initial_state();
        }

        let mut text = self.line_buffers.pop().unwrap_or_default();
        text.clear();
        text.reserve(line.len() + 1);
        text.push_str(line);
        text.push('\n');
        let mut position = 0;

        let mut captures = self.capture_buffers.pop().unwrap_or_default();
        self.check_while_conditions(
            &text,
            &mut position,
            &mut state,
            &mut tokens,
            is_first_line,
            &mut captures,
        )?;
        let mut zero_width_states = HashSet::new();

        while position < text.len() {
            let candidates = self.candidates(
                state.stack.last_mut().expect("root frame"),
                is_first_line,
                position,
            )?;
            let frame = state.stack.last().copied().expect("root frame");
            let Some(action) = self.find_next(
                candidates,
                &text,
                position,
                is_first_line,
                position == frame.anchor_position,
                &mut captures,
            )?
            else {
                emit(&mut tokens, position, line.len(), frame.content_scopes);
                break;
            };
            let Some(full) = captures.first().and_then(Clone::clone) else {
                break;
            };
            if full.start > position {
                emit(
                    &mut tokens,
                    position,
                    full.start.min(line.len()),
                    frame.content_scopes,
                );
            }

            if full.end == position {
                let stack = state
                    .stack
                    .iter()
                    .map(|frame| frame.rule)
                    .collect::<Vec<_>>();
                if !zero_width_states.insert((position, stack, action)) {
                    emit(&mut tokens, position, line.len(), frame.content_scopes);
                    break;
                }
            } else {
                zero_width_states.clear();
            }

            match action {
                Action::End => {
                    let rule = &self.grammar.rules[frame.rule as usize];
                    let RuleKind::BeginEnd { end_captures, .. } = &rule.kind else {
                        unreachable!()
                    };
                    let tasks = emit_captures(
                        &mut self.scopes,
                        &self.grammar.scope_names,
                        &self.grammar.scope_templates,
                        &mut tokens,
                        &captures,
                        frame.scopes,
                        None,
                        end_captures,
                        line.len(),
                        &text,
                    );
                    self.apply_retokenizations(&mut tokens, tasks, &text)?;
                    state.stack.pop();
                }
                Action::Rule(id) => {
                    let rule = &self.grammar.rules[id as usize];
                    match &rule.kind {
                        RuleKind::Match {
                            captures: rule_captures,
                            ..
                        } => {
                            let tasks = emit_captures(
                                &mut self.scopes,
                                &self.grammar.scope_names,
                                &self.grammar.scope_templates,
                                &mut tokens,
                                &captures,
                                frame.content_scopes,
                                rule.name,
                                rule_captures,
                                line.len(),
                                &text,
                            );
                            self.apply_retokenizations(&mut tokens, tasks, &text)?;
                        }
                        RuleKind::BeginEnd {
                            begin_captures,
                            end,
                            ..
                        } => {
                            let name = rule.name;
                            let content_name = rule.content_name;
                            let end_pattern = resolve_backrefs(
                                &self.grammar.patterns[*end as usize],
                                &captures,
                                &text,
                            );
                            let end_pattern = intern_pattern(
                                &mut self.pattern_ids,
                                &mut self.patterns,
                                &end_pattern,
                            );
                            let tasks = emit_captures(
                                &mut self.scopes,
                                &self.grammar.scope_names,
                                &self.grammar.scope_templates,
                                &mut tokens,
                                &captures,
                                frame.content_scopes,
                                name,
                                begin_captures,
                                line.len(),
                                &text,
                            );
                            self.apply_retokenizations(&mut tokens, tasks, &text)?;
                            let scopes = extend_scopes(
                                &mut self.scopes,
                                &self.grammar.scope_names,
                                &self.grammar.scope_templates,
                                frame.content_scopes,
                                name,
                                &captures,
                                &text,
                            );
                            let content_scopes = extend_scopes(
                                &mut self.scopes,
                                &self.grammar.scope_names,
                                &self.grammar.scope_templates,
                                scopes,
                                content_name,
                                &captures,
                                &text,
                            );
                            state.stack.push(Frame {
                                rule: id,
                                scopes,
                                content_scopes,
                                end_pattern: Some(end_pattern),
                                while_pattern: None,
                                anchor_position: full.end,
                                scanner: NO_SCANNER,
                                while_scanner: NO_SCANNER,
                            });
                        }
                        RuleKind::BeginWhile {
                            begin_captures,
                            while_pattern,
                            ..
                        } => {
                            let name = rule.name;
                            let content_name = rule.content_name;
                            let while_pattern = resolve_backrefs(
                                &self.grammar.patterns[*while_pattern as usize],
                                &captures,
                                &text,
                            );
                            let while_pattern = intern_pattern(
                                &mut self.pattern_ids,
                                &mut self.patterns,
                                &while_pattern,
                            );
                            let tasks = emit_captures(
                                &mut self.scopes,
                                &self.grammar.scope_names,
                                &self.grammar.scope_templates,
                                &mut tokens,
                                &captures,
                                frame.content_scopes,
                                name,
                                begin_captures,
                                line.len(),
                                &text,
                            );
                            self.apply_retokenizations(&mut tokens, tasks, &text)?;
                            let scopes = extend_scopes(
                                &mut self.scopes,
                                &self.grammar.scope_names,
                                &self.grammar.scope_templates,
                                frame.content_scopes,
                                name,
                                &captures,
                                &text,
                            );
                            let content_scopes = extend_scopes(
                                &mut self.scopes,
                                &self.grammar.scope_names,
                                &self.grammar.scope_templates,
                                scopes,
                                content_name,
                                &captures,
                                &text,
                            );
                            state.stack.push(Frame {
                                rule: id,
                                scopes,
                                content_scopes,
                                end_pattern: None,
                                while_pattern: Some(while_pattern),
                                anchor_position: full.end,
                                scanner: NO_SCANNER,
                                while_scanner: NO_SCANNER,
                            });
                        }
                        RuleKind::IncludeOnly { .. } | RuleKind::Placeholder => unreachable!(),
                    }
                }
            }
            position = full.end.max(position);
        }

        tokens
            .retain(|token| token.range.start < token.range.end && token.range.start < line.len());
        for token in tokens.iter_mut() {
            token.range.end = token.range.end.min(line.len());
        }
        self.capture_buffers.push(captures);
        self.line_buffers.push(text);
        Ok((tokens, state))
    }

    pub(crate) fn styles(&mut self, stack: ScopeStackId) -> &[Style] {
        self.ensure_styles(stack);
        let theme_count = self.themes.len();
        let start = stack as usize * theme_count;
        &self.style_rows[start..start + theme_count]
    }

    pub(crate) fn contains_scope_stack(&self, stack: ScopeStackId) -> bool {
        (stack as usize) < self.scopes.nodes.len()
    }

    pub(crate) fn scope_names(&self, stack: ScopeStackId) -> Result<Vec<String>> {
        if stack as usize >= self.scopes.nodes.len() {
            return Err(Error::InvalidScopeStack(stack));
        }
        Ok(self
            .scopes
            .path(stack)
            .into_iter()
            .map(|scope| scope_chunks(&self.grammar, &self.scopes, scope).concat())
            .collect())
    }

    fn ensure_styles(&mut self, stack: ScopeStackId) {
        let index = stack as usize;
        if self.scopes.nodes[index].styles_ready {
            return;
        }
        let theme_count = self.themes.len();
        let required = self.scopes.nodes.len() * theme_count;
        if self.style_rows.len() < required {
            self.style_rows.resize(required, Style::default());
        }
        let parent = self.scopes.parents[index];
        self.ensure_styles(parent);
        for theme in &mut self.themes {
            while theme.scope_count() < self.scopes.values.len() {
                let chunks =
                    scope_chunks(&self.grammar, &self.scopes, theme.scope_count() as ScopeId);
                theme.register_scope(&chunks);
            }
        }
        let parent_start = parent as usize * theme_count;
        let start = index * theme_count;
        let path = self.scopes.path(stack);
        for (theme_index, theme) in self.themes.iter().enumerate() {
            self.style_rows[start + theme_index] =
                theme.resolve_scope(&path, self.style_rows[parent_start + theme_index]);
        }
        self.scopes.nodes[index].styles_ready = true;
    }

    fn check_while_conditions(
        &mut self,
        text: &str,
        position: &mut usize,
        state: &mut GrammarState,
        tokens: &mut Vec<ScopeToken>,
        is_first_line: bool,
        captures: &mut Vec<Option<Range<usize>>>,
    ) -> Result<()> {
        let mut index = 0;
        while index < state.stack.len() {
            let frame = state.stack[index];
            let frame_index = index;
            index += 1;
            let Some(while_pattern) = frame.while_pattern else {
                continue;
            };
            let allow_g = *position == frame.anchor_position;
            let key = ScannerKey::Dynamic {
                rule: frame.rule,
                pattern: while_pattern,
            };
            let scanner = if frame.while_scanner != NO_SCANNER {
                frame.while_scanner
            } else {
                let scanner = self.compile_scanner(
                    key,
                    &[(
                        Action::Rule(frame.rule),
                        ScannerPatternRef::Dynamic(while_pattern),
                    )],
                )?;
                state.stack[frame_index].while_scanner = scanner;
                scanner
            };
            if self
                .find_next(scanner, text, *position, is_first_line, allow_g, captures)?
                .is_none()
            {
                state.stack.truncate(frame_index);
                break;
            }
            let full = captures[0].clone().unwrap();
            if full.start != *position {
                state.stack.truncate(frame_index);
                break;
            }
            let while_captures = match &self.grammar.rules[frame.rule as usize].kind {
                RuleKind::BeginWhile { while_captures, .. } => Some(while_captures),
                _ => None,
            };
            if let Some(while_captures) = while_captures {
                let tasks = emit_captures(
                    &mut self.scopes,
                    &self.grammar.scope_names,
                    &self.grammar.scope_templates,
                    tokens,
                    captures,
                    frame.content_scopes,
                    None,
                    while_captures,
                    text.len().saturating_sub(1),
                    text,
                );
                self.apply_retokenizations(tokens, tasks, text)?;
            }
            *position = full.end;
        }
        Ok(())
    }

    fn candidates(
        &mut self,
        frame: &mut Frame,
        _is_first_line: bool,
        _position: usize,
    ) -> Result<ScannerId> {
        if frame.scanner != NO_SCANNER {
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
                collect_patterns(&self.grammar, patterns, &mut HashSet::new(), &mut raw);
            }
            RuleKind::BeginEnd {
                patterns,
                apply_end_last,
                ..
            } => {
                if !apply_end_last {
                    raw.push((
                        Action::End,
                        frame
                            .end_pattern
                            .map(ScannerPatternRef::Dynamic)
                            .unwrap_or(ScannerPatternRef::Empty),
                    ));
                }
                collect_patterns(&self.grammar, patterns, &mut HashSet::new(), &mut raw);
                if *apply_end_last {
                    raw.push((
                        Action::End,
                        frame
                            .end_pattern
                            .map(ScannerPatternRef::Dynamic)
                            .unwrap_or(ScannerPatternRef::Empty),
                    ));
                }
            }
            RuleKind::BeginWhile { patterns, .. } => {
                collect_patterns(&self.grammar, patterns, &mut HashSet::new(), &mut raw);
            }
            _ => {}
        }
        let mut left_injections = Vec::new();
        let mut other_injections = Vec::new();
        for &(is_left, rule) in injections.iter() {
            {
                let target = if is_left {
                    &mut left_injections
                } else {
                    &mut other_injections
                };
                collect_patterns(&self.grammar, &[rule], &mut HashSet::new(), target);
            }
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
        for scope in self.injection_scope_matches.len()..self.scopes.values.len() {
            let chunks = scope_chunks(&self.grammar, &self.scopes, scope as ScopeId);
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
        let id = if let Some(id) = self.injection_set_ids.get(injections.as_slice()) {
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

    fn compile_scanner(
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
            });
            self.scanner_ids.insert(key, id);
            return Ok(id);
        }
        initialize_oniguruma();
        let mut regexes = Vec::with_capacity(patterns.len());
        let mut leading_literals = Vec::new();
        let mut collect_leading_literals = true;
        for (_, pattern_ref) in patterns {
            let pattern = match pattern_ref {
                ScannerPatternRef::Grammar(id) => &self.grammar.patterns[*id as usize],
                ScannerPatternRef::Dynamic(id) => &self.patterns[*id as usize],
                ScannerPatternRef::Empty => "",
            };
            if collect_leading_literals {
                // A matching literal in this priority prefix wins over every later rule.
                if let Some(literal) = exact_regex_literal(pattern) {
                    leading_literals.push(literal);
                } else {
                    collect_leading_literals = false;
                }
            }
            let regex = if let Some(id) = self.regex_ids.get(pattern_ref) {
                self.regexes[*id as usize]
            } else {
                let mut regex = ptr::null_mut();
                let mut error_info = unsafe { std::mem::zeroed() };
                let bytes = pattern.as_bytes();
                let code = unsafe {
                    onig_sys::onig_new(
                        &mut regex,
                        bytes.as_ptr(),
                        bytes.as_ptr().add(bytes.len()),
                        onig_sys::ONIG_OPTION_CAPTURE_GROUP,
                        ptr::addr_of_mut!(onig_sys::OnigEncodingUTF8),
                        onig_sys::OnigDefaultSyntax,
                        &mut error_info,
                    )
                };
                if code != onig_sys::ONIG_NORMAL as i32 {
                    return Err(Error::InvalidRegex {
                        pattern: pattern.to_owned(),
                        message: onig_error(code, &mut error_info),
                    });
                }
                let id = self.regexes.len() as u32;
                self.regexes.push(regex);
                self.regex_ids.insert(*pattern_ref, id);
                regex
            };
            regexes.push(regex);
        }
        let set = create_scanner_set(&regexes)?;
        let match_params = match create_match_params(regexes.len(), self.regex_limits) {
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
        });
        self.scanner_ids.insert(key, id);
        Ok(id)
    }

    fn find_next(
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

    fn apply_retokenizations(
        &mut self,
        tokens: &mut Vec<ScopeToken>,
        mut tasks: Vec<Retokenize>,
        text: &str,
    ) -> Result<()> {
        tasks.sort_by_key(|task| std::cmp::Reverse(task.range.end - task.range.start));
        for task in tasks {
            if task.range.is_empty() || task.range.end > text.len() {
                continue;
            }
            let fragment = &text[task.range.clone()];
            let state = GrammarState {
                grammar: self.grammar_id,
                stack: vec![Frame {
                    rule: task.rule,
                    scopes: task.scopes,
                    content_scopes: task.scopes,
                    end_pattern: None,
                    while_pattern: None,
                    anchor_position: 0,
                    scanner: NO_SCANNER,
                    while_scanner: NO_SCANNER,
                }],
            };
            let (mut replacement, _) = self.tokenize_line_owned(fragment, Some(state), false)?;
            for token in &mut replacement {
                token.range.start += task.range.start;
                token.range.end += task.range.start;
            }
            replace_range(tokens, task.range, replacement);
        }
        Ok(())
    }
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
            '.' | '^' | '$' | '*' | '+' | '?' | '(' | ')' | '[' | ']' | '{' | '}' | '|' => {
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
                literal.extend_from_slice(escaped.encode_utf8(&mut buffer).as_bytes());
            }
            literal_char => {
                let mut buffer = [0; 4];
                literal.extend_from_slice(literal_char.encode_utf8(&mut buffer).as_bytes());
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
    let mut chunk_start = start;
    let mut options = scanner_options(allow_a, allow_g);
    loop {
        let mut chunk_end = if text.len() < 1_000 {
            text.len()
        } else {
            (chunk_start + 64).min(text.len())
        };
        while chunk_end < text.len() && !text.is_char_boundary(chunk_end) {
            chunk_end += 1;
        }
        if let Some((index, region)) =
            search_regset(scanner, text.as_bytes(), chunk_start, chunk_end, options)?
        {
            copy_captures(region, captures);
            return Ok(Some(scanner.actions[index]));
        }
        if chunk_end == text.len() {
            return Ok(None);
        }
        chunk_start = chunk_end;
        options |= onig_sys::ONIG_OPTION_NOT_BEGIN_POSITION;
    }
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
    let region = unsafe { onig_sys::onig_regset_get_region(scanner.set, index) };
    if region.is_null() {
        return Err(Error::RegexSearch {
            message: "Oniguruma returned a match without a capture region".to_owned(),
        });
    }
    Ok(Some((index as usize, region)))
}

fn copy_captures(region: *const onig_sys::OnigRegion, captures: &mut Vec<Option<Range<usize>>>) {
    let region = unsafe { &*region };
    captures.clear();
    captures.reserve(region.num_regs as usize);
    for capture in 0..region.num_regs as usize {
        let begin = unsafe { *region.beg.add(capture) };
        let end = unsafe { *region.end.add(capture) };
        captures.push((begin >= 0 && end >= 0).then_some(begin as usize..end as usize));
    }
}

impl Drop for Tokenizer {
    fn drop(&mut self) {
        self.scanners.clear();
        for regex in self.regexes.drain(..) {
            unsafe {
                onig_sys::onig_free(regex);
            }
        }
    }
}

fn intern_pattern(
    pattern_ids: &mut HashMap<Arc<str>, PatternId>,
    patterns: &mut Vec<Arc<str>>,
    pattern: &str,
) -> PatternId {
    if let Some(id) = pattern_ids.get(pattern) {
        return *id;
    }
    let id = patterns.len() as PatternId;
    let pattern: Arc<str> = Arc::from(pattern);
    patterns.push(pattern.clone());
    pattern_ids.insert(pattern, id);
    id
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
                output.push((Action::Rule(*id), ScannerPatternRef::Grammar(*pattern)));
            }
            RuleKind::BeginEnd { begin, .. } | RuleKind::BeginWhile { begin, .. } => {
                output.push((Action::Rule(*id), ScannerPatternRef::Grammar(*begin)));
            }
            RuleKind::IncludeOnly { patterns } => {
                collect_patterns(grammar, patterns, seen, output);
            }
            RuleKind::Placeholder => {}
        }
    }
}

fn emit(tokens: &mut Vec<ScopeToken>, start: usize, end: usize, scopes: ScopeStackId) {
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
fn emit_captures(
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
        if let Some(range) = matches.get(capture.index).and_then(Clone::clone) {
            boundaries.push(range.start.max(full.start));
            boundaries.push(range.end.min(full.end));
        }
    }
    boundaries.sort_unstable();
    boundaries.dedup();
    for pair in boundaries.windows(2) {
        let start = pair[0];
        let end = pair[1];
        let mut scopes = base;
        for capture in captures {
            if let Some(range) = matches.get(capture.index).and_then(Clone::clone)
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
            let range = matches.get(capture.index).and_then(Clone::clone)?;
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

fn replace_range(tokens: &mut Vec<ScopeToken>, range: Range<usize>, replacement: Vec<ScopeToken>) {
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

fn extend_scopes(
    arena: &mut ScopeArena,
    scope_names: &[ScopeName],
    scope_templates: &[ScopeTemplate],
    parent: ScopeStackId,
    name: Option<ScopeNameId>,
    captures: &[Option<Range<usize>>],
    text: &str,
) -> ScopeStackId {
    push_scope_name(
        arena,
        scope_names,
        scope_templates,
        parent,
        name,
        captures,
        text,
    )
}

fn push_scope_names(
    arena: &mut ScopeArena,
    scope_names: &[ScopeName],
    scope_templates: &[ScopeTemplate],
    scopes: &mut ScopeStackId,
    name: Option<ScopeNameId>,
    captures: &[Option<Range<usize>>],
    text: &str,
) {
    *scopes = push_scope_name(
        arena,
        scope_names,
        scope_templates,
        *scopes,
        name,
        captures,
        text,
    );
}

fn push_scope_name(
    arena: &mut ScopeArena,
    scope_names: &[ScopeName],
    scope_templates: &[ScopeTemplate],
    mut parent: ScopeStackId,
    name: Option<ScopeNameId>,
    captures: &[Option<Range<usize>>],
    text: &str,
) -> ScopeStackId {
    let Some(name) = name else {
        return parent;
    };
    for template in scope_names[name as usize].scopes.iter().copied() {
        let values = scope_templates[template as usize]
            .parts
            .iter()
            .filter_map(|part| {
                let ScopePart::Capture(index) = part else {
                    return None;
                };
                let value = captures
                    .get(*index)
                    .and_then(Clone::clone)
                    .map_or("", |range| &text[range]);
                Some(arena.intern_capture(value))
            })
            .collect();
        parent = arena.push(parent, template, values);
    }
    parent
}

fn scope_chunks<'a>(
    grammar: &'a CompiledGrammar,
    arena: &'a ScopeArena,
    scope: ScopeId,
) -> Vec<&'a str> {
    let value = &arena.values[scope as usize];
    let mut captures = value.captures.iter();
    grammar.scope_templates[value.template as usize]
        .parts
        .iter()
        .map(|part| match part {
            ScopePart::Literal(value) => value.as_ref(),
            ScopePart::Capture(_) => {
                let capture = captures.next().expect("compiled scope capture");
                arena.captures[*capture as usize].as_ref()
            }
        })
        .collect()
}

fn resolve_backrefs<'a>(
    pattern: &'a str,
    captures: &[Option<Range<usize>>],
    text: &str,
) -> Cow<'a, str> {
    if !pattern
        .as_bytes()
        .windows(2)
        .any(|pair| pair[0] == b'\\' && pair[1].is_ascii_digit())
    {
        return Cow::Borrowed(pattern);
    }
    let mut output = String::with_capacity(pattern.len());
    let bytes = pattern.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'\\' && index + 1 < bytes.len() && bytes[index + 1].is_ascii_digit() {
            let capture = (bytes[index + 1] - b'0') as usize;
            if let Some(range) = captures.get(capture).and_then(Clone::clone) {
                output.push_str(&regex_escape(&text[range]));
            }
            index += 2;
        } else {
            output.push(bytes[index] as char);
            index += 1;
        }
    }
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

fn initialize_oniguruma() {
    static INITIALIZED: OnceLock<()> = OnceLock::new();
    INITIALIZED.get_or_init(|| {
        let code = unsafe { onig_sys::onig_init() };
        assert_eq!(code, onig_sys::ONIG_NORMAL as i32, "onig_init failed");
    });
}

fn onig_error(code: i32, error_info: *mut onig_sys::OnigErrorInfo) -> String {
    let mut buffer = [0_u8; 256];
    unsafe {
        if !error_info.is_null() && onig_sys::onig_is_error_code_needs_param(code) != 0 {
            onig_sys::onig_error_code_to_str(buffer.as_mut_ptr(), code, error_info);
        } else {
            onig_sys::onig_error_code_to_str(buffer.as_mut_ptr(), code);
        }
        CStr::from_ptr(buffer.as_ptr().cast())
            .to_string_lossy()
            .into_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::exact_regex_literal;

    #[test]
    fn recognizes_only_exact_regex_literals() {
        assert_eq!(exact_regex_literal("=>").as_deref(), Some(b"=>".as_slice()));
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
