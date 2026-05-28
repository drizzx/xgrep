//! Boundary cases that target fast-path decision logic specifically — these
//! are the cases that would NOT be exercised by sharing v0.1 fixtures alone.

use std::path::PathBuf;

use rust_xlsxwriter::Workbook;
use tempfile::TempDir;

use xgrep::matcher::{CaseMode, Pattern};
use xgrep::reader::ReaderOptions;
use xgrep::{search_file, MatchEvent};

fn matches(path: &std::path::Path, pat: &str, disable_fp: bool) -> Vec<String> {
    let pattern = Pattern::compile(pat, CaseMode::Sensitive, false, false).unwrap();
    let opts = ReaderOptions {
        disable_fast_path: disable_fp,
        ..ReaderOptions::default()
    };
    let block = search_file(path, &pattern, &opts, false);
    let mut out: Vec<_> = block
        .events
        .into_iter()
        .filter_map(|e| match e {
            MatchEvent::Match { text, .. } => Some(text),
            _ => None,
        })
        .collect();
    out.sort();
    out
}

fn write_two_sst_pool(dir: &std::path::Path) -> PathBuf {
    // Workbook with a sharedStrings pool — fast-path A relevant.
    let p = dir.join("sst_pool.xlsx");
    let mut wb = Workbook::new();
    let s = wb.add_worksheet().set_name("S").unwrap();
    s.write_string(0, 0, "alpha").unwrap();
    s.write_string(1, 0, "beta").unwrap();
    s.write_string(2, 0, "alpha").unwrap();
    wb.save(&p).unwrap();
    p
}

#[test]
fn sst_with_zero_matches_skips_correctly() {
    let dir = TempDir::new().unwrap();
    let p = write_two_sst_pool(dir.path());
    let r = matches(&p, "ZZZ", false);
    assert!(r.is_empty());
    // Equivalence: disabling fast-path also gives empty.
    assert_eq!(r, matches(&p, "ZZZ", true));
}

#[test]
fn pattern_in_sst_finds_all_referencing_cells() {
    let dir = TempDir::new().unwrap();
    let p = write_two_sst_pool(dir.path());
    let r = matches(&p, "alpha", false);
    // Two cells reference the sst entry "alpha".
    assert_eq!(r, vec!["alpha".to_string(), "alpha".to_string()]);
}

#[test]
fn pattern_containing_xml_meta_does_not_break_augment() {
    // The pattern itself looks like an XML tag. fast_path::augment must
    // isolate it via (?:...) so it doesn't collide with the injected <v>idx</v>.
    let dir = TempDir::new().unwrap();
    let p = dir.path().join("xml_meta.xlsx");
    let mut wb = Workbook::new();
    let s = wb.add_worksheet().set_name("S").unwrap();
    s.write_string(0, 0, "literal <v> in cell").unwrap();
    s.write_string(1, 0, "unrelated").unwrap();
    wb.save(&p).unwrap();
    let r = matches(&p, "<v>", false);
    assert_eq!(r, vec!["literal <v> in cell".to_string()]);
    // And equivalent with fast-path off.
    assert_eq!(r, matches(&p, "<v>", true));
}

#[test]
fn formula_cached_value_in_sheet_xml_found_via_fast_path_b() {
    let dir = TempDir::new().unwrap();
    let p = dir.path().join("formula_cached.xlsx");
    let mut wb = Workbook::new();
    let s = wb.add_worksheet().set_name("S").unwrap();
    // The cached numeric value lives in the sheet xml as <v>42</v>; sst is empty
    // for this cell. fast-path B alone must keep this sheet from being skipped.
    // Note: write_formula_with_result doesn't exist in rust_xlsxwriter 0.79.
    // Use write_formula + set_formula_result instead.
    s.write_formula(0, 0, "=21+21").unwrap();
    s.set_formula_result(0, 0, "42");
    wb.save(&p).unwrap();
    let r = matches(&p, "42", false);
    assert_eq!(r, vec!["42".to_string()]);
    assert_eq!(r, matches(&p, "42", true));
}

