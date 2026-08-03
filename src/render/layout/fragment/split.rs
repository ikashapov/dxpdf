//! Cluster-level splitting of over-wide text fragments.
//!
//! A word wider than the space available can't be broken at a normal break
//! opportunity, so it is split into one fragment per grapheme cluster and the
//! line-fitter breaks between them. Used for narrow table cells and deep
//! indents.
//!
//! The unit is the same one spacing is inserted between
//! ([`crate::render::spacing`]) — and for the same reason. Splitting per
//! *scalar*, which this module used to do, let the line-fitter break between a
//! letter and its own combining mark and carry the accent to the next line.

use std::rc::Rc;

use super::{FontProps, Fragment, TextMetrics};
use crate::render::dimension::Pt;
use crate::render::spacing;

/// Text measurement callback. Structurally identical to
/// `paragraph::MeasureTextFn`, restated here so this module doesn't depend on
/// the paragraph layer — callers in either layer pass the same closures.
pub type MeasureFn<'a> = Option<&'a dyn Fn(&str, &FontProps) -> (Pt, TextMetrics)>;

/// True for a text fragment that is both too wide and actually splittable.
///
/// The *cluster* count is what matters: a single cluster cannot be split
/// however many scalars or bytes it occupies. An earlier byte-length
/// (`text.len() > 1`) spelling of this test disagreed with the split itself for
/// any non-ASCII single character — it reported "needs split", then split
/// nothing, and the caller paid for a full clone of the fragment vector. A
/// scalar count has the same disagreement for a one-cluster accented letter.
fn needs_split(fragment: &Fragment, max_width: Pt) -> bool {
    matches!(
        fragment,
        Fragment::Text { width, text, .. } if *width > max_width && spacing::unit_count(text) > 1
    )
}

