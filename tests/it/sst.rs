use rust_xlsxwriter::Workbook;
use tempfile::TempDir;
use xgrep::matcher::{CaseMode, Pattern};
use xgrep::reader::sst::{build_hit_set, parse};
use xgrep::reader::zip_index::ZipIndex;

fn write_sst_demo(dir: &std::path::Path) -> std::path::PathBuf {
    let p = dir.join("sst.xlsx");
    let mut wb = Workbook::new();
    let s = wb.add_worksheet().set_name("Sheet1").unwrap();
    s.write_string(0, 0, "alpha").unwrap();
    s.write_string(1, 0, "beta").unwrap();
    s.write_string(2, 0, "alphabet").unwrap();
    s.write_string(3, 0, "alpha").unwrap(); // dedup
    wb.save(&p).unwrap();
    p
}

#[test]
fn parse_returns_unique_strings_in_workbook_order() {
    let dir = TempDir::new().unwrap();
    let path = write_sst_demo(dir.path());
    let mut idx = ZipIndex::open(&path).unwrap();
    let sst = parse(&mut idx).unwrap();
    // rust_xlsxwriter dedups, so 4 writes -> 3 entries (alpha,beta,alphabet).
    assert!(sst.contains(&"alpha".to_string()));
    assert!(sst.contains(&"beta".to_string()));
    assert!(sst.contains(&"alphabet".to_string()));
    assert_eq!(sst.len(), 3);
}

#[test]
fn hit_set_with_alpha_pattern_marks_alpha_and_alphabet() {
    let dir = TempDir::new().unwrap();
    let path = write_sst_demo(dir.path());
    let mut idx = ZipIndex::open(&path).unwrap();
    let sst = parse(&mut idx).unwrap();
    let pat = Pattern::compile("alpha", CaseMode::Sensitive, false, false).unwrap();
    let hs = build_hit_set(&sst, Some(&pat));
    assert_eq!(hs.count(), 2);
}
