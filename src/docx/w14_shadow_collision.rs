//! Elide the one confirmed `<w14:shadow>` / `<w:shadow>` local-name collision.
//!
//! # The problem
//!
//! `quick-xml`'s serde support matches a struct field against an XML tag by
//! **local name only** — the part after `:` — not a namespace-resolved
//! qualified name (confirmed against `quick-xml` 0.41's `de::map::not_in`,
//! which builds its match key from `BytesStart::local_name()`, discarding the
//! prefix). Every `XxxXml` schema in this crate relies on that for the
//! ordinary case: a Rust field renamed `"rStyle"` matches `<w:rStyle>`
//! however the producer chose to bind the `w:` prefix. But it also means two
//! elements from **different** namespaces that happen to share a local name
//! collide as the same field — and there is no fix available inside the
//! parse itself: a custom `Deserialize` for the colliding struct sees only a
//! generic `D: serde::Deserializer<'de>` (it is reached through the parent's
//! ordinary derive composition), and quick-xml's own `MapAccess`/`EnumAccess`
//! already discard the namespace before *any* visitor — derived or
//! hand-written — ever sees a key. The only point namespace-resolved XML text
//! is ever in hand is before `quick_xml::de` runs at all, which is what this
//! module does.
//!
//! That is exactly what a real document from issue reproduction
//! `joern.hendrich@vdwbayern.de.docx` hits: a numbering level's `<w:rPr>`
//! carries both the ordinary `<w:shadow w:val="0"/>` boolean toggle
//! (§17.3.2.37) and, later in the same element, a Word-2010 `<w14:shadow>` —
//! a WordArt-style text-effect extension (blur/offset/color) this renderer
//! does not model at the run level. Both match
//! `crate::docx::parse::properties::schema::run::RPrXml`'s `shadow` field by
//! local name, and because they are not adjacent in the source (other
//! `w:rPr` children sit between them), `quick-xml` reports the second as a
//! duplicate rather than folding it into the `Vec` — deserialization fails
//! outright instead of just losing the extension data.
//!
//! # The fix, deliberately narrow
//!
//! This elides only that one exact collision — tag prefix `w14`, local name
//! `shadow` — rather than every element from every namespace the document's
//! `mc:Ignorable` declares safe to disregard. `w14:glow`, `w14:reflection`
//! and the rest of the Word-2010 text-effect extensions already parse fine
//! today (nothing in `RPrXml` names them, so quick-xml drops them as
//! unmatched fields with no error); only `shadow` collides, because only
//! `shadow` happens to also be a real `RPrXml` field name. Reopen this scope
//! only against a second observed collision, not in anticipation of one —
//! seeing exactly one real-world case does not justify guessing at the next.
//!
//! Still gated on the root's `mc:Ignorable` actually listing `w14`: per
//! Markup Compatibility and Extensibility (ECMA-376 Part 3), that is the
//! producer's own declaration that a `w14`-unaware consumer may disregard
//! `w14` content, so stripping this one element is not a guess about the
//! document even at this narrow scope.
//!
//! `<mc:AlternateContent>` is a *different* MCE mechanism — offering several
//! representations of the same content for the consumer to choose between —
//! and already has its own handling in `crate::docx::parse::body`
//! (`McRequires`, keyed off each `mc:Choice`'s own `Requires` list). This
//! pass leaves any `AlternateContent` subtree untouched, at any depth, so it
//! never second-guesses that existing branch selection.
//!
//! Like `crate::docx::parse::body::mc_requires`, the `w14` prefix is matched
//! as the literal token the producer wrote, not a resolved URI — every
//! producer this engine has seen binds this well-known Microsoft extension
//! namespace to its conventional prefix, and the existing `Requires`-list
//! handling already makes the same simplifying assumption.

const TARGET_PREFIX: &[u8] = b"w14";
const TARGET_LOCAL_NAME: &[u8] = b"shadow";