#[test]
fn pattern_at_cell_boundary_still_found() {
    // pattern "foo bar" spans across what would be a cell boundary in pretty
    // output, but inside a single cell text the byte scan must find it.
    let dir = TempDir::new().unwrap();
    let p = dir.path().join("boundary.xlsx");
    let mut wb = Workbook::new();
    let s = wb.add_worksheet().set_name("S").unwrap();
    s.write_string(0, 0, "foo bar baz").unwrap();
    wb.save(&p).unwrap();
    let r = matches(&p, "foo bar", false);
    assert_eq!(r, vec!["foo bar baz".to_string()]);
}

#[test]
fn anchored_pattern_handled_conservatively() {
    // ^foo$ should still find a "foo" cell — fast_path::strip_anchors removes
    // ^/$ before byte scanning to keep correctness; the outer matcher in
    // search_file still enforces the anchors against the full cell text.
    let dir = TempDir::new().unwrap();
    let p = dir.path().join("anchored.xlsx");
    let mut wb = Workbook::new();
    let s = wb.add_worksheet().set_name("S").unwrap();
    s.write_string(0, 0, "foo").unwrap();
    s.write_string(1, 0, "foo bar").unwrap();
    wb.save(&p).unwrap();
    let r = matches(&p, "^foo$", false);
    // Only A1 ("foo") matches; A2 ("foo bar") does not (the cell-level matcher
    // enforces ^/$ even though the byte scan dropped them).
    assert_eq!(r, vec!["foo".to_string()]);
    assert_eq!(r, matches(&p, "^foo$", true));
}

#[test]
fn preskip_sst_falls_back_safely_when_pattern_is_rare() {
    // Constructs a workbook with the SHAPE that triggers preskip
    // (large compressed sst, 3 sheets) but with a pattern that
    // matches only a single sst entry. The preskip path skips sst::parse
    // and routes through calamine; the disable_fast_path path also routes
    // through calamine. Both must yield the SAME match set — preskip must
    // not produce false negatives.
    use xgrep::reader::{read_cells, ReaderOptions};

    let dir = TempDir::new().unwrap();
    let path = dir.path().join("preskip_sparse.xlsx");

    // Build a workbook with a large sst (> 100KB compressed) and 3 sheets.
    // We achieve this by writing ~24,000 distinct strings across 3 sheets.
    // Only ONE string contains the target marker — the rest are unique noise.
    let mut wb = Workbook::new();
    let sheet_count = 3usize;
    let per_sheet_rows = 8_000usize;
    for sheet_idx in 0..sheet_count {
        let s = wb.add_worksheet().set_name(&format!("Sheet{}", sheet_idx + 1)).unwrap();
        for row in 0..per_sheet_rows {
            // Unique string per cell, only one match in total.
            let value = if sheet_idx == 1 && row == 4242 {
                "RARE-MATCH-MARKER".to_string()
            } else {
                format!("noise-{sheet_idx}-{row}-padding-to-bulk-up-sst")
            };
            s.write_string(row as u32, 0, &value).unwrap();
        }
    }
    wb.save(&path).unwrap();

    let pat = Pattern::compile("RARE-MATCH-MARKER", CaseMode::Sensitive, true, false).unwrap();

    // Fast-path ON (default) — should hit preskip given workbook shape.
    let opts_on = ReaderOptions {
        pattern: Some(&pat),
        ..ReaderOptions::default()
    };
    let cells_on = read_cells(&path, &opts_on).unwrap();
    let matches_on: Vec<_> = cells_on
        .iter()
        .filter(|c| c.text.contains("RARE-MATCH-MARKER"))
        .collect();

    // Fast-path OFF — baseline.
    let opts_off = ReaderOptions {
        pattern: Some(&pat),
        disable_fast_path: true,
        ..ReaderOptions::default()
    };
    let cells_off = read_cells(&path, &opts_off).unwrap();
    let matches_off: Vec<_> = cells_off
        .iter()
        .filter(|c| c.text.contains("RARE-MATCH-MARKER"))
        .collect();

    // Both paths must yield exactly one match at Sheet2 cell A4243.
    assert_eq!(matches_on.len(), 1, "fast-path on: expected 1 match, got {}", matches_on.len());
    assert_eq!(matches_off.len(), 1, "fast-path off: expected 1 match, got {}", matches_off.len());
    assert_eq!(matches_on[0].sheet, "Sheet2");
    assert_eq!(matches_off[0].sheet, "Sheet2");
    assert_eq!(matches_on[0].text, matches_off[0].text);
}
