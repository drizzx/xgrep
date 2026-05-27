use tempfile::TempDir;
use xgrep::config::Layer;
use xgrep::reader::{read_cells, ReaderOptions};

use super::common::write_formula_xlsx;

#[test]
fn cached_formula_value_is_emitted_as_cached_layer() {
    let dir = TempDir::new().unwrap();
    let path = write_formula_xlsx(dir.path());

    let cells = read_cells(&path, &ReaderOptions::default()).unwrap();

    let cached: Vec<_> = cells.iter()
        .filter(|c| c.layer == Layer::Cached)
        .collect();
    assert!(cached.iter().any(|c| c.cell == "B1" && c.text == "5"));

    // And no display record for that cell (formula cells emit only Cached/Formula).
    let display_b1: Vec<_> = cells.iter()
        .filter(|c| c.cell == "B1" && c.layer == Layer::Display)
        .collect();
    assert!(display_b1.is_empty(), "formula cell B1 should not also emit a display record");
}
