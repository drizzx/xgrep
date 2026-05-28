//! End-to-end context-line tests: `-A`, `-B`, `-C`, separator behavior,
//! cross-sheet isolation, and CLI flag validation.

use std::fs;
use std::io::Write;
use std::path::Path;
use std::process::Command;

use rust_xlsxwriter::Workbook;
use tempfile::TempDir;

use xgrep::config::{ColorChoice, OutputMode};
use xgrep::matcher::{CaseMode, Pattern};
use xgrep::printer::print_block;
use xgrep::reader::ReaderOptions;
use xgrep::{search_file, ContextOptions, FileBlock, MatchEvent, Submatch, SubmatchText};

fn write_bytes(path: &Path, bytes: &[u8]) {
    let mut f = fs::File::create(path).unwrap();
    f.write_all(bytes).unwrap();
}

fn search_with_ctx(path: &Path, pat: &str, ctx: ContextOptions) -> Vec<MatchEvent> {
    let pattern = Pattern::compile(pat, CaseMode::Sensitive, false, false).unwrap();
    let block = search_file(path, &pattern, &ReaderOptions::default(), false, ctx);
    block.events
}

#[test]
fn a_after_csv() {
    let dir = TempDir::new().unwrap();
    let p = dir.path().join("a.csv");
    write_bytes(&p, b"row1\nTARGET\nrow3\nrow4\nrow5\n");
    let events = search_with_ctx(&p, "TARGET", ContextOptions { before: 0, after: 2 });
    let cells: Vec<String> = events
        .iter()
        .filter_map(|e| match e {
            MatchEvent::Match { cell, .. } => Some(cell.clone()),
            MatchEvent::Context { cell, .. } => Some(cell.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(cells, vec!["A2", "A3", "A4"]);
}

#[test]
fn b_before_csv() {
    let dir = TempDir::new().unwrap();
    let p = dir.path().join("b.csv");
    write_bytes(&p, b"row1\nrow2\nrow3\nTARGET\nrow5\n");
    let events = search_with_ctx(&p, "TARGET", ContextOptions { before: 2, after: 0 });
    let cells: Vec<String> = events
        .iter()
        .filter_map(|e| match e {
            MatchEvent::Match { cell, .. } => Some(cell.clone()),
            MatchEvent::Context { cell, .. } => Some(cell.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(cells, vec!["A2", "A3", "A4"]);
}

#[test]
fn c_disjoint_spans_get_separator() {
    let dir = TempDir::new().unwrap();
    let p = dir.path().join("c.csv");
    let mut data = String::new();
    for i in 1..=20 {
        if i == 5 || i == 15 {
            data.push_str("TARGET\n");
        } else {
            data.push_str(&format!("row{i}\n"));
        }
    }
    write_bytes(&p, data.as_bytes());
    let events = search_with_ctx(&p, "TARGET", ContextOptions { before: 1, after: 1 });
    let sep_count = events
        .iter()
        .filter(|e| matches!(e, MatchEvent::Separator))
        .count();
    assert_eq!(sep_count, 1, "expected one Separator between disjoint spans");
}

#[test]
fn xlsx_a_within_same_sheet_only() {
    let dir = TempDir::new().unwrap();
    let p = dir.path().join("two_sheets.xlsx");
    let mut wb = Workbook::new();
    let s1 = wb.add_worksheet().set_name("S1").unwrap();
    s1.write_string(0, 0, "row1").unwrap();
    s1.write_string(1, 0, "row2").unwrap();
    s1.write_string(2, 0, "TARGET").unwrap();
    s1.write_string(3, 0, "row4").unwrap();
    s1.write_string(4, 0, "row5").unwrap();
    let s2 = wb.add_worksheet().set_name("S2").unwrap();
    s2.write_string(0, 0, "x1").unwrap();
    s2.write_string(1, 0, "x2").unwrap();
    s2.write_string(2, 0, "TARGET").unwrap();
    s2.write_string(3, 0, "x4").unwrap();
    s2.write_string(4, 0, "x5").unwrap();
    wb.save(&p).unwrap();

    let events = search_with_ctx(&p, "TARGET", ContextOptions { before: 0, after: 2 });
    let kinds: Vec<&str> = events
        .iter()
        .filter_map(|e| match e {
            MatchEvent::Match { .. } => Some("M"),
            MatchEvent::Context { .. } => Some("C"),
            MatchEvent::Separator => Some("S"),
            _ => None,
        })
        .collect();
    assert_eq!(kinds, vec!["M", "C", "C", "S", "M", "C", "C"]);
}

#[test]
fn json_event_shapes_for_context_and_separator() {
    let dir = TempDir::new().unwrap();
    let p = dir.path().join("j.csv");
    write_bytes(&p, b"row1\nTARGET\nrow3\nrow4\n");
    let events = search_with_ctx(&p, "TARGET", ContextOptions { before: 0, after: 1 });
    let context_event = events
        .iter()
        .find(|e| matches!(e, MatchEvent::Context { .. }))
        .expect("expected a Context event");
    let json = serde_json::to_string(context_event).unwrap();
    assert!(
        json.contains(r#""type":"context""#),
        "expected serialized Context to include type:\"context\"; got {json}"
    );
    assert!(json.contains(r#""cell":"A3""#));
    assert!(json.contains(r#""text":"row3""#));
}

#[test]
fn printer_context_surfaces_non_display_layer_tag() {
    use std::path::PathBuf;
    let block = FileBlock {
        events: vec![
            MatchEvent::FileBegin { path: PathBuf::from("x.xlsx") },
            MatchEvent::Match {
                path: PathBuf::from("x.xlsx"),
                sheet: "S1".to_string(),
                cell: "A2".to_string(),
                layer: "display".to_string(),
                text: "TARGET".to_string(),
                submatches: vec![Submatch {
                    matched: SubmatchText { text: "TARGET".to_string() },
                    start: 0,
                    end: 6,
                }],
            },
            MatchEvent::Context {
                path: PathBuf::from("x.xlsx"),
                sheet: "S1".to_string(),
                cell: "A3".to_string(),
                layer: "formula".to_string(),
                text: "=SUM(B1:B2)".to_string(),
            },
            MatchEvent::Context {
                path: PathBuf::from("x.xlsx"),
                sheet: "S1".to_string(),
                cell: "A4".to_string(),
                layer: "display".to_string(),
                text: "plain".to_string(),
            },
        ],
    };
    let mut buf: Vec<u8> = Vec::new();
    print_block(&block, &mut buf, OutputMode::Pretty, ColorChoice::Never, false).unwrap();
    let out = String::from_utf8(buf).unwrap();
    assert!(
        out.contains("=SUM(B1:B2) [context] [formula]"),
        "expected formula context row to carry both [context] and [formula] tags; got:\n{out}"
    );
    assert!(
        out.contains("plain [context]\n"),
        "display-layer context row must NOT carry a [layer] tag; got:\n{out}"
    );
    assert!(
        !out.contains("plain [context] [display]"),
        "display layer should be hidden by default; got:\n{out}"
    );
}

// ---- CLI validation tests ----

fn xgrep_bin() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_BIN_EXE_xgrep"))
}

#[test]
fn cli_e_unknown_encoding_exits_2() {
    let dir = TempDir::new().unwrap();
    let p = dir.path().join("any.csv");
    write_bytes(&p, b"a,b\n");
    let out = Command::new(xgrep_bin())
        .args(["foo", "-E", "definitely-not-an-encoding-xyz", p.to_str().unwrap()])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.to_lowercase().contains("unknown encoding"),
        "stderr should mention unknown encoding; got: {stderr}"
    );
}

#[test]
fn cli_a_overlimit_exits_2() {
    let dir = TempDir::new().unwrap();
    let p = dir.path().join("any.csv");
    write_bytes(&p, b"a,b\n");
    let out = Command::new(xgrep_bin())
        .args(["foo", "-A", "100000", p.to_str().unwrap()])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
}

/// `-C` combined with `-A`/`-B` should take the `max()` per side (rg-aligned),
/// not be silently dropped when either of A/B is non-zero.
#[test]
fn cli_c_combined_with_a_takes_max() {
    let dir = TempDir::new().unwrap();
    let p = dir.path().join("ctxmax.csv");
    // 30 rows; match on row 11 ("TARGET"). Everything else is "rowN".
    let mut data = String::new();
    for i in 1..=30 {
        if i == 11 {
            data.push_str("TARGET\n");
        } else {
            data.push_str(&format!("row{i}\n"));
        }
    }
    write_bytes(&p, data.as_bytes());

    // Case 1: -C 5 -A 10 → after = max(10, 5) = 10
    let out = Command::new(xgrep_bin())
        .args(["TARGET", "-C", "5", "-A", "10", "--json", p.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(out.status.success() || out.status.code() == Some(0));
    let stdout = String::from_utf8_lossy(&out.stdout);
    let context_count = stdout.matches(r#""type":"context""#).count();
    // Expected: 5 before (rows 6..=10) + 10 after (rows 12..=21) = 15
    assert_eq!(
        context_count, 15,
        "-C 5 -A 10 should yield 5 before + 10 after = 15 context events; got {context_count}.\nstdout:\n{stdout}"
    );

    // Case 2: -C 10 -A 3 → after = max(3, 10) = 10
    let out = Command::new(xgrep_bin())
        .args(["TARGET", "-C", "10", "-A", "3", "--json", p.to_str().unwrap()])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    let context_count = stdout.matches(r#""type":"context""#).count();
    // Expected: 10 before (rows 1..=10) + 10 after (rows 12..=21) = 20
    assert_eq!(
        context_count, 20,
        "-C 10 -A 3 should yield 10 before + 10 after = 20 context events; got {context_count}.\nstdout:\n{stdout}"
    );
}

#[test]
fn cli_a_within_limit_runs() {
    let dir = TempDir::new().unwrap();
    let p = dir.path().join("any.csv");
    write_bytes(&p, b"a,b\nTARGET,d\ne,f\n");
    let out = Command::new(xgrep_bin())
        .args(["TARGET", "-A", "9999", p.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(out.status.code() != Some(2));
}
