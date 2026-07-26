use std::{
    collections::HashMap,
    ops::Range,
    sync::{Arc, OnceLock},
};

use crate::{
    grammar::{CompiledGrammar, ScopePart},
    highlighter::HighlighterEngine,
    theme::Theme,
    tokenizer::RegexLimits,
};

mod reader;
#[cfg(test)]
mod tests;
mod validate;
mod writer;

use reader::Reader;
use validate::*;
use writer::Writer;

const MAGIC: &[u8; 8] = b"SHIKI\0\x05\0";

#[derive(Clone)]
enum SnapshotSource {
    Static(&'static [u8]),
    Owned(Arc<[u8]>),
}

impl SnapshotSource {
    fn as_slice(&self) -> &[u8] {
        match self {
            Self::Static(source) => source,
            Self::Owned(source) => source,
        }
    }
}

pub(crate) struct GrammarSnapshot {
    strings: Arc<SnapshotStrings>,
    range: Range<usize>,
}

impl GrammarSnapshot {
    pub(crate) fn decode(&self) -> Result<CompiledGrammar, SnapshotError> {
        decode_grammar_block(
            &self.strings.source.as_slice()[self.range.clone()],
            &self.strings,
        )
    }
}

struct SnapshotStrings {
    source: SnapshotSource,
    data_start: usize,
    offsets: Box<[u32]>,
    values: Box<[OnceLock<Arc<str>>]>,
}

impl SnapshotStrings {
    fn get(&self, index: usize) -> Result<Arc<str>, SnapshotError> {
        let start = self
            .offsets
            .get(index)
            .ok_or(SnapshotError("snapshot contains an invalid string ID"))?;
        let end = self
            .offsets
            .get(index.checked_add(1).ok_or(SnapshotError(
                "snapshot contains an invalid string ID",
            ))?)
            .ok_or(SnapshotError("snapshot contains an invalid string ID"))?;
        let cache = self
            .values
            .get(index)
            .expect("string cache and range table must have equal lengths");
        if let Some(value) = cache.get() {
            return Ok(value.clone());
        }
        let range =
            self.data_start + *start as usize..self.data_start + *end as usize;
        let bytes = &self.source.as_slice()[range];
        let value = std::str::from_utf8(bytes)
            .map_err(|_| SnapshotError("snapshot contains invalid UTF-8"))?;
        Ok(cache.get_or_init(|| Arc::from(value)).clone())
    }
}

pub(crate) struct SnapshotParts {
    pub languages: Vec<(String, usize)>,
    pub grammars: Vec<GrammarSnapshot>,
    pub themes: Vec<(String, String, Theme)>,
    pub regex_limits: RegexLimits,
}

pub(crate) fn validate_owned_parts(
    languages: &[(String, usize)],
    grammars: &[CompiledGrammar],
    themes: &[(String, String, Theme)],
) -> Result<(), SnapshotError> {
    validate_language_indices(languages, grammars.len())?;
    if themes.len() > usize::from(u16::MAX) + 1 {
        return Err(SnapshotError("snapshot contains too many themes"));
    }
    for grammar in grammars {
        validate_grammar(grammar)?;
    }
    for (_, _, theme) in themes {
        validate_theme(theme)?;
    }
    Ok(())
}

pub(crate) fn encode(engine: &HighlighterEngine) -> Vec<u8> {
    let mut strings = StringTable::default();
    collect_strings(engine, &mut strings);
    let mut writer = Writer::with_capacity(strings.encoded_len());
    writer.bytes(MAGIC);

    let mut languages = engine.inner.languages.iter().collect::<Vec<_>>();
    languages.sort_unstable_by(|left, right| left.0.cmp(right.0));
    writer.len(languages.len());
    for (name, index) in languages {
        writer.text(name);
        writer.index(*index);
    }

    writer.snapshot_strings(&strings);

    writer.len(engine.inner.compiled.len());
    for language in &engine.inner.compiled {
        writer.sized_bytes(&encode_grammar_block(
            &language.grammar_for_snapshot(),
            &strings,
        ));
    }

    writer.sized_bytes(&encode_theme_block(engine, &strings));

    writer.u64(u64::from(engine.inner.regex_limits.match_retry_limit));
    writer.u64(u64::from(engine.inner.regex_limits.search_retry_limit));
    writer.output
}

pub(crate) fn decode_static(
    source: &'static [u8],
) -> Result<SnapshotParts, SnapshotError> {
    decode(SnapshotSource::Static(source))
}

pub(crate) fn decode_owned(
    source: &[u8],
) -> Result<SnapshotParts, SnapshotError> {
    decode(SnapshotSource::Owned(Arc::from(source)))
}

fn decode(source: SnapshotSource) -> Result<SnapshotParts, SnapshotError> {
    let bytes = source.as_slice();
    let mut reader = Reader::new(bytes);
    if reader.take(MAGIC.len())? != MAGIC {
        return Err(SnapshotError("unsupported precompiled snapshot format"));
    }

    let languages = reader.vec(|reader| {
        let name = reader.text()?.to_owned();
        let index = reader.index()?;
        Ok((name, index))
    })?;
    let strings = decode_strings(&mut reader, source.clone(), bytes.len())?;
    let grammar_ranges = reader.vec(|reader| {
        let len = reader.index()?;
        let start = bytes.len() - reader.remaining.len();
        reader.take(len)?;
        Ok(start..start + len)
    })?;
    let themes = decode_theme_block(reader.sized_bytes()?, &strings)?;
    let regex_limits =
        RegexLimits {
            match_retry_limit: reader.u64()?.try_into().map_err(|_| {
                SnapshotError("snapshot regex limit is too large")
            })?,
            search_retry_limit: reader.u64()?.try_into().map_err(|_| {
                SnapshotError("snapshot regex limit is too large")
            })?,
        };
    if !reader.remaining.is_empty() {
        return Err(SnapshotError("snapshot contains trailing data"));
    }
    validate_language_indices(&languages, grammar_ranges.len())?;
    let grammars = grammar_ranges
        .into_iter()
        .map(|range| GrammarSnapshot {
            strings: strings.clone(),
            range,
        })
        .collect();

    Ok(SnapshotParts {
        languages,
        grammars,
        themes,
        regex_limits,
    })
}

fn validate_language_indices(
    languages: &[(String, usize)],
    grammar_count: usize,
) -> Result<(), SnapshotError> {
    if languages.iter().all(|(_, index)| *index < grammar_count) {
        Ok(())
    } else {
        Err(SnapshotError("snapshot contains an invalid language index"))
    }
}

fn decode_strings(
    reader: &mut Reader<'_>,
    source: SnapshotSource,
    source_len: usize,
) -> Result<Arc<SnapshotStrings>, SnapshotError> {
    let count = reader.collection_len()?;
    let mut offsets = Vec::new();
    offsets
        .try_reserve_exact(count.saturating_add(1))
        .map_err(|_| SnapshotError("snapshot string table is too large"))?;
    offsets.push(0);
    let mut total = 0_usize;
    for _ in 0..count {
        let len = reader.index()?;
        total = total
            .checked_add(len)
            .ok_or(SnapshotError("snapshot string table is too large"))?;
        offsets.push(total.try_into().map_err(|_| {
            SnapshotError("snapshot string table is too large")
        })?);
    }
    let start = source_len - reader.remaining.len();
    reader.take(total)?;

    let values = (0..count)
        .map(|_| OnceLock::new())
        .collect::<Vec<_>>()
        .into_boxed_slice();
    Ok(Arc::new(SnapshotStrings {
        source,
        data_start: start,
        offsets: offsets.into_boxed_slice(),
        values,
    }))
}

fn encode_grammar_block(
    grammar: &CompiledGrammar,
    strings: &StringTable,
) -> Vec<u8> {
    let mut writer = Writer::with_capacity(grammar.rules.len());
    writer.grammar(strings, grammar);
    lz4_flex::block::compress_prepend_size(&writer.output)
}

fn decode_grammar_block(
    source: &[u8],
    strings: &SnapshotStrings,
) -> Result<CompiledGrammar, SnapshotError> {
    let uncompressed = decompress_block(
        source,
        MAX_GRAMMAR_BLOCK_BYTES,
        "grammar snapshot is not valid LZ4",
        "grammar snapshot is too large",
    )?;
    let mut reader = Reader::new(&uncompressed);
    let grammar = reader.grammar(strings)?;
    if !reader.remaining.is_empty() {
        return Err(SnapshotError("grammar snapshot contains trailing data"));
    }
    validate_grammar(&grammar)?;
    Ok(grammar)
}

fn encode_theme_block(
    engine: &HighlighterEngine,
    strings: &StringTable,
) -> Vec<u8> {
    let mut writer = Writer::with_capacity(engine.inner.themes.len());
    writer.len(engine.inner.themes.len());
    for named in &engine.inner.themes {
        writer.string(strings, &named.name);
        writer.string(strings, &named.css_name);
        writer.theme(strings, &named.theme);
    }
    lz4_flex::block::compress_prepend_size(&writer.output)
}

fn decode_theme_block(
    source: &[u8],
    strings: &SnapshotStrings,
) -> Result<Vec<(String, String, Theme)>, SnapshotError> {
    let uncompressed = decompress_block(
        source,
        MAX_THEME_BLOCK_BYTES,
        "theme snapshot is not valid LZ4",
        "theme snapshot is too large",
    )?;
    let mut reader = Reader::new(&uncompressed);
    let themes = reader.vec_limited(
        usize::from(u16::MAX) + 1,
        "snapshot contains too many themes",
        |reader| {
            let name = reader.string(strings)?.as_ref().to_owned();
            let css_name = reader.string(strings)?.as_ref().to_owned();
            let theme = reader.theme(strings)?;
            validate_theme(&theme)?;
            Ok((name, css_name, theme))
        },
    )?;
    if !reader.remaining.is_empty() {
        return Err(SnapshotError("theme snapshot contains trailing data"));
    }
    Ok(themes)
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
            + self.values.len() * 4
            + self.values.iter().map(|value| value.len()).sum::<usize>()
    }
}

fn collect_strings(engine: &HighlighterEngine, strings: &mut StringTable) {
    for language in &engine.inner.compiled {
        collect_grammar_strings(&language.grammar_for_snapshot(), strings);
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

fn checked_u32(value: usize) -> u32 {
    u32::try_from(value).expect("precompiled snapshot exceeds 32-bit limits")
}
