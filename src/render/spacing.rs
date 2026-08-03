//! §17.3.2.35 / §17.3.1.13: the unit that inter-character spacing is applied
//! *between*.
//!
//! Two features insert horizontal space inside a run: `w:spacing` (§17.3.2.35)
//! adds a fixed amount everywhere, and `w:jc="distribute"` (§17.3.1.13) shares
//! a line's spare width out at the same positions. Both need one answer to the
//! same question — *where may space go?* — and layout and paint must give the
//! same answer, or a run measures to one width and paints at another.
//!
//! That answer is the **UAX #29 extended grapheme cluster**, not the Unicode
//! scalar. `e` + `U+0301` is one unit, so no space is wedged between a letter
//! and its accent; a keycap, a variation-selector pair, a ZWJ sequence and a
//! regional-indicator flag are each one unit for the same reason. Splitting on
//! scalars — what this module replaced — detached every combining mark it met.
//!
//! # Why not the shaped cluster
//!
//! A shaping engine reports finer, script-aware boundaries, and #82 asks for
//! them. They would be boundaries the painter cannot honour: `draw_str` and
//! `TextBlob::from_str` map codepoints through the cmap only, with no GSUB, so
//! body text is never shaped in the first place (README's *Complex-script
//! shaping* row). Promising shaped boundaries while painting unshaped glyphs
//! would buy nothing and hide the real gap. When the paint path does shape,
//! this module is the seam that changes: the unit becomes the shaped cluster
//! and every caller below stays as it is.

use unicode_segmentation::UnicodeSegmentation;

/// True when one byte of `text` is exactly one spacing unit, so the grapheme
/// segmenter can be skipped.
///
/// `\r` is excluded even though it is ASCII: UAX #29 GB3 keeps CRLF together as
/// a *single* cluster. Text reaching layout has its C0 controls stripped
/// (`fragment::text::emit_text_fragments`), but this module is also called from
/// the painter and must not inherit that assumption.
fn is_one_byte_per_unit(text: &str) -> bool {
    text.is_ascii() && !text.as_bytes().contains(&b'\r')
}

/// The spacing units of `text`, in order — the substrings that spacing is
/// inserted *between*, and that the painter draws one at a time.
pub fn units(text: &str) -> impl Iterator<Item = &str> + '_ {
    // Two concrete iterators behind one `impl Iterator`: the ASCII path walks
    // byte slices, which is what the overwhelming majority of runs take.
    let (ascii, graphemes) = if is_one_byte_per_unit(text) {
        (Some((0..text.len()).map(|i| &text[i..i + 1])), None)
    } else {
        (None, Some(text.graphemes(true)))
    };
    ascii
        .into_iter()
        .flatten()
        .chain(graphemes.into_iter().flatten())
}

/// How many spacing units `text` holds — i.e. how many times a per-unit amount
/// is added when the run is measured.
pub fn unit_count(text: &str) -> usize {
    if is_one_byte_per_unit(text) {
        text.len()
    } else {
        text.graphemes(true).count()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The defect this module exists to fix: a combining mark must never be a
    /// unit of its own, or spacing lands between the letter and its accent.
    #[test]
    fn combining_mark_joins_its_base() {
        let accented = "e\u{301}";
        assert_eq!(accented.chars().count(), 2, "two scalars");
        assert_eq!(unit_count(accented), 1, "but one spacing unit");
        assert_eq!(units(accented).collect::<Vec<_>>(), vec!["e\u{301}"]);
    }

    /// The same rule across the multi-scalar sequences a DOCX actually carries:
    /// keycap, variation selector, ZWJ family, regional-indicator flag.
    #[test]
    fn multi_scalar_sequences_are_single_units() {
        for (label, text) in [
            ("keycap", "1\u{FE0F}\u{20E3}"),
            ("variation selector", "\u{2764}\u{FE0F}"),
            ("ZWJ family", "\u{1F468}\u{200D}\u{1F469}\u{200D}\u{1F467}"),
            ("regional indicator", "\u{1F1E9}\u{1F1EA}"),
            ("devanagari akshara", "\u{0915}\u{094D}\u{0937}"),
        ] {
            assert_eq!(unit_count(text), 1, "{label} must be one spacing unit");
        }
    }

    /// UAX #29 GB3 — the reason the ASCII fast path rejects `\r`. A CRLF split
    /// into two units would put spacing inside a single cluster, the very thing
    /// this module forbids.
    #[test]
    fn crlf_is_one_unit_despite_being_ascii() {
        assert!(!is_one_byte_per_unit("a\r\nb"));
        assert_eq!(unit_count("\r\n"), 1);
        assert_eq!(units("a\r\nb").collect::<Vec<_>>(), vec!["a", "\r\n", "b"]);
    }

    /// The fast path is an optimisation, not a second definition: it must agree
    /// with the segmenter everywhere it is taken.
    #[test]
    fn ascii_fast_path_agrees_with_the_segmenter() {
        for text in ["", "a", "hello world", "tab\there", "a-b-c", "  "] {
            assert!(is_one_byte_per_unit(text), "{text:?} should take fast path");
            assert_eq!(
                unit_count(text),
                text.graphemes(true).count(),
                "unit_count disagrees with the segmenter on {text:?}"
            );
            assert_eq!(
                units(text).collect::<Vec<_>>(),
                text.graphemes(true).collect::<Vec<_>>(),
                "units disagrees with the segmenter on {text:?}"
            );
        }
    }

    /// Non-ASCII text without combining marks still counts one unit per
    /// character — spacing is not silently suppressed for Cyrillic or CJK.
    #[test]
    fn simple_non_ascii_counts_one_unit_per_character() {
        assert_eq!(unit_count("привет"), 6);
        assert_eq!(unit_count("日本語"), 3);
    }

    /// `units` and `unit_count` are one definition seen two ways; a caller that
    /// measures with one and paints with the other must not drift.
    #[test]
    fn units_and_unit_count_agree() {
        for text in [
            "",
            "abc",
            "e\u{301}x",
            "日本語",
            "a\r\nb",
            "\u{1F1E9}\u{1F1EA}!",
        ] {
            assert_eq!(
                units(text).count(),
                unit_count(text),
                "count mismatch on {text:?}"
            );
            assert_eq!(
                units(text).collect::<String>(),
                text,
                "units must reconstruct the input exactly for {text:?}"
            );
        }
    }
}
