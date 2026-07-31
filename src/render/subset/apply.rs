//! Apply subsetting — for each used typeface, subset its bytes via `fontcull`,
//! rebuild as a Skia `Typeface`, and replace it in the [`FontRegistry`].
//!
//! Outcomes are surfaced exhaustively as [`SubsetOutcome`] variants — never
//! silently swallowed. The whole pipeline is best-effort: any single
//! typeface that can't be subsetted leaves the registry untouched for that
//! entry, so paint embeds the full original font; other typefaces still get
//! their savings.

use std::collections::HashSet;
use std::fmt;

use skia_safe::Data;

use crate::render::fonts::{FontRegistry, TypefaceId, TypefaceOrigin};
use crate::render::subset::collect::CodepointUsage;
use crate::render::subset::extract::{extract, ExtractionError};
use crate::render::subset::format::FontFormat;

/// Single source of truth for "what happened to each typeface?" — one variant
/// per terminal state of the apply pipeline. Exhaustive; if you add a state,
/// add a variant.
#[derive(Debug, Clone)]
pub enum SubsetOutcome {
    /// Subsetting produced strictly smaller bytes; new typeface installed.
    Subsetted {
        id: TypefaceId,
        bytes_before: usize,
        bytes_after: usize,
        codepoints_kept: usize,
    },
    /// Subsetting did not shrink the bytes — typically because every glyph
    /// in the typeface is referenced by the requested codepoint set, so
    /// there's nothing to drop. Original typeface left in place.
    UnchangedNoSavings {
        id: TypefaceId,
        bytes: usize,
        codepoints_kept: usize,
    },
    /// Capability boundary — the typeface bytes are in a format we don't
    /// extract (WOFF1, TTC). Original typeface left in place.
    UnsupportedFormat { id: TypefaceId, format: FontFormat },
    /// `Typeface::to_font_data` returned `None` for a system font — never
    /// observed on macOS/Linux in Phase 0 but possible on other backends.
    NoBytesAvailable { id: TypefaceId },
    /// `fontcull` rejected the input or produced an error during subsetting.
    SubsetterError { id: TypefaceId, message: String },
    /// `FontMgr::new_from_data` failed to build a Skia typeface from the
    /// subsetted bytes — would indicate fontcull produced malformed SFNT.
    SkiaRebuildFailed { id: TypefaceId },
    /// The subsetted typeface is structurally valid (Skia accepted the
    /// bytes) but subsetting **lost** cmap coverage the original had —
    /// a kept codepoint that shaped before now yields `.notdef`. Observed
    /// with macOS Apple-vendored fonts (Helvetica Neue, Arial Unicode MS)
    /// where klippa's cmap reconstruction silently drops mappings.
    /// Original typeface left in place; PDF embeds the full font.
    ///
    /// `regressed` counts codepoints that went from a real glyph to
    /// `.notdef`; `shapeable_before` is how many the *original* could shape.
    /// A codepoint the original could not shape either is not counted —
    /// it is a gap in the font, not damage from subsetting.
    UnshapeableSubset {
        id: TypefaceId,
        codepoints_kept: usize,
        regressed: usize,
        shapeable_before: usize,
    },
}

impl SubsetOutcome {
    pub fn id(&self) -> TypefaceId {
        match *self {
            Self::Subsetted { id, .. }
            | Self::UnchangedNoSavings { id, .. }
            | Self::UnsupportedFormat { id, .. }
            | Self::NoBytesAvailable { id }
            | Self::SubsetterError { id, .. }
            | Self::SkiaRebuildFailed { id }
            | Self::UnshapeableSubset { id, .. } => id,
        }
    }

    pub fn savings(&self) -> usize {
        match *self {
            Self::Subsetted {
                bytes_before,
                bytes_after,
                ..
            } => bytes_before.saturating_sub(bytes_after),
            _ => 0,
        }
    }
}

