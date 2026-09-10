//! Elide elements from namespaces the document itself declares ignorable.
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
//! collide as the same field.
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
//! # The fix
//!
//! Per Markup Compatibility and Extensibility (ECMA-376 Part 3), a producer
//! that writes `mc:Ignorable="w14 w15 ..."` on the package part's root
//! element is declaring exactly this: a consumer that does not understand
//! those namespaces may disregard every element in them. This engine does
//! not parse `w14`/`w15`/`w16*`/`wp14` content, so honoring that declaration
//! is not a guess about the document — it is what the attribute means.
//! Stripping the declared-ignorable elements before handing the part to
//! `quick_xml::de` removes the colliding local name along with data this
//! parser was never going to read anyway.
//!
//! `<mc:AlternateContent>` is a *different* MCE mechanism — offering several
//! representations of the same content for the consumer to choose between —
//! and already has its own handling in `crate::docx::parse::body`
//! (`McRequires`, keyed off each `mc:Choice`'s own `Requires` list). This
//! pass leaves any `AlternateContent` subtree untouched, at any depth,
//! regardless of what namespaces its content uses, so it never second-guesses
//! that existing branch selection.
//!
//! Like `crate::docx::parse::body::mc_requires`, namespace prefixes here are
//! matched as the literal tokens the producer wrote (`"w14"`, not a
//! resolved URI) rather than through full namespace resolution — every
//! producer this engine has seen binds these well-known Microsoft extension
//! namespaces to their conventional prefixes, and the existing
//! `Requires`-list handling already makes the same simplifying assumption.

/// Elide every element whose tag prefix is listed in the document root's
/// `mc:Ignorable` attribute, except inside an `AlternateContent` subtree.
/// Returns the original buffer unchanged when there is nothing to elide, or
/// when the XML can't be walked (left for `quick_xml::de` to reject with a
/// proper parse error).
pub(crate) fn elide_ignorable_extensions(xml: &[u8]) -> Vec<u8> {
    let ignorable = root_ignorable_prefixes(xml);
    if ignorable.is_empty() {
        return xml.to_vec();
    }

    let spans = ignorable_element_spans(xml, &ignorable);
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

/// The root element's `mc:Ignorable` value, split on whitespace. Empty when
/// absent, empty-valued, or the XML can't be read that far.
fn root_ignorable_prefixes(xml: &[u8]) -> Vec<Vec<u8>> {
    use quick_xml::events::Event;
    use quick_xml::reader::Reader;

    let mut reader = Reader::from_reader(xml);
    reader.config_mut().trim_text(false);
    loop {
        let event = match reader.read_event() {
            Ok(event) => event,
            Err(_) => return Vec::new(),
        };
        match event {
            Event::Start(tag) | Event::Empty(tag) => {
                for attr in tag.attributes().flatten() {
                    if local_name(attr.key.as_ref()) == b"Ignorable" {
                        return attr
                            .value
                            .split(|&b| b == b' ')
                            .filter(|p| !p.is_empty())
                            .map(<[u8]>::to_vec)
                            .collect();
                    }
                }
                return Vec::new();
            }
            Event::Eof => return Vec::new(),
            _ => {}
        }
    }
}

/// Byte spans (start of open tag, end of matching close tag) to remove.
/// Empty (and the caller left with nothing to do) on malformed XML.
fn ignorable_element_spans(xml: &[u8], ignorable: &[Vec<u8>]) -> Vec<(usize, usize)> {
    use quick_xml::events::Event;
    use quick_xml::reader::Reader;

    let mut reader = Reader::from_reader(xml);
    reader.config_mut().trim_text(false);

    let mut spans = Vec::new();
    let mut depth: usize = 0;
    // (depth, start offset) of the ignorable element currently being elided.
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
                    && tag_prefix(name.as_ref())
                        .is_some_and(|p| ignorable.iter().any(|i| i.as_slice() == p))
                {
                    eliding = Some((depth, start_pos));
                }
            }
            Event::Empty(tag) => {
                if protected_since.is_none()
                    && eliding.is_none()
                    && tag_prefix(tag.name().as_ref())
                        .is_some_and(|p| ignorable.iter().any(|i| i.as_slice() == p))
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
        let out = s(elide_ignorable_extensions(xml.as_bytes()));
        out.strip_prefix(ROOT_OPEN)
            .and_then(|out| out.strip_suffix("</w:document>"))
            .expect("wrapper survives")
            .to_string()
    }

    #[test]
    fn no_ignorable_attribute_is_unchanged() {
        let xml = br#"<w:document xmlns:w="x"><w:rPr><w14:shadow/></w:rPr></w:document>"#;
        assert_eq!(elide_ignorable_extensions(xml), xml);
    }

    #[test]
    fn self_closing_ignorable_element_is_removed() {
        let out = elide_fragment(r#"<w:rPr><w:shadow w:val="0"/><w14:shadow/></w:rPr>"#);
        assert_eq!(out, r#"<w:rPr><w:shadow w:val="0"/></w:rPr>"#);
    }

    #[test]
    fn ignorable_element_with_children_is_removed_whole() {
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

    #[test]
    fn multiple_ignorable_siblings_are_all_removed() {
        let out =
            elide_fragment(r#"<w:rPr><w14:glow/><w:b/><w14:shadow/><w14:reflection/></w:rPr>"#);
        assert_eq!(out, r#"<w:rPr><w:b/></w:rPr>"#);
    }

    #[test]
    fn alternate_content_subtree_is_left_untouched() {
        // Covers both the self-closing (`Event::Empty`) and open/close
        // (`Event::Start`/`Event::End`) forms of an ignorable element, since
        // the protection check is independent in each branch.
        let fragment = r#"<mc:AlternateContent><mc:Choice Requires="w14"><w14:shadow/><w14:glow><w14:srgbClr/></w14:glow></mc:Choice><mc:Fallback/></mc:AlternateContent>"#;
        assert_eq!(elide_fragment(fragment), fragment);
    }

    #[test]
    fn malformed_xml_is_returned_unchanged() {
        let xml = br#"<w:document mc:Ignorable="w14"><w:rPr><w14:shadow>"#;
        assert_eq!(elide_ignorable_extensions(xml), xml);
    }

    #[test]
    fn empty_ignorable_value_is_a_no_op() {
        let xml =
            br#"<w:document xmlns:w="x" mc:Ignorable=""><w:rPr><w14:shadow/></w:rPr></w:document>"#;
        assert_eq!(elide_ignorable_extensions(xml), xml);
    }
}
