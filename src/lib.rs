//! xgrep — Excel-aware grep.

pub mod cell;
pub mod config;
pub mod error;
pub mod matcher;
pub mod printer;
pub mod reader;
pub mod walker;
pub mod worker;

use serde::Serialize;
use std::path::PathBuf;

/// Context-line configuration (rg-aligned). When both `before` and `after`
/// are 0, `search_file` takes the single-pass code path with zero overhead.
#[derive(Debug, Clone, Copy, Default)]
pub struct ContextOptions {
    pub before: u32,
    pub after: u32,
}

impl ContextOptions {
    pub fn is_zero(&self) -> bool {
        self.before == 0 && self.after == 0
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", content = "data", rename_all = "snake_case")]
pub enum MatchEvent {
    /// `{"type":"begin","data":{"path":...}}` per spec §4.5
    #[serde(rename = "begin")]
    FileBegin {
        path: PathBuf,
    },
    Match {
        path: PathBuf,
        sheet: String,
        cell: String,  // A1 notation
        layer: String, // Layer::as_str()
        text: String,
        submatches: Vec<Submatch>,
    },
    /// A non-matching cell surfaced because it falls within the row-context
    /// window (`-A`/`-B`/`-C`) of an adjacent match. Identical shape to
    /// `Match` minus the `submatches` field.
    Context {
        path: PathBuf,
        sheet: String,
        cell: String,
        layer: String,
        text: String,
    },
    /// Inserted between disjoint context spans within the same file (rg's
    /// `--` separator). Carries no payload.
    Separator,
    #[serde(rename = "end")]
    FileEnd {
        path: PathBuf,
        stats: FileStats,
    },
    Error {
        path: PathBuf,
        message: String,
    },
}

#[derive(Debug, Clone, Serialize)]
pub struct Submatch {
    /// rg-compatible: `"match": {"text": "..."}`.
    #[serde(rename = "match")]
    pub matched: SubmatchText,
    /// 0-indexed Unicode character offset (inclusive).
    pub start: usize,
    /// 0-indexed Unicode character offset (exclusive).
    pub end: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct SubmatchText {
    pub text: String,
}

#[derive(Debug, Clone, Copy, Default, Serialize)]
pub struct FileStats {
    pub matches: u64,
    pub sheets_scanned: u32,
}

/// A FileBlock is the unit of atomic output: FileBegin .. Match* .. FileEnd
/// (or an Error event in place of the body). Workers emit one per file.
#[derive(Debug, Clone)]
pub struct FileBlock {
    pub events: Vec<MatchEvent>,
}

/// Convert a regex byte range (matcher returns bytes) into a 0-indexed Unicode
/// character range with the matched substring captured.
pub fn bytes_to_char_range(haystack: &str, start_b: usize, end_b: usize) -> Submatch {
    let start = haystack[..start_b].chars().count();
    let end = start + haystack[start_b..end_b].chars().count();
    Submatch {
        matched: SubmatchText {
            text: haystack[start_b..end_b].to_owned(),
        },
        start,
        end,
    }
}

use crate::matcher::Pattern;
use crate::reader::{read_cells, ReaderOptions};
use std::collections::HashSet;

/// Search a single xlsx file, returning a complete FileBlock ready for the printer.
///
/// `invert`: if true, emit matches for cells whose text *does not* match the pattern
/// (rg `-v`).
///
/// `ctx`: context-line options. When `ctx.is_zero()`, the existing v0.2.2
/// single-pass path runs unchanged with zero overhead. When `!ctx.is_zero()`,
/// a three-pass pipeline (collect → expand → emit) runs instead.
pub fn search_file(
    path: &std::path::Path,
    pattern: &Pattern,
    reader_opts: &ReaderOptions<'_>,
    invert: bool,
    ctx: ContextOptions,
) -> FileBlock {
    let mut enriched = reader_opts.clone();
    enriched.pattern = Some(pattern);

    let mut events = vec![MatchEvent::FileBegin {
        path: path.to_path_buf(),
    }];
    let mut matches = 0u64;
    let mut sheets_seen: HashSet<String> = HashSet::new();

    match read_cells(path, &enriched) {
        Ok(cells) => {
            if ctx.is_zero() {
                // v0.2.2 single-pass path — UNCHANGED behavior.
                for c in cells {
                    sheets_seen.insert(c.sheet.clone());
                    let hits: Vec<_> = pattern.find_iter(&c.text).collect();
                    let has_hit = !hits.is_empty();
                    let emit = if invert { !has_hit } else { has_hit };
                    if !emit {
                        continue;
                    }
                    let submatches = if invert {
                        Vec::new()
                    } else {
                        hits.into_iter()
                            .map(|(s, e)| bytes_to_char_range(&c.text, s, e))
                            .collect()
                    };
                    matches += 1;
                    events.push(MatchEvent::Match {
                        path: path.to_path_buf(),
                        sheet: c.sheet,
                        cell: c.cell,
                        layer: c.layer.as_str().into(),
                        text: c.text,
                        submatches,
                    });
                }
            } else {
                // v0.3 three-pass path: collect matches → expand → emit.

                // Pass 1: collect (sheet, row) of every match.
                let mut match_rows: Vec<(String, u32)> = Vec::new();
                for c in &cells {
                    sheets_seen.insert(c.sheet.clone());
                    let has_hit = pattern.find_iter(&c.text).next().is_some();
                    let is_match = if invert { !has_hit } else { has_hit };
                    if is_match {
                        matches += 1;
                        // Only register as a context anchor if we can parse the row. Matches
                        // on cells with unparseable addresses still get counted (here) but
                        // can't participate in row-context expansion in Pass 2/3.
                        if let Some(r) = crate::cell::row_from_a1(&c.cell) {
                            match_rows.push((c.sheet.clone(), r));
                        }
                    }
                }

                // Pass 2+3: emit_with_context returns skeleton events; re-fill
                // path + submatches here.
                let context_events = emit_with_context(&cells, &match_rows, ctx);
                for e in context_events {
                    match e {
                        MatchEvent::Match { sheet, cell, text, layer, .. } => {
                            let hits: Vec<_> = pattern.find_iter(&text).collect();
                            let submatches = if invert {
                                Vec::new()
                            } else {
                                hits.into_iter()
                                    .map(|(s, e)| bytes_to_char_range(&text, s, e))
                                    .collect()
                            };
                            events.push(MatchEvent::Match {
                                path: path.to_path_buf(),
                                sheet,
                                cell,
                                layer,
                                text,
                                submatches,
                            });
                        }
                        MatchEvent::Context { sheet, cell, text, layer, .. } => {
                            events.push(MatchEvent::Context {
                                path: path.to_path_buf(),
                                sheet,
                                cell,
                                layer,
                                text,
                            });
                        }
                        MatchEvent::Separator => events.push(MatchEvent::Separator),
                        _ => unreachable!("emit_with_context only emits Match/Context/Separator"),
                    }
                }
            }
            events.push(MatchEvent::FileEnd {
                path: path.to_path_buf(),
                stats: FileStats {
                    matches,
                    sheets_scanned: sheets_seen.len() as u32,
                },
            });
        }
        Err(e) => {
            events.push(MatchEvent::Error {
                path: path.to_path_buf(),
                message: e.to_string(),
            });
            events.push(MatchEvent::FileEnd {
                path: path.to_path_buf(),
                stats: FileStats::default(),
            });
        }
    }
    FileBlock { events }
}

/// Three-pass context emit: given the full `cells` vector (in reader order)
/// and a flat list of (sheet, row) match positions, returns Match / Context /
/// Separator events with `--` inserted between disjoint spans within a sheet
/// AND at sheet boundaries.
///
/// Returned events use `path: PathBuf::new()`. Caller (`search_file`) re-fills
/// `path` and computes `submatches`.
pub(crate) fn emit_with_context(
    cells: &[crate::reader::CellRecord],
    match_rows: &[(String, u32)],
    ctx: ContextOptions,
) -> Vec<MatchEvent> {
    use std::collections::{BTreeSet, HashMap, HashSet};

    // Pass 2: per-sheet, union (match_row ± N) into a BTreeSet<u32>.
    let mut context_rows: HashMap<String, BTreeSet<u32>> = HashMap::new();
    for (sheet, r) in match_rows {
        let lo = r.saturating_sub(ctx.before).max(1);
        let hi = r.saturating_add(ctx.after);
        let set = context_rows.entry(sheet.clone()).or_default();
        for rr in lo..=hi {
            set.insert(rr);
        }
    }
    let match_set: HashSet<(String, u32)> = match_rows.iter().cloned().collect();

    // Pass 3: walk cells in order, emit Match/Context/Separator.
    let mut out: Vec<MatchEvent> = Vec::new();
    let mut prev: Option<(String, u32)> = None;
    for c in cells {
        let r = match crate::cell::row_from_a1(&c.cell) {
            Some(r) => r,
            None => continue,
        };
        let in_ctx = context_rows
            .get(&c.sheet)
            .is_some_and(|s| s.contains(&r));
        if !in_ctx {
            continue;
        }

        // Separator: when we leave a contiguous run inside the same sheet
        // (row gap > 1), OR when we transition between sheets.
        if let Some((ref prev_sheet, prev_row)) = prev {
            let need_sep = prev_sheet != &c.sheet || r > prev_row + 1;
            if need_sep {
                out.push(MatchEvent::Separator);
            }
        }
        prev = Some((c.sheet.clone(), r));

        let layer = c.layer.as_str().to_string();
        if match_set.contains(&(c.sheet.clone(), r)) {
            out.push(MatchEvent::Match {
                path: std::path::PathBuf::new(),
                sheet: c.sheet.clone(),
                cell: c.cell.clone(),
                layer,
                text: c.text.clone(),
                submatches: Vec::new(),
            });
        } else {
            out.push(MatchEvent::Context {
                path: std::path::PathBuf::new(),
                sheet: c.sheet.clone(),
                cell: c.cell.clone(),
                layer,
                text: c.text.clone(),
            });
        }
    }
    out
}

#[cfg(test)]
mod context_tests {
    use super::*;
    use crate::config::Layer;
    use crate::reader::CellRecord;

    fn cell(sheet: &str, cell: &str, text: &str) -> CellRecord {
        CellRecord {
            sheet: sheet.to_string(),
            cell: cell.to_string(),
            layer: Layer::Display,
            text: text.to_string(),
        }
    }

    fn kinds(events: &[MatchEvent]) -> Vec<&'static str> {
        events
            .iter()
            .filter_map(|e| match e {
                MatchEvent::Match { .. } => Some("M"),
                MatchEvent::Context { .. } => Some("C"),
                MatchEvent::Separator => Some("S"),
                _ => None,
            })
            .collect()
    }

    fn cells_seen(events: &[MatchEvent]) -> Vec<String> {
        events
            .iter()
            .filter_map(|e| match e {
                MatchEvent::Match { cell, .. } | MatchEvent::Context { cell, .. } => {
                    Some(cell.clone())
                }
                _ => None,
            })
            .collect()
    }

    #[test]
    fn context_expansion_unions_adjacent_spans() {
        // Sheet1 rows 1..=10. Matches at rows 5 and 7. -C 2 → ctx rows
        // {3..=9}. Expect: ctx 3,4, M 5, ctx 6, M 7, ctx 8,9. NO Separator.
        let cells: Vec<_> = (1..=10)
            .map(|r| cell("Sheet1", &format!("A{r}"), &format!("v{r}")))
            .collect();
        let matches = vec![("Sheet1".to_string(), 5u32), ("Sheet1".to_string(), 7u32)];
        let events = emit_with_context(&cells, &matches, ContextOptions { before: 2, after: 2 });
        let k = kinds(&events);
        assert!(!k.contains(&"S"), "expected no separator in adjacent union; got {:?}", k);
        assert_eq!(k.iter().filter(|&&x| x == "M").count(), 2);
        assert_eq!(k.iter().filter(|&&x| x == "C").count(), 5);
    }

    #[test]
    fn context_expansion_inserts_separator_between_disjoint() {
        // Matches at rows 5 and 20. -C 1 → {4,5,6, 19,20,21}. One "--" between.
        let cells: Vec<_> = (1..=25)
            .map(|r| cell("Sheet1", &format!("A{r}"), &format!("v{r}")))
            .collect();
        let matches = vec![("Sheet1".to_string(), 5u32), ("Sheet1".to_string(), 20u32)];
        let events = emit_with_context(&cells, &matches, ContextOptions { before: 1, after: 1 });
        let k = kinds(&events);
        assert_eq!(k.iter().filter(|&&x| x == "S").count(), 1, "got {:?}", k);
    }

    #[test]
    fn context_clips_at_row_one() {
        // Match at row 2 with -B 5 should clip at row 1 (no A0, no panic).
        let cells: Vec<_> = (1..=5)
            .map(|r| cell("Sheet1", &format!("A{r}"), &format!("v{r}")))
            .collect();
        let matches = vec![("Sheet1".to_string(), 2u32)];
        let events = emit_with_context(&cells, &matches, ContextOptions { before: 5, after: 0 });
        let cs = cells_seen(&events);
        assert!(cs.contains(&"A1".to_string()));
        assert!(cs.contains(&"A2".to_string()));
        assert!(!cs.iter().any(|c| c == "A0"));
    }

    #[test]
    fn context_cross_sheet_isolation() {
        // Sheet1 + Sheet2 each have a match at row 5, -A 2. Expect:
        // S1: M, C, C, Separator, S2: M, C, C.
        let mut cells = Vec::new();
        for r in 1..=10 {
            cells.push(cell("Sheet1", &format!("A{r}"), &format!("s1-{r}")));
        }
        for r in 1..=10 {
            cells.push(cell("Sheet2", &format!("A{r}"), &format!("s2-{r}")));
        }
        let matches = vec![
            ("Sheet1".to_string(), 5u32),
            ("Sheet2".to_string(), 5u32),
        ];
        let events = emit_with_context(&cells, &matches, ContextOptions { before: 0, after: 2 });
        let k = kinds(&events);
        assert_eq!(k, vec!["M", "C", "C", "S", "M", "C", "C"]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Layer;

    #[test]
    fn bytes_to_char_ascii() {
        let s = "hello world";
        let sm = bytes_to_char_range(s, 6, 11);
        assert_eq!(sm.start, 6);
        assert_eq!(sm.end, 11);
        assert_eq!(sm.matched.text, "world");
    }

    #[test]
    fn bytes_to_char_cjk() {
        let s = "abc张三def";
        let sm = bytes_to_char_range(s, 3, 9);
        assert_eq!(sm.start, 3);
        assert_eq!(sm.end, 5);
        assert_eq!(sm.matched.text, "张三");
    }

    #[test]
    fn file_stats_default() {
        let s = FileStats::default();
        assert_eq!(s.matches, 0);
        assert_eq!(s.sheets_scanned, 0);
    }

    #[test]
    fn match_event_serializes_to_json() {
        let ev = MatchEvent::Match {
            path: PathBuf::from("a.xlsx"),
            sheet: "Sheet1".into(),
            cell: "B3".into(),
            layer: Layer::Display.as_str().into(),
            text: "张三".into(),
            submatches: vec![Submatch {
                matched: SubmatchText {
                    text: "张三".into(),
                },
                start: 0,
                end: 2,
            }],
        };
        let json = serde_json::to_string(&ev).unwrap();
        assert!(json.contains("\"type\":\"match\""));
        assert!(json.contains("\"data\":"));
        assert!(json.contains("\"match\":{\"text\":\"张三\"}"));
        assert!(json.contains("\"cell\":\"B3\""));
        assert!(json.contains("\"layer\":\"display\""));
    }
}
