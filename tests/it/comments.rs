use tempfile::TempDir;
use xgrep::config::Layer;
use xgrep::reader::{read_cells, ReaderOptions};

use super::common::write_comment_xlsx;

#[test]
fn comment_text_is_emitted_as_comment_layer_on_target_cell() {
    let dir = TempDir::new().unwrap();
    let path = write_comment_xlsx(dir.path());

    let cells = read_cells(&path, &ReaderOptions::default()).unwrap();

    let comments: Vec<_> = cells.iter().filter(|c| c.layer == Layer::Comment).collect();
    assert!(
        comments
            .iter()
            .any(|c| c.cell == "B2" && c.text.contains("张三留言")),
        "expected a comment record on B2 containing 张三留言; got {:?}",
        comments,
    );
}
