//! xlsx → cell stream. Single-open ZipIndex routes work between fast-paths
//! (sst + sheet-xml byte scan — added in later tasks) and the full calamine
//! parse path. Per-cell logic lives in `cells.rs`.

pub mod cells;
pub mod comments;
pub mod fast_path;
pub mod hidden;
#[cfg(debug_assertions)]
pub mod oracle;
pub mod sst;
pub mod zip_index;
pub mod xml_scan;

use std::collections::HashMap;
use std::path::Path;

use calamine::{open_workbook, Reader, Xlsx};

use crate::config::{Layer, LayerSet};
use crate::error::SearchError;
use crate::reader::cells::SheetParseInput;
use crate::reader::zip_index::ZipIndex;

#[derive(Debug, Clone)]
pub struct ReaderOptions<'a> {
    pub layers: LayerSet,
    pub include_hidden: bool,
    pub sheet_filter: Option<globset::GlobMatcher>,
    /// Pattern visible to the reader for fast-path decisions. When None, the
    /// reader walks the full v0.1 path (no skip, no sst pre-scan).
    pub pattern: Option<&'a crate::matcher::Pattern>,
    /// True to bypass all fast-paths and walk the v0.1 path regardless of
    /// `pattern`. Wired to `XGREP_DISABLE_FAST_PATH=1` in main.rs (Task 22).
    pub disable_fast_path: bool,
    /// Override CSV decoding encoding (e.g. "gbk", "utf-16le"). `None` =
    /// UTF-8 default with BOM auto-detect. Ignored for xlsx (always UTF-8
    /// by spec). Validated at CLI parse time via `encoding_rs::Encoding::for_label`.
    pub encoding: Option<String>,
}

impl<'a> ReaderOptions<'a> {
    pub fn defaults_for_v01() -> Self {
        Self {
            layers: LayerSet::defaults(),
            include_hidden: true,
            sheet_filter: None,
            pattern: None,
            disable_fast_path: false,
            encoding: None,
        }
    }
}

impl<'a> Default for ReaderOptions<'a> {
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

pub fn read_cells<'a>(
    path: &Path,
    opts: &ReaderOptions<'a>,
) -> Result<Vec<CellRecord>, SearchError> {
    // One ZipIndex per file — used by hidden + comments. Workbook (calamine) is
    // opened separately because calamine needs an owned file handle (it parses
    // the zip itself). This is fine: zip 2's File-based open is fast (no decompression
    // happens until we ask for an entry). The cost we save is the *repeated*
    // open-then-parse-workbook.xml cycle that v0.1 did inside each helper.
    let mut zip_index = ZipIndex::open(path)?;

    let want_fast_path = !opts.disable_fast_path && opts.pattern.is_some();
    let (_sst_size, hit_set, aborted) = if want_fast_path {
        sst::parse_with_early_abort(&mut zip_index, opts.pattern, fast_path::THRESHOLD)?
    } else {
        (0usize, sst::HitSet::new(0), false)
    };
    // When aborted, sst parsing stopped early to cap cost. The hit_set is
    // truncated and unsafe to use for fast-path skip decisions, so we bypass
    // fast-path entirely (augmented = None) and parse all sheets the v0.1 way.
    //
    // When the hit set is dense, every sheet almost certainly references at
    // least one matching index — the per-sheet decide() scan would return
    // true for every sheet, making it pure overhead. Bypass fast-path in that
    // case too (see fast_path::should_dense_bypass).
    let dense_bypass = want_fast_path
        && !aborted
        && fast_path::should_dense_bypass(hit_set.count(), hit_set.len());
    let augmented = if want_fast_path && !aborted && !dense_bypass {
        Some(fast_path::augment(opts.pattern.unwrap(), &hit_set))
    } else {
        None
    };

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
            if !filter.is_match(name) {
                continue;
            }
        }
        if !opts.include_hidden && !matches!(sheet_meta.visible, calamine::SheetVisible::Visible) {
            continue;
        }

        let needs_parse = if let Some(aug) = augmented.as_ref() {
            let sheet_xml_path = path_lookup.get(name);
            match sheet_xml_path {
                Some(sxp) => {
                    let xml_bytes = zip_index
                        .read_to_vec(sxp)
                        .ok()
                        .flatten()
                        .unwrap_or_default();
                    fast_path::decide(&xml_bytes, aug, hit_set.is_empty())
                }
                None => true, // can't byte-scan -> conservative parse
            }
        } else {
            true
        };

        if !needs_parse {
            #[cfg(debug_assertions)]
            {
                if let Some(pat) = opts.pattern {
                    // Parse the sheet "the old way" and ensure none of its cells match.
                    // This is purely a safety check; result is discarded.
                    let range = workbook
                        .worksheet_range(name)
                        .map_err(|e| SearchError::Sheet {
                            sheet: name.clone(),
                            msg: e.to_string(),
                        })?;
                    let formulas = workbook.worksheet_formula(name).ok();
                    let empty_rows: std::collections::HashSet<u32> =
                        std::collections::HashSet::new();
                    let empty_cols: std::collections::HashSet<u32> =
                        std::collections::HashSet::new();
                    let input = SheetParseInput {
                        sheet_name: name,
                        range: &range,
                        formulas: formulas.as_ref(),
                        layers: opts.layers,
                        hidden_rows: &empty_rows,
                        hidden_cols: &empty_cols,
                        include_hidden: opts.include_hidden,
                    };
                    let fallback = cells::parse_sheet(&input);
                    oracle::assert_skipped_safely(name, &fallback, pat);
                }
            }
            continue;
        }

        let range = workbook
            .worksheet_range(name)
            .map_err(|e| SearchError::Sheet {
                sheet: name.clone(),
                msg: e.to_string(),
            })?;
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

fn map_calamine_err(e: calamine::XlsxError) -> SearchError {
    let msg = e.to_string();
    let lower = msg.to_lowercase();
    if lower.contains("encrypted") || lower.contains("password") {
        SearchError::Encrypted
    } else {
        SearchError::Parse(msg)
    }
}