/// Split text fragments wider than `max_width` into per-cluster fragments.
///
/// Returns `None` when nothing needs splitting — the common case. That lets a
/// caller holding an owned `Vec` keep it untouched, and a caller holding a
/// slice hand back `Cow::Borrowed`; neither pays for a copy on the fast path.
///
/// Per-cluster widths come from `measure` when one is supplied. Without a
/// measurer the fragment's width is divided evenly across its clusters,
/// which is only a positioning approximation — the total is preserved.
pub fn split_oversized_fragments(
    fragments: &[Fragment],
    max_width: Pt,
    measure: MeasureFn<'_>,
) -> Option<Vec<Fragment>> {
    // A non-positive budget can't be met by any split, and dividing by it
    // below would be meaningless.
    if max_width <= Pt::ZERO {
        return None;
    }
    if !fragments.iter().any(|f| needs_split(f, max_width)) {
        return None;
    }

    let mut result = Vec::with_capacity(fragments.len());
    for frag in fragments {
        let Fragment::Text {
            text,
            width,
            font,
            color,
            shading,
            border,
            metrics,
            hyperlink_url,
            baseline_offset,
            ..
        } = frag
        else {
            result.push(frag.clone());
            continue;
        };
        if !needs_split(frag, max_width) {
            result.push(frag.clone());
            continue;
        }

        let unit_count = spacing::unit_count(text);
        let per_unit_fallback = *width / unit_count as f32;
        for unit in spacing::units(text) {
            let (w, unit_metrics) = match measure {
                Some(m) => m(unit, font),
                None => (per_unit_fallback, *metrics),
            };
            result.push(Fragment::Text {
                text: Rc::from(unit),
                font: font.clone(),
                color: *color,
                shading: *shading,
                border: *border,
                width: w,
                trimmed_width: w,
                metrics: unit_metrics,
                hyperlink_url: hyperlink_url.clone(),
                baseline_offset: *baseline_offset,
                text_offset: Pt::ZERO,
                // Per-cluster split of an over-wide word — not a mark.
                is_footnote_ref: false,
            });
        }
    }
    Some(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render::resolve::color::RgbColor;

    fn text_frag(text: &str, width: f32) -> Fragment {
        Fragment::Text {
            text: Rc::from(text),
            font: Rc::new(FontProps {
                family: Rc::from("Test"),
                size: Pt::new(12.0),
                bold: false,
                italic: false,
                underline: false,
                char_spacing: Pt::ZERO,
                text_scale: 1.0,
                underline_position: Pt::ZERO,
                underline_thickness: Pt::ZERO,
            }),
            color: RgbColor::BLACK,
            width: Pt::new(width),
            trimmed_width: Pt::new(width),
            metrics: TextMetrics {
                ascent: Pt::new(10.0),
                descent: Pt::new(4.0),
                leading: Pt::ZERO,
            },
            hyperlink_url: None,
            shading: None,
            border: None,
            baseline_offset: Pt::ZERO,
            text_offset: Pt::ZERO,
            is_footnote_ref: false,
        }
    }

    fn texts(frags: &[Fragment]) -> Vec<String> {
        frags
            .iter()
            .map(|f| match f {
                Fragment::Text { text, .. } => text.to_string(),
                _ => panic!("expected Text fragment"),
            })
            .collect()
    }

    #[test]
    fn splits_into_one_fragment_per_character() {
        // "ab" at 60pt is wider than max_width=20pt → two 30pt characters
        // (uniform fallback — no measurer provided).
        let frags = vec![text_frag("ab", 60.0)];
        let result = split_oversized_fragments(&frags, Pt::new(20.0), None).expect("splits");
        assert_eq!(
            texts(&result),
            ["a", "b"],
            "one fragment per character, in order"
        );
        for frag in &result {
            let Fragment::Text { width, .. } = frag else {
                unreachable!()
            };
            assert!((width.raw() - 30.0).abs() < 1e-4, "uniform fallback 60/2");
        }
    }

    #[test]
    fn measurer_supplies_per_character_widths() {
        // A measurer that gives 'w' a wider advance than 'i' — the uniform
        // fallback would give both the same width.
        let measure = |t: &str, _: &FontProps| {
            let w = if t == "w" { 40.0 } else { 10.0 };
            (
                Pt::new(w),
                TextMetrics {
                    ascent: Pt::new(9.0),
                    descent: Pt::new(3.0),
                    leading: Pt::ZERO,
                },
            )
        };
        let frags = vec![text_frag("wi", 50.0)];
        let result =
            split_oversized_fragments(&frags, Pt::new(20.0), Some(&measure)).expect("splits");
        let widths: Vec<f32> = result
            .iter()
            .map(|f| match f {
                Fragment::Text { width, .. } => width.raw(),
                _ => unreachable!(),
            })
            .collect();
        assert_eq!(widths, [40.0, 10.0], "measured, not evenly divided");
        // Metrics come from the measurer too, not the parent fragment.
        let Fragment::Text { metrics, .. } = &result[0] else {
            unreachable!()
        };
        assert_eq!(metrics.ascent.raw(), 9.0);
    }

    #[test]
    fn nothing_to_split_returns_none() {
        let frags = vec![text_frag("hi", 10.0)];
        assert!(split_oversized_fragments(&frags, Pt::new(100.0), None).is_none());
    }

    #[test]
    fn single_character_is_never_split() {
        // Wider than max_width, but one character — nothing to break.
        let frags = vec![text_frag("M", 200.0)];
        assert!(split_oversized_fragments(&frags, Pt::new(10.0), None).is_none());
    }

    /// The character count, not the byte length, decides splittability. A
    /// byte-length test reports "needs split" for these and then splits
    /// nothing, costing the caller a pointless clone of the whole vector.
    #[test]
    fn multi_byte_single_character_is_never_split() {
        for ch in ["é", "😀", "字"] {
            let frags = vec![text_frag(ch, 200.0)];
            assert!(
                split_oversized_fragments(&frags, Pt::new(10.0), None).is_none(),
                "{ch:?} is one character regardless of its byte length"
            );
        }
    }

    #[test]
    fn non_positive_max_width_returns_none() {
        let frags = vec![text_frag("ab", 60.0)];
        assert!(split_oversized_fragments(&frags, Pt::ZERO, None).is_none());
        assert!(split_oversized_fragments(&frags, Pt::new(-5.0), None).is_none());
    }

    #[test]
    fn fragments_that_fit_pass_through_unchanged() {
        let frags = vec![
            text_frag("ab", 60.0), // splits
            text_frag("ok", 5.0),  // fits — must survive whole
        ];
        let result = split_oversized_fragments(&frags, Pt::new(20.0), None).expect("splits");
        assert_eq!(texts(&result), ["a", "b", "ok"]);
    }

    /// A combining mark must travel with its base. Splitting per scalar put the
    /// accent in its own fragment, which the line-fitter is then free to break
    /// before — carrying a bare `U+0301` to the start of the next line.
    #[test]
    fn split_never_separates_a_combining_mark_from_its_base() {
        let frags = vec![text_frag("e\u{301}x", 60.0)];
        let result = split_oversized_fragments(&frags, Pt::new(20.0), None).expect("splits");
        assert_eq!(
            texts(&result),
            ["e\u{301}", "x"],
            "the accent stays in the same fragment as its base letter"
        );
    }

    /// The cluster count decides splittability, so a single accented letter is
    /// as unsplittable as a single plain one — `needs_split` must not report
    /// work it cannot then do.
    #[test]
    fn multi_scalar_single_cluster_is_never_split() {
        for text in ["e\u{301}", "1\u{FE0F}\u{20E3}", "\u{1F1E9}\u{1F1EA}"] {
            let frags = vec![text_frag(text, 200.0)];
            assert!(
                split_oversized_fragments(&frags, Pt::new(10.0), None).is_none(),
                "{text:?} is one grapheme cluster regardless of its scalar count"
            );
        }
    }
}
