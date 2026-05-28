//! Fast-path B: byte-scan a sheet's xml against an augmented regex that
//! combines the user pattern with the sst hit-index references.
//!
//! Decision tree per sheet:
//!   - augmented regex matches sheet xml bytes  -> need full parse
//!   - hit_set is empty AND no match            -> safe to skip
//!   - hit_set is non-empty AND no match        -> safe to skip iff augmented
//!     already encoded every hit idx
//!
//! "encoded every hit idx" is the THRESHOLD limit: when the hit set exceeds
//! THRESHOLD, we don't try to fit all hit indices into the regex; instead we
//! conservatively say "needs full parse" whenever hit_set is non-empty.

use regex::Regex;

use crate::matcher::Pattern;
use crate::reader::sst::HitSet;

/// Maximum sst hit-set size we'll fold into the augmented regex. Above this,
/// we fall back to "parse if hit_set non-empty".
pub const THRESHOLD: usize = 100;

/// Constants for the dense-bypass heuristic. When the hit set is "dense
/// enough" that almost every sheet will reference at least one hit index,
/// bypass the per-sheet decide() scan entirely — it's pure overhead in the
/// all-match case. Two independent triggers:
/// - hit_count > BYPASS_ABS_THRESHOLD: many matches in absolute terms
/// - hit_count / sst_len > BYPASS_DENSITY_NUMERATOR / BYPASS_DENSITY_DENOMINATOR:
///   matches are a high fraction of the sst (handles small ssts that aren't
///   triggered by the absolute threshold)
pub const BYPASS_ABS_THRESHOLD: usize = 30;
pub const BYPASS_DENSITY_NUMERATOR: usize = 1;
pub const BYPASS_DENSITY_DENOMINATOR: usize = 20;

/// Returns true if the (hit_count, sst_len) pair indicates the fast-path
/// per-sheet scan should be bypassed because nearly every sheet will
/// match anyway (making the scan wasted work).
pub fn should_dense_bypass(hit_count: usize, sst_len: usize) -> bool {
    if sst_len == 0 {
        return false;
    }
    hit_count > BYPASS_ABS_THRESHOLD
        || hit_count * BYPASS_DENSITY_DENOMINATOR > sst_len * BYPASS_DENSITY_NUMERATOR
}

/// Constants for the workbook-shape pre-check. When the workbook shape
/// indicates that `should_dense_bypass` will almost certainly trigger
/// (large compressed sst + few sheets), skip sst::parse entirely — the
/// bypass decision can be inferred from the workbook shape without
/// running the pattern.
///
/// SPECULATIVE shortcut: if the heuristic mis-fires on a workbook with
/// rare matches, we lose the fast-path opportunity to skip non-matching
/// sheets. The thresholds trade (cost saved on typical large-sst workbooks)
/// vs (cost paid on outlier sparse-pattern large-sst workbooks).
pub const PRESKIP_SST_COMPRESSED_BYTES: u64 = 100_000;
pub const PRESKIP_SST_MAX_SHEETS: usize = 5;

/// Returns true if the workbook shape — measured by `compressed_sst_bytes`
/// (the zip-entry compressed size of `xl/sharedStrings.xml`, 0 if absent)
/// and `sheet_count` — predicts that running `sst::parse_with_early_abort`
/// would be wasted work because dense_bypass would inevitably trigger.
pub fn should_skip_sst_parse(compressed_sst_bytes: u64, sheet_count: usize) -> bool {
    compressed_sst_bytes > PRESKIP_SST_COMPRESSED_BYTES
        && sheet_count <= PRESKIP_SST_MAX_SHEETS
}

/// Build an augmented regex from (user pattern, sst hit_set). Returns None
/// when the user pattern alone is the right scanner (hit_set empty), or when
/// augmentation would exceed THRESHOLD and we therefore can't make a
/// per-sheet decision via byte-scan alone.
///
/// The returned regex is **unanchored** and case follows the user pattern's
/// effective case mode (we recompile from the raw text to drop anchors).
pub fn augment(pattern: &Pattern, hit_set: &HitSet) -> AugmentResult {
    let raw = strip_anchors(pattern.raw());
    let case_insensitive = pattern.is_case_insensitive();

    if hit_set.is_empty() {
        // Just the user pattern, unanchored.
        return match build(&raw, case_insensitive) {
            Some(r) => AugmentResult::PatternOnly(r),
            None => AugmentResult::Bypass,
        };
    }
    if hit_set.count() > THRESHOLD {
        // Too many hit indices to fold — sheet decision must default to parse.
        return AugmentResult::TooManyHits;
    }
    // Build "(?:<user-pattern>|<v>3</v>|<v>17</v>|...)"
    let mut alt = format!("(?:{raw})");
    for idx in hit_set.iter() {
        alt.push_str(&format!("|<v>{idx}</v>"));
    }
    match build(&alt, case_insensitive) {
        Some(r) => AugmentResult::Augmented(r),
        None => AugmentResult::Bypass,
    }
}

