mod capture;
mod line;
mod scanner;
mod scope;

use std::{
    collections::HashMap,
    ops::Range,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
};

pub(crate) use scanner::RegexPool;
use scanner::{
    CompiledRegex, Scanner, ScannerKey, ScannerPatternRef, StaticRegexSlots,
};
use scope::{ScopeArena, push_scope_name, scope_chunks};

use crate::{
    error::{Error, Result},
    grammar::{CompiledGrammar, RuleId},
    theme::{FontStyle, Style, Theme, ThemeMatcher},
};

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

#[derive(Debug, Clone, Copy)]
pub struct RegexLimits {
    pub match_retry_limit: u32,
    pub search_retry_limit: u32,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TokenizerCacheStats {
    pub scanners: usize,
    pub shared_regex_slots: usize,
    pub dynamic_regexes: usize,
    pub dynamic_patterns: usize,
    pub scope_values: usize,
    pub scope_stacks: usize,
    pub capture_values: usize,
    pub injection_sets: usize,
    pub style_rows: usize,
    pub reusable_buffer_bytes: usize,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RegexCacheStats {
    pub entries: usize,
    pub successful_compiles: u64,
    pub failed_compiles: u64,
    pub cache_hits: u64,
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

#[derive(Debug, Clone, Copy)]
struct Frame {
    rule: RuleId,
    scopes: ScopeStackId,
    content_scopes: ScopeStackId,
    end_pattern: Option<ScannerPatternRef>,
    while_pattern: Option<ScannerPatternRef>,
    anchor_position: usize,
    scanner: ScannerId,
    while_scanner: ScannerId,
}

#[derive(Debug, Clone, Default)]
pub struct GrammarState {
    grammar: u64,
    tokenizer: u64,
    stack: Vec<Frame>,
}

pub(crate) struct Tokenizer {
    grammar: Arc<CompiledGrammar>,
    regex_pool: Arc<RegexPool>,
    grammar_id: u64,
    tokenizer_id: u64,
    regex_limits: RegexLimits,
    scanner_ids: HashMap<ScannerKey, ScannerId>,
    scanners: Vec<Scanner>,
    static_regexes: Arc<StaticRegexSlots>,
    pattern_ids: HashMap<Arc<str>, PatternId>,
    patterns: Vec<Arc<str>>,
    dynamic_regexes: Vec<Option<Arc<CompiledRegex>>>,
    injection_set_ids: HashMap<Arc<[(bool, RuleId)]>, InjectionSetId>,
    injection_sets: Vec<Arc<[(bool, RuleId)]>>,
    injection_scope_matches: Vec<Box<[bool]>>,
    scopes: ScopeArena,
    themes: Vec<ThemeMatcher>,
    style_rows: Vec<Style>,
    root_scope: ScopeStackId,
    line_buffers: Vec<String>,
    capture_buffers: Vec<Vec<Option<Range<usize>>>>,
    scope_path: Vec<ScopeId>,
}

static NEXT_TOKENIZER_ID: AtomicU64 = AtomicU64::new(1);

impl Tokenizer {
    pub fn new(
        grammar: Arc<CompiledGrammar>,
        regex_pool: Arc<RegexPool>,
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
        let static_regexes =
            regex_pool.static_slots(grammar_id, &grammar.patterns);
        Self {
            grammar,
            regex_pool,
            grammar_id,
            tokenizer_id: NEXT_TOKENIZER_ID.fetch_add(1, Ordering::Relaxed),
            regex_limits,
            scanner_ids: HashMap::new(),
            scanners: Vec::new(),
            static_regexes,
            pattern_ids: HashMap::new(),
            patterns: Vec::new(),
            dynamic_regexes: Vec::new(),
            injection_set_ids,
            injection_sets: vec![Arc::from([])],
            injection_scope_matches: Vec::new(),
            scopes,
            themes: themes.iter().map(Theme::matcher).collect(),
            style_rows: default_styles,
            root_scope,
            line_buffers: Vec::new(),
            capture_buffers: Vec::new(),
            scope_path: Vec::new(),
        }
    }

    pub(crate) fn initial_state(&self) -> GrammarState {
        GrammarState {
            grammar: self.grammar_id,
            tokenizer: self.tokenizer_id,
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

    pub(crate) fn validate_state(&self, state: &GrammarState) -> Result<()> {
        if !state.stack.is_empty()
            && (state.grammar != self.grammar_id
                || state.tokenizer != self.tokenizer_id)
        {
            return Err(Error::GrammarStateMismatch);
        }
        Ok(())
    }

    pub(crate) fn cache_stats(&self) -> TokenizerCacheStats {
        let line_bytes = self
            .line_buffers
            .iter()
            .map(String::capacity)
            .sum::<usize>();
        let capture_bytes = self
            .capture_buffers
            .iter()
            .map(|buffer| {
                buffer.capacity() * std::mem::size_of::<Option<Range<usize>>>()
            })
            .sum::<usize>();
        TokenizerCacheStats {
            scanners: self.scanners.len(),
            shared_regex_slots: self.static_regexes.initialized_len(),
            dynamic_regexes: self
                .dynamic_regexes
                .iter()
                .filter(|regex| regex.is_some())
                .count(),
            dynamic_patterns: self.patterns.len(),
            scope_values: self.scopes.values.len(),
            scope_stacks: self.scopes.nodes.len(),
            capture_values: self.scopes.captures.len(),
            injection_sets: self.injection_sets.len(),
            style_rows: self.style_rows.len(),
            reusable_buffer_bytes: line_bytes
                + capture_bytes
                + self.scope_path.capacity() * std::mem::size_of::<ScopeId>(),
        }
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

    pub(crate) fn scope_names(
        &self,
        stack: ScopeStackId,
    ) -> Result<Vec<String>> {
        if stack as usize >= self.scopes.nodes.len() {
            return Err(Error::InvalidScopeStack(stack));
        }
        Ok(self
            .scopes
            .path(stack)
            .into_iter()
            .map(|scope| {
                scope_chunks(&self.grammar, &self.scopes, scope).concat()
            })
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
                let chunks = scope_chunks(
                    &self.grammar,
                    &self.scopes,
                    theme.scope_count() as ScopeId,
                );
                theme.register_scope(&chunks);
            }
        }
        let parent_start = parent as usize * theme_count;
        let start = index * theme_count;
        self.scopes.fill_path(stack, &mut self.scope_path);
        for (theme_index, theme) in self.themes.iter().enumerate() {
            self.style_rows[start + theme_index] = theme.resolve_scope(
                &self.scope_path,
                self.style_rows[parent_start + theme_index],
            );
        }
        self.scopes.nodes[index].styles_ready = true;
    }
}

impl Drop for Tokenizer {
    fn drop(&mut self) {
        // RegSet owns raw regex pointers unless they are detached first.
        // Scanner::drop performs that detachment before these Arcs are released.
        self.scanners.clear();
    }
}