/// Elide every `<w14:shadow>` element, except inside an `AlternateContent`
/// subtree, but only when the document root's `mc:Ignorable` lists `w14`.
/// Returns the original buffer unchanged when there is nothing to elide, or
/// when the XML can't be walked (left for `quick_xml::de` to reject with a
/// proper parse error).
pub(crate) fn elide_w14_shadow(xml: &[u8]) -> Vec<u8> {
    if !root_declares_w14_ignorable(xml) {
        return xml.to_vec();
    }

    let spans = w14_shadow_spans(xml);
    if spans.is_empty() {
        return xml.to_vec();
    }

    let mut out = Vec::with_capacity(xml.len());
    let mut cursor = 0;
    for (start, end) in spans {
        out.extend_from_slice(&xml[cursor..start]);
        cursor = end;
    }
    out.extend_from_slice(&xml[cursor..]);
    out
}

fn local_name(tag: &[u8]) -> &[u8] {
    match tag.iter().position(|&b| b == b':') {
        Some(colon) => &tag[colon + 1..],
        None => tag,
    }
}

fn tag_prefix(tag: &[u8]) -> Option<&[u8]> {
    let colon = tag.iter().position(|&b| b == b':')?;
    Some(&tag[..colon])
}

fn is_w14_shadow(tag: &[u8]) -> bool {
    tag_prefix(tag) == Some(TARGET_PREFIX) && local_name(tag) == TARGET_LOCAL_NAME
}

/// Whether the root element's `mc:Ignorable` attribute lists `w14`. False
/// when absent, when `w14` is not one of its tokens, or when the XML can't
/// be read that far.
fn root_declares_w14_ignorable(xml: &[u8]) -> bool {
    use quick_xml::events::Event;
    use quick_xml::reader::Reader;

    let mut reader = Reader::from_reader(xml);
    reader.config_mut().trim_text(false);
    loop {
        let event = match reader.read_event() {
            Ok(event) => event,
            Err(_) => return false,
        };
        match event {
            Event::Start(tag) | Event::Empty(tag) => {
                for attr in tag.attributes().flatten() {
                    if local_name(attr.key.as_ref()) == b"Ignorable" {
                        return attr
                            .value
                            .split(|&b| b == b' ')
                            .any(|token| token == TARGET_PREFIX);
                    }
                }
                return false;
            }
            Event::Eof => return false,
            _ => {}
        }
    }
}

