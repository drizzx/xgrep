//! Error types and the rg-aligned process exit code mapping.

use std::io;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum SearchError {
    #[error("xlsx parse error: {0}")]
    Parse(String),
    #[error("sheet {sheet:?}: {msg}")]
    Sheet { sheet: String, msg: String },
    #[error("encrypted workbook (not supported)")]
    Encrypted,
    #[error("io error: {0}")]
    Io(#[from] io::Error),
}

/// rg-aligned process exit code.
///
/// - 0: at least one match was found.
/// - 1: no matches and no fatal errors.
/// - 2: a fatal error happened (CLI/regex/early-exit). Per-file errors do not
///   themselves produce code 2 — they are reported on stderr while scanning continues.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExitCode {
    Match,
    NoMatch,
    Fatal,
}

impl ExitCode {
    pub fn as_i32(self) -> i32 {
        match self {
            ExitCode::Match => 0,
            ExitCode::NoMatch => 1,
            ExitCode::Fatal => 2,
        }
    }

    pub fn from_outcome(matches: u64, fatal: bool) -> ExitCode {
        if fatal {
            ExitCode::Fatal
        } else if matches > 0 {
            ExitCode::Match
        } else {
            ExitCode::NoMatch
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exit_code_match() {
        assert_eq!(ExitCode::from_outcome(3, false), ExitCode::Match);
        assert_eq!(ExitCode::Match.as_i32(), 0);
    }

    #[test]
    fn exit_code_no_match() {
        assert_eq!(ExitCode::from_outcome(0, false), ExitCode::NoMatch);
        assert_eq!(ExitCode::NoMatch.as_i32(), 1);
    }

    #[test]
    fn exit_code_fatal_overrides_matches() {
        // rg semantic: even with matches, a fatal early-exit error should be 2.
        assert_eq!(ExitCode::from_outcome(5, true), ExitCode::Fatal);
        assert_eq!(ExitCode::Fatal.as_i32(), 2);
    }
}
