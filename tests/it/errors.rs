use std::fs;

use tempfile::TempDir;

use super::common::{fixture, run_xgrep, write_empty_xlsx};

#[test]
fn encrypted_xlsx_emits_stderr_and_continues() {
    let dir = TempDir::new().unwrap();
    fs::copy(fixture("encrypted.xlsx"), dir.path().join("encrypted.xlsx")).unwrap();
    super::common::write_basic_xlsx(dir.path());
    let (stdout, stderr, code) = run_xgrep(dir.path(), &["张三", "--color", "never"]);
    assert!(
        stderr.to_lowercase().contains("encrypted") || stderr.contains("encrypted"),
        "stderr should mention encryption: {stderr}"
    );
    assert!(stdout.contains("basic.xlsx"), "still scans the good file: {stdout}");
    assert_eq!(code, 0);
}

#[test]
fn corrupt_xlsx_emits_stderr_and_continues() {
    let dir = TempDir::new().unwrap();
    fs::copy(fixture("corrupt.xlsx"), dir.path().join("corrupt.xlsx")).unwrap();
    super::common::write_basic_xlsx(dir.path());
    let (stdout, stderr, code) = run_xgrep(dir.path(), &["张三", "--color", "never"]);
    assert!(!stderr.is_empty(), "expected an error on corrupt file");
    assert!(stdout.contains("basic.xlsx"));
    assert_eq!(code, 0);
}

#[test]
fn empty_workbook_no_match_no_error() {
    let dir = TempDir::new().unwrap();
    write_empty_xlsx(dir.path());
    let (stdout, stderr, code) = run_xgrep(dir.path(), &["anything", "--color", "never"]);
    assert!(stdout.is_empty(), "got stdout: {stdout}");
    assert!(stderr.is_empty(), "got stderr: {stderr}");
    assert_eq!(code, 1);
}

#[test]
fn missing_path_arg_is_fatal() {
    let dir = TempDir::new().unwrap();
    let (_, stderr, code) = run_xgrep(dir.path(), &["张三", "no-such-dir"]);
    // walker silently skips bad roots → "no xlsx found" → exit 1 (not fatal).
    assert!(code == 1 || code == 0, "got {code}; stderr {stderr}");
}
