//! Compiled regex pattern + smart-case inference.

use regex::{Regex, RegexBuilder};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CaseMode {
    /// Default: case-insensitive unless pattern contains an uppercase ASCII letter.
    Smart,
    /// `-i` / `--ignore-case`.
    Insensitive,
    /// `-s` / `--case-sensitive`.
    Sensitive,
}

#[derive(Debug, Clone)]
pub struct Pattern {
    regex: Regex,
    raw: String,
}

impl Pattern {
    pub fn compile(
        raw: &str,
        case: CaseMode,
        fixed_string: bool,
        word: bool,
    ) -> Result<Self, regex::Error> {
        let escaped: String;
        let mut effective = if fixed_string {
            escaped = regex::escape(raw);
            escaped.as_str()
        } else {
            raw
        };
        let owned;
        if word {
            owned = format!(r"\b(?:{}){}", effective, r"\b");
            effective = owned.as_str();
        }
        let insensitive = match case {
            CaseMode::Insensitive => true,
            CaseMode::Sensitive => false,
            CaseMode::Smart => !raw.chars().any(|c| c.is_ascii_uppercase()),
        };
        let regex = RegexBuilder::new(effective)
            .case_insensitive(insensitive)
            .unicode(true)
            .build()?;
        Ok(Self {
            regex,
            raw: raw.to_owned(),
        })
    }

    pub fn is_match(&self, haystack: &str) -> bool {
        self.regex.is_match(haystack)
    }

    /// Iterate over (byte_start, byte_end) of each non-overlapping match.
    pub fn find_iter<'a>(&'a self, haystack: &'a str) -> impl Iterator<Item = (usize, usize)> + 'a {
        self.regex.find_iter(haystack).map(|m| (m.start(), m.end()))
    }

    pub fn raw(&self) -> &str {
        &self.raw
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn smart_case_lowercase_is_insensitive() {
        let p = Pattern::compile("foo", CaseMode::Smart, false, false).unwrap();
        assert!(p.is_match("FOO"));
        assert!(p.is_match("Foo"));
    }

    #[test]
    fn smart_case_with_upper_is_sensitive() {
        let p = Pattern::compile("Foo", CaseMode::Smart, false, false).unwrap();
        assert!(p.is_match("Foo bar"));
        assert!(!p.is_match("foo bar"));
    }

    #[test]
    fn force_insensitive_overrides_uppercase() {
        let p = Pattern::compile("Foo", CaseMode::Insensitive, false, false).unwrap();
        assert!(p.is_match("foo"));
    }

    #[test]
    fn force_sensitive_overrides_smart() {
        let p = Pattern::compile("foo", CaseMode::Sensitive, false, false).unwrap();
        assert!(!p.is_match("FOO"));
    }

    #[test]
    fn fixed_string_escapes_regex_meta() {
        let p = Pattern::compile("a.b", CaseMode::Sensitive, true, false).unwrap();
        assert!(p.is_match("a.b"));
        assert!(!p.is_match("aXb"));
    }

    #[test]
    fn word_regexp_requires_boundary() {
        let p = Pattern::compile("foo", CaseMode::Sensitive, false, true).unwrap();
        assert!(p.is_match("foo bar"));
        assert!(!p.is_match("foobar"));
    }

    #[test]
    fn find_iter_returns_byte_ranges() {
        let p = Pattern::compile("ab", CaseMode::Sensitive, false, false).unwrap();
        let hits: Vec<_> = p.find_iter("abXab").collect();
        assert_eq!(hits, vec![(0, 2), (3, 5)]);
    }

    #[test]
    fn invalid_regex_returns_error() {
        assert!(Pattern::compile("a(", CaseMode::Sensitive, false, false).is_err());
    }
}