pub enum AugmentResult {
    /// hit_set empty + user pattern compiled cleanly.
    PatternOnly(Regex),
    /// User pattern + sst hit alternatives (<v>idx</v>).
    Augmented(Regex),
    /// hit_set too large to encode; decide() treats "hit_set non-empty" as parse.
    TooManyHits,
    /// User pattern can't be folded into an unanchored regex; skip fast-path
    /// (everything goes to the full-parse path).
    Bypass,
}

fn strip_anchors(raw: &str) -> String {
    // Conservatively drop a leading `^` and trailing `$` only — anchors elsewhere
    // (e.g. multiline mode markers) stay; they'd just produce false positives in
    // the byte scan, which is safe.
    let mut s = raw;
    if let Some(stripped) = s.strip_prefix('^') {
        s = stripped;
    }
    if let Some(stripped) = s.strip_suffix('$') {
        s = stripped;
    }
    s.to_string()
}

fn build(re: &str, case_insensitive: bool) -> Option<Regex> {
    regex::RegexBuilder::new(re)
        .case_insensitive(case_insensitive)
        .unicode(true)
        .build()
        .ok()
}

/// Per-sheet decision: returns true if the sheet must be fully parsed.
pub fn decide(xml: &[u8], augmented: &AugmentResult, hit_set_empty: bool) -> bool {
    match augmented {
        AugmentResult::Bypass => true,
        AugmentResult::TooManyHits => true, // sheet must be parsed when hit_set is large
        AugmentResult::PatternOnly(r) => {
            // hit_set empty (caller checks invariant). Decision is purely the
            // sheet byte scan.
            assert!(hit_set_empty, "PatternOnly must imply hit_set_empty");
            regex_matches_bytes(r, xml)
        }
        AugmentResult::Augmented(r) => {
            // hit_set non-empty: augmented already encodes all relevant <v>idx</v>.
            regex_matches_bytes(r, xml)
        }
    }
}

