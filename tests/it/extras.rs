use tempfile::TempDir;
use xgrep::reader::{read_cells, ReaderOptions};

use super::common::{write_dates_xlsx, write_merged_xlsx, write_richtext_xlsx};

#[test]
fn date_cell_renders_to_searchable_text() {
    let dir = TempDir::new().unwrap();
    let path = write_dates_xlsx(dir.path());
    let cells = read_cells(&path, &ReaderOptions::default()).unwrap();
    // The display value should contain "2024" — exact format may vary across
    // calamine versions, but the year is stable.
    assert!(
        cells.iter().any(|c| c.text.contains("2024")),
        "expected a cell text containing 2024; got {cells:?}"
    );
}

#[test]
fn richtext_cell_concatenates_runs_into_single_text() {
    let dir = TempDir::new().unwrap();
    let path = write_richtext_xlsx(dir.path());
    let cells = read_cells(&path, &ReaderOptions::default()).unwrap();
    let a1 = cells.iter().find(|c| c.sheet == "Sheet1" && c.cell == "A1").unwrap();
    assert!(a1.text.contains("张三"));
    assert!(a1.text.contains("应收账款"));
}

#[test]
fn merged_range_emits_only_anchor_cell() {
    let dir = TempDir::new().unwrap();
    let path = write_merged_xlsx(dir.path());
    let cells = read_cells(&path, &ReaderOptions::default()).unwrap();
    let with_value: Vec<_> = cells.iter()
        .filter(|c| c.sheet == "Sheet1" && c.text == "merged-anchor")
        .collect();
    assert_eq!(with_value.len(), 1, "expected exactly one anchor cell, got {with_value:?}");
    assert_eq!(with_value[0].cell, "A1");
}
