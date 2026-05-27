//! Debug-only invariant: if the fast-path decided to skip a sheet, the
//! fallback parse on that sheet must yield zero `CellRecord`s that contain a
//! match for the user pattern. Otherwise, the fast-path is incorrect and we
//! must panic loudly in tests.
//!
//! This file is `cfg(debug_assertions)`-gated at the call site (see
//! `reader/mod.rs::read_cells`). The functions themselves are unconditionally
//! compiled to keep the module testable on its own.

use crate::matcher::Pattern;
use crate::reader::CellRecord;

/// Panic if any of the `fallback_cells` matches `pattern`. Called from the
/// "skip" branch of `read_cells` in debug builds.
pub fn assert_skipped_safely(sheet_name: &str, fallback_cells: &[CellRecord], pattern: &Pattern) {
    for c in fallback_cells {
        if pattern.is_match(&c.text) {
            panic!(
                "fast-path silently skipped sheet {:?} but cell {} (layer {:?}) text {:?} matches pattern {:?}",
                sheet_name, c.cell, c.layer, c.text, pattern.raw(),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Layer;
    use crate::matcher::{CaseMode, Pattern};

    fn rec(text: &str) -> CellRecord {
        CellRecord {
            sheet: "S".into(),
            cell: "A1".into(),
            layer: Layer::Display,
            text: text.into(),
        }
    }
    fn p(s: &str) -> Pattern {
        Pattern::compile(s, CaseMode::Sensitive, false, false).unwrap()
    }

    #[test]
    fn empty_fallback_is_safe() {
        assert_skipped_safely("S", &[], &p("foo"));
    }

    #[test]
    fn fallback_without_match_is_safe() {
        assert_skipped_safely("S", &[rec("nothing here")], &p("foo"));
    }

    #[test]
    #[should_panic(expected = "fast-path silently skipped")]
    fn fallback_with_match_panics() {
        assert_skipped_safely("S", &[rec("contains foo here")], &p("foo"));
    }
}
