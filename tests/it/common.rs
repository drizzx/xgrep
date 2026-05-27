//! Shared helpers for integration tests.
//!
//! - `write_basic_xlsx(dir)` builds a small workbook used by many tests.
//! - All fixtures live in tempdirs unless they are committed binaries
//!   (encrypted/corrupt — see tests/fixtures/).

#![allow(dead_code)] // helpers are picked up incrementally by later task tests

use std::path::{Path, PathBuf};

use rust_xlsxwriter::{ExcelDateTime, Format, Workbook};

/// Build a small workbook at `dir/basic.xlsx` and return its path.
pub fn write_basic_xlsx(dir: &Path) -> PathBuf {
    let path = dir.join("basic.xlsx");
    let mut wb = Workbook::new();
    let s1 = wb.add_worksheet().set_name("Sheet1").unwrap();
    s1.write_string(0, 0, "name").unwrap();
    s1.write_string(0, 1, "amount").unwrap();
    s1.write_string(1, 0, "张三").unwrap();
    s1.write_number(1, 1, 100.0).unwrap();
    s1.write_string(2, 0, "李四").unwrap();
    s1.write_number(2, 1, 250.0).unwrap();

    let s2 = wb.add_worksheet().set_name("汇总").unwrap();
    s2.write_string(0, 0, "张三应收账款").unwrap();
    s2.write_formula(0, 1, "=100+250").unwrap();

    wb.save(&path).unwrap();
    path
}

/// Workbook with a formula whose cached value is the search target.
///
/// Note: `write_formula_with_result` does not exist in rust_xlsxwriter 0.79.
/// Instead we write the formula then call `set_formula_result` on the cell.
pub fn write_formula_xlsx(dir: &Path) -> PathBuf {
    let path = dir.join("formula.xlsx");
    let mut wb = Workbook::new();
    let s = wb.add_worksheet().set_name("Sheet1").unwrap();
    s.write_string(0, 0, "total").unwrap();
    s.write_formula(0, 1, "=SUM(2,3)").unwrap();
    s.set_formula_result(0, 1, "5");
    wb.save(&path).unwrap();
    path
}

/// Workbook with a hidden sheet, hidden row, and hidden column.
///
/// Note: `set_hidden()` in rust_xlsxwriter 0.79 takes a bool argument: `set_hidden(true)`.
pub fn write_hidden_xlsx(dir: &Path) -> PathBuf {
    let path = dir.join("hidden.xlsx");
    let mut wb = Workbook::new();
    let visible = wb.add_worksheet().set_name("Visible").unwrap();
    visible.write_string(0, 0, "open-marker").unwrap();
    visible.set_row_hidden(1).unwrap();
    visible.write_string(1, 0, "hidden-row-marker").unwrap();
    visible.set_column_hidden(2).unwrap();
    visible.write_string(0, 2, "hidden-col-marker").unwrap();

    let hidden_sheet = wb.add_worksheet().set_name("HiddenSheet").unwrap();
    hidden_sheet.write_string(0, 0, "hidden-sheet-marker").unwrap();
    hidden_sheet.set_hidden(true);
    wb.save(&path).unwrap();
    path
}

/// Workbook with a cell note on Sheet1!B2.
pub fn write_comment_xlsx(dir: &Path) -> PathBuf {
    let path = dir.join("comments.xlsx");
    let mut wb = Workbook::new();
    let s = wb.add_worksheet().set_name("Sheet1").unwrap();
    s.write_string(1, 1, "data").unwrap();
    let note = rust_xlsxwriter::Note::new("张三留言: please verify");
    s.insert_note(1, 1, &note).unwrap();
    wb.save(&path).unwrap();
    path
}

pub fn write_empty_xlsx(dir: &Path) -> PathBuf {
    let path = dir.join("empty.xlsx");
    let mut wb = Workbook::new();
    wb.add_worksheet().set_name("Sheet1").unwrap();
    wb.save(&path).unwrap();
    path
}

pub fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join(name)
}

/// Workbook with a typed date cell (rust_xlsxwriter writes the serial number
/// plus a date format string, which is exactly what real Excel does).
pub fn write_dates_xlsx(dir: &Path) -> PathBuf {
    let path = dir.join("dates.xlsx");
    let mut wb = Workbook::new();
    let s = wb.add_worksheet().set_name("Sheet1").unwrap();
    let fmt = Format::new().set_num_format("yyyy-mm-dd");
    let dt = ExcelDateTime::from_ymd(2024, 5, 27).unwrap();
    s.write_datetime_with_format(0, 0, &dt, &fmt).unwrap();
    wb.save(&path).unwrap();
    path
}

/// Workbook with a rich-text cell that has two runs ("张三" + "应收账款").
pub fn write_richtext_xlsx(dir: &Path) -> PathBuf {
    let path = dir.join("richtext.xlsx");
    let mut wb = Workbook::new();
    let s = wb.add_worksheet().set_name("Sheet1").unwrap();
    let red = Format::new().set_font_color("red");
    s.write_rich_string(
        0, 0,
        &[(&red, "张三"), (&Format::default(), "应收账款")],
    ).unwrap();
    wb.save(&path).unwrap();
    path
}

/// Workbook with a merged 2x2 range A1:B2 containing "merged-anchor".
pub fn write_merged_xlsx(dir: &Path) -> PathBuf {
    let path = dir.join("merged.xlsx");
    let mut wb = Workbook::new();
    let s = wb.add_worksheet().set_name("Sheet1").unwrap();
    s.merge_range(0, 0, 1, 1, "merged-anchor", &Format::default()).unwrap();
    wb.save(&path).unwrap();
    path
}

use assert_cmd::Command;

/// Run the compiled `xgrep` binary with the given args inside `cwd`.
/// Returns (stdout, stderr, exit_code).
pub fn run_xgrep(cwd: &Path, args: &[&str]) -> (String, String, i32) {
    let assert = Command::cargo_bin("xgrep").unwrap()
        .current_dir(cwd)
        .args(args)
        .env("NO_COLOR", "1")
        .assert();
    let out = assert.get_output();
    (
        String::from_utf8_lossy(&out.stdout).to_string(),
        String::from_utf8_lossy(&out.stderr).to_string(),
        out.status.code().unwrap_or(-1),
    )
}
