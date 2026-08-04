//! What a document asked for, and what that means once a face has been found.
//!
//! # The tri-state
//!
//! ECMA-376 §17.3.2.1 (`w:b`) and §17.3.2.16 (`w:i`) are toggle properties, and
//! §17.7.2 gives them three states, not two: absent from the whole cascade,
//! explicitly on, explicitly off. The model preserves all three as
//! `Option<bool>` — and then the render side collapsed them to `bool` at a
//! single line in `fragment::font_props_from_run`, two layers above font
//! resolution.
//!
//! That collapse is why resolution used to need a special case. With a `bool`,
//! "not bold" arrives as a *request for weight 400*, indistinguishable from a
//! document that genuinely wants Regular. So a face name carrying its own weight
//! — `"Calibri Light"` at 342 — was overruled by a default that meant nothing,
//! and the old `merged_alias_weight` had to carry a comment explaining that the
//! requested weight "is not really a weight" and must be ignored when it is
//! `NORMAL`.
//!
//! [`Toggle`] removes the need for that reasoning. [`Absent`](Toggle::Absent)
//! asks for no weight at all, so the face's own weight stands; only
//! [`On`](Toggle::On) asks for one.
//!
//! # Absent and Off select the same face
//!
//! Deliberately, and it is worth being explicit about why the third state still
//! earns its place. The two differ *during* the §17.7.2 cascade, where an
//! explicit `w:val="0"` must override an inherited `w:b` — the model already
//! handles that, and it is what produces `Off` here rather than `Absent`. At
//! face selection they agree: neither asks for extra weight, so both leave the
//! matched face's intrinsic weight alone.
//!
//! The one behaviour that would separate them is *synthetic* emboldening — Word
//! thickens a face when `w:b` is set and no bolder face exists, and under that
//! rule `Off` on an already-bold face would mean "do not thicken further" while
//! `Absent` would mean nothing at all. This engine does no synthetic
//! emboldening, so the distinction is currently unobservable. Modelling it
//! anyway keeps the information available at the point where it would be needed,
//! instead of reconstructing it later from a `bool` that never had it.

use skia_safe::font_style::{Slant, Weight, Width};
use skia_safe::FontStyle;

use super::face::IntrinsicStyle;

/// A §17.7.2 toggle property as it reaches face selection.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub enum Toggle {
    /// No `w:b` / `w:i` anywhere in the cascade. The matched face's own style
    /// stands.
    #[default]
    Absent,
    /// `<w:b w:val="0"/>` — the run explicitly declines the toggle.
    Off,
    /// `<w:b/>` or `<w:b w:val="1"/>`.
    On,
}

impl Toggle {
    /// Build from the model's `Option<bool>`, which is the shape the §17.7.2
    /// cascade produces.
    pub fn from_option(value: Option<bool>) -> Self {
        match value {
            None => Self::Absent,
            Some(false) => Self::Off,
            Some(true) => Self::On,
        }
    }

    /// Whether the toggle is asking for something, as opposed to leaving the
    /// face alone. `Off` is not asking — see the module doc.
    pub fn is_on(self) -> bool {
        matches!(self, Self::On)
    }
}

/// A request for a face, as the document expressed it.
///
/// `name` is the `w:rFonts` value after theme resolution (§17.3.2.26) — it may
/// name a family, a face, a PostScript name, or something that is none of those.
/// Deciding which is the resolver's job, not the caller's.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct FaceRequest<'a> {
    pub name: &'a str,
    pub bold: Toggle,
    pub italic: Toggle,
}

impl<'a> FaceRequest<'a> {
    pub fn new(name: &'a str, bold: Toggle, italic: Toggle) -> Self {
        Self { name, bold, italic }
    }

    /// A request that names a family and asks for nothing else.
    pub fn plain(name: &'a str) -> Self {
        Self::new(name, Toggle::Absent, Toggle::Absent)
    }
}

