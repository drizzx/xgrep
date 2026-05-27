//! xlsx → cell stream. Wraps calamine and yields one `CellRecord` per
//! (sheet, cell, layer) tuple matched by the configured `LayerSet` /
//! `include_hidden` flags.

pub mod comments;
pub mod zip_index;

use std::path::Path;

use calamine::{open_workbook, Data, DataType, Reader, Xlsx};
use chrono::Timelike as _;

use crate::cell::to_a1;
use crate::config::{Layer, LayerSet};
use crate::error::SearchError;
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
    let mut workbook: Xlsx<_> = open_workbook(path).map_err(map_calamine_err)?;
    let sheets = workbook.sheets_metadata().to_vec();
    let mut out = Vec::new();

    let path_lookup: std::collections::HashMap<String, String> = sheet_xml_paths(path)
        .unwrap_or_default()
        .into_iter()
        .collect();

    for sheet_meta in &sheets {
        let name = &sheet_meta.name;

        if let Some(filter) = &opts.sheet_filter {
            if !filter.is_match(name) {
                continue;
            }
        }
        if !opts.include_hidden && !matches!(sheet_meta.visible, calamine::SheetVisible::Visible) {
            continue;
        }

        let range = workbook
            .worksheet_range(name)
            .map_err(|e| SearchError::Sheet {
                sheet: name.clone(),
                msg: e.to_string(),
            })?;
        let formulas = workbook.worksheet_formula(name).ok();

        let wants_formula = opts.layers.contains(LayerSet::FORMULA);
        let wants_cached = opts.layers.contains(LayerSet::CACHED);
        let wants_display = opts.layers.contains(LayerSet::DISPLAY);

        let (hidden_rows, hidden_cols) = if opts.include_hidden {
            (Default::default(), Default::default())
        } else if let Some(sheet_xml) = path_lookup.get(name) {
            hidden_row_col_for_sheet(path, sheet_xml)
        } else {
            (Default::default(), Default::default())
        };

        for (row, col, data) in range.cells() {
            if data.is_empty() {
                continue;
            }
            if !opts.include_hidden {
                if hidden_rows.contains(&(row as u32)) {
                    continue;
                }
                if hidden_cols.contains(&(col as u32)) {
                    continue;
                }
            }
            let a1 = to_a1(row as u32, col as u32);

            // Look up formula text for this (row, col). calamine 0.26's Range::get_value
            // takes a (u32, u32) tuple of position. The formula range parallels the
            // cell range and returns an empty String for non-formula cells.
            let formula_text: Option<String> = formulas
                .as_ref()
                .and_then(|f| f.get_value((row as u32, col as u32)).cloned())
                .filter(|s| !s.is_empty());

            if let Some(ftxt) = formula_text {
                if wants_formula {
                    out.push(CellRecord {
                        sheet: name.clone(),
                        cell: a1.clone(),
                        layer: Layer::Formula,
                        text: ftxt,
                    });
                }
                if wants_cached {
                    let cached = display_value(data);
                    if !cached.is_empty() {
                        out.push(CellRecord {
                            sheet: name.clone(),
                            cell: a1,
                            layer: Layer::Cached,
                            text: cached,
                        });
                    }
                }
            } else if wants_display {
                let text = display_value(data);
                if !text.is_empty() {
                    out.push(CellRecord {
                        sheet: name.clone(),
                        cell: a1,
                        layer: Layer::Display,
                        text,
                    });
                }
            }
        }
    }
    if opts.layers.contains(LayerSet::COMMENT) {
        let visible_sheets: std::collections::HashSet<String> = sheets
            .iter()
            .filter(|m| matches!(m.visible, calamine::SheetVisible::Visible))
            .map(|m| m.name.clone())
            .collect();
        let comments: Vec<(String, String, String)> = ZipIndex::open(path)
            .ok()
            .as_mut()
            .map(|idx| comments::extract(idx).unwrap_or_default())
            .unwrap_or_default();
        for (sheet, cell, text) in comments {
            if let Some(filter) = &opts.sheet_filter {
                if !filter.is_match(&sheet) {
                    continue;
                }
            }
            if !opts.include_hidden && !visible_sheets.contains(&sheet) {
                continue;
            }
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

fn display_value(d: &Data) -> String {
    match d {
        Data::Empty => String::new(),
        Data::String(s) => s.clone(),
        Data::Float(f) => format_number(*f),
        Data::Int(i) => i.to_string(),
        Data::Bool(b) => b.to_string(),
        Data::DateTime(dt) => {
            // ExcelDateTime::Display just prints the raw serial float (e.g. "45438").
            // When the `dates` feature is active, as_datetime() returns a
            // chrono::NaiveDateTime which we format as "YYYY-MM-DD HH:MM:SS".
            // For date-only values (time == 00:00:00) we output just the date part.
            if let Some(ndt) = dt.as_datetime() {
                let t = ndt.time();
                if t.hour() == 0 && t.minute() == 0 && t.second() == 0 {
                    ndt.format("%Y-%m-%d").to_string()
                } else {
                    ndt.format("%Y-%m-%d %H:%M:%S").to_string()
                }
            } else {
                dt.to_string()
            }
        }
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

use std::io::Read;

/// Map sheet name → "xl/worksheets/sheetN.xml" zip path. Done by reading
/// `xl/workbook.xml` and `xl/_rels/workbook.xml.rels`.
fn sheet_xml_paths(path: &std::path::Path) -> Result<Vec<(String, String)>, SearchError> {
    use std::collections::HashMap;
    let file = std::fs::File::open(path).map_err(SearchError::Io)?;
    let mut zip =
        zip::ZipArchive::new(file).map_err(|e| SearchError::Parse(format!("zip: {e}")))?;

    let mut s = String::new();
    zip.by_name("xl/workbook.xml")
        .map_err(|e| SearchError::Parse(format!("workbook.xml: {e}")))?
        .read_to_string(&mut s)?;
    let re_sheet = regex::Regex::new(r#"<sheet[^>]*name="([^"]+)"[^>]*r:id="(rId\d+)""#).unwrap();
    let mut rids: Vec<(String, String)> = re_sheet
        .captures_iter(&s)
        .map(|c| (c[1].to_string(), c[2].to_string()))
        .collect();

    // workbook.xml sometimes orders attributes differently — also try r:id BEFORE name.
    if rids.is_empty() {
        let re_alt = regex::Regex::new(r#"<sheet[^>]*r:id="(rId\d+)"[^>]*name="([^"]+)""#).unwrap();
        rids = re_alt
            .captures_iter(&s)
            .map(|c| (c[2].to_string(), c[1].to_string()))
            .collect();
    }

    let mut rels = String::new();
    if let Ok(mut f) = zip.by_name("xl/_rels/workbook.xml.rels") {
        f.read_to_string(&mut rels)?;
    }
    let re_rel =
        regex::Regex::new(r#"<Relationship[^>]*Id="(rId\d+)"[^>]*Target="([^"]+)""#).unwrap();
    let mut rid_to_target: HashMap<String, String> = HashMap::new();
    for cap in re_rel.captures_iter(&rels) {
        rid_to_target.insert(cap[1].to_string(), cap[2].to_string());
    }
    Ok(rids
        .into_iter()
        .filter_map(|(name, rid)| {
            rid_to_target
                .get(&rid)
                .map(|t| (name, format!("xl/{}", t.trim_start_matches('/'))))
        })
        .collect())
}


fn hidden_row_col_for_sheet(
    path: &std::path::Path,
    sheet_xml_zip_path: &str,
) -> (
    std::collections::HashSet<u32>,
    std::collections::HashSet<u32>,
) {
    use std::collections::HashSet;
    let mut hidden_rows = HashSet::new();
    let mut hidden_cols = HashSet::new();
    let Ok(file) = std::fs::File::open(path) else {
        return (hidden_rows, hidden_cols);
    };
    let Ok(mut zip) = zip::ZipArchive::new(file) else {
        return (hidden_rows, hidden_cols);
    };
    let mut xml = String::new();
    if let Ok(mut f) = zip.by_name(sheet_xml_zip_path) {
        let _ = f.read_to_string(&mut xml);
    } else {
        return (hidden_rows, hidden_cols);
    }
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

