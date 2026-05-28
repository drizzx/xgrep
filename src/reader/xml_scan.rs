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
pub fn attr<'a>(_attrs: &'a [u8], _name: &str) -> Option<&'a [u8]> {
    unimplemented!("Task 3")
}

/// XML entity unescape. Recognizes exactly `&amp;`, `&lt;`, `&gt;`, `&quot;`,
/// `&apos;`. Unknown entities like `&xyz;` pass through verbatim. Input is
/// expected to be UTF-8; output is a UTF-8 String.
pub fn xml_unescape(_bytes: &[u8]) -> String {
    unimplemented!("Task 2")
}

#[cfg(test)]
mod tests {
    // Populated in Tasks 2-5.
}