/// The style to select, once the request's name has been matched against
/// something with a known intrinsic style.
///
/// Not a `FontStyle` because the difference between "the request pinned this
/// weight" and "this is just the base the toggles were applied to" matters when
/// ranking candidate faces — see [`EffectiveStyle::weight_is_requested`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EffectiveStyle {
    pub weight: i32,
    pub width: Width,
    pub slant: Slant,
    /// `w:b` was explicitly on, so `weight` is a floor the request set rather
    /// than the base face's own weight.
    weight_requested: bool,
    /// `w:i` was explicitly on.
    slant_requested: bool,
}

impl EffectiveStyle {
    /// Apply a request's toggles to the style of the thing its name matched.
    ///
    /// `base` is [`IntrinsicStyle::NEUTRAL`] when the name matched a *family*,
    /// which carries no style of its own, and the matched face's own intrinsic
    /// style when it matched a face. That split is the whole behavioural change:
    /// a bare `"Calibri"` still resolves to Regular exactly as before, while
    /// `"Calibri Light"` now starts from 342 instead of being flattened to 400.
    pub fn resolve(request: &FaceRequest<'_>, base: IntrinsicStyle) -> Self {
        // `On` raises a lighter face but never lowers a heavier one: `<w:b/>`
        // on an ExtraBold face wants at least Bold, and flattening 800 to 700
        // would be a downgrade the document did not ask for.
        let weight = if request.bold.is_on() {
            base.weight.max(*Weight::BOLD)
        } else {
            base.weight
        };
        let slant = if request.italic.is_on() {
            Slant::Italic
        } else {
            base.slant
        };
        Self {
            weight,
            width: base.width,
            slant,
            weight_requested: request.bold.is_on(),
            slant_requested: request.italic.is_on(),
        }
    }

    /// Whether the document explicitly asked to be bold, so a candidate face
    /// lighter than [`weight`](Self::weight) is a genuine failure to honour the
    /// request rather than merely a different face.
    pub fn weight_is_requested(&self) -> bool {
        self.weight_requested
    }

    /// Whether the document explicitly asked to be italic.
    pub fn slant_is_requested(&self) -> bool {
        self.slant_requested
    }

    pub fn is_italic(&self) -> bool {
        matches!(self.slant, Slant::Italic | Slant::Oblique)
    }

