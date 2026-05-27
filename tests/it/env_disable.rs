//! XGREP_DISABLE_FAST_PATH env var must produce identical output to the
//! default code path — it just forces the v0.1 reader path internally.

use assert_cmd::Command;
use rust_xlsxwriter::Workbook;
use tempfile::TempDir;

#[test]
fn env_disable_fast_path_produces_same_output() {
    let dir = TempDir::new().unwrap();
    let p = dir.path().join("envcheck.xlsx");
    let mut wb = Workbook::new();
    let s = wb.add_worksheet().set_name("Sheet1").unwrap();
    s.write_string(0, 0, "needle").unwrap();
    s.write_string(1, 0, "haystack").unwrap();
    wb.save(&p).unwrap();

    let with_fast = Command::cargo_bin("xgrep")
        .unwrap()
        .args(["needle", p.to_str().unwrap()])
        .output()
        .unwrap();
    let without_fast = Command::cargo_bin("xgrep")
        .unwrap()
        .env("XGREP_DISABLE_FAST_PATH", "1")
        .args(["needle", p.to_str().unwrap()])
        .output()
        .unwrap();

    assert_eq!(with_fast.stdout, without_fast.stdout);
    assert_eq!(with_fast.status.code(), without_fast.status.code());
}