/// Aggregate report — one [`SubsetOutcome`] per typeface that appeared in the
/// usage map. Display formats a one-line human-readable summary suitable for
/// `log::info!`.
#[derive(Debug, Clone, Default)]
pub struct SubsetReport {
    pub outcomes: Vec<SubsetOutcome>,
}

impl SubsetReport {
    pub fn total_savings(&self) -> usize {
        self.outcomes.iter().map(|o| o.savings()).sum()
    }

    pub fn subsetted_count(&self) -> usize {
        self.outcomes
            .iter()
            .filter(|o| matches!(o, SubsetOutcome::Subsetted { .. }))
            .count()
    }
}

impl SubsetReport {
    pub fn unshapeable_count(&self) -> usize {
        self.outcomes
            .iter()
            .filter(|o| matches!(o, SubsetOutcome::UnshapeableSubset { .. }))
            .count()
    }
}

impl fmt::Display for SubsetReport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let subsetted = self.subsetted_count();
        let savings = self.total_savings();
        let total = self.outcomes.len();
        let other = total - subsetted;
        let unshapeable = self.unshapeable_count();
        if unshapeable == 0 {
            write!(
                f,
                "{subsetted}/{total} typefaces subsetted ({} bytes saved, {other} unchanged)",
                savings
            )
        } else {
            write!(
                f,
                "{subsetted}/{total} typefaces subsetted ({} bytes saved, {other} unchanged, {unshapeable} rejected as unshapeable)",
                savings
            )
        }
    }
}

/// Subset every used typeface and replace it in the registry. Unused
/// typefaces are left untouched; failures surface as outcome variants
/// rather than being silently dropped.
pub fn apply(usage: CodepointUsage, registry: &mut FontRegistry) -> SubsetReport {
    let mut outcomes = Vec::new();
    let mut processed: HashSet<TypefaceId> = HashSet::new();

    // Snapshot entries up front — we'll mutate the registry inside the loop.
    let entries = registry.cached_entries();

    for (_key, entry) in entries {
        let id = TypefaceId::from(&entry.typeface);
        // A typeface can be reachable from multiple cache keys (e.g. via
        // FONT_SUBSTITUTIONS). Subset it once; replace covers all keys.
        if !processed.insert(id) {
            continue;
        }
        let cps = match usage.codepoints(id) {
            Some(c) => c,
            None => continue, // unused — leave the original in place
        };

        let outcome = process_one(id, &entry, cps, registry);
        outcomes.push(outcome);
    }

    SubsetReport { outcomes }
}