    /// The Skia style to hand a font manager that matches by style rather than
    /// by index.
    pub fn font_style(&self) -> FontStyle {
        FontStyle::new(Weight::from(self.weight), self.width, self.slant)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render::fonts::FaceRequest;
    use crate::render::fonts::Toggle;

    fn at(weight: i32, slant: Slant) -> IntrinsicStyle {
        IntrinsicStyle {
            weight,
            slant,
            ..IntrinsicStyle::NEUTRAL
        }
    }

    // ── the tri-state itself ─────────────────────────────────────────────

    #[test]
    fn the_cascade_s_option_bool_maps_onto_all_three_states() {
        assert_eq!(Toggle::from_option(None), Toggle::Absent);
        assert_eq!(Toggle::from_option(Some(false)), Toggle::Off);
        assert_eq!(Toggle::from_option(Some(true)), Toggle::On);
    }

    /// `Off` declines the toggle; it does not request the absence of weight.
    #[test]
    fn only_on_is_asking_for_something() {
        assert!(Toggle::On.is_on());
        assert!(!Toggle::Off.is_on());
        assert!(!Toggle::Absent.is_on());
    }

    #[test]
    fn absent_is_the_default() {
        assert_eq!(Toggle::default(), Toggle::Absent);
    }

    // ── the behavioural change ───────────────────────────────────────────

    /// The regression this whole tri-state exists to prevent: a face that
    /// carries its own weight must keep it when nothing asked otherwise.
    /// Under the old `bool` this arrived as "weight 400" and flattened 342.
    #[test]
    fn an_unrequested_weight_leaves_a_light_face_light() {
        let base = at(342, Slant::Upright);
        for toggle in [Toggle::Absent, Toggle::Off] {
            let e = EffectiveStyle::resolve(
                &FaceRequest::new("Calibri Light", toggle, Toggle::Absent),
                base,
            );
            assert_eq!(e.weight, 342, "{toggle:?} must not thicken the face");
            assert!(!e.weight_is_requested());
        }
    }

    /// And the guard on the other side: a bare family name still resolves to
    /// Regular, which is what keeps every existing document paginating the same.
    #[test]
    fn a_family_name_with_no_toggles_still_resolves_to_regular() {
        let e = EffectiveStyle::resolve(&FaceRequest::plain("Calibri"), IntrinsicStyle::NEUTRAL);
        assert_eq!(e.weight, 400);
        assert_eq!(e.width, Width::NORMAL);
        assert_eq!(e.slant, Slant::Upright);
        assert_eq!(e.font_style(), FontStyle::normal());
    }

    #[test]
    fn an_explicit_bold_raises_a_lighter_face() {
        let e = EffectiveStyle::resolve(
            &FaceRequest::new("Calibri Light", Toggle::On, Toggle::Absent),
            at(342, Slant::Upright),
        );
        assert_eq!(e.weight, *Weight::BOLD);
        assert!(e.weight_is_requested());
    }

    /// …but never lowers a heavier one. `<w:b/>` on an ExtraBold face means
    /// "at least bold", not "exactly bold".
    #[test]
    fn an_explicit_bold_never_lowers_a_heavier_face() {
        let e = EffectiveStyle::resolve(
            &FaceRequest::new("Inter ExtraBold", Toggle::On, Toggle::Absent),
            at(*Weight::EXTRA_BOLD, Slant::Upright),
        );
        assert_eq!(e.weight, *Weight::EXTRA_BOLD);
    }

    // ── slant ────────────────────────────────────────────────────────────

    #[test]
    fn an_explicit_italic_overrides_an_upright_face() {
        let e = EffectiveStyle::resolve(
            &FaceRequest::new("Inter", Toggle::Absent, Toggle::On),
            at(400, Slant::Upright),
        );
        assert_eq!(e.slant, Slant::Italic);
        assert!(e.is_italic());
        assert!(e.slant_is_requested());
    }

    /// An unrequested slant keeps whatever the matched face has — including
    /// oblique, which must not be silently normalised to italic.
    #[test]
    fn an_unrequested_slant_preserves_the_face_s_own() {
        for toggle in [Toggle::Absent, Toggle::Off] {
            let e = EffectiveStyle::resolve(
                &FaceRequest::new("Inter Oblique", Toggle::Absent, toggle),
                at(400, Slant::Oblique),
            );
            assert_eq!(e.slant, Slant::Oblique, "{toggle:?}");
            assert!(e.is_italic());
            assert!(!e.slant_is_requested());
        }
    }

    #[test]
    fn width_is_carried_through_untouched() {
        let base = IntrinsicStyle {
            width: Width::CONDENSED,
            ..at(600, Slant::Upright)
        };
        let e = EffectiveStyle::resolve(
            &FaceRequest::new("X Condensed SemiBold", Toggle::On, Toggle::On),
            base,
        );
        assert_eq!(
            e.width,
            Width::CONDENSED,
            "no OOXML toggle addresses width, so the face's own must survive"
        );
        assert_eq!(*e.font_style().weight(), *Weight::BOLD);
        assert_eq!(e.font_style().slant(), Slant::Italic);
    }

    #[test]
    fn both_toggles_compose() {
        let e = EffectiveStyle::resolve(
            &FaceRequest::new("Inter", Toggle::On, Toggle::On),
            IntrinsicStyle::NEUTRAL,
        );
        assert_eq!(e.font_style(), FontStyle::bold_italic());
    }
}
