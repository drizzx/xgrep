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

    // Correctness guard: `strip_anchors` only peels a single OUTER `^`/`$`. For
    // multi-pattern/-e joins like `(?:^42$)|(?:^99$)` (raw starts with `(`), the
    // embedded anchors survive. The byte-scan regex runs with multi_line(false), so
    // those anchors bind to the absolute start/end of the WHOLE sheet xml — `^42$`
    // then never matches `<v>42</v>`, and decide() would wrongly SKIP a sheet that
    // truly matches (values stored literally in sheet xml: numbers/dates/bools/cached/
    // inline strings; SST strings are rescued by the <v>idx</v> alternatives, literals
    // are not). Any surviving anchor makes the screen unsafe, so bail to the
    // always-correct full-parse path.
    if raw.contains('^') || raw.contains('$') {
        return AugmentResult::Bypass;
    }

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
    fn augment_anchored_multipattern_does_not_skip_matching_sheet() {
        // Regression for the silent false-negative (correctness-8): main.rs joins
        // `-e '^42$' -e '^99$'` into `(?:^42$)|(?:^99$)`. strip_anchors only peels the
        // OUTER ^/$, so the embedded anchors survive into the byte-scan regex, which
        // runs with multi_line(false) and therefore anchors to the absolute start/end
        // of the WHOLE sheet xml. `^42$` then fails to match `<v>42</v>` even though the
        // cell value "42" matches the user pattern — so decide() would WRONGLY skip a
        // sheet that truly matches. The number 42 is stored literally in sheet xml (not
        // in sharedStrings), so the injected <v>idx</v> alternatives cannot rescue it.
        // Fix: augment must Bypass (force full parse) when anchors survive the strip.
        let pat = p("(?:^42$)|(?:^99$)");
        let hs = HitSet::new(0); // empty: a numeric value is NOT in the shared-string table
        let aug = augment(&pat, &hs);
        let xml = br#"<c r="A1"><v>42</v></c>"#; // sheet genuinely contains a matching cell value
        assert!(
            decide(xml, &aug, true),
            "decide() must NOT skip a sheet whose literal cell value matches an anchored multi-pattern query"
        );
    }

    #[test]
    fn augment_bypasses_when_literal_dollar_survives() {
        // Sibling of the anchored-multipattern case: a pattern like `$100` (also what
        // `-F '$100'` yields, since Pattern::raw() keeps the UNescaped input) would have
        // its `$` compiled by augment as a regex end-anchor ("end-of-text then 100" =
        // matches nothing), wrongly skipping every sheet. Any surviving `$`/`^` must
        // force Bypass to the full-parse path.
        let pat = p("$100");
        let hs = HitSet::new(0);
        assert!(matches!(augment(&pat, &hs), AugmentResult::Bypass));
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
}
