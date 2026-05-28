//! End-to-end oracle: for every fixture used by v0.1 integration tests,
//! every test pattern must produce the same Match events under fast-path
//! ON vs OFF.

use std::path::Path;

use rust_xlsxwriter::Workbook;
use tempfile::TempDir;

use xgrep::matcher::{CaseMode, Pattern};
use xgrep::reader::ReaderOptions;
use xgrep::{search_file, MatchEvent};

use crate::common;

fn collect_matches(
    path: &Path,
    pattern_text: &str,
    case: CaseMode,
    disable_fp: bool,
) -> Vec<(String, String, String, String)> {
    let pattern = Pattern::compile(pattern_text, case, false, false).unwrap();
    let opts = ReaderOptions {
        disable_fast_path: disable_fp,
        ..ReaderOptions::default()
    };
    let block = search_file(path, &pattern, &opts, false, xgrep::ContextOptions::default());
    let mut out: Vec<_> = block
        .events
        .into_iter()
        .filter_map(|e| match e {
            MatchEvent::Match {
                sheet,
                cell,
                layer,
                text,
                ..
            } => Some((sheet, cell, layer, text)),
            _ => None,
        })
        .collect();
    out.sort();
    out
}

fn assert_oracle_equivalent(path: &Path, pattern_text: &str, case: CaseMode) {
    let fast = collect_matches(path, pattern_text, case, false);
    let slow = collect_matches(path, pattern_text, case, true);
    assert_eq!(
        fast, slow,
        "fast-path divergence on {:?} pattern={:?}",
        path, pattern_text
    );
}

#[test]
fn oracle_basic_fixture_strings_pattern() {
    let dir = TempDir::new().unwrap();
    let path = common::write_basic_xlsx(dir.path());
    assert_oracle_equivalent(&path, "张三", CaseMode::Sensitive);
}

#[test]
fn oracle_basic_fixture_number_pattern() {
    let dir = TempDir::new().unwrap();
    let path = common::write_basic_xlsx(dir.path());
    assert_oracle_equivalent(&path, "100", CaseMode::Sensitive);
}

#[test]
fn oracle_basic_fixture_nonexistent_pattern() {
    let dir = TempDir::new().unwrap();
    let path = common::write_basic_xlsx(dir.path());
    assert_oracle_equivalent(&path, "ZZZ_NEVER_MATCH", CaseMode::Sensitive);
}

#[test]
fn oracle_formula_fixture_cached_value() {
    let dir = TempDir::new().unwrap();
    let path = common::write_formula_xlsx(dir.path());
    assert_oracle_equivalent(&path, "5", CaseMode::Sensitive);
}

#[test]
fn oracle_hidden_fixture_all_markers() {
    let dir = TempDir::new().unwrap();
    let path = common::write_hidden_xlsx(dir.path());
    for marker in &[
        "open-marker",
        "hidden-row-marker",
        "hidden-col-marker",
        "hidden-sheet-marker",
    ] {
        assert_oracle_equivalent(&path, marker, CaseMode::Sensitive);
    }
}

#[test]
fn oracle_comment_fixture() {
    let dir = TempDir::new().unwrap();
    let path = common::write_comment_xlsx(dir.path());
    assert_oracle_equivalent(&path, "张三留言", CaseMode::Sensitive);
}

#[test]
fn oracle_regex_pattern() {
    // Build a fresh fixture with predictable digit columns.
    let dir = TempDir::new().unwrap();
    let p = dir.path().join("regex.xlsx");
    let mut wb = Workbook::new();
    let s = wb.add_worksheet().set_name("S").unwrap();
    s.write_string(0, 0, "2024-01-15").unwrap();
    s.write_string(1, 0, "no-date-here").unwrap();
    s.write_string(2, 0, "2025-12-31").unwrap();
    wb.save(&p).unwrap();
    assert_oracle_equivalent(&p, r"\d{4}-\d{2}-\d{2}", CaseMode::Sensitive);
}
