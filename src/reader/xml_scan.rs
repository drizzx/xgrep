//! Internal hand-rolled XML byte scanner. Used by `sst.rs` and `comments.rs`
//! to avoid regex compile/frame cost on the small xlsx subgrammar
//! (`<si>...<t>...</t>...</si>`, `<comment ref="..." />`, `Relationship Target="..."`).
//!
//! Replaces 4 regex parse points across reader/ with a single byte scanner.
//! Behavior is intentionally a strict subset of the regexes it replaces — see
//! the per-function "Limitations" notes.

use std::ops::ControlFlow;

/// Visit each `<TAG[attrs]>...</TAG>` element in `xml`. For each match invokes
/// `visit(attrs, body)`. `attrs` is the byte slice between `<TAG` and `>`;
/// `body` is the byte slice between `>` and `</TAG>`.
///
/// Returns `true` iff `visit` returned `ControlFlow::Break(())`.
///
/// Limitations (match the regex this replaces):
/// - Does not match self-closing `<TAG/>`.
/// - On nested same-name tags, picks the FIRST matching `</TAG>` (lazy match).
/// - `body` is returned verbatim — caller must `xml_unescape` if needed.
/// - Open-tag boundary: requires `<tag` to be followed by one of
///   ` `, `\t`, `\n`, `\r`, `>`, `/` so that e.g. `tag="si"` does not collide
///   with `<sst>`.
pub fn for_each_tag(
    _xml: &[u8],
    _tag: &str,
    _visit: impl FnMut(&[u8], &[u8]) -> ControlFlow<()>,
) -> bool {
    unimplemented!("Task 4")
}

/// Return the unquoted value of `name="..."` from an attrs slice. Only
/// double-quoted form is recognized (matches existing regex behavior).
/// `name` must be preceded by start-of-input or whitespace so that
/// `attr(b"name_full=\"x\"", "name")` does NOT return `Some`.
pub fn attr<'a>(attrs: &'a [u8], name: &str) -> Option<&'a [u8]> {
    let name_bytes = name.as_bytes();
    // Build needle: `name=\"`. Caller verifies prefix is at start or after whitespace.
    let mut i = 0;
    while i + name_bytes.len() + 2 <= attrs.len() {
        let prefix_ok = i == 0 || matches!(attrs[i - 1], b' ' | b'\t' | b'\n' | b'\r');
        if prefix_ok
            && attrs[i..i + name_bytes.len()] == *name_bytes
            && attrs.get(i + name_bytes.len()) == Some(&b'=')
            && attrs.get(i + name_bytes.len() + 1) == Some(&b'"')
        {
            let value_start = i + name_bytes.len() + 2;
            // Find closing quote.
            let value_end = attrs[value_start..]
                .iter()
                .position(|&b| b == b'"')
                .map(|p| value_start + p)?;
            return Some(&attrs[value_start..value_end]);
        }
        i += 1;
    }
    None
}

/// XML entity unescape. Recognizes exactly `&amp;`, `&lt;`, `&gt;`, `&quot;`,
/// `&apos;`. Unknown entities like `&xyz;` pass through verbatim. Input is
/// expected to be UTF-8; output is a UTF-8 String.
pub fn xml_unescape(bytes: &[u8]) -> String {
    let mut buf: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'&' {
            if let Some((replacement, consumed)) = match_entity(&bytes[i..]) {
                buf.push(replacement);
                i += consumed;
                continue;
            }
        }
        buf.push(bytes[i]);
        i += 1;
    }
    // Input is UTF-8 + ASCII-only entity replacements → output is UTF-8.
    String::from_utf8(buf).expect("xml_unescape: input must be UTF-8")
}

/// Try to match a known XML entity starting at `s[0..]`. Returns
/// `(replacement_ascii, bytes_consumed)` on match, else `None`.
fn match_entity(s: &[u8]) -> Option<(u8, usize)> {
    const ENTITIES: &[(&[u8], u8)] = &[
        (b"&amp;", b'&'),
        (b"&lt;", b'<'),
        (b"&gt;", b'>'),
        (b"&quot;", b'"'),
        (b"&apos;", b'\''),
    ];
    for (pat, repl) in ENTITIES {
        if s.starts_with(pat) {
            return Some((*repl, pat.len()));
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn xml_unescape_passes_through_plain_ascii() {
        assert_eq!(xml_unescape(b"hello world"), "hello world");
    }

    #[test]
    fn xml_unescape_passes_through_utf8_multibyte() {
        let s = "你好 héllo";
        assert_eq!(xml_unescape(s.as_bytes()), s);
    }

    #[test]
    fn xml_unescape_handles_all_five_known_entities() {
        assert_eq!(xml_unescape(b"&amp;"), "&");
        assert_eq!(xml_unescape(b"&lt;"), "<");
        assert_eq!(xml_unescape(b"&gt;"), ">");
        assert_eq!(xml_unescape(b"&quot;"), "\"");
        assert_eq!(xml_unescape(b"&apos;"), "'");
    }

    #[test]
    fn xml_unescape_mixes_entities_and_plain_text() {
        assert_eq!(xml_unescape(b"a&amp;b&lt;c&gt;d"), "a&b<c>d");
    }

    #[test]
    fn xml_unescape_leaves_unknown_entities_verbatim() {
        // Numeric or non-standard refs are not transformed.
        assert_eq!(xml_unescape(b"&xyz;"), "&xyz;");
        assert_eq!(xml_unescape(b"&#x20;"), "&#x20;");
    }

    #[test]
    fn xml_unescape_handles_bare_ampersand() {
        // A `&` not followed by a recognized entity stays as `&`.
        assert_eq!(xml_unescape(b"a & b"), "a & b");
        assert_eq!(xml_unescape(b"&"), "&");
    }

    #[test]
    fn attr_finds_value_by_name() {
        assert_eq!(attr(b"x=\"1\" y=\"2\"", "x"), Some(&b"1"[..]));
        assert_eq!(attr(b"x=\"1\" y=\"2\"", "y"), Some(&b"2"[..]));
    }

    #[test]
    fn attr_returns_none_when_absent() {
        assert_eq!(attr(b"x=\"1\"", "z"), None);
        assert_eq!(attr(b"", "x"), None);
    }

    #[test]
    fn attr_does_not_match_substring_of_another_attr_name() {
        // "name" is a substring of "name_full"; must not match.
        assert_eq!(attr(b"name_full=\"a\"", "name"), None);
        // But should still find a real "name" attr alongside.
        assert_eq!(
            attr(b"name_full=\"a\" name=\"b\"", "name"),
            Some(&b"b"[..])
        );
    }

    #[test]
    fn attr_handles_leading_whitespace_before_attribute() {
        assert_eq!(attr(b"  x=\"1\"", "x"), Some(&b"1"[..]));
    }

    #[test]
    fn attr_value_may_contain_special_chars_except_double_quote() {
        // Real-world: ref="A1", Target="../sharedStrings.xml"
        assert_eq!(
            attr(b"Target=\"../sharedStrings.xml\"", "Target"),
            Some(&b"../sharedStrings.xml"[..])
        );
    }

    #[test]
    fn attr_returns_none_on_malformed_attribute() {
        // No closing quote — function should not panic.
        assert_eq!(attr(b"x=\"unterminated", "x"), None);
    }
}
