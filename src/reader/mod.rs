//! xlsx → cell stream. Single-open ZipIndex routes work between fast-paths
//! (sst + sheet-xml byte scan — added in later tasks) and the full calamine
//! parse path. Per-cell logic lives in `cells.rs`.

pub mod cells;
pub mod comments;
pub mod hidden;
pub mod zip_index;

use std::collections::HashMap;
use std::path::Path;

use calamine::{open_workbook, Reader, Xlsx};

use crate::config::{Layer, LayerSet};
use crate::error::SearchError;
use crate::reader::cells::SheetParseInput;
use crate::reader::zip_index::ZipIndex;

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
    // One ZipIndex per file — used by hidden + comments. Workbook (calamine) is
    // opened separately because calamine needs an owned file handle (it parses
    // the zip itself). This is fine: zip 2's File-based open is fast (no decompression
    // happens until we ask for an entry). The cost we save is the *repeated*
    // open-then-parse-workbook.xml cycle that v0.1 did inside each helper.
    let mut zip_index = ZipIndex::open(path)?;
    let mut workbook: Xlsx<_> = open_workbook(path).map_err(map_calamine_err)?;
    let sheets = workbook.sheets_metadata().to_vec();
    let mut out = Vec::new();

    let path_lookup: HashMap<String, String> = zip_index
        .sheets()
        .iter()
        .map(|s| (s.name.clone(), s.xml_path.clone()))
        .collect();

    for sheet_meta in &sheets {
        let name = &sheet_meta.name;
        if let Some(filter) = &opts.sheet_filter {
            if !filter.is_match(name) { continue; }
        }
        if !opts.include_hidden && !matches!(sheet_meta.visible, calamine::SheetVisible::Visible) {
            continue;
        }
        let range = workbook
            .worksheet_range(name)
            .map_err(|e| SearchError::Sheet { sheet: name.clone(), msg: e.to_string() })?;
        let formulas = workbook.worksheet_formula(name).ok();
        let (hidden_rows, hidden_cols) = if opts.include_hidden {
            (Default::default(), Default::default())
        } else if let Some(sheet_xml) = path_lookup.get(name) {
            hidden::detect(&mut zip_index, sheet_xml)
        } else {
            (Default::default(), Default::default())
        };
        let input = SheetParseInput {
            sheet_name: name,
            range: &range,
            formulas: formulas.as_ref(),
            layers: opts.layers,
            hidden_rows: &hidden_rows,
            hidden_cols: &hidden_cols,
            include_hidden: opts.include_hidden,
        };
        out.extend(cells::parse_sheet(&input));
    }

    if opts.layers.contains(LayerSet::COMMENT) {
        let visible_sheets: std::collections::HashSet<String> = sheets
            .iter()
            .filter(|m| matches!(m.visible, calamine::SheetVisible::Visible))
            .map(|m| m.name.clone())
            .collect();
        let raw = comments::extract(&mut zip_index).unwrap_or_default();
        for (sheet, cell, text) in raw {
            if let Some(filter) = &opts.sheet_filter {
                if !filter.is_match(&sheet) { continue; }
            }
            if !opts.include_hidden && !visible_sheets.contains(&sheet) { continue; }
            out.push(CellRecord {
                sheet,
                cell,
                layer: Layer::Comment,
                text,
            });
        }
    }
    Ok(out)
}

fn map_calamine_err(e: calamine::XlsxError) -> SearchError {
    let msg = e.to_string();
    let lower = msg.to_lowercase();
    if lower.contains("encrypted") || lower.contains("password") {
        SearchError::Encrypted
    } else {
        SearchError::Parse(msg)
    }
}
