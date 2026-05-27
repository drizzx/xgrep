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

#[test]
fn sheet_glob_excludes_unmatched_sheets() {
    use globset::Glob;
    let dir = TempDir::new().unwrap();
    let path = write_basic_xlsx(dir.path());

    let opts = ReaderOptions {
        layers: xgrep::config::LayerSet::defaults(),
        include_hidden: true,
        sheet_filter: Some(Glob::new("汇总").unwrap().compile_matcher()),
    };
    let cells = read_cells(&path, &opts).unwrap();
    assert!(cells.iter().all(|c| c.sheet == "汇总"));
    assert!(!cells.is_empty());
}

#[test]
fn search_file_emits_file_block_with_matches() {
    use xgrep::{search_file, FileBlock, MatchEvent};
    use xgrep::config::LayerSet;
    use xgrep::matcher::{CaseMode, Pattern};

    let dir = TempDir::new().unwrap();
    let path = write_basic_xlsx(dir.path());

    let pat = Pattern::compile("张三", CaseMode::Smart, false, false).unwrap();
    let opts = ReaderOptions {
        layers: LayerSet::defaults(),
        include_hidden: true,
        sheet_filter: None,
    };
    let block: FileBlock = search_file(&path, &pat, &opts, false);

    let matches: Vec<_> = block.events.iter()
        .filter_map(|e| match e {
            MatchEvent::Match { sheet, cell, layer, text, .. } =>
                Some((sheet.clone(), cell.clone(), layer.clone(), text.clone())),
            _ => None,
        })
        .collect();

    assert!(matches.iter().any(|(s, c, _, _)| s == "Sheet1" && c == "A2"));
    assert!(matches.iter().any(|(s, c, _, _)| s == "汇总" && c == "A1"));
    assert!(matches!(block.events.first(), Some(MatchEvent::FileBegin { .. })));
    assert!(matches!(block.events.last(), Some(MatchEvent::FileEnd { .. })));
}
