use std::{borrow::Cow, collections::HashSet};

#[cfg(debug_assertions)]
use super::capture::assert_token_partition;
use super::{
    Frame, GrammarState, NO_SCANNER, ScopeToken, Tokenizer,
    capture::{
        Retokenize, clamp_captures, emit, emit_captures, intern_pattern,
        replace_range, resolve_backrefs,
    },
    scanner::{Action, ScannerKey, ScannerPatternRef},
    scope::extend_scopes,
};
use crate::{
    error::{Error, Result},
    grammar::RuleKind,
};

const MAX_CAPTURE_RETOKENIZATION_DEPTH: usize = 64;
const MIN_CAPTURE_RETOKENIZATION_WORK: usize = 1024 * 1024;

struct RetokenizationBudget {
    used: usize,
    limit: usize,
}

struct RetokenizationContext<'a> {
    budget: &'a mut RetokenizationBudget,
    depth: usize,
    is_first_line: bool,
}

impl RetokenizationBudget {
    fn for_line(line_len: usize) -> Self {
        Self {
            used: 0,
            limit: line_len
                .saturating_mul(8)
                .max(MIN_CAPTURE_RETOKENIZATION_WORK),
        }
    }

    fn enter_fragment(
        &mut self,
        current_depth: usize,
        fragment_len: usize,
    ) -> Result<usize> {
        let depth = current_depth.saturating_add(1);
        if depth > MAX_CAPTURE_RETOKENIZATION_DEPTH {
            return Err(Error::CaptureRetokenizationDepthLimit {
                limit: MAX_CAPTURE_RETOKENIZATION_DEPTH,
            });
        }
        let Some(used) = self.used.checked_add(fragment_len) else {
            return Err(Error::CaptureRetokenizationWorkLimit {
                limit: self.limit,
            });
        };
        if used > self.limit {
            return Err(Error::CaptureRetokenizationWorkLimit {
                limit: self.limit,
            });
        }
        self.used = used;
        Ok(depth)
    }
}

impl Tokenizer {
    pub(crate) fn tokenize_line(
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
        let mut budget = RetokenizationBudget::for_line(line.len());
        self.tokenize_line_owned_with_budget(
            line,
            previous,
            is_first_line,
            &mut budget,
            0,
        )
    }

    fn tokenize_line_owned_with_budget(
        &mut self,
        line: &str,
        previous: Option<GrammarState>,
        is_first_line: bool,
        budget: &mut RetokenizationBudget,
        retokenization_depth: usize,
    ) -> Result<(Vec<ScopeToken>, GrammarState)> {
        let mut tokens = Vec::new();
        let state = self.tokenize_line_into_owned_with_budget(
            line,
            previous,
            is_first_line,
            &mut tokens,
            budget,
            retokenization_depth,
        )?;
        Ok((tokens, state))
    }

    pub(crate) fn tokenize_line_into_owned(
        &mut self,
        line: &str,
        previous: Option<GrammarState>,
        is_first_line: bool,
        tokens: &mut Vec<ScopeToken>,
    ) -> Result<GrammarState> {
        let mut budget = RetokenizationBudget::for_line(line.len());
        self.tokenize_line_into_owned_with_budget(
            line,
            previous,
            is_first_line,
            tokens,
            &mut budget,
            0,
        )
    }

