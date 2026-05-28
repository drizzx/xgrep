//! Shared strings (`xl/sharedStrings.xml`) parse + hit-index set construction.
//!
//! ## Why the regex-only parse
//! sharedStrings is a flat sequence of `<si>...</si>` entries; rich-text entries
//! contain multiple `<t>` runs which we concatenate. This is the same pattern
//! used for comments — both are "find all <t>...</t> within each item." We
//! deliberately do NOT pull in quick-xml etc.: regex is already a runtime dep,
//! and the sst grammar is tiny enough.

use crate::error::SearchError;
use crate::matcher::Pattern;
use crate::reader::zip_index::ZipIndex;

#[derive(Debug, Default)]
pub struct HitSet {
    bits: Vec<u64>,
    len: usize,
}

impl HitSet {
    pub fn new(n: usize) -> Self {
        let words = n.div_ceil(64);
        Self {
            bits: vec![0; words],
            len: n,
        }
    }
    pub fn insert(&mut self, i: usize) {
        if i >= self.len {
            return;
        }
        self.bits[i / 64] |= 1u64 << (i % 64);
    }
    pub fn contains(&self, i: usize) -> bool {
        if i >= self.len {
            return false;
        }
        (self.bits[i / 64] >> (i % 64)) & 1 == 1
    }
    pub fn is_empty(&self) -> bool {
        self.bits.iter().all(|w| *w == 0)
    }
    /// Returns the total number of sst entries this HitSet was sized for
    /// (i.e. the sst length at construction time).
    pub fn len(&self) -> usize {
        self.len
    }
    pub fn iter(&self) -> impl Iterator<Item = usize> + '_ {
        (0..self.len).filter(|&i| self.contains(i))
    }
    pub fn count(&self) -> usize {
        self.bits.iter().map(|w| w.count_ones() as usize).sum()
    }
}

/// Parse `xl/sharedStrings.xml` into a Vec<String>. Index order matches the
/// sst's <si> order, which is what xlsx `<v>idx</v>` references use.
pub fn parse(index: &mut ZipIndex) -> Result<Vec<String>, SearchError> {
    use std::ops::ControlFlow;
    let Some(xml) = index.read_to_string("xl/sharedStrings.xml")? else {
        return Ok(Vec::new());
    };
    let mut out = Vec::new();
    crate::reader::xml_scan::for_each_tag(xml.as_bytes(), "si", |_attrs, body| {
        let mut text = String::new();
        crate::reader::xml_scan::for_each_tag(body, "t", |_t_attrs, t_body| {
            text.push_str(&crate::reader::xml_scan::xml_unescape(t_body));
            ControlFlow::Continue(())
        });
        out.push(text);
        ControlFlow::Continue(())
    });
    Ok(out)
}

/// Build a HitSet marking sst indices whose string matches `pattern`.
/// If pattern is None, returns an empty (all-zero) HitSet of the right length.
pub fn build_hit_set(sst: &[String], pattern: Option<&Pattern>) -> HitSet {
    let mut hs = HitSet::new(sst.len());
    if let Some(p) = pattern {
        for (i, s) in sst.iter().enumerate() {
            if p.is_match(s) {
                hs.insert(i);
            }
        }
    }
    hs
}

/// Streaming variant of `parse` + `build_hit_set` with early abort.
///
/// Scans `xl/sharedStrings.xml` one `<si>` entry at a time. For each entry,
/// applies `pattern` and accumulates hit indices. As soon as the hit count
/// exceeds `abort_threshold`, parsing stops and returns `aborted = true`.
///
/// When aborted, the returned `sst` is partial (only the entries seen so far)
/// and `hit_set` may not include all matching indices. Callers MUST bypass
/// fast-path entirely when aborted is true — using the truncated `hit_set`
/// in `fast_path::augment` would produce a regex that misses references to
/// unscanned sst entries, leading to silent wrong-result bugs.
///
/// Motivation: `parse` always scans the full sst, paying O(sst_size) regex
/// cost. For workbooks with very large sst pools (>50k entries) AND patterns
/// that match many entries, this cost is pure waste because the fast-path
/// can't skip any sheets anyway (TooManyHits branch). Early abort caps the
/// cost at O(abort_threshold) for the wasted-effort case.
pub fn parse_with_early_abort(
    index: &mut ZipIndex,
    pattern: Option<&Pattern>,
    abort_threshold: usize,
) -> Result<(Vec<String>, HitSet, bool), SearchError> {
    let Some(xml) = index.read_to_string("xl/sharedStrings.xml")? else {
        return Ok((Vec::new(), HitSet::new(0), false));
    };
    Ok(parse_xml_with_early_abort(&xml, pattern, abort_threshold))
}

