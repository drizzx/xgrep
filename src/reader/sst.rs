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
use crate::reader::comments::xml_unescape;
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
    let Some(xml) = index.read_to_string("xl/sharedStrings.xml")? else {
        return Ok(Vec::new());
    };
    // Each <si> is one sst entry; entries may contain multiple <t> children
    // (rich text). Capture each <si>...</si>, then concatenate its <t>...</t>
    // children with xml-unescape applied to each text node.
    let re_si = regex::Regex::new(r#"<si\b[^>]*>([\s\S]*?)</si>"#).unwrap();
    let re_t = regex::Regex::new(r#"<t[^>]*>([\s\S]*?)</t>"#).unwrap();
    let mut out = Vec::new();
    for cap in re_si.captures_iter(&xml) {
        let body = &cap[1];
        let s: String = re_t
            .captures_iter(body)
            .map(|c| xml_unescape(&c[1]))
            .collect::<Vec<_>>()
            .join("");
        out.push(s);
    }
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
}
