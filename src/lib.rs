//! xgrep — Excel-aware grep.

pub mod cell;
pub mod config;
pub mod error;
pub mod matcher;
pub mod printer;
pub mod reader;
pub mod walker;
pub mod worker;

use std::path::PathBuf;
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", content = "data", rename_all = "snake_case")]
pub enum MatchEvent {
    /// `{"type":"begin","data":{"path":...}}` per spec §4.5
    #[serde(rename = "begin")]
    FileBegin { path: PathBuf },
    Match {
        path: PathBuf,
        sheet: String,
        cell: String,       // A1 notation
        layer: String,      // Layer::as_str()
        text: String,
        submatches: Vec<Submatch>,
    },
    #[serde(rename = "end")]
    FileEnd { path: PathBuf, stats: FileStats },
    Error { path: PathBuf, message: String },
}

#[derive(Debug, Clone, Copy, Serialize)]
pub struct Submatch {
    /// 0-indexed Unicode character offset, half-open [start, end).
    pub start: usize,
    pub end: usize,
}

#[derive(Debug, Clone, Copy, Default, Serialize)]
pub struct FileStats {
    pub matches: u64,
    pub sheets_scanned: u32,
}

/// A FileBlock is the unit of atomic output: FileBegin .. Match* .. FileEnd
/// (or an Error event in place of the body). Workers emit one per file.
#[derive(Debug, Clone)]
pub struct FileBlock { pub events: Vec<MatchEvent> }

/// Convert a regex byte range (matcher returns bytes) into a 0-indexed Unicode
/// character range over the same haystack.
pub fn bytes_to_char_range(haystack: &str, start_b: usize, end_b: usize) -> Submatch {
    let start = haystack[..start_b].chars().count();
    let end = start + haystack[start_b..end_b].chars().count();
    Submatch { start, end }
}

use crate::matcher::Pattern;
use crate::reader::{read_cells, ReaderOptions};
use std::collections::HashSet;

/// Search a single xlsx file, returning a complete FileBlock ready for the printer.
///
/// `invert`: if true, emit matches for cells whose text *does not* match the pattern
/// (rg `-v`).
pub fn search_file(
    path: &std::path::Path,
    pattern: &Pattern,
    reader_opts: &ReaderOptions,
    invert: bool,
) -> FileBlock {
    let mut events = vec![MatchEvent::FileBegin { path: path.to_path_buf() }];
    let mut matches = 0u64;
    let mut sheets_seen: HashSet<String> = HashSet::new();

    match read_cells(path, reader_opts) {
        Ok(cells) => {
            for c in cells {
                sheets_seen.insert(c.sheet.clone());
                let hits: Vec<_> = pattern.find_iter(&c.text).collect();
                let has_hit = !hits.is_empty();
                let emit = if invert { !has_hit } else { has_hit };
                if !emit { continue; }
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
    }

    #[test]
    fn bytes_to_char_cjk() {
        // "张三" = 6 bytes (3 each), 2 chars
        let s = "abc张三def";
        // Match "张三" at bytes 3..9 -> chars 3..5
        let sm = bytes_to_char_range(s, 3, 9);
        assert_eq!(sm.start, 3);
        assert_eq!(sm.end, 5);
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
            submatches: vec![Submatch { start: 0, end: 2 }],
        };
        let json = serde_json::to_string(&ev).unwrap();
        assert!(json.contains("\"type\":\"match\""));
        assert!(json.contains("\"data\":"));
        assert!(json.contains("\"cell\":\"B3\""));
        assert!(json.contains("\"layer\":\"display\""));
    }
}