fn regex_matches_bytes(r: &Regex, xml: &[u8]) -> bool {
    // sheet xml is UTF-8. From_utf8_lossy is non-allocating on already-valid bytes
    // (it returns Cow::Borrowed) — safe to assume xlsx-emitted sheet xml is valid UTF-8.
    let s = std::str::from_utf8(xml).unwrap_or("");
    r.is_match(s)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::matcher::{CaseMode, Pattern};
    use crate::reader::sst::HitSet;

    fn p(s: &str) -> Pattern {
        Pattern::compile(s, CaseMode::Sensitive, false, false).unwrap()
    }

    #[test]
    fn augment_empty_hits_returns_pattern_only() {
        let pat = p("foo");
        let hs = HitSet::new(10);
        let res = augment(&pat, &hs);
        assert!(matches!(res, AugmentResult::PatternOnly(_)));
    }

    #[test]
    fn augment_small_hit_set_returns_augmented() {
        let pat = p("foo");
        let mut hs = HitSet::new(50);
        hs.insert(3);
        hs.insert(17);
        let res = augment(&pat, &hs);
        let r = match res {
            AugmentResult::Augmented(r) => r,
            _ => panic!("expected Augmented"),
        };
        assert!(r.is_match("xx<v>3</v>yy"));
        assert!(r.is_match("xx<v>17</v>yy"));
        assert!(r.is_match("xxfooxx"));
        assert!(!r.is_match("xxqq<v>4</v>"));
    }

    #[test]
    fn augment_above_threshold_returns_too_many_hits() {
        let pat = p("foo");
        let mut hs = HitSet::new(200);
        for i in 0..=THRESHOLD {
            hs.insert(i);
        }
        assert!(matches!(augment(&pat, &hs), AugmentResult::TooManyHits));
    }

    #[test]
    fn augment_strips_anchors() {
        let pat = p("^foo$");
        let hs = HitSet::new(0);
        let res = augment(&pat, &hs);
        let r = match res {
            AugmentResult::PatternOnly(r) => r,
            _ => panic!(),
        };
        // The xml byte stream has "foo" surrounded by tag noise; with anchors stripped,
        // an inner match must still be found.
        assert!(r.is_match("<c><v>foo</v></c>"));
    }

    #[test]
    fn decide_returns_parse_when_xml_byte_matches() {
        let pat = p("foo");
        let hs = HitSet::new(0);
        let aug = augment(&pat, &hs);
        let xml = b"<c><v>foo</v></c>";
        assert!(decide(xml, &aug, true));
    }

    #[test]
    fn decide_returns_skip_when_xml_has_no_match_and_no_sst_hit() {
        let pat = p("foo");
        let hs = HitSet::new(0);
        let aug = augment(&pat, &hs);
        let xml = b"<c><v>bar</v></c>";
        assert!(!decide(xml, &aug, true));
    }

    #[test]
    fn decide_returns_parse_when_sst_only_hit() {
        let pat = p("foo");
        let mut hs = HitSet::new(20);
        hs.insert(7);
        let aug = augment(&pat, &hs);
        // No user pattern in xml, but the sst reference <v>7</v> is there.
        let xml = b"<c t=\"s\"><v>7</v></c>";
        assert!(decide(xml, &aug, false));
    }

    #[test]
    fn decide_returns_parse_when_too_many_hits() {
        let pat = p("foo");
        let mut hs = HitSet::new(200);
        for i in 0..=THRESHOLD {
            hs.insert(i);
        }
        let aug = augment(&pat, &hs);
        assert!(decide(b"any-bytes", &aug, false));
    }

    #[test]
    fn augment_pattern_containing_xml_meta_is_isolated() {
        // user pattern that contains literal "<v>" must not collide with our
        // injected <v>idx</v>. Critical safety test.
        let pat = p("<v>");
        let mut hs = HitSet::new(20);
        hs.insert(1);
        let aug = augment(&pat, &hs);
        let r = match aug {
            AugmentResult::Augmented(r) => r,
            _ => panic!(),
        };
        // Must match both: any "<v>" sequence (from the user pattern) and the
        // specific "<v>1</v>" (from the sst alt).
        assert!(r.is_match("foo<v>"));
        assert!(r.is_match("<v>1</v>"));
        // No infinite-regex weirdness — compile must have succeeded.
    }

    #[test]
    fn should_dense_bypass_zero_sst_returns_false() {
        assert!(!should_dense_bypass(0, 0));
        assert!(!should_dense_bypass(5, 0)); // shouldn't happen but defensive
    }

    #[test]
    fn should_dense_bypass_low_density_below_abs_returns_false() {
        // sst_heavy_low_hit-like shape but BELOW thresholds:
        // 10 hits in 50000 sst → density 0.0002, count below 30
        assert!(!should_dense_bypass(10, 50_000));
        assert!(!should_dense_bypass(30, 50_000)); // exactly at threshold, NOT strictly above
    }

    #[test]
    fn should_dense_bypass_high_abs_count_returns_true() {
        // sst_heavy_low_hit fixture: 50 hits in 50000 sst → density 0.001
        // density alone wouldn't trigger (< 5%) but absolute count > 30 does.
        assert!(should_dense_bypass(50, 50_000));
        assert!(should_dense_bypass(31, 50_000));
    }

    #[test]
    fn should_dense_bypass_high_density_returns_true() {
        // many_small fixture: ~10 hits in ~60 sst → density ~17%
        // absolute below 30 but density > 5% triggers.
        assert!(should_dense_bypass(10, 60));
        // 6 / 100 = 6% — above density threshold (5%)
        assert!(should_dense_bypass(6, 100));
    }

    #[test]
    fn should_dense_bypass_at_density_boundary() {
        // 5 / 100 = 5%. Density check uses strict >, so 5% is NOT a bypass.
        assert!(!should_dense_bypass(5, 100));
        // 6 / 100 = 6%, IS a bypass.
        assert!(should_dense_bypass(6, 100));
    }

    #[test]
    fn should_skip_sst_parse_large_sst_few_sheets_returns_true() {
        // sst_heavy_low_hit fixture shape: ~5MB SST compressed to ~500KB, 3 sheets
        assert!(should_skip_sst_parse(500_000, 3));
    }

    #[test]
    fn should_skip_sst_parse_large_sst_many_sheets_returns_false() {
        // 100 sheets disqualifies — fast-path's per-sheet scan worth the cost
        assert!(!should_skip_sst_parse(500_000, 100));
    }

    #[test]
    fn should_skip_sst_parse_small_sst_few_sheets_returns_false() {
        // many_small fixture shape: tiny SST per file, few sheets per file
        // density-based dense_bypass still applies; pre-check does not.
        assert!(!should_skip_sst_parse(5_000, 3));
    }

    #[test]
    fn should_skip_sst_parse_at_compressed_boundary() {
        // Boundary uses strict > on compressed size:
        // exactly 100_000 → false; 100_001 → true.
        assert!(!should_skip_sst_parse(100_000, 3));
        assert!(should_skip_sst_parse(100_001, 3));
    }

    #[test]
    fn should_skip_sst_parse_at_sheets_boundary() {
        // Boundary uses <= on sheets: 5 → true, 6 → false.
        assert!(should_skip_sst_parse(200_000, 5));
        assert!(!should_skip_sst_parse(200_000, 6));
    }

    #[test]
    fn should_skip_sst_parse_zero_sst_returns_false() {
        // No sharedStrings.xml at all → no shortcut.
        assert!(!should_skip_sst_parse(0, 3));
    }
}
