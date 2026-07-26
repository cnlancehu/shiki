use std::{collections::HashMap, ops::Range, sync::Arc};

use super::{CaptureValueId, InjectionSetId, ScopeId, ScopeStackId};
use crate::grammar::{
    CompiledGrammar, ScopeName, ScopeNameId, ScopePart, ScopeTemplate,
    ScopeTemplateId,
};

#[derive(Clone, PartialEq, Eq, Hash)]
pub(super) struct ScopeValue {
    template: ScopeTemplateId,
    captures: Arc<[CaptureValueId]>,
}

pub(super) struct ScopeNode {
    pub(super) scope: Option<ScopeId>,
    pub(super) styles_ready: bool,
    pub(super) injections: Option<InjectionSetId>,
}

pub(super) struct ScopeArena {
    scope_ids: HashMap<ScopeValue, ScopeId>,
    pub(super) values: Vec<ScopeValue>,
    capture_ids: HashMap<Arc<str>, CaptureValueId>,
    pub(super) captures: Vec<Arc<str>>,
    pub(super) nodes: Vec<ScopeNode>,
    pub(super) parents: Vec<ScopeStackId>,
    transitions: HashMap<(ScopeStackId, ScopeId), ScopeStackId>,
}

impl ScopeArena {
    pub(super) fn new() -> Self {
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

    pub(super) fn fill_path(
        &self,
        mut stack: ScopeStackId,
        path: &mut Vec<ScopeId>,
    ) {
        path.clear();
        while stack != 0 {
            let node = &self.nodes[stack as usize];
            path.push(node.scope.expect("non-root scope node"));
            stack = self.parents[stack as usize];
        }
        path.reverse();
    }

    pub(super) fn path(&self, stack: ScopeStackId) -> Vec<ScopeId> {
        let mut path = Vec::new();
        self.fill_path(stack, &mut path);
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

pub(super) fn extend_scopes(
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

pub(super) fn push_scope_names(
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

pub(super) fn push_scope_name(
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

pub(super) fn scope_chunks<'a>(
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