fn process_one(
    id: TypefaceId,
    entry: &crate::render::fonts::TypefaceEntry,
    codepoints: &std::collections::BTreeSet<crate::render::subset::collect::Codepoint>,
    registry: &mut FontRegistry,
) -> SubsetOutcome {
    let extracted = match extract(entry, registry) {
        Ok(e) => e,
        Err(ExtractionError::NoBytesAvailable) => return SubsetOutcome::NoBytesAvailable { id },
        Err(ExtractionError::UnsupportedFormat(format)) => {
            return SubsetOutcome::UnsupportedFormat { id, format };
        }
        Err(e) => {
            return SubsetOutcome::SubsetterError {
                id,
                message: format!("extract: {e}"),
            };
        }
    };

    let bytes_before = extracted.bytes.len();
    let unicodes: Vec<u32> = codepoints.iter().map(|c| c.0).collect();

    let subsetted = match subset_with_fontcull(&extracted.bytes, &unicodes) {
        Ok(b) => b,
        Err(e) => {
            return SubsetOutcome::SubsetterError {
                id,
                message: format!("subset: {e}"),
            };
        }
    };

    // fontcull's API drops every record from the `name` table; restore it from
    // the original so the embedded PDF font keeps its real PostScript name
    // (otherwise Skia falls back to a synthetic `font<hex>` identifier).
    let subsetted = match crate::render::subset::name_splice::splice_original_name(
        &subsetted,
        &extracted.bytes,
    ) {
        Ok(b) => b,
        Err(_) => subsetted, // Splice is best-effort; fall back to unnamed subset.
    };

    let bytes_after = subsetted.len();
    if bytes_after >= bytes_before {
        return SubsetOutcome::UnchangedNoSavings {
            id,
            bytes: bytes_before,
            codepoints_kept: codepoints.len(),
        };
    }

    let new_tf = {
        let data = Data::new_copy(&subsetted);
        registry.font_mgr().new_from_data(&data, 0)
    };
    let new_tf = match new_tf {
        Some(t) => t,
        None => return SubsetOutcome::SkiaRebuildFailed { id },
    };

    // Post-validation: a structurally valid SFNT can still ship a broken
    // cmap. Verified by shaping the kept codepoints against the rebuilt
    // typeface and checking for `.notdef`. If the subsetter's output is
    // unshapeable we keep the original — PDF gets bigger but text renders.
    if let Some(failure) = check_shapeability(&entry.typeface, &new_tf, codepoints) {
        return SubsetOutcome::UnshapeableSubset {
            id,
            codepoints_kept: codepoints.len(),
            regressed: failure.regressed,
            shapeable_before: failure.shapeable_before,
        };
    }

    let new_origin = TypefaceOrigin::System {
        typeface_id: TypefaceId::from(&new_tf),
    };
    registry.replace_typeface_by_id(id, new_tf, new_origin);

    SubsetOutcome::Subsetted {
        id,
        bytes_before,
        bytes_after,
        codepoints_kept: codepoints.len(),
    }
}

struct ShapeabilityFailure {
    /// Kept codepoints that shaped in the original but are `.notdef` in the
    /// subset.
    regressed: usize,
    /// Kept codepoints the *original* could shape — the denominator that
    /// makes `regressed` meaningful.
    shapeable_before: usize,
}

/// Reject a subset that **lost** cmap coverage the original typeface had.
///
/// Catches the macOS-Apple-font pathology where Skia accepts the subsetted
/// bytes but the cmap no longer binds glyph IDs, so every glyph paints as
/// `.notdef` and the page comes out blank.
///
/// **The comparison against `original` is the whole point.** Asking only
/// "does this codepoint shape in the subset?" cannot tell a subsetter that
/// destroyed the cmap from a font that never had the glyph — and the second is
/// ordinary. A single U+25AA bullet, or one SOFT HYPHEN, is enough to make a
/// perfectly good subset look broken; measured, that mistake embedded a 606 KB
/// face instead of its 13 KB subset. Only a codepoint that shaped *before* and
/// fails *after* is evidence of damage.
///
/// This is also why there is no whitespace/control exemption. The old code
/// skipped those because their `.notdef`-ness is font-dependent — which is true
/// of *every* codepoint, and is exactly what the baseline now handles. Worse,
/// the exemption hid real regressions: a subset that dropped a space the
/// original had would have gone uncaught.
///
/// Uses `Typeface::unichar_to_glyph` per codepoint rather than shaping a probe
/// string, so no assumption is needed about glyphs corresponding one-to-one
/// with `char`s.
fn check_shapeability(
    original: &skia_safe::Typeface,
    subsetted: &skia_safe::Typeface,
    codepoints: &std::collections::BTreeSet<crate::render::subset::collect::Codepoint>,
) -> Option<ShapeabilityFailure> {
    let mut regressed = 0usize;
    let mut shapeable_before = 0usize;

    for cp in codepoints {
        let Ok(unichar) = i32::try_from(cp.0) else {
            continue;
        };
        if original.unichar_to_glyph(unichar) == 0 {
            // The original could not shape it either — a gap in the font, not
            // damage from subsetting.
            continue;
        }
        shapeable_before += 1;
        if subsetted.unichar_to_glyph(unichar) == 0 {
            regressed += 1;
        }
    }

    if regressed == 0 {
        return None;
    }
    Some(ShapeabilityFailure {
        regressed,
        shapeable_before,
    })
}

