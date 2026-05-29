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
    xml: &[u8],
    tag: &str,
    mut visit: impl FnMut(&[u8], &[u8]) -> ControlFlow<()>,
) -> bool {
    let tag_bytes = tag.as_bytes();
    let mut marker_buf = [0u8; CLOSING_MARKER_CAP];
    let open_close = write_closing_marker(tag, &mut marker_buf);
    // Build the substring searcher once and reuse it across every `<si>`/`<t>`
    // body — hoisting it out of the loop avoids re-priming the SIMD scanner per
    // element and keeps the inner loop allocation-free.
    let close_finder = memchr::memmem::Finder::new(open_close);
    let mut pos = 0;
    while pos < xml.len() {
        // Find the next `<` from `pos`.
        let lt = match memchr::memchr(b'<', &xml[pos..]) {
            Some(off) => pos + off,
            None => return false,
        };
        // Confirm `<tag` followed by a tag-boundary byte.
        let after_lt = lt + 1;
        if after_lt + tag_bytes.len() > xml.len() {
            return false;
        }
        if &xml[after_lt..after_lt + tag_bytes.len()] != tag_bytes {
            pos = lt + 1;
            continue;
        }
        let boundary_idx = after_lt + tag_bytes.len();
        let boundary = xml.get(boundary_idx).copied();
        let is_boundary = matches!(
            boundary,
            Some(b' ') | Some(b'\t') | Some(b'\n') | Some(b'\r') | Some(b'>') | Some(b'/')
        );
        if !is_boundary {
            pos = lt + 1;
            continue;
        }
        // Find the `>` ending the open tag.
        let gt_rel = match memchr::memchr(b'>', &xml[boundary_idx..]) {
            Some(off) => off,
            None => return false,
        };
        let gt = boundary_idx + gt_rel;
        // If the byte before `>` is `/`, it's a self-closing tag — skip (regex
        // behavior also misses these).
        if gt > 0 && xml[gt - 1] == b'/' {
            pos = gt + 1;
            continue;
        }
        let attrs = &xml[boundary_idx..gt];
        let body_start = gt + 1;
        // Find the first `</tag>` after body_start.
        let close = match close_finder.find(&xml[body_start..]) {
            Some(off) => body_start + off,
            None => return false,
        };
        let body = &xml[body_start..close];
        match visit(attrs, body) {
            ControlFlow::Break(()) => return true,
            ControlFlow::Continue(()) => {}
        }
        pos = close + open_close.len();
    }
    false
}

/// Visit each self-closing `<TAG[attrs]/>` element in `xml`. For each match
/// invokes `visit(attrs)`. `attrs` is the byte slice between `<TAG` and `/>`.
///
/// Returns `true` iff `visit` returned `ControlFlow::Break(())`.
///
/// This is the dual of `for_each_tag` for the `<TAG ... />` form (no body).
/// The same tag-boundary rule applies: `<TAG` must be followed by whitespace,
/// `>`, or `/`.
pub(crate) fn for_each_self_closing_tag(
    xml: &[u8],
    tag: &str,
    mut visit: impl FnMut(&[u8]) -> ControlFlow<()>,
) -> bool {
    let tag_bytes = tag.as_bytes();
    let mut pos = 0;
    while pos < xml.len() {
        let lt = match memchr::memchr(b'<', &xml[pos..]) {
            Some(off) => pos + off,
            None => return false,
        };
        let after_lt = lt + 1;
        if after_lt + tag_bytes.len() > xml.len() {
            return false;
        }
        if &xml[after_lt..after_lt + tag_bytes.len()] != tag_bytes {
            pos = lt + 1;
            continue;
        }
        let boundary_idx = after_lt + tag_bytes.len();
        let boundary = xml.get(boundary_idx).copied();
        let is_boundary = matches!(
            boundary,
            Some(b' ') | Some(b'\t') | Some(b'\n') | Some(b'\r') | Some(b'>') | Some(b'/')
        );
        if !is_boundary {
            pos = lt + 1;
            continue;
        }
        // Find the `>` ending the tag.
        let gt_rel = match memchr::memchr(b'>', &xml[boundary_idx..]) {
            Some(off) => off,
            None => return false,
        };
        let gt = boundary_idx + gt_rel;
        // Must be self-closing: byte before `>` must be `/`.
        if gt == 0 || xml[gt - 1] != b'/' {
            pos = gt + 1;
            continue;
        }
        // attrs slice is between boundary_idx and the `/` before `>`.
        let attrs = &xml[boundary_idx..gt - 1];
        match visit(attrs) {
            ControlFlow::Break(()) => return true,
            ControlFlow::Continue(()) => {}
        }
        pos = gt + 1;
    }
    false
}

