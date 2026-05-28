use tempfile::TempDir;
use xgrep::config::{ColorChoice, OutputMode};
use xgrep::matcher::{CaseMode, Pattern};
use xgrep::printer::print_block;
use xgrep::reader::ReaderOptions;
use xgrep::{search_file, FileBlock};

use super::common::write_basic_xlsx;

fn search_basic(pattern: &str) -> FileBlock {
    let dir = TempDir::new().unwrap();
    let path = write_basic_xlsx(dir.path());
    let pat = Pattern::compile(pattern, CaseMode::Smart, false, false).unwrap();
    search_file(&path, &pat, &ReaderOptions::default(), false, xgrep::ContextOptions::default())
}

#[test]
fn pretty_output_groups_under_filename_with_sheet_cell_prefix() {
    let block = search_basic("张三");
    let mut buf = Vec::new();
    print_block(
        &block,
        &mut buf,
        OutputMode::Pretty,
        ColorChoice::Never,
        false,
    )
    .unwrap();
    let out = String::from_utf8(buf).unwrap();

    assert!(out.contains("basic.xlsx\n"), "path header missing: {out}");
    assert!(
        out.contains("Sheet1!A2:1: 张三"),
        "sheet/cell prefix missing: {out}"
    );
}

#[test]
fn json_output_emits_ndjson_events() {
    let block = search_basic("张三");
    let mut buf = Vec::new();
    print_block(
        &block,
        &mut buf,
        OutputMode::Json,
        ColorChoice::Never,
        false,
    )
    .unwrap();
    let out = String::from_utf8(buf).unwrap();

    let lines: Vec<_> = out.lines().collect();
    assert!(lines.first().unwrap().contains("\"type\":\"begin\""));
    assert!(lines.iter().any(|l| l.contains("\"type\":\"match\"")));
    assert!(lines.last().unwrap().contains("\"type\":\"end\""));

    for l in lines {
        let _: serde_json::Value = serde_json::from_str(l).expect(l);
    }
}

#[test]
fn count_output_emits_path_count_line() {
    let block = search_basic("张三");
    let mut buf = Vec::new();
    print_block(
        &block,
        &mut buf,
        OutputMode::CountOnly,
        ColorChoice::Never,
        false,
    )
    .unwrap();
    let out = String::from_utf8(buf).unwrap();
    assert!(
        out.trim().ends_with(":2"),
        "expected count of 2, got {out:?}"
    );
}

#[test]
fn files_with_matches_emits_path_only_for_matched_files() {
    let block = search_basic("张三");
    let mut buf = Vec::new();
    print_block(
        &block,
        &mut buf,
        OutputMode::FilesOnly,
        ColorChoice::Never,
        false,
    )
    .unwrap();
    let out = String::from_utf8(buf).unwrap();
    assert!(out.trim().ends_with("basic.xlsx"));
    assert!(!out.contains("Sheet1"));
}

#[test]
fn pretty_output_layer_tag_omitted_for_display_unless_forced() {
    let block = search_basic("张三");
    let mut buf = Vec::new();
    print_block(
        &block,
        &mut buf,
        OutputMode::Pretty,
        ColorChoice::Never,
        false,
    )
    .unwrap();
    let s = String::from_utf8(buf).unwrap();
    assert!(!s.contains("[display]"));

    let mut buf2 = Vec::new();
    print_block(
        &block,
        &mut buf2,
        OutputMode::Pretty,
        ColorChoice::Never,
        true,
    )
    .unwrap();
    let s2 = String::from_utf8(buf2).unwrap();
    assert!(s2.contains("[display]"));
}

#[test]
fn pretty_output_emits_ansi_when_color_always() {
    let block = search_basic("张三");
    let mut buf = Vec::new();
    print_block(
        &block,
        &mut buf,
        OutputMode::Pretty,
        ColorChoice::Always,
        false,
    )
    .unwrap();
    let out = String::from_utf8(buf).unwrap();
    assert!(
        out.contains('\x1b'),
        "expected ANSI escape in colored output"
    );
}

#[test]
fn pretty_output_omits_ansi_when_color_never() {
    let block = search_basic("张三");
    let mut buf = Vec::new();
    print_block(
        &block,
        &mut buf,
        OutputMode::Pretty,
        ColorChoice::Never,
        false,
    )
    .unwrap();
    let out = String::from_utf8(buf).unwrap();
    assert!(!out.contains('\x1b'));
}

use super::common::run_xgrep;

#[test]
fn cli_finds_basic_match_and_exits_zero() {
    let dir = TempDir::new().unwrap();
    write_basic_xlsx(dir.path());
    let (stdout, _, code) = run_xgrep(dir.path(), &["张三", "--color", "never"]);
    assert_eq!(code, 0, "expected match → exit 0");
    assert!(stdout.contains("basic.xlsx"));
    assert!(stdout.contains("Sheet1!A2"));
}

#[test]
fn cli_no_match_exits_one() {
    let dir = TempDir::new().unwrap();
    write_basic_xlsx(dir.path());
    let (_, _, code) = run_xgrep(dir.path(), &["nonexistent-needle-XYZ", "--color", "never"]);
    assert_eq!(code, 1);
}

#[test]
fn cli_invalid_regex_exits_two() {
    let dir = TempDir::new().unwrap();
    write_basic_xlsx(dir.path());
    let (_, stderr, code) = run_xgrep(dir.path(), &["a("]);
    assert_eq!(code, 2);
    assert!(stderr.contains("invalid regex"));
}

#[test]
fn cli_count_flag_prints_path_count() {
    let dir = TempDir::new().unwrap();
    write_basic_xlsx(dir.path());
    let (stdout, _, code) = run_xgrep(dir.path(), &["-c", "张三", "--color", "never"]);
    assert_eq!(code, 0);
    assert!(stdout.trim().ends_with(":2"), "got {stdout:?}");
}

#[test]
fn cli_files_with_matches_prints_path_only() {
    let dir = TempDir::new().unwrap();
    write_basic_xlsx(dir.path());
    let (stdout, _, code) = run_xgrep(dir.path(), &["-l", "张三", "--color", "never"]);
    assert_eq!(code, 0);
    let trimmed = stdout.trim();
    assert!(trimmed.ends_with("basic.xlsx"));
    assert!(!trimmed.contains("Sheet1"));
}

#[test]
fn cli_json_flag_emits_valid_ndjson() {
    let dir = TempDir::new().unwrap();
    write_basic_xlsx(dir.path());
    let (stdout, _, code) = run_xgrep(dir.path(), &["--json", "张三"]);
    assert_eq!(code, 0);
    for line in stdout.lines() {
        let _: serde_json::Value = serde_json::from_str(line).expect(line);
    }
}

#[test]
fn cli_formula_flag_includes_formula_text() {
    let dir = TempDir::new().unwrap();
    let _ = super::common::write_formula_xlsx(dir.path());
    let (stdout, _, code) = run_xgrep(dir.path(), &["--formula", "SUM", "--color", "never"]);
    assert_eq!(code, 0);
    assert!(stdout.contains("[formula]"));
    assert!(stdout.contains("SUM"));
}