    fn tokenize_line_into_owned_with_budget(
        &mut self,
        line: &str,
        previous: Option<GrammarState>,
        is_first_line: bool,
        tokens: &mut Vec<ScopeToken>,
        budget: &mut RetokenizationBudget,
        retokenization_depth: usize,
    ) -> Result<GrammarState> {
        tokens.clear();
        let mut state = previous.unwrap_or_else(|| self.initial_state());
        self.validate_state(&state)?;
        if state.stack.is_empty() {
            state = self.initial_state();
        }

        let mut text = self.line_buffers.pop().unwrap_or_default();
        text.clear();
        text.reserve(line.len() + 1);
        text.push_str(line);
        text.push('\n');
        let mut position = 0;
        let mut retokenization = RetokenizationContext {
            budget,
            depth: retokenization_depth,
            is_first_line,
        };

        let mut captures = self.capture_buffers.pop().unwrap_or_default();
        self.check_while_conditions(
            &text,
            &mut position,
            &mut state,
            tokens,
            &mut captures,
            &mut retokenization,
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
                emit(tokens, position, line.len(), frame.content_scopes);
                break;
            };
            clamp_captures(&mut captures, line.len());
            let Some(full) = captures.first().and_then(Clone::clone) else {
                break;
            };
            if full.start > position {
                emit(
                    tokens,
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
                    emit(tokens, position, line.len(), frame.content_scopes);
                    break;
                }
            } else {
                zero_width_states.clear();
            }

            match action {
                Action::End => {
                    let rule = &self.grammar.rules[frame.rule as usize];
                    let RuleKind::BeginEnd { end_captures, .. } = &rule.kind
                    else {
                        unreachable!()
                    };
                    let tasks = emit_captures(
                        &mut self.scopes,
                        &self.grammar.scope_names,
                        &self.grammar.scope_templates,
                        tokens,
                        &captures,
                        frame.scopes,
                        None,
                        end_captures,
                        line.len(),
                        &text,
                    );
                    self.apply_retokenizations(
                        tokens,
                        tasks,
                        &text,
                        &mut retokenization,
                    )?;
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
                                tokens,
                                &captures,
                                frame.content_scopes,
                                rule.name,
                                rule_captures,
                                line.len(),
                                &text,
                            );
                            self.apply_retokenizations(
                                tokens,
                                tasks,
                                &text,
                                &mut retokenization,
                            )?;
                        }
                        RuleKind::BeginEnd {
                            begin_captures,
                            end,
                            ..
                        } => {
                            let name = rule.name;
                            let content_name = rule.content_name;
                            let end_pattern = match resolve_backrefs(
                                &self.grammar.patterns[*end as usize],
                                &captures,
                                &text,
                            ) {
                                Cow::Borrowed(_) => {
                                    ScannerPatternRef::Grammar(*end)
                                }
                                Cow::Owned(pattern) => {
                                    ScannerPatternRef::Dynamic(intern_pattern(
                                        &mut self.pattern_ids,
                                        &mut self.patterns,
                                        &mut self.dynamic_regexes,
                                        &pattern,
                                    ))
                                }
                            };
                            let tasks = emit_captures(
                                &mut self.scopes,
                                &self.grammar.scope_names,
                                &self.grammar.scope_templates,
                                tokens,
                                &captures,
                                frame.content_scopes,
                                name,
                                begin_captures,
                                line.len(),
                                &text,
                            );
                            self.apply_retokenizations(
                                tokens,
                                tasks,
                                &text,
                                &mut retokenization,
                            )?;
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
                            let while_pattern = match resolve_backrefs(
                                &self.grammar.patterns[*while_pattern as usize],
                                &captures,
                                &text,
                            ) {
                                Cow::Borrowed(_) => {
                                    ScannerPatternRef::Grammar(*while_pattern)
                                }
                                Cow::Owned(pattern) => {
                                    ScannerPatternRef::Dynamic(intern_pattern(
                                        &mut self.pattern_ids,
                                        &mut self.patterns,
                                        &mut self.dynamic_regexes,
                                        &pattern,
                                    ))
                                }
                            };
                            let tasks = emit_captures(
                                &mut self.scopes,
                                &self.grammar.scope_names,
                                &self.grammar.scope_templates,
                                tokens,
                                &captures,
                                frame.content_scopes,
                                name,
                                begin_captures,
                                line.len(),
                                &text,
                            );
                            self.apply_retokenizations(
                                tokens,
                                tasks,
                                &text,
                                &mut retokenization,
                            )?;
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
                        RuleKind::IncludeOnly { .. }
                        | RuleKind::Placeholder => unreachable!(),
                    }
                }
            }
            position = full.end.max(position);
        }

        tokens.retain(|token| {
            token.range.start < token.range.end
                && token.range.start < line.len()
        });
        for token in tokens.iter_mut() {
            token.range.end = token.range.end.min(line.len());
        }
        #[cfg(debug_assertions)]
        assert_token_partition(tokens, line.len());
        self.capture_buffers.push(captures);
        self.line_buffers.push(text);
        Ok(state)
    }

    fn check_while_conditions(
        &mut self,
        text: &str,
        position: &mut usize,
        state: &mut GrammarState,
        tokens: &mut Vec<ScopeToken>,
        captures: &mut Vec<Option<std::ops::Range<usize>>>,
        retokenization: &mut RetokenizationContext<'_>,
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
            let key = ScannerKey::While {
                rule: frame.rule,
                pattern: while_pattern,
            };
            let scanner = if frame.while_scanner != NO_SCANNER {
                frame.while_scanner
            } else {
                let scanner = self.compile_scanner(
                    key,
                    &[(Action::Rule(frame.rule), while_pattern)],
                )?;
                state.stack[frame_index].while_scanner = scanner;
                scanner
            };
            if self
                .find_next(
                    scanner,
                    text,
                    *position,
                    retokenization.is_first_line,
                    allow_g,
                    captures,
                )?
                .is_none()
            {
                state.stack.truncate(frame_index);
                break;
            }
            clamp_captures(captures, text.len().saturating_sub(1));
            let full = captures[0].clone().unwrap();
            if full.start != *position {
                state.stack.truncate(frame_index);
                break;
            }
            let while_captures =
                match &self.grammar.rules[frame.rule as usize].kind {
                    RuleKind::BeginWhile { while_captures, .. } => {
                        Some(while_captures)
                    }
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
                self.apply_retokenizations(
                    tokens,
                    tasks,
                    text,
                    retokenization,
                )?;
            }
            *position = full.end;
        }
        Ok(())
    }

    fn apply_retokenizations(
        &mut self,
        tokens: &mut Vec<ScopeToken>,
        mut tasks: Vec<Retokenize>,
        text: &str,
        retokenization: &mut RetokenizationContext<'_>,
    ) -> Result<()> {
        tasks.sort_by_key(|task| {
            std::cmp::Reverse(task.range.end - task.range.start)
        });
        for task in tasks {
            if task.range.is_empty() || task.range.end > text.len() {
                continue;
            }
            let fragment = &text[task.range.clone()];
            let fragment_depth = retokenization
                .budget
                .enter_fragment(retokenization.depth, fragment.len())?;
            let state = GrammarState {
                grammar: self.grammar_id,
                tokenizer: self.tokenizer_id,
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
            let (mut replacement, _) = self.tokenize_line_owned_with_budget(
                fragment,
                Some(state),
                false,
                retokenization.budget,
                fragment_depth,
            )?;
            for token in &mut replacement {
                token.range.start += task.range.start;
                token.range.end += task.range.start;
            }
            replace_range(tokens, task.range, replacement);
        }
        Ok(())
    }
}

