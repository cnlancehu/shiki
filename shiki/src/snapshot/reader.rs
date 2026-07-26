use std::sync::Arc;

use super::{
    SnapshotError, SnapshotStrings,
    validate::{MAX_COLLECTION_ITEMS, MAX_SELECTOR_EXPRESSION_DEPTH},
};
use crate::{
    grammar::{
        Capture, CompiledGrammar, Injection, Rule, RuleKind, ScopeName,
        ScopePart, ScopeTemplate,
    },
    matcher::{Expression, Priority, ScopeSelector},
    theme::{ColorId, FontStyle, Style, Theme, ThemeRule},
};

pub(super) struct Reader<'a> {
    pub(super) remaining: &'a [u8],
    collection_items_left: usize,
}

impl<'a> Reader<'a> {
    pub(super) fn new(source: &'a [u8]) -> Self {
        Self {
            remaining: source,
            collection_items_left: MAX_COLLECTION_ITEMS,
        }
    }

    pub(super) fn take(
        &mut self,
        len: usize,
    ) -> Result<&'a [u8], SnapshotError> {
        if len > self.remaining.len() {
            return Err(SnapshotError("precompiled snapshot is truncated"));
        }
        let (value, remaining) = self.remaining.split_at(len);
        self.remaining = remaining;
        Ok(value)
    }

    pub(super) fn u8(&mut self) -> Result<u8, SnapshotError> {
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

    pub(super) fn u64(&mut self) -> Result<u64, SnapshotError> {
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

    pub(super) fn index(&mut self) -> Result<usize, SnapshotError> {
        Ok(self.u32()? as usize)
    }

    pub(super) fn sized_bytes(&mut self) -> Result<&'a [u8], SnapshotError> {
        let len = self.index()?;
        self.take(len)
    }

    pub(super) fn text(&mut self) -> Result<&'a str, SnapshotError> {
        std::str::from_utf8(self.sized_bytes()?)
            .map_err(|_| SnapshotError("snapshot contains invalid UTF-8"))
    }

    pub(super) fn vec<T>(
        &mut self,
        read: impl FnMut(&mut Self) -> Result<T, SnapshotError>,
    ) -> Result<Vec<T>, SnapshotError> {
        self.vec_limited(
            MAX_COLLECTION_ITEMS,
            "snapshot contains too many collection items",
            read,
        )
    }

    pub(super) fn vec_limited<T>(
        &mut self,
        max_len: usize,
        too_large_message: &'static str,
        mut read: impl FnMut(&mut Self) -> Result<T, SnapshotError>,
    ) -> Result<Vec<T>, SnapshotError> {
        let len = self.collection_len()?;
        if len > max_len {
            return Err(SnapshotError(too_large_message));
        }
        let mut values = Vec::new();
        values
            .try_reserve_exact(len)
            .map_err(|_| SnapshotError("snapshot collection is too large"))?;
        for _ in 0..len {
            values.push(read(self)?);
        }
        Ok(values)
    }

    pub(super) fn collection_len(&mut self) -> Result<usize, SnapshotError> {
        let len = self.index()?;
        if len > self.remaining.len() {
            return Err(SnapshotError(
                "snapshot collection exceeds the remaining input",
            ));
        }
        if len > self.collection_items_left {
            return Err(SnapshotError(
                "snapshot contains too many collection items",
            ));
        }
        self.collection_items_left -= len;
        Ok(len)
    }

    pub(super) fn string(
        &mut self,
        strings: &SnapshotStrings,
    ) -> Result<Arc<str>, SnapshotError> {
        strings.get(self.index()?)
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

    pub(super) fn grammar(
        &mut self,
        strings: &SnapshotStrings,
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

    pub(super) fn expression(&mut self) -> Result<Expression, SnapshotError> {
        self.expression_at_depth(0)
    }

    fn expression_at_depth(
        &mut self,
        depth: usize,
    ) -> Result<Expression, SnapshotError> {
        if depth >= MAX_SELECTOR_EXPRESSION_DEPTH {
            return Err(SnapshotError(
                "snapshot selector expression is nested too deeply",
            ));
        }
        match self.u8()? {
            0 => Ok(Expression::Path(self.u32_vec()?)),
            1 => Ok(Expression::And(
                self.vec(|reader| reader.expression_at_depth(depth + 1))?,
            )),
            2 => Ok(Expression::Or(
                self.vec(|reader| reader.expression_at_depth(depth + 1))?,
            )),
            3 => Ok(Expression::Not(Box::new(
                self.expression_at_depth(depth + 1)?,
            ))),
            _ => Err(SnapshotError(
                "snapshot contains an invalid selector expression tag",
            )),
        }
    }

    pub(super) fn theme(
        &mut self,
        strings: &SnapshotStrings,
    ) -> Result<Theme, SnapshotError> {
        let name = self.string(strings)?;
        let foreground = self.string(strings)?;
        let background = self.string(strings)?;
        let colors = self.vec(|reader| reader.string(strings))?;
        let foreground_id = ColorId(self.u32()?);
        let ansi_colors: [ColorId; 16] = self
            .u32_vec()?
            .into_iter()
            .map(ColorId)
            .collect::<Vec<_>>()
            .try_into()
            .map_err(|_| {
                SnapshotError("snapshot has an invalid ANSI palette")
            })?;
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
        Ok(Theme {
            name,
            foreground,
            background,
            colors,
            foreground_id,
            ansi_colors,
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