/// Byte spans (start of open tag, end of matching close tag) of every
/// `<w14:shadow>` element to remove. Empty (and the caller left with nothing
/// to do) on malformed XML.
fn w14_shadow_spans(xml: &[u8]) -> Vec<(usize, usize)> {
    use quick_xml::events::Event;
    use quick_xml::reader::Reader;

    let mut reader = Reader::from_reader(xml);
    reader.config_mut().trim_text(false);

    let mut spans = Vec::new();
    let mut depth: usize = 0;
    // (depth, start offset) of the `w14:shadow` element currently being elided.
    let mut eliding: Option<(usize, usize)> = None;
    // Depth at which an `AlternateContent` subtree was entered; suppresses
    // elision anywhere inside it.
    let mut protected_since: Option<usize> = None;

    loop {
        let start_pos = reader.buffer_position() as usize;
        let event = match reader.read_event() {
            Ok(event) => event,
            Err(_) => return Vec::new(),
        };
        match &event {
            Event::Start(tag) => {
                depth += 1;
                let name = tag.name();
                if protected_since.is_none() && local_name(name.as_ref()) == b"AlternateContent" {
                    protected_since = Some(depth);
                } else if protected_since.is_none()
                    && eliding.is_none()
                    && is_w14_shadow(name.as_ref())
                {
                    eliding = Some((depth, start_pos));
                }
            }
            Event::Empty(tag) => {
                if protected_since.is_none()
                    && eliding.is_none()
                    && is_w14_shadow(tag.name().as_ref())
                {
                    spans.push((start_pos, reader.buffer_position() as usize));
                }
            }
            Event::End(_) => {
                if let Some((d, s)) = eliding {
                    if d == depth {
                        spans.push((s, reader.buffer_position() as usize));
                        eliding = None;
                    }
                }
                if protected_since == Some(depth) {
                    protected_since = None;
                }
                depth = depth.saturating_sub(1);
            }
            Event::Eof => {
                return if depth == 0 { spans } else { Vec::new() };
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(b: Vec<u8>) -> String {
        String::from_utf8(b).unwrap()
    }

    const ROOT_OPEN: &str = concat!(
        r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" "#,
        r#"xmlns:w14="http://schemas.microsoft.com/office/word/2010/wordml" "#,
        r#"xmlns:mc="http://schemas.openxmlformats.org/markup-compatibility/2006" "#,
        r#"mc:Ignorable="w14">"#
    );

    fn wrap(fragment: &str) -> String {
        format!("{ROOT_OPEN}{fragment}</w:document>")
    }

    fn elide_fragment(fragment: &str) -> String {
        let xml = wrap(fragment);
        let out = s(elide_w14_shadow(xml.as_bytes()));
        out.strip_prefix(ROOT_OPEN)
            .and_then(|out| out.strip_suffix("</w:document>"))
            .expect("wrapper survives")
            .to_string()
    }

    #[test]
    fn no_ignorable_attribute_is_unchanged() {
        let xml = br#"<w:document xmlns:w="x"><w:rPr><w14:shadow/></w:rPr></w:document>"#;
        assert_eq!(elide_w14_shadow(xml), xml);
    }

    #[test]
    fn ignorable_without_w14_token_is_unchanged() {
        let xml = br#"<w:document xmlns:w="x" mc:Ignorable="w15"><w:rPr><w14:shadow/></w:rPr></w:document>"#;
        assert_eq!(elide_w14_shadow(xml), xml);
    }

    #[test]
    fn self_closing_w14_shadow_is_removed() {
        let out = elide_fragment(r#"<w:rPr><w:shadow w:val="0"/><w14:shadow/></w:rPr>"#);
        assert_eq!(out, r#"<w:rPr><w:shadow w:val="0"/></w:rPr>"#);
    }

    #[test]
    fn w14_shadow_with_children_is_removed_whole() {
        let out = elide_fragment(
            r#"<w:rPr><w:b/><w14:shadow w14:blurRad="0"><w14:srgbClr w14:val="000000"/></w14:shadow><w:i/></w:rPr>"#,
        );
        assert_eq!(out, r#"<w:rPr><w:b/><w:i/></w:rPr>"#);
    }

    #[test]
    fn non_ignorable_prefix_is_kept() {
        let out = elide_fragment(r#"<w:rPr><w:shadow w:val="0"/><m:oMath/></w:rPr>"#);
        assert_eq!(out, r#"<w:rPr><w:shadow w:val="0"/><m:oMath/></w:rPr>"#);
    }

    /// The narrowed scope's whole point: siblings from the *same* declared-
    /// ignorable `w14` namespace that don't collide with any `RPrXml` field
    /// name are left alone — they already parse fine today (dropped as
    /// unmatched fields, no error), so touching them isn't this fix's job.
    #[test]
    fn other_w14_extensions_are_not_removed() {
        let out =
            elide_fragment(r#"<w:rPr><w14:glow/><w:b/><w14:shadow/><w14:reflection/></w:rPr>"#);
        assert_eq!(out, r#"<w:rPr><w14:glow/><w:b/><w14:reflection/></w:rPr>"#);
    }

    #[test]
    fn multiple_w14_shadow_siblings_are_all_removed() {
        let out = elide_fragment(r#"<w:rPr><w14:shadow/><w:b/><w14:shadow/></w:rPr>"#);
        assert_eq!(out, r#"<w:rPr><w:b/></w:rPr>"#);
    }

    #[test]
    fn alternate_content_subtree_is_left_untouched() {
        // Covers both the self-closing (`Event::Empty`) and open/close
        // (`Event::Start`/`Event::End`) forms of the element, since the
        // protection check is independent in each branch.
        let fragment = r#"<mc:AlternateContent><mc:Choice Requires="w14"><w14:shadow/><w14:shadow><w14:srgbClr/></w14:shadow></mc:Choice><mc:Fallback/></mc:AlternateContent>"#;
        assert_eq!(elide_fragment(fragment), fragment);
    }

    #[test]
    fn malformed_xml_is_returned_unchanged() {
        let xml = br#"<w:document mc:Ignorable="w14"><w:rPr><w14:shadow>"#;
        assert_eq!(elide_w14_shadow(xml), xml);
    }

    #[test]
    fn empty_ignorable_value_is_a_no_op() {
        let xml =
            br#"<w:document xmlns:w="x" mc:Ignorable=""><w:rPr><w14:shadow/></w:rPr></w:document>"#;
        assert_eq!(elide_w14_shadow(xml), xml);
    }
}
