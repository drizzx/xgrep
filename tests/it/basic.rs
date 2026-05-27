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
    assert!(got.contains(&(
        "汇总".into(),
        "A1".into(),
        Layer::Display,
        "张三应收账款".into()
    )));
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
    use xgrep::config::LayerSet;
    use xgrep::matcher::{CaseMode, Pattern};
    use xgrep::{search_file, FileBlock, MatchEvent};

    let dir = TempDir::new().unwrap();
    let path = write_basic_xlsx(dir.path());

    let pat = Pattern::compile("张三", CaseMode::Smart, false, false).unwrap();
    let opts = ReaderOptions {
        layers: LayerSet::defaults(),
        include_hidden: true,
        sheet_filter: None,
    };
    let block: FileBlock = search_file(&path, &pat, &opts, false);

    let matches: Vec<_> = block
        .events
        .iter()
        .filter_map(|e| match e {
            MatchEvent::Match {
                sheet,
                cell,
                layer,
                text,
                ..
            } => Some((sheet.clone(), cell.clone(), layer.clone(), text.clone())),
            _ => None,
        })
        .collect();

    assert!(matches
        .iter()
        .any(|(s, c, _, _)| s == "Sheet1" && c == "A2"));
    assert!(matches.iter().any(|(s, c, _, _)| s == "汇总" && c == "A1"));
    assert!(matches!(
        block.events.first(),
        Some(MatchEvent::FileBegin { .. })
    ));
    assert!(matches!(
        block.events.last(),
        Some(MatchEvent::FileEnd { .. })
    ));
}

#[test]
fn worker_runs_files_in_parallel_and_emits_one_block_per_file() {
    use std::fs;
    use xgrep::config::LayerSet;
    use xgrep::matcher::{CaseMode, Pattern};
    use xgrep::worker::run_search;

    let dir = TempDir::new().unwrap();
    let mut paths = Vec::new();
    for i in 0..5 {
        let sub = dir.path().join(format!("d{i}"));
        fs::create_dir_all(&sub).unwrap();
        paths.push(write_basic_xlsx(&sub));
    }

    let pat = Pattern::compile("张三", CaseMode::Smart, false, false).unwrap();
    let opts = ReaderOptions {
        layers: LayerSet::defaults(),
        include_hidden: true,
        sheet_filter: None,
    };
    let blocks = run_search(paths.clone(), &pat, &opts, false, 4);

    assert_eq!(blocks.len(), paths.len());
    for b in &blocks {
        assert!(matches!(
            b.events.first(),
            Some(xgrep::MatchEvent::FileBegin { .. })
        ));
        assert!(matches!(
            b.events.last(),
            Some(xgrep::MatchEvent::FileEnd { .. })
        ));
    }
}

#[test]
fn walker_finds_xlsx_files_recursively_and_skips_others() {
    use std::fs;
    use xgrep::walker::walk_xlsx;

    let dir = TempDir::new().unwrap();
    let sub = dir.path().join("nested");
    fs::create_dir_all(&sub).unwrap();
    write_basic_xlsx(dir.path());
    write_basic_xlsx(&sub);
    fs::write(dir.path().join("note.txt"), "ignore me").unwrap();
    fs::write(dir.path().join("data.csv"), "ignore,me").unwrap();

    let mut found: Vec<_> = walk_xlsx(&[dir.path().to_path_buf()], None).unwrap();
    found.sort();
    assert_eq!(found.len(), 2);
    assert!(found
        .iter()
        .all(|p| p.extension().map(|e| e == "xlsx").unwrap_or(false)));
}
