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
    search_file(&path, &pat, &ReaderOptions::default(), false)
}

#[test]
fn pretty_output_groups_under_filename_with_sheet_cell_prefix() {
    let block = search_basic("张三");
    let mut buf = Vec::new();
    print_block(&block, &mut buf, OutputMode::Pretty, ColorChoice::Never, false).unwrap();
    let out = String::from_utf8(buf).unwrap();

    assert!(out.contains("basic.xlsx\n"), "path header missing: {out}");
    assert!(out.contains("Sheet1!A2:1: 张三"), "sheet/cell prefix missing: {out}");
}

#[test]
fn json_output_emits_ndjson_events() {
    let block = search_basic("张三");
    let mut buf = Vec::new();
    print_block(&block, &mut buf, OutputMode::Json, ColorChoice::Never, false).unwrap();
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
    print_block(&block, &mut buf, OutputMode::CountOnly, ColorChoice::Never, false).unwrap();
    let out = String::from_utf8(buf).unwrap();
    assert!(out.trim().ends_with(":2"), "expected count of 2, got {out:?}");
}

#[test]
fn files_with_matches_emits_path_only_for_matched_files() {
    let block = search_basic("张三");
    let mut buf = Vec::new();
    print_block(&block, &mut buf, OutputMode::FilesOnly, ColorChoice::Never, false).unwrap();
    let out = String::from_utf8(buf).unwrap();
    assert!(out.trim().ends_with("basic.xlsx"));
    assert!(!out.contains("Sheet1"));
}

#[test]
fn pretty_output_layer_tag_omitted_for_display_unless_forced() {
    let block = search_basic("张三");
    let mut buf = Vec::new();
    print_block(&block, &mut buf, OutputMode::Pretty, ColorChoice::Never, false).unwrap();
    let s = String::from_utf8(buf).unwrap();
    assert!(!s.contains("[display]"));

    let mut buf2 = Vec::new();
    print_block(&block, &mut buf2, OutputMode::Pretty, ColorChoice::Never, true).unwrap();
    let s2 = String::from_utf8(buf2).unwrap();
    assert!(s2.contains("[display]"));
}