fn subset_with_fontcull(bytes: &[u8], unicodes: &[u32]) -> Result<Vec<u8>, String> {
    fontcull::subset_font_data_unicode(bytes, unicodes, &[]).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::EmbeddedFontVariant;
    use crate::render::dimension::Pt;
    use crate::render::geometry::{PtOffset, PtSize};
    use crate::render::layout::draw_command::{DrawCommand, LayoutedPage};
    use crate::render::resolve::color::RgbColor;
    use crate::render::subset::collect::collect;
    use skia_safe::{Font, FontMgr, FontStyle};
    use std::rc::Rc;

    fn fmgr() -> FontMgr {
        FontMgr::new()
    }

    fn arbitrary_system_sfnt_bytes() -> Vec<u8> {
        let mgr = fmgr();
        // Prefer Helvetica/Arial-style fonts that are known TrueType — these
        // are the realistic subsetting targets for DOCX-class documents.
        let candidates = ["Arial", "Helvetica", "Liberation Sans", "Noto Sans"];
        for name in candidates {
            if let Some(tf) = mgr.match_family_style(name, FontStyle::normal()) {
                if tf.family_name().eq_ignore_ascii_case(name) {
                    if let Some((b, _)) = tf.to_font_data() {
                        if !b.is_empty() && b[0] == 0x00 {
                            return b;
                        }
                    }
                }
            }
        }
        // Fallback to any default — even if it's CFF, fontcull handles it.
        let tf = mgr
            .legacy_make_typeface(None::<&str>, FontStyle::normal())
            .expect("system has no default typeface");
        tf.to_font_data().expect("default lacks bytes").0
    }

    fn page_with_text(text: &str, family: &str) -> LayoutedPage {
        LayoutedPage {
            commands: vec![DrawCommand::Text {
                position: PtOffset::new(Pt::new(72.0), Pt::new(100.0)),
                text: Rc::from(text),
                font_family: Rc::from(family),
                char_spacing: Pt::ZERO,
                font_size: Pt::new(12.0),
                bold: false,
                italic: false,
                color: RgbColor::BLACK,
                text_scale: 1.0,
            }],
            page_size: PtSize::new(Pt::new(612.0), Pt::new(792.0)),
        }
    }

    #[test]
    fn subset_shrinks_size_for_partial_codepoint_set() {
        // Direct fontcull invocation — proves the integration produces
        // smaller output for a small codepoint set.
        let bytes = arbitrary_system_sfnt_bytes();
        let unicodes: Vec<u32> = "abc".chars().map(|c| c as u32).collect();
        let subsetted = subset_with_fontcull(&bytes, &unicodes)
            .expect("fontcull subsetting must succeed for a normal font");
        assert!(
            subsetted.len() < bytes.len(),
            "subsetted bytes ({}) must be smaller than original ({})",
            subsetted.len(),
            bytes.len()
        );
    }

    #[test]
    fn subset_output_is_skia_shapeable() {
        // The critical invariant: after subsetting, Skia can still shape
        // text against the resulting typeface. This is what was broken with
        // the earlier `subsetter` crate (which strips cmap).
        let bytes = arbitrary_system_sfnt_bytes();
        let unicodes: Vec<u32> = "abc".chars().map(|c| c as u32).collect();
        let subsetted = subset_with_fontcull(&bytes, &unicodes).unwrap();

        let mgr = fmgr();
        let new_tf = mgr
            .new_from_data(&Data::new_copy(&subsetted), 0)
            .expect("Skia must accept fontcull's output as a valid Typeface");
        let font = Font::from_typeface(new_tf, 12.0);
        let glyphs = font.text_to_glyphs_vec("abc");
        assert!(
            glyphs.iter().all(|&g| g != 0),
            "shaped glyphs must not be .notdef ({glyphs:?}) — subsetted font must retain cmap"
        );
        let (width, _) = font.measure_str("abc", None);
        assert!(width > 0.0, "measured width must be positive, got {width}");
    }

    #[test]
    fn apply_replaces_typeface_in_registry() {
        let bytes = arbitrary_system_sfnt_bytes();
        let mut r = FontRegistry::new(fmgr());
        r.register_embedded("ApplyProbe", EmbeddedFontVariant::Regular, bytes)
            .unwrap();
        let original = r.resolve("ApplyProbe", FontStyle::normal());
        let original_id = TypefaceId::from(&original.typeface);

        let pages = vec![page_with_text("abc", "ApplyProbe")];
        let usage = collect(&pages, &r);
        let report = apply(usage, &mut r);

        assert_eq!(report.outcomes.len(), 1);
        match &report.outcomes[0] {
            SubsetOutcome::Subsetted {
                bytes_after,
                bytes_before,
                ..
            } => {
                assert!(
                    bytes_after < bytes_before,
                    "report must record real shrinkage"
                );
            }
            other => panic!("expected Subsetted, got {other:?}"),
        }

        // Resolution now returns a different typeface — the subsetted one.
        let after = r.resolve("ApplyProbe", FontStyle::normal());
        let after_id = TypefaceId::from(&after.typeface);
        assert_ne!(
            after_id, original_id,
            "registry resolution must return the subsetted typeface"
        );
    }

    #[test]
    fn apply_leaves_unused_typefaces_untouched() {
        let bytes = arbitrary_system_sfnt_bytes();
        let mut r = FontRegistry::new(fmgr());
        // Register one font but never reference it in pages.
        r.register_embedded("UnusedProbe", EmbeddedFontVariant::Regular, bytes.clone())
            .unwrap();
        let original = r.resolve("UnusedProbe", FontStyle::normal());
        let original_id = TypefaceId::from(&original.typeface);

        // Pages reference a *different* typeface so usage doesn't include the
        // unused entry.
        let pages = vec![page_with_text("xyz", "UsedFamilyABC")];
        let usage = collect(&pages, &r);
        let report = apply(usage, &mut r);

        // Unused entry retains its original id.
        let unchanged = r.resolve("UnusedProbe", FontStyle::normal());
        assert_eq!(
            TypefaceId::from(&unchanged.typeface),
            original_id,
            "unused typefaces must not be touched"
        );
        // Report is keyed by usage entries, not registry entries.
        for o in &report.outcomes {
            assert_ne!(o.id(), original_id, "unused id must not appear in report");
        }
    }

    #[test]
    fn apply_emits_one_outcome_per_typeface_in_usage() {
        let bytes = arbitrary_system_sfnt_bytes();
        let mut r = FontRegistry::new(fmgr());
        r.register_embedded("OutcomeProbe", EmbeddedFontVariant::Regular, bytes)
            .unwrap();
        let pages = vec![
            page_with_text("hello", "OutcomeProbe"),
            page_with_text("world", "OutcomeProbe"), // same typeface as above
        ];
        let usage = collect(&pages, &r);
        assert_eq!(usage.typeface_count(), 1);
        let report = apply(usage, &mut r);
        assert_eq!(
            report.outcomes.len(),
            1,
            "one outcome per distinct typeface in usage, even with multiple commands"
        );
    }

    #[test]
    fn report_display_summarizes_outcomes() {
        let report = SubsetReport {
            outcomes: vec![
                SubsetOutcome::Subsetted {
                    id: TypefaceId(1),
                    bytes_before: 100_000,
                    bytes_after: 10_000,
                    codepoints_kept: 50,
                },
                SubsetOutcome::UnsupportedFormat {
                    id: TypefaceId(2),
                    format: FontFormat::Woff(crate::render::subset::WoffVersion::V1),
                },
            ],
        };
        let s = report.to_string();
        assert!(s.contains("1/2"));
        assert!(s.contains("90000")); // savings
    }

    /// Helper: resolve a font from the host or skip the test cleanly.
    fn host_font(family: &str) -> Option<skia_safe::Typeface> {
        let mgr = fmgr();
        let tf = mgr.match_family_style(family, FontStyle::normal())?;
        if tf.family_name().eq_ignore_ascii_case(family) {
            Some(tf)
        } else {
            None
        }
    }

    /// Direct end-to-end of the apply path against a host system font.
    /// `apply` swaps the typeface in the registry only when the subsetted
    /// typeface can shape its kept codepoints; if the post-validation
    /// catches an unshapeable subset, the registry retains the original.
    /// Either way, the registry's resolved typeface must shape correctly
    /// after `apply` returns.
    #[test]
    fn apply_never_installs_unshapeable_subset() {
        // macOS's Helvetica Neue is the canonical case: fontcull produces
        // structurally valid bytes whose cmap doesn't bind glyph IDs.
        // On hosts without it, fall back to whatever standard font is
        // available; the invariant ("registry shapes correctly after
        // apply") must hold for every host font.
        let candidates = ["Helvetica Neue", "Arial Unicode MS", "Helvetica", "Arial"];
        let target = match candidates.iter().find(|f| host_font(f).is_some()) {
            Some(t) => *t,
            None => {
                eprintln!("skipping: no candidate system font available");
                return;
            }
        };

        let mut r = FontRegistry::new(fmgr());
        // Force the registry to cache this exact typeface so apply() picks
        // it up. The simplest path is just a normal resolve(...) — that
        // populates the typefaces cache via FontMgr.
        let _ = r.resolve(target, FontStyle::normal());

        // Build pages whose text uses real codepoints from the font.
        let pages = vec![page_with_text("Numbers: 1, 2, 3, 4, 5", target)];
        let usage = collect(&pages, &r);
        assert_eq!(
            usage.typeface_count(),
            1,
            "test precondition — exactly one typeface in use"
        );

        let _report = apply(usage, &mut r);

        // The contract: regardless of which SubsetOutcome variant fires,
        // the registry's resolved typeface must shape the original probe
        // string to non-.notdef glyphs. For a font fontcull mishandles,
        // this is the post-validation rejecting the broken subset and
        // leaving the original; for fonts fontcull handles, it's the
        // subsetted version still shaping correctly.
        let after = r.resolve(target, FontStyle::normal());
        let font = skia_safe::Font::from_typeface(after.typeface, 12.0);
        let probe = "Numbers: 1, 2, 3, 4, 5";
        let glyphs = font.text_to_glyphs_vec(probe);
        let nondef: Vec<u16> = glyphs.iter().filter(|&&g| g != 0).copied().collect();
        let zeros = glyphs.iter().filter(|&&g| g == 0).count();
        assert_eq!(
            zeros,
            0,
            "post-`apply` typeface for '{target}' must shape every probe codepoint \
             to a non-.notdef glyph (got {zeros}/{} .notdef out of {nondef:?})",
            glyphs.len()
        );
    }

    fn cps_of(text: &str) -> std::collections::BTreeSet<crate::render::subset::collect::Codepoint> {
        text.chars()
            .map(crate::render::subset::collect::Codepoint::from)
            .collect()
    }

    /// A typeface compared against itself can never regress, whatever it can or
    /// cannot shape.
    #[test]
    fn check_shapeability_passes_when_nothing_changed() {
        let mgr = fmgr();
        let tf = mgr
            .legacy_make_typeface(None::<&str>, FontStyle::normal())
            .expect("system has a default typeface");
        assert!(
            check_shapeability(&tf, &tf, &cps_of("abc")).is_none(),
            "identical typefaces cannot have lost coverage"
        );
    }

    /// **This test asserted the opposite before F1#1.** It required the
    /// predicate to fire when a Latin font could not shape a CJK codepoint —
    /// which is a gap in the font, not damage from subsetting, and is exactly
    /// the confusion that made one SOFT HYPHEN embed a whole 606 KB face.
    ///
    /// A codepoint neither side can shape is not a regression.
    #[test]
    fn a_codepoint_the_original_never_had_is_not_a_regression() {
        let tf = match host_font("Helvetica").or_else(|| host_font("Arial")) {
            Some(t) => t,
            None => {
                eprintln!("skipping: no Latin-only system font");
                return;
            }
        };
        // 烏 is not in standard Helvetica/Arial — precondition, asserted rather
        // than assumed, so this cannot pass vacuously on a host where it is.
        assert_eq!(
            tf.unichar_to_glyph('\u{70CF}' as i32),
            0,
            "precondition: the chosen font must lack this codepoint"
        );
        assert!(
            check_shapeability(&tf, &tf, &cps_of("\u{70CF}")).is_none(),
            "the original could not shape it either — not the subsetter's doing"
        );
    }

    /// The case the check exists for: coverage the original *had* is gone after
    /// subsetting. Built with the real subsetter rather than two unrelated host
    /// fonts, so it does not depend on which fonts a machine happens to carry.
    #[test]
    fn losing_coverage_the_original_had_is_a_regression() {
        let mgr = fmgr();
        let Some(tf) = host_font("Carlito")
            .or_else(|| host_font("Helvetica"))
            .or_else(|| host_font("Arial"))
        else {
            eprintln!("skipping: no system font");
            return;
        };
        let Some((bytes, _)) = tf.to_font_data() else {
            eprintln!("skipping: no font data");
            return;
        };
        // Subset down to 'a' only; 'b' is then absent from the result's cmap.
        let Ok(narrow) = fontcull::subset_font_data_unicode(&bytes, &['a' as u32], &[]) else {
            eprintln!("skipping: subsetter declined");
            return;
        };
        let Some(narrow_tf) = mgr.new_from_data(&Data::new_copy(&narrow), 0) else {
            eprintln!("skipping: rebuild failed");
            return;
        };
        // Preconditions, so a subsetter change cannot make this vacuous.
        assert_ne!(tf.unichar_to_glyph('b' as i32), 0, "original shapes 'b'");
        assert_eq!(
            narrow_tf.unichar_to_glyph('b' as i32),
            0,
            "narrow drops 'b'"
        );

        let failure = check_shapeability(&tf, &narrow_tf, &cps_of("ab"))
            .expect("losing 'b' must be reported");
        assert_eq!(failure.regressed, 1, "only 'b' regressed");
        assert_eq!(
            failure.shapeable_before, 2,
            "'a' and 'b' both shaped before"
        );
    }

    /// Whitespace is no longer exempt, and that is deliberate: the old
    /// exemption existed because a space's `.notdef`-ness is font-dependent,
    /// which the baseline now handles properly. Dropping a space the original
    /// had is a real regression and must be caught.
    #[test]
    fn a_dropped_space_is_not_exempt() {
        let mgr = fmgr();
        let Some(tf) = host_font("Carlito")
            .or_else(|| host_font("Helvetica"))
            .or_else(|| host_font("Arial"))
        else {
            eprintln!("skipping: no system font");
            return;
        };
        let Some((bytes, _)) = tf.to_font_data() else {
            eprintln!("skipping: no font data");
            return;
        };
        let Ok(narrow) = fontcull::subset_font_data_unicode(&bytes, &['a' as u32], &[]) else {
            eprintln!("skipping: subsetter declined");
            return;
        };
        let Some(narrow_tf) = mgr.new_from_data(&Data::new_copy(&narrow), 0) else {
            eprintln!("skipping: rebuild failed");
            return;
        };
        if tf.unichar_to_glyph(' ' as i32) == 0 || narrow_tf.unichar_to_glyph(' ' as i32) != 0 {
            eprintln!("skipping: this font/subsetter pair keeps the space glyph");
            return;
        }
        assert!(
            check_shapeability(&tf, &narrow_tf, &cps_of(" ")).is_some(),
            "a space present before and gone after is a regression"
        );
    }
}
