use tempfile::TempDir;
use xgrep::config::LayerSet;
use xgrep::reader::{read_cells, ReaderOptions};

use super::common::write_hidden_xlsx;

#[test]
fn hidden_sheet_included_by_default() {
    let dir = TempDir::new().unwrap();
    let path = write_hidden_xlsx(dir.path());
    let cells = read_cells(&path, &ReaderOptions::default()).unwrap();
    assert!(cells.iter().any(|c| c.text == "hidden-sheet-marker"));
    assert!(cells.iter().any(|c| c.text == "hidden-row-marker"));
    assert!(cells.iter().any(|c| c.text == "hidden-col-marker"));
}

#[test]
fn hidden_sheet_excluded_when_no_hidden() {
    let dir = TempDir::new().unwrap();
    let path = write_hidden_xlsx(dir.path());
    let opts = ReaderOptions {
        layers: LayerSet::defaults(),
        include_hidden: false,
        sheet_filter: None,
    };
    let cells = read_cells(&path, &opts).unwrap();
    assert!(!cells.iter().any(|c| c.text == "hidden-sheet-marker"));
}

#[test]
fn hidden_row_and_col_excluded_when_no_hidden() {
    let dir = TempDir::new().unwrap();
    let path = write_hidden_xlsx(dir.path());
    let opts = ReaderOptions {
        layers: LayerSet::defaults(),
        include_hidden: false,
        sheet_filter: None,
    };
    let cells = read_cells(&path, &opts).unwrap();
    assert!(!cells.iter().any(|c| c.text == "hidden-row-marker"));
    assert!(!cells.iter().any(|c| c.text == "hidden-col-marker"));
}
