//! xlsx → cell stream. Wraps calamine and yields one `CellRecord` per
//! (sheet, cell, layer) tuple matched by the configured `LayerSet` /
//! `include_hidden` flags.

use std::path::Path;

use calamine::{open_workbook, Data, DataType, Reader, Xlsx};

use crate::cell::to_a1;
use crate::config::{Layer, LayerSet};
use crate::error::SearchError;

#[derive(Debug, Clone)]
pub struct ReaderOptions {
    pub layers: LayerSet,
    pub include_hidden: bool,
    pub sheet_filter: Option<globset::GlobMatcher>,
}

impl ReaderOptions {
    pub fn defaults_for_v01() -> Self {
        Self {
            layers: LayerSet::defaults(),
            include_hidden: true,
            sheet_filter: None,
        }
    }
}

impl Default for ReaderOptions {
    fn default() -> Self {
        Self::defaults_for_v01()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CellRecord {
    pub sheet: String,
    pub cell: String,
    pub layer: Layer,
    pub text: String,
}

pub fn read_cells(path: &Path, opts: &ReaderOptions) -> Result<Vec<CellRecord>, SearchError> {
    let mut workbook: Xlsx<_> = open_workbook(path).map_err(map_calamine_err)?;
    let sheets = workbook.sheets_metadata().to_vec();
    let mut out = Vec::new();

    for sheet_meta in &sheets {
        let name = &sheet_meta.name;

        if let Some(filter) = &opts.sheet_filter {
            if !filter.is_match(name) {
                continue;
            }
        }
        if !opts.include_hidden
            && !matches!(sheet_meta.visible, calamine::SheetVisible::Visible)
        {
            continue;
        }

        let range = workbook
            .worksheet_range(name)
            .map_err(|e| SearchError::Sheet { sheet: name.clone(), msg: e.to_string() })?;

        if opts.layers.contains(LayerSet::DISPLAY) {
            for (row, col, data) in range.cells() {
                if data.is_empty() {
                    continue;
                }
                let text = display_value(data);
                if text.is_empty() {
                    continue;
                }
                out.push(CellRecord {
                    sheet: name.clone(),
                    cell: to_a1(row as u32, col as u32),
                    layer: Layer::Display,
                    text,
                });
            }
        }
    }
    Ok(out)
}

fn display_value(d: &Data) -> String {
    match d {
        Data::Empty => String::new(),
        Data::String(s) => s.clone(),
        Data::Float(f) => format_number(*f),
        Data::Int(i) => i.to_string(),
        Data::Bool(b) => b.to_string(),
        Data::DateTime(dt) => dt.to_string(),
        Data::DateTimeIso(s) => s.clone(),
        Data::DurationIso(s) => s.clone(),
        Data::Error(e) => format!("#{e:?}"),
    }
}

fn format_number(f: f64) -> String {
    if f.fract() == 0.0 && f.abs() < 1e15 {
        format!("{}", f as i64)
    } else {
        let s = format!("{:.10}", f);
        let trimmed = s.trim_end_matches('0').trim_end_matches('.');
        trimmed.to_owned()
    }
}

fn map_calamine_err(e: calamine::XlsxError) -> SearchError {
    let msg = e.to_string();
    let lower = msg.to_lowercase();
    if lower.contains("encrypted") || lower.contains("password") {
        SearchError::Encrypted
    } else {
        // We don't distinguish XlsxError::Io from other parse errors at this
        // layer — callers see the stringified message either way.
        SearchError::Parse(msg)
    }
}