/// Pure helper exposed for unit testing: takes the raw xml string and runs
/// the streaming parse + early-abort logic. `parse_with_early_abort` is just
/// `read_to_string` + this.
fn parse_xml_with_early_abort(
    xml: &str,
    pattern: Option<&Pattern>,
    abort_threshold: usize,
) -> (Vec<String>, HitSet, bool) {
    use std::ops::ControlFlow;
    let mut sst = Vec::new();
    let mut hit_indices: Vec<usize> = Vec::new();
    let aborted = crate::reader::xml_scan::for_each_tag(
        xml.as_bytes(),
        "si",
        |_attrs, body| {
            let idx = sst.len();
            let mut text = String::new();
            crate::reader::xml_scan::for_each_tag(body, "t", |_t_attrs, t_body| {
                text.push_str(&crate::reader::xml_scan::xml_unescape(t_body));
                ControlFlow::Continue(())
            });
            if let Some(p) = pattern {
                if p.is_match(&text) {
                    hit_indices.push(idx);
                    if hit_indices.len() > abort_threshold {
                        sst.push(text);
                        return ControlFlow::Break(());
                    }
                }
            }
            sst.push(text);
            ControlFlow::Continue(())
        },
    );
    let mut hs = HitSet::new(sst.len());
    for &i in &hit_indices {
        hs.insert(i);
    }
    (sst, hs, aborted)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::matcher::{CaseMode, Pattern};

    fn p(s: &str) -> Pattern {
        Pattern::compile(s, CaseMode::Sensitive, false, false).unwrap()
    }

    #[test]
    fn hitset_basic_insert_contains() {
        let mut hs = HitSet::new(130);
        hs.insert(0);
        hs.insert(64);
        hs.insert(129);
        assert!(hs.contains(0));
        assert!(hs.contains(64));
        assert!(hs.contains(129));
        assert!(!hs.contains(1));
        assert!(!hs.contains(65));
        assert!(!hs.contains(128));
        assert_eq!(hs.count(), 3);
        assert!(!hs.is_empty());
    }

    #[test]
    fn hitset_empty_has_no_bits() {
        let hs = HitSet::new(10);
        assert!(hs.is_empty());
        assert_eq!(hs.count(), 0);
        assert_eq!(hs.iter().count(), 0);
    }

    #[test]
    fn build_hit_set_marks_matching_indices() {
        let sst = vec!["foo".into(), "bar".into(), "foobar".into()];
        let hs = build_hit_set(&sst, Some(&p("foo")));
        assert!(hs.contains(0));
        assert!(!hs.contains(1));
        assert!(hs.contains(2));
        assert_eq!(hs.count(), 2);
    }

    #[test]
    fn build_hit_set_no_pattern_yields_empty() {
        let sst = vec!["foo".into(), "bar".into()];
        let hs = build_hit_set(&sst, None);
        assert!(hs.is_empty());
    }

    #[test]
    fn build_hit_set_empty_when_no_matches() {
        let sst = vec!["alpha".into(), "beta".into()];
        let hs = build_hit_set(&sst, Some(&p("zeta")));
        assert!(hs.is_empty());
    }

    #[test]
    fn parse_xml_with_early_abort_no_abort_below_threshold() {
        // 5 matches, threshold 10 -> no abort, full sst returned
        let mut xml = String::from("<sst>");
        for i in 0..5 {
            xml.push_str(&format!("<si><t>hit-{i}</t></si>"));
        }
        for i in 0..5 {
            xml.push_str(&format!("<si><t>miss-{i}</t></si>"));
        }
        xml.push_str("</sst>");
        let (sst, hs, aborted) = parse_xml_with_early_abort(&xml, Some(&p("hit")), 10);
        assert!(!aborted);
        assert_eq!(sst.len(), 10);
        assert_eq!(hs.count(), 5);
    }

    #[test]
    fn parse_xml_with_early_abort_aborts_at_threshold() {
        // 50 matches, threshold 10 -> aborts after 11 hits (10+1), sst is partial
        let mut xml = String::from("<sst>");
        for i in 0..50 {
            xml.push_str(&format!("<si><t>hit-{i}</t></si>"));
        }
        xml.push_str("</sst>");
        let (sst, _hs, aborted) = parse_xml_with_early_abort(&xml, Some(&p("hit")), 10);
        assert!(aborted);
        // We stopped after seeing 11 matches; sst contains those 11 entries.
        assert!(sst.len() <= 12, "sst should be partial, got len {}", sst.len());
    }

    #[test]
    fn parse_xml_with_early_abort_no_pattern_never_aborts() {
        // pattern = None means no matches counted, threshold doesn't matter
        let mut xml = String::from("<sst>");
        for i in 0..100 {
            xml.push_str(&format!("<si><t>x-{i}</t></si>"));
        }
        xml.push_str("</sst>");
        let (sst, hs, aborted) = parse_xml_with_early_abort(&xml, None, 1);
        assert!(!aborted);
        assert_eq!(sst.len(), 100);
        assert!(hs.is_empty());
    }

    #[test]
    fn parse_xml_with_early_abort_empty_xml_returns_empty() {
        let (sst, hs, aborted) = parse_xml_with_early_abort("", Some(&p("foo")), 10);
        assert!(!aborted);
        assert_eq!(sst.len(), 0);
        assert!(hs.is_empty());
    }
}
