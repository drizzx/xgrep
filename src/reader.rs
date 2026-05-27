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
            if !filter.is_match(name) { continue; }
        }
        if !opts.include_hidden && !matches!(sheet_meta.visible, calamine::SheetVisible::Visible) {
            continue;
        }

        let range = workbook
            .worksheet_range(name)
            .map_err(|e| SearchError::Sheet { sheet: name.clone(), msg: e.to_string() })?;
        let formulas = workbook.worksheet_formula(name).ok();

        let wants_formula = opts.layers.contains(LayerSet::FORMULA);
        let wants_cached  = opts.layers.contains(LayerSet::CACHED);
        let wants_display = opts.layers.contains(LayerSet::DISPLAY);

        for (row, col, data) in range.cells() {
            if data.is_empty() { continue; }
            let a1 = to_a1(row as u32, col as u32);

            // Look up formula text for this (row, col). calamine 0.26's Range::get_value
            // takes a (u32, u32) tuple of position. The formula range parallels the
            // cell range and returns an empty String for non-formula cells.
            let formula_text: Option<String> = formulas.as_ref()
                .and_then(|f| f.get_value((row as u32, col as u32)).cloned())
                .filter(|s| !s.is_empty());

            if let Some(ftxt) = formula_text {
                if wants_formula {
                    out.push(CellRecord {
                        sheet: name.clone(), cell: a1.clone(), layer: Layer::Formula,
                        text: ftxt,
                    });
                }
                if wants_cached {
                    let cached = display_value(data);
                    if !cached.is_empty() {
                        out.push(CellRecord {
                            sheet: name.clone(), cell: a1, layer: Layer::Cached,
                            text: cached,
                        });
                    }
                }
            } else if wants_display {
                let text = display_value(data);
                if !text.is_empty() {
                    out.push(CellRecord {
                        sheet: name.clone(), cell: a1, layer: Layer::Display,
                        text,
                    });
                }
            }
        }
    }
    if opts.layers.contains(LayerSet::COMMENT) {
        let comments = extract_comments(path).unwrap_or_default();
        for (sheet, cell, text) in comments {
            if let Some(filter) = &opts.sheet_filter {
                if !filter.is_match(&sheet) { continue; }
            }
            out.push(CellRecord {
                sheet, cell, layer: Layer::Comment, text,
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

use std::io::Read;

/// Map sheet name → "xl/worksheets/sheetN.xml" zip path. Done by reading
/// `xl/workbook.xml` and `xl/_rels/workbook.xml.rels`.
fn sheet_xml_paths(path: &std::path::Path) -> Result<Vec<(String, String)>, SearchError> {
    use std::collections::HashMap;
    let file = std::fs::File::open(path).map_err(SearchError::Io)?;
    let mut zip = zip::ZipArchive::new(file)
        .map_err(|e| SearchError::Parse(format!("zip: {e}")))?;

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
    let re_rel = regex::Regex::new(r#"<Relationship[^>]*Id="(rId\d+)"[^>]*Target="([^"]+)""#).unwrap();
    let mut rid_to_target: HashMap<String, String> = HashMap::new();
    for cap in re_rel.captures_iter(&rels) {
        rid_to_target.insert(cap[1].to_string(), cap[2].to_string());
    }
    Ok(rids
        .into_iter()
        .filter_map(|(name, rid)| {
            rid_to_target.get(&rid).map(|t| (name, format!("xl/{}", t.trim_start_matches('/'))))
        })
        .collect())
}

/// Read all cell comments by directly parsing xl/comments*.xml entries.
/// Returns Vec<(sheet_name, cell_a1, text)>.
fn extract_comments(path: &std::path::Path) -> Result<Vec<(String, String, String)>, SearchError> {
    let file = std::fs::File::open(path).map_err(SearchError::Io)?;
    let mut zip = zip::ZipArchive::new(file)
        .map_err(|e| SearchError::Parse(format!("zip: {e}")))?;

    let sheets = sheet_xml_paths(path)?;
    let mut out = Vec::new();
    let re_comment = regex::Regex::new(
        r#"<comment[^>]*ref="([^"]+)"[^>]*>([\s\S]*?)</comment>"#
    ).unwrap();
    let re_t = regex::Regex::new(r#"<t[^>]*>([\s\S]*?)</t>"#).unwrap();
    let re_comments_target = regex::Regex::new(r#"Target="([^"]*comments[^"]+\.xml)""#).unwrap();

    for (sheet_name, sheet_xml) in sheets {
        // sheet_xml is like "xl/worksheets/sheet1.xml" — its rels live at
        // "xl/worksheets/_rels/sheet1.xml.rels"
        let rels_path = sheet_xml.replacen("worksheets/", "worksheets/_rels/", 1) + ".rels";
        let mut rels = String::new();
        if let Ok(mut f) = zip.by_name(&rels_path) {
            f.read_to_string(&mut rels)?;
        } else { continue; }
        let Some(cap) = re_comments_target.captures(&rels)
        else { continue; };
        let target = cap[1].to_string();
        // Targets like "../comments1.xml" → resolve to "xl/comments1.xml"
        // Targets like "comments1.xml" → resolve to "xl/worksheets/comments1.xml"
        let comments_path = if let Some(stripped) = target.strip_prefix("../") {
            format!("xl/{stripped}")
        } else {
            format!("xl/worksheets/{target}")
        };
        let mut comments_xml = String::new();
        if let Ok(mut f) = zip.by_name(&comments_path) {
            f.read_to_string(&mut comments_xml)?;
        } else { continue; }

        for cap in re_comment.captures_iter(&comments_xml) {
            let cell = cap[1].to_string();
            let body = &cap[2];
            let text: String = re_t.captures_iter(body)
                .map(|c| xml_unescape(&c[1]))
                .collect::<Vec<_>>()
                .join("");
            if !text.is_empty() {
                out.push((sheet_name.clone(), cell, text));
            }
        }
    }
    Ok(out)
}

fn xml_unescape(s: &str) -> String {
    s.replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
}
