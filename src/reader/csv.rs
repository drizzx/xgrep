//! CSV / TSV reader. Returns `CellRecord`s with the same shape the xlsx reader
//! produces, so `search_file` and the printer treat both formats uniformly.
//!
//! Delimiter is chosen by extension (`.csv` = comma, `.tsv` = tab). Encoding is
//! UTF-8 by default with BOM auto-detect for UTF-8, UTF-16LE, UTF-16BE; an
//! `opts.encoding` override (from `-E ENCODING`) supersedes the BOM hint.
//! RFC 4180 quoting via the `csv` crate's `flexible(true)` mode.

use std::fs;
use std::path::Path;

use crate::cell::to_a1;
use crate::config::Layer;
use crate::error::SearchError;
use crate::reader::{CellRecord, ReaderOptions};

pub fn read_csv_cells(
    path: &Path,
    opts: &ReaderOptions,
) -> Result<Vec<CellRecord>, SearchError> {
    let bytes = fs::read(path).map_err(SearchError::Io)?;
    let text = decode(&bytes, opts.encoding.as_deref())?;

    let delimiter = match path.extension().and_then(|s| s.to_str()) {
        Some(ext) if ext.eq_ignore_ascii_case("csv") => b',',
        Some(ext) if ext.eq_ignore_ascii_case("tsv") => b'\t',
        Some(other) => {
            return Err(SearchError::Parse(format!(
                "internal: unexpected extension '{other}' routed to csv reader"
            )))
        }
        None => return Err(SearchError::Parse("file has no extension".into())),
    };

    let mut rdr = ::csv::ReaderBuilder::new()
        .has_headers(false)
        .flexible(true)
        .delimiter(delimiter)
        .quoting(true)
        .from_reader(text.as_bytes());

    let mut out = Vec::new();
    for (row_idx, rec) in rdr.records().enumerate() {
        let rec = rec.map_err(|e| {
            SearchError::Parse(format!("csv row {}: {e}", row_idx + 1))
        })?;
        for (col_idx, field) in rec.iter().enumerate() {
            if field.is_empty() {
                continue;
            }
            out.push(CellRecord {
                sheet: String::new(),
                cell: to_a1(row_idx as u32, col_idx as u32),
                layer: Layer::Display,
                text: field.to_string(),
            });
        }
    }
    Ok(out)
}

/// Decode bytes per the encoding pipeline:
/// 1. BOM detection → hint (utf-8 / utf-16le / utf-16be).
/// 2. Precedence: explicit `override_enc` > BOM hint > default "utf-8".
/// 3. `encoding_rs::Encoding::for_label(label)`; unknown label → Parse error.
/// 4. Decode with malformed-byte replacement (U+FFFD) — same as rg.
fn decode(bytes: &[u8], override_enc: Option<&str>) -> Result<String, SearchError> {
    let (bom_hint, body_start): (Option<&str>, usize) = match bytes {
        [0xEF, 0xBB, 0xBF, ..] => (Some("utf-8"), 3),
        [0xFF, 0xFE, ..] => (Some("utf-16le"), 0),
        [0xFE, 0xFF, ..] => (Some("utf-16be"), 0),
        _ => (None, 0),
    };

    let enc_label = override_enc.unwrap_or_else(|| bom_hint.unwrap_or("utf-8"));

    let encoding = encoding_rs::Encoding::for_label(enc_label.as_bytes())
        .ok_or_else(|| SearchError::Parse(format!("unknown encoding: {enc_label}")))?;

    let (cow, _used, _had_errors) = encoding.decode(&bytes[body_start..]);
    Ok(cow.into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::TempDir;

    fn write(path: &Path, bytes: &[u8]) {
        let mut f = fs::File::create(path).unwrap();
        f.write_all(bytes).unwrap();
    }

    fn opts() -> ReaderOptions<'static> {
        ReaderOptions::default()
    }

    #[test]
    fn empty_file_returns_empty() {
        let dir = TempDir::new().unwrap();
        let p = dir.path().join("empty.csv");
        write(&p, b"");
        let cells = read_csv_cells(&p, &opts()).unwrap();
        assert!(cells.is_empty());
    }

    #[test]
    fn ascii_rows_yield_cells_with_a1_addresses() {
        let dir = TempDir::new().unwrap();
        let p = dir.path().join("ascii.csv");
        write(&p, b"a,b\nc,d\n");
        let cells = read_csv_cells(&p, &opts()).unwrap();
        assert_eq!(cells.len(), 4);
        assert_eq!(cells[0].cell, "A1");
        assert_eq!(cells[0].text, "a");
        assert_eq!(cells[1].cell, "B1");
        assert_eq!(cells[1].text, "b");
        assert_eq!(cells[2].cell, "A2");
        assert_eq!(cells[2].text, "c");
        assert_eq!(cells[3].cell, "B2");
        assert_eq!(cells[3].text, "d");
        // Sheet always empty for CSV.
        for c in &cells {
            assert_eq!(c.sheet, "");
            assert_eq!(c.layer, Layer::Display);
        }
    }

    #[test]
    fn utf8_bom_is_stripped() {
        let dir = TempDir::new().unwrap();
        let p = dir.path().join("bom.csv");
        let mut bytes = vec![0xEF, 0xBB, 0xBF];
        bytes.extend_from_slice(b"header,value\n");
        write(&p, &bytes);
        let cells = read_csv_cells(&p, &opts()).unwrap();
        assert_eq!(cells.len(), 2);
        // The BOM must NOT leak into the first cell's text.
        assert_eq!(cells[0].text, "header");
        assert_eq!(cells[1].text, "value");
    }

    #[test]
    fn quoted_fields_preserve_internal_commas() {
        let dir = TempDir::new().unwrap();
        let p = dir.path().join("quoted.csv");
        write(&p, b"\"foo,bar\",baz\n");
        let cells = read_csv_cells(&p, &opts()).unwrap();
        assert_eq!(cells.len(), 2);
        assert_eq!(cells[0].text, "foo,bar");
        assert_eq!(cells[1].text, "baz");
    }

    #[test]
    fn tsv_extension_uses_tab() {
        let dir = TempDir::new().unwrap();
        let p = dir.path().join("data.tsv");
        write(&p, b"a\tb\nc\td\n");
        let cells = read_csv_cells(&p, &opts()).unwrap();
        assert_eq!(cells.len(), 4);
        assert_eq!(cells[0].text, "a");
        assert_eq!(cells[1].text, "b");
    }

    #[test]
    fn gbk_override_decodes_chinese() {
        let dir = TempDir::new().unwrap();
        let p = dir.path().join("gbk.csv");
        let utf8 = "张三,李四\n";
        let (gbk_bytes, _enc, had_errors) = encoding_rs::GBK.encode(utf8);
        assert!(!had_errors, "GBK encode of test fixture should succeed");
        write(&p, &gbk_bytes);
        let mut o = opts();
        o.encoding = Some("gbk".to_string());
        let cells = read_csv_cells(&p, &o).unwrap();
        assert_eq!(cells.len(), 2);
        assert_eq!(cells[0].text, "张三");
        assert_eq!(cells[1].text, "李四");
    }
}
