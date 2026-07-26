use super::*;
use crate::{
    grammar::{Rule, RuleKind, ScopeName},
    theme::{ColorId, Style, ThemeRule},
};

#[test]
fn rejects_an_oversized_lz4_block_before_decompression() {
    let mut writer = Writer::with_capacity(32);
    writer.bytes(MAGIC);
    writer.len(0); // languages
    writer.len(0); // strings
    writer.len(0); // grammars
    writer.sized_bytes(
        &u32::try_from(MAX_THEME_BLOCK_BYTES + 1)
            .unwrap()
            .to_le_bytes(),
    );
    writer.u64(1);
    writer.u64(1);

    let error = decode_owned(&writer.output)
        .err()
        .expect("oversized theme block must be rejected");
    assert_eq!(error.to_string(), "theme snapshot is too large");
}

#[test]
fn rejects_regex_limits_that_do_not_fit_the_platform_api() {
    let mut themes = Writer::with_capacity(1);
    themes.len(0);
    let themes = lz4_flex::block::compress_prepend_size(&themes.output);

    let mut writer = Writer::with_capacity(32);
    writer.bytes(MAGIC);
    writer.len(0); // languages
    writer.len(0); // strings
    writer.len(0); // grammars
    writer.sized_bytes(&themes);
    writer.u64(u64::from(u32::MAX) + 1);
    writer.u64(1);

    let error = decode_owned(&writer.output)
        .err()
        .expect("oversized regex limit must be rejected");
    assert_eq!(error.to_string(), "snapshot regex limit is too large");
}

#[test]
fn rejects_collection_lengths_larger_than_the_remaining_input() {
    let mut reader = Reader::new(&[2, 0]);
    let error = reader
        .vec(Reader::u8)
        .expect_err("collection cannot contain two items in one byte");
    assert_eq!(
        error.to_string(),
        "snapshot collection exceeds the remaining input"
    );
}

#[test]
fn rejects_deeply_nested_selector_expressions() {
    let mut encoded = vec![3; MAX_SELECTOR_EXPRESSION_DEPTH];
    encoded.extend([0, 0]);
    let error = Reader::new(&encoded)
        .expression()
        .expect_err("deep selector expression must be rejected");
    assert_eq!(
        error.to_string(),
        "snapshot selector expression is nested too deeply"
    );
}

#[test]
fn grammar_decoder_rejects_cross_references_outside_the_tables() {
    let grammar = CompiledGrammar {
        root_scope_name: 0,
        root: 0,
        rules: vec![
            Rule {
                name: None,
                content_name: None,
                kind: RuleKind::IncludeOnly { patterns: vec![1] },
            },
            Rule {
                name: None,
                content_name: None,
                kind: RuleKind::Match {
                    pattern: 0,
                    captures: Vec::new(),
                },
            },
        ],
        patterns: Vec::new(),
        scope_names: vec![ScopeName {
            scopes: Box::new([]),
        }],
        scope_templates: Vec::new(),
        injection_selectors: Vec::new(),
        injections: Vec::new(),
    };
    let mut writer = Writer::with_capacity(32);
    writer.grammar(&StringTable::default(), &grammar);
    let block = lz4_flex::block::compress_prepend_size(&writer.output);

    let error = decode_grammar_block(&block, &empty_strings())
        .err()
        .expect("invalid pattern ID must be rejected");
    assert_eq!(
        error.to_string(),
        "grammar snapshot contains an invalid regex pattern ID"
    );
}

#[test]
fn theme_decoder_rejects_invalid_rule_references() {
    let theme = Theme {
        name: Arc::from("test"),
        foreground: Arc::from("#fff"),
        background: Arc::from("#000"),
        colors: vec![Arc::from("#fff")],
        foreground_id: ColorId(0),
        ansi_colors: [ColorId(0); 16],
        selectors: Vec::new(),
        rules: vec![ThemeRule {
            target: 0,
            parents: Vec::new(),
            target_depth: 1,
            style: Style::default(),
            order: 0,
        }],
    };
    let mut table = StringTable::default();
    table.intern("named");
    table.intern("named-css");
    collect_theme_strings(&theme, &mut table);
    let strings = test_strings(&table);
    let mut writer = Writer::with_capacity(64);
    writer.len(1);
    writer.string(&table, "named");
    writer.string(&table, "named-css");
    writer.theme(&table, &theme);
    let block = lz4_flex::block::compress_prepend_size(&writer.output);

    let error = decode_theme_block(&block, &strings)
        .expect_err("invalid selector ID must be rejected");
    assert_eq!(
        error.to_string(),
        "snapshot contains an invalid theme selector ID"
    );
}

fn empty_strings() -> SnapshotStrings {
    SnapshotStrings {
        source: SnapshotSource::Static(&[]),
        data_start: 0,
        offsets: vec![0].into_boxed_slice(),
        values: Vec::<OnceLock<Arc<str>>>::new().into_boxed_slice(),
    }
}

fn test_strings(table: &StringTable) -> Arc<SnapshotStrings> {
    let mut writer = Writer::with_capacity(table.encoded_len());
    writer.snapshot_strings(table);
    let source = SnapshotSource::Owned(Arc::from(writer.output));
    let source_len = source.as_slice().len();
    let mut reader = Reader::new(source.as_slice());
    let strings = decode_strings(&mut reader, source.clone(), source_len)
        .expect("test string table must decode");
    assert!(reader.remaining.is_empty());
    strings
}