/// Upper bound for the stack buffer that holds a `</tag>` closing marker. The
/// longest tag this scanner is invoked with is "Relationship" (`</Relationship>`
/// = 15 bytes), so 16 leaves a byte of headroom. `write_closing_marker`
/// debug-asserts the marker fits.
const CLOSING_MARKER_CAP: usize = 16;

/// Write `</tag>` into `buf` and return the filled prefix slice. Avoids the
/// per-`for_each_tag` heap allocation the old `closing_marker(tag) -> Vec<u8>`
/// incurred (one alloc per `<si>`/`<t>` scan over a large sharedStrings).
fn write_closing_marker<'b>(tag: &str, buf: &'b mut [u8]) -> &'b [u8] {
    let tag_bytes = tag.as_bytes();
    let n = tag_bytes.len() + 3; // "</" + tag + ">"
    debug_assert!(
        n <= buf.len(),
        "closing-marker buffer too small for </{tag}>"
    );
    buf[0] = b'<';
    buf[1] = b'/';
    buf[2..2 + tag_bytes.len()].copy_from_slice(tag_bytes);
    buf[2 + tag_bytes.len()] = b'>';
    &buf[..n]
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

    #[test]
    fn for_each_tag_finds_single_element() {
        let mut hits: Vec<(Vec<u8>, Vec<u8>)> = Vec::new();
        for_each_tag(b"<a>body</a>", "a", |attrs, body| {
            hits.push((attrs.to_vec(), body.to_vec()));
            ControlFlow::Continue(())
        });
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].0, b"");
        assert_eq!(hits[0].1, b"body");
    }

    #[test]
    fn for_each_tag_returns_attrs_slice() {
        let mut hits: Vec<(Vec<u8>, Vec<u8>)> = Vec::new();
        for_each_tag(b"<a x=\"1\" y=\"2\">b</a>", "a", |attrs, body| {
            hits.push((attrs.to_vec(), body.to_vec()));
            ControlFlow::Continue(())
        });
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].0, b" x=\"1\" y=\"2\"");
        assert_eq!(hits[0].1, b"b");
    }

    #[test]
    fn for_each_tag_finds_multiple_elements() {
        let mut bodies: Vec<Vec<u8>> = Vec::new();
        for_each_tag(b"<a>1</a><a>2</a><a>3</a>", "a", |_attrs, body| {
            bodies.push(body.to_vec());
            ControlFlow::Continue(())
        });
        assert_eq!(bodies, vec![b"1".to_vec(), b"2".to_vec(), b"3".to_vec()]);
    }

    #[test]
    fn for_each_tag_break_stops_iteration_and_returns_true() {
        let mut count = 0;
        let broken = for_each_tag(b"<a>1</a><a>2</a><a>3</a>", "a", |_a, _b| {
            count += 1;
            if count == 2 {
                ControlFlow::Break(())
            } else {
                ControlFlow::Continue(())
            }
        });
        assert!(broken);
        assert_eq!(count, 2);
    }

    #[test]
    fn for_each_tag_continue_throughout_returns_false() {
        let broken = for_each_tag(b"<a>1</a>", "a", |_a, _b| ControlFlow::Continue(()));
        assert!(!broken);
    }

    #[test]
    fn for_each_tag_handles_nested_elements_with_lazy_close() {
        // Like the regex `<si\b[^>]*>([\s\S]*?)</si>` — first close wins.
        let mut hits: Vec<Vec<u8>> = Vec::new();
        for_each_tag(b"<a><a>inner</a></a>", "a", |_attrs, body| {
            hits.push(body.to_vec());
            ControlFlow::Continue(())
        });
        // First open at offset 0; first `</a>` is the inner close at offset 11.
        // So we get one outer match with body "<a>inner".
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0], b"<a>inner");
    }

    #[test]
    fn for_each_tag_does_not_collide_with_prefix_match() {
        // `<sst>` must NOT match `tag="s"`. Tag must be followed by a boundary.
        let mut hits = 0;
        for_each_tag(b"<sst><s>1</s></sst>", "s", |_a, _b| {
            hits += 1;
            ControlFlow::Continue(())
        });
        assert_eq!(hits, 1, "must match <s> exactly, not <sst>");
    }

    #[test]
    fn for_each_tag_empty_input_yields_nothing() {
        let mut hits = 0;
        for_each_tag(b"", "a", |_a, _b| {
            hits += 1;
            ControlFlow::Continue(())
        });
        assert_eq!(hits, 0);
    }

    #[test]
    fn for_each_tag_missing_close_yields_nothing_no_panic() {
        let mut hits = 0;
        for_each_tag(b"<a>body never closed", "a", |_a, _b| {
            hits += 1;
            ControlFlow::Continue(())
        });
        assert_eq!(hits, 0);
    }

    #[test]
    fn for_each_tag_skips_self_closing_form() {
        // Like the regex, self-closing <a/> is not matched.
        let mut hits = 0;
        for_each_tag(b"<a/><a>body</a>", "a", |_a, _b| {
            hits += 1;
            ControlFlow::Continue(())
        });
        assert_eq!(hits, 1, "only the non-self-closing element counts");
    }

    #[test]
    fn for_each_tag_handles_empty_body() {
        let mut bodies: Vec<Vec<u8>> = Vec::new();
        for_each_tag(b"<a></a>", "a", |_a, body| {
            bodies.push(body.to_vec());
            ControlFlow::Continue(())
        });
        assert_eq!(bodies, vec![b"".to_vec()]);
    }

    #[test]
    fn for_each_tag_recursion_finds_t_inside_si() {
        // Real sst pattern: <si><r><t>hello</t></r></si>
        let xml = b"<si><r><t>hello</t></r></si>";
        let mut t_bodies: Vec<Vec<u8>> = Vec::new();
        for_each_tag(xml, "si", |_si_attrs, si_body| {
            for_each_tag(si_body, "t", |_t_attrs, t_body| {
                t_bodies.push(t_body.to_vec());
                ControlFlow::Continue(())
            });
            ControlFlow::Continue(())
        });
        assert_eq!(t_bodies, vec![b"hello".to_vec()]);
    }

    #[test]
    fn for_each_tag_rich_text_concatenates_multiple_t_per_si() {
        // <si><r><t>hel</t></r><r><t>lo</t></r></si>
        let xml = b"<si><r><t>hel</t></r><r><t>lo</t></r></si>";
        let mut concat = String::new();
        for_each_tag(xml, "si", |_a, body| {
            for_each_tag(body, "t", |_a2, t_body| {
                concat.push_str(std::str::from_utf8(t_body).unwrap());
                ControlFlow::Continue(())
            });
            ControlFlow::Continue(())
        });
        assert_eq!(concat, "hello");
    }

    #[test]
    fn for_each_self_closing_tag_finds_simple_element() {
        let mut hits: Vec<Vec<u8>> = Vec::new();
        for_each_self_closing_tag(b"<a x=\"1\"/>", "a", |attrs| {
            hits.push(attrs.to_vec());
            ControlFlow::Continue(())
        });
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0], b" x=\"1\"");
    }

    #[test]
    fn for_each_self_closing_tag_skips_non_self_closing() {
        // Plain <a>body</a> must NOT match — that's for_each_tag's job.
        let mut hits = 0;
        for_each_self_closing_tag(b"<a>body</a>", "a", |_attrs| {
            hits += 1;
            ControlFlow::Continue(())
        });
        assert_eq!(hits, 0, "non-self-closing element must not match");
    }

    #[test]
    fn for_each_self_closing_tag_finds_multiple_relationships() {
        // Realistic xlsx rels file shape.
        let xml = br#"<Relationships>
<Relationship Id="rId1" Target="../theme.xml" Type="theme"/>
<Relationship Id="rId2" Target="../comments1.xml" Type="comments"/>
</Relationships>"#;
        let mut targets: Vec<String> = Vec::new();
        for_each_self_closing_tag(xml, "Relationship", |attrs| {
            if let Some(t) = attr(attrs, "Target") {
                targets.push(std::str::from_utf8(t).unwrap().to_string());
            }
            ControlFlow::Continue(())
        });
        assert_eq!(targets, vec!["../theme.xml".to_string(), "../comments1.xml".to_string()]);
    }

    #[test]
    fn for_each_self_closing_tag_break_stops_iteration() {
        let xml = b"<a/><a/><a/>";
        let mut count = 0;
        let broken = for_each_self_closing_tag(xml, "a", |_attrs| {
            count += 1;
            if count == 2 { ControlFlow::Break(()) } else { ControlFlow::Continue(()) }
        });
        assert!(broken);
        assert_eq!(count, 2);
    }

    #[test]
    fn for_each_self_closing_tag_empty_input_yields_nothing() {
        let mut hits = 0;
        for_each_self_closing_tag(b"", "a", |_attrs| {
            hits += 1;
            ControlFlow::Continue(())
        });
        assert_eq!(hits, 0);
    }

    #[test]
    fn for_each_self_closing_tag_missing_gt_yields_nothing_no_panic() {
        // Unterminated open tag — must not panic.
        let mut hits = 0;
        for_each_self_closing_tag(b"<a x=\"unterm", "a", |_attrs| {
            hits += 1;
            ControlFlow::Continue(())
        });
        assert_eq!(hits, 0);
    }

    #[test]
    fn for_each_self_closing_tag_does_not_collide_with_prefix_match() {
        // `<ab/>` must NOT match tag="a". Same boundary rule as for_each_tag.
        let mut hits = 0;
        for_each_self_closing_tag(b"<ab/><a/>", "a", |_attrs| {
            hits += 1;
            ControlFlow::Continue(())
        });
        assert_eq!(hits, 1, "must match <a/> exactly, not <ab/>");
    }
}
