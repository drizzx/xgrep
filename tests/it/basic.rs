use std::collections::HashSet;

use tempfile::TempDir;
use xgrep::config::Layer;
use xgrep::reader::{read_cells, ReaderOptions};

use super::common::write_basic_xlsx;

#[test]
fn reader_emits_display_values_for_strings_and_numbers() {
    let dir = TempDir::new().unwrap();
    let path = write_basic_xlsx(dir.path());

    let cells = read_cells(&path, &ReaderOptions::default()).unwrap();

    let got: HashSet<_> = cells
        .iter()
        .map(|c| (c.sheet.clone(), c.cell.clone(), c.layer, c.text.clone()))
        .collect();

    assert!(got.contains(&("Sheet1".into(), "A1".into(), Layer::Display, "name".into())));
    assert!(got.contains(&("Sheet1".into(), "A2".into(), Layer::Display, "张三".into())));
    assert!(got.contains(&("Sheet1".into(), "B2".into(), Layer::Display, "100".into())));
    assert!(got.contains(&("汇总".into(), "A1".into(), Layer::Display, "张三应收账款".into())));
}
