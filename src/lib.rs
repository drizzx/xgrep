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
pub fn search_file(
    path: &std::path::Path,
    pattern: &Pattern,
    reader_opts: &ReaderOptions<'_>,
    invert: bool,
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
