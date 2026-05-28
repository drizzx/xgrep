//! End-to-end CSV / TSV grep tests. Covers: ASCII matching, BOM stripping,
//! quoted fields, TSV via .tsv extension, and -E encoding override (GBK).

use std::fs;
use std::io::Write;
use std::path::Path;

use tempfile::TempDir;

use xgrep::matcher::{CaseMode, Pattern};
use xgrep::reader::ReaderOptions;
use xgrep::{search_file, MatchEvent};

fn write(path: &Path, bytes: &[u8]) {
    let mut f = fs::File::create(path).unwrap();
    f.write_all(bytes).unwrap();
}

fn match_texts(path: &Path, pat: &str) -> Vec<String> {
    let pattern = Pattern::compile(pat, CaseMode::Sensitive, false, false).unwrap();
    let block = search_file(path, &pattern, &ReaderOptions::default(), false);
    block
        .events
        .into_iter()
        .filter_map(|e| match e {
            MatchEvent::Match { text, .. } => Some(text),
            _ => None,
        })
        .collect()
}

#[test]
fn csv_ascii_simple_match() {
    let dir = TempDir::new().unwrap();
    let p = dir.path().join("data.csv");
    write(&p, b"alpha,beta\ngamma,delta\n");
    let hits = match_texts(&p, "gamma");
    assert_eq!(hits, vec!["gamma".to_string()]);
}

#[test]
fn csv_utf8_bom_match() {
    let dir = TempDir::new().unwrap();
    let p = dir.path().join("bom.csv");
    let mut bytes = vec![0xEF, 0xBB, 0xBF];
    bytes.extend_from_slice(b"header,value\n");
    write(&p, &bytes);
    // Pattern "header" must match the BOM-stripped first cell, not the BOM-prefixed version.
    let hits = match_texts(&p, "header");
    assert_eq!(hits, vec!["header".to_string()]);
}

#[test]
fn csv_quoted_field_with_comma() {
    let dir = TempDir::new().unwrap();
    let p = dir.path().join("quoted.csv");
    write(&p, b"\"foo,bar\",baz\n");
    let hits = match_texts(&p, "foo,bar");
    assert_eq!(hits, vec!["foo,bar".to_string()]);
}

#[test]
fn tsv_tab_separator_works() {
    let dir = TempDir::new().unwrap();
    let p = dir.path().join("data.tsv");
    write(&p, b"alpha\tbeta\ngamma\tdelta\n");
    let hits = match_texts(&p, "delta");
    assert_eq!(hits, vec!["delta".to_string()]);
}

#[test]
fn csv_e_gbk_round_trip() {
    let dir = TempDir::new().unwrap();
    let p = dir.path().join("gbk.csv");
    let utf8 = "张三,李四\n王五,赵六\n";
    let (gbk_bytes, _enc, had_errors) = encoding_rs::GBK.encode(utf8);
    assert!(!had_errors);
    write(&p, &gbk_bytes);

    let pattern = Pattern::compile("李四", CaseMode::Sensitive, false, false).unwrap();
    let opts = ReaderOptions {
        encoding: Some("gbk".to_string()),
        ..ReaderOptions::default()
    };
    let block = search_file(&p, &pattern, &opts, false);
    let hits: Vec<_> = block
        .events
        .into_iter()
        .filter_map(|e| match e {
            MatchEvent::Match { text, sheet, cell, .. } => Some((sheet, cell, text)),
            _ => None,
        })
        .collect();
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].0, "");           // sheet empty for CSV
    assert_eq!(hits[0].1, "B1");          // cell B1
    assert_eq!(hits[0].2, "李四");        // decoded text
}