#[cfg(all(test, feature = "json"))]
mod tests {
    use super::{
        MAX_CAPTURE_RETOKENIZATION_DEPTH, MIN_CAPTURE_RETOKENIZATION_WORK,
        RetokenizationBudget,
    };
    use crate::{Error, Highlighter, HighlighterEngine, RawGrammar, RawTheme};

    fn exercise_static_end_and_while(engine: &HighlighterEngine) {
        let mut session = engine.session("static-slots").unwrap();

        let mut end_state = session.initial_state();
        session
            .tokenize_line("BEGIN", &mut end_state, true)
            .unwrap();
        session.tokenize_line("END", &mut end_state, false).unwrap();

        let mut while_state = session.initial_state();
        session
            .tokenize_line("LOOP", &mut while_state, true)
            .unwrap();
        session
            .tokenize_line("CONT", &mut while_state, false)
            .unwrap();
    }

    #[test]
    fn static_end_and_while_regexes_are_shared_across_sessions() {
        let grammar = RawGrammar::from_json(
            "static-slots",
            r#"{
                "scopeName": "source.static-slots",
                "patterns": [
                    { "begin": "BEGIN", "end": "END" },
                    { "begin": "LOOP", "while": "CONT" }
                ]
            }"#,
        )
        .unwrap();
        let engine = Highlighter::builder()
            .language("static-slots", grammar)
            .theme(
                RawTheme::from_json(
                    "test",
                    r##"{
                        "name": "test",
                        "settings": [
                            { "settings": { "foreground": "#ffffff" } }
                        ]
                    }"##,
                )
                .unwrap(),
            )
            .build_engine()
            .unwrap();

        exercise_static_end_and_while(&engine);
        let warmed = engine.regex_cache_stats();
        assert_eq!(warmed.successful_compiles, 4);

        exercise_static_end_and_while(&engine);
        let reused = engine.regex_cache_stats();
        assert_eq!(reused.successful_compiles, warmed.successful_compiles);
        assert!(reused.cache_hits > warmed.cache_hits);
    }

    #[test]
    fn self_referential_capture_retokenization_returns_an_error() {
        let grammar = RawGrammar::from_json(
            "retokenization-cycle",
            r##"{
                "scopeName": "source.retokenization-cycle",
                "patterns": [{ "include": "#cycle" }],
                "repository": {
                    "cycle": {
                        "match": "(a)",
                        "captures": {
                            "1": {
                                "patterns": [{ "include": "#cycle" }]
                            }
                        }
                    }
                }
            }"##,
        )
        .unwrap();
        let engine = Highlighter::builder()
            .language("retokenization-cycle", grammar)
            .theme(
                RawTheme::from_json(
                    "test",
                    r##"{
                        "name": "test",
                        "settings": [
                            { "settings": { "foreground": "#ffffff" } }
                        ]
                    }"##,
                )
                .unwrap(),
            )
            .build_engine()
            .unwrap();
        let mut session = engine.session("retokenization-cycle").unwrap();
        let mut state = session.initial_state();

        let error = session
            .tokenize_line("a", &mut state, true)
            .expect_err("a recursive capture grammar must be bounded");

        assert!(matches!(
            error,
            Error::CaptureRetokenizationDepthLimit {
                limit: MAX_CAPTURE_RETOKENIZATION_DEPTH
            }
        ));
    }

    #[test]
    fn recursive_fragment_work_is_bounded_per_top_level_line() {
        let mut budget = RetokenizationBudget::for_line(0);
        budget
            .enter_fragment(0, MIN_CAPTURE_RETOKENIZATION_WORK)
            .unwrap();

        let error = budget
            .enter_fragment(0, 1)
            .expect_err("recursive work beyond the line budget must fail");

        assert!(matches!(
            error,
            Error::CaptureRetokenizationWorkLimit {
                limit: MIN_CAPTURE_RETOKENIZATION_WORK
            }
        ));
    }
}
