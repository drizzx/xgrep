//! Hidden-row / hidden-column detection by parsing the sheet xml's
//! `<row r="N" hidden="1">` and `<col min=".." max=".." hidden="1">` markers.

use std::collections::HashSet;

use crate::reader::zip_index::ZipIndex;

/// Returns (hidden_rows, hidden_cols), both 0-indexed.
/// Errors are swallowed (returns empty sets) — hidden detection is best-effort;
/// the worst case is including a row/col we should have hidden, which keeps
/// matches but doesn't lose them.
pub fn detect(
    index: &mut ZipIndex,
    sheet_xml_zip_path: &str,
) -> (HashSet<u32>, HashSet<u32>) {
    let mut hidden_rows = HashSet::new();
    let mut hidden_cols = HashSet::new();
    let xml = match index.read_to_string(sheet_xml_zip_path) {
        Ok(Some(s)) => s,
        _ => return (hidden_rows, hidden_cols),
    };
    let re_row = regex::Regex::new(r#"<row[^>]*r="(\d+)"[^>]*hidden="1""#).unwrap();
    for cap in re_row.captures_iter(&xml) {
        if let Ok(n) = cap[1].parse::<u32>() {
            hidden_rows.insert(n.saturating_sub(1));
        }
    }
    let re_col =
        regex::Regex::new(r#"<col[^>]*min="(\d+)"[^>]*max="(\d+)"[^>]*hidden="1""#).unwrap();
    for cap in re_col.captures_iter(&xml) {
        let lo: u32 = cap[1].parse().unwrap_or(1);
        let hi: u32 = cap[2].parse().unwrap_or(lo);
        for c in lo..=hi {
            hidden_cols.insert(c.saturating_sub(1));
        }
    }
    (hidden_rows, hidden_cols)
}
