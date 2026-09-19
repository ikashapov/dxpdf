//! §17.18.78 `ST_Shd` — what a `w:shd` element actually paints (issue #149).
//!
//! The parser has carried the full pattern enum all along
//! ([`crate::model::ShadingPattern`]); this is where a pattern becomes
//! something a renderer can draw. Two shapes come out:
//!
//! - **Flat** — one colour. `clear` is the fill alone; `solid` is the
//!   *pattern* colour alone (its fill sits behind a 100% pattern, i.e. is
//!   invisible); and every `pctN` is the §17.18.78 blend — the pattern
//!   colour at N% over the fill. Word renders the percentage tints as a
//!   flat blend, not a dot raster: every Word-compatible implementation
//!   read for this computes the same per-channel interpolation and paints
//!   one colour (ONLYOFFICE `sdkjs` `CDocumentShd.GetSimpleColor`,
//!   LibreOffice `writerfilter` `CellColorHandler` — the latter carrying
//!   the comment "Code from binary word filter (the values are out of
//!   1000)").
//! - **Pattern** — a geometric stripe/cross family plus its colours, drawn
//!   by the table emitter as fill-behind-stripes. The twelve values
//!   collapse to six families × thick/thin, which is how the spec names
//!   them; per [MS-DOC]'s Ipat table the plain values are Word 97's "dark"
//!   patterns and the `thin*` values its light ones.
//!
//! # `nil`, and `auto` inside a shading
//!
//! `nil` resolves to `None`. [MS-OI29500] §2.1.550 records the product
//! behaviour: Word treats `nil` on a table, row or cell as *equivalent to
//! not specifying shading* (the older [MS-OE376] §2.18.85 said "treats nil
//! as solid" — the current note supersedes it). It is still not the same
//! as an absent element to the *cascade*: an explicit `nil` at one §17.7.6
//! level suppresses the shading a lower-priority level declared, which is
//! why the level is picked before this function runs.
//!
//! Inside a pattern, `w:color="auto"` means **black** and `w:fill="auto"`
//! means **white** — ink over paper unless the document says otherwise.
//! Both reference implementations hard-code exactly that (LibreOffice:
//! "shading color auto means black", "fill color auto means white").
//! `clear` is the exception on the fill side: `val="clear" w:fill="auto"`
//! is **no shading at all**, not a white box — LibreOffice maps it to
//! `FillStyle_NONE`, and painting white instead would cover whatever the
//! page put underneath. A geometric pattern's auto fill is treated the
//! same way: stripes over nothing.
//!
//! # The fractional percentages, and the blend's rounding
//!
//! `pct12`, `pct37`, `pct62` and `pct87` are 12.5%, 37.5%, 62.5% and
//! 87.5% — eighths, not their names. §17.18.78's own prose titles them
//! "12.5% Fill Pattern" etc., and [MS-DOC]'s Ipat table maps
//! `ipatPctNew12` to "12.5%, ST_Shd: pct12"; LibreOffice carries 125‰.
//! (ONLYOFFICE and docx4j use the literal 12/37/62/87 — a ≤0.5%-per-channel
//! simplification; the spec value is kept here.) Percentages are stored in
//! tenths of a percent so the halves stay exact, and the blend **truncates**
//! rather than rounds — both reference implementations do (`| 0` in sdkjs,
//! integer division in LibreOffice), and following them keeps a channel
//! byte-identical with documents they produced.

use crate::model::{Color, Shading, ShadingPattern};

use super::color::{resolve_color, ColorContext, RgbColor};

/// What a `w:shd` paints, ready to draw.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ResolvedShading {
    /// One flat colour covering the shaded box.
    Flat(RgbColor),
    /// A geometric §17.18.78 pattern: `background` behind (`None` for an
    /// `auto` fill — stripes over nothing), stripes in `foreground` — the
    /// element's `w:fill` and `w:color` respectively.
    Pattern {
        geometry: PatternGeometry,
        foreground: RgbColor,
        background: Option<RgbColor>,
    },
}

/// One of the six stripe/cross families, thick or thin.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PatternGeometry {
    pub family: PatternFamily,
    /// The `thin*` variants — Word 97's "light" patterns: same family,
    /// stripes 1 px wide instead of 2.
    pub thin: bool,
}

/// The six geometric families of §17.18.78.
///
/// The diagonal directions were first settled by [MS-DOC]'s Ipat names and
/// the spec's own Word-rendered swatches alone: `diagStripe` is
/// `ipatDkBackDiag`, stripes rising to the right (`/`); `reverseDiagStripe`
/// is `ipatDkForeDiag`, falling to the right (`\`). Unlike this module's
/// other choices (pct values, `solid`, `nil`, blend truncation), that had
/// no second, independent source cross-checking it (ONLYOFFICE/
/// LibreOffice name neither direction explicitly) — until PR #180 review
/// finding #6 asked for one. **Confirmed against a real Word render**:
/// `test-files/shading-patterns.docx`'s `diagStripe` cell (row 4, col 1)
/// renders `/`, and `reverseDiagStripe` (row 4, col 2) renders `\`, exactly
/// as named below. Pinned by `diagonal_stripes_slope_and_mirror_each_other`
/// in `tests/shading_patterns.rs`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PatternFamily {
    /// `horzStripe` — horizontal stripes.
    Horz,
    /// `vertStripe` — vertical stripes.
    Vert,
    /// `diagStripe` — stripes rising to the right (`/`).
    Diag,
    /// `reverseDiagStripe` — stripes falling to the right (`\`).
    ReverseDiag,
    /// `horzCross` — horizontal and vertical stripes together.
    ///
    /// Read as the union its name says, like the other three crosses — for
    /// which the union reading is *proven*: the spec's 8×8 swatch tiles for
    /// `diagCross`, `thinDiagCross` and `thinHorzCross` decompose exactly
    /// into their two constituent stripe families. The dark `horzCross`
    /// swatch alone shows a diagonal diamond lattice instead; in an image
    /// set that demonstrably misplaces at least one tile (`pct10` and
    /// `pct12` share one image), a lone anomaly against the value's own
    /// name was not followed, on the strength of that pattern alone.
    ///
    /// **Confirmed against a real Word render** (PR #180 review finding
    /// #5): `test-files/shading-patterns.docx`'s `horzCross` cell renders
    /// as straight horizontal and vertical lines crossing — not tilted,
    /// not a diamond lattice. The spec's dark `horzCross` swatch was the
    /// anomaly, as suspected; the union reading here was right all along.
    /// Pinned by `crosses_draw_both_directions` in `tests/shading_patterns.rs`,
    /// which already asserted axis-aligned (not diagonal) lines for this
    /// family before this was confirmed.
    HorzCross,
    /// `diagCross` — both diagonal directions together.
    DiagCross,
}

/// Resolve a parsed `w:shd` to what it paints, or `None` for `nil` and for
/// the transparent `clear`-over-`auto`.
pub fn resolve_shading(s: &Shading) -> Option<ResolvedShading> {
    use PatternFamily::*;
    let fill = || resolve_color(s.fill, ColorContext::Background);
    let ink = || resolve_color(s.color, ColorContext::Text);
    let tint = |tenths| Some(ResolvedShading::Flat(blend(ink(), fill(), tenths)));
    // `auto` on the fill side means "no shading at all" (see the module
    // doc) rather than the white it resolves to everywhere else — a `clear`
    // over it and a geometric pattern's background both need this same
    // exception, so it is spelled out once rather than at each call site.
    let resolved_fill = || match s.fill {
        Color::Auto => None,
        _ => Some(fill()),
    };
    let pattern = |family, thin| {
        Some(ResolvedShading::Pattern {
            geometry: PatternGeometry { family, thin },
            foreground: ink(),
            background: resolved_fill(),
        })
    };
    match s.pattern {
        ShadingPattern::Nil => None,
        // A clear shading over an auto fill states no colour at all —
        // FillStyle_NONE, not white. See the module doc.
        ShadingPattern::Clear => resolved_fill().map(ResolvedShading::Flat),
        // 100% pattern colour: the fill is fully covered, so `solid` *is*
        // `w:color` — painting the fill here is the bug this module replaces.
        ShadingPattern::Solid => Some(ResolvedShading::Flat(ink())),
        ShadingPattern::Pct5 => tint(50),
        ShadingPattern::Pct10 => tint(100),
        ShadingPattern::Pct12 => tint(125),
        ShadingPattern::Pct15 => tint(150),
        ShadingPattern::Pct20 => tint(200),
        ShadingPattern::Pct25 => tint(250),
        ShadingPattern::Pct30 => tint(300),
        ShadingPattern::Pct35 => tint(350),
        ShadingPattern::Pct37 => tint(375),
        ShadingPattern::Pct40 => tint(400),
        ShadingPattern::Pct45 => tint(450),
        ShadingPattern::Pct50 => tint(500),
        ShadingPattern::Pct55 => tint(550),
        ShadingPattern::Pct60 => tint(600),
        ShadingPattern::Pct62 => tint(625),
        ShadingPattern::Pct65 => tint(650),
        ShadingPattern::Pct70 => tint(700),
        ShadingPattern::Pct75 => tint(750),
        ShadingPattern::Pct80 => tint(800),
        ShadingPattern::Pct85 => tint(850),
        ShadingPattern::Pct87 => tint(875),
        ShadingPattern::Pct90 => tint(900),
        ShadingPattern::Pct95 => tint(950),
        ShadingPattern::HorzStripe => pattern(Horz, false),
        ShadingPattern::ThinHorzStripe => pattern(Horz, true),
        ShadingPattern::VertStripe => pattern(Vert, false),
        ShadingPattern::ThinVertStripe => pattern(Vert, true),
        ShadingPattern::DiagStripe => pattern(Diag, false),
        ShadingPattern::ThinDiagStripe => pattern(Diag, true),
        ShadingPattern::ReverseDiagStripe => pattern(ReverseDiag, false),
        ShadingPattern::ThinReverseDiagStripe => pattern(ReverseDiag, true),
        ShadingPattern::HorzCross => pattern(HorzCross, false),
        ShadingPattern::ThinHorzCross => pattern(HorzCross, true),
        ShadingPattern::DiagCross => pattern(DiagCross, false),
        ShadingPattern::ThinDiagCross => pattern(DiagCross, true),
    }
}

/// The §17.18.78 blend: `fg` at `tenths` tenths of a percent over `bg`,
/// per channel. Truncating integer arithmetic — see the module doc for why
/// truncation and not rounding.
///
/// `tenths` is documented ≤ 1000 (every call site today passes a hardcoded
/// literal from the closed `ShadingPattern` match, so this is provably true
/// now), but a `debug_assert!` alone compiles to nothing in release, and
/// `1000 - tenths` would wrap rather than panic if that guard were the only
/// thing standing between an out-of-range value and this arithmetic. The
/// `.min(1000)` below costs nothing on every path that already respects the
/// bound and saturates rather than wraps on every path that doesn't, in
/// every build profile — so this function needs no debug/release split to
/// stay correct if its contract ever widens.
fn blend(fg: RgbColor, bg: RgbColor, tenths: u32) -> RgbColor {
    let tenths = tenths.min(1000);
    let mix = |f: u8, b: u8| -> u8 {
        ((u32::from(f) * tenths + u32::from(b) * (1000 - tenths)) / 1000) as u8
    };
    RgbColor {
        r: mix(fg.r, bg.r),
        g: mix(fg.g, bg.g),
        b: mix(fg.b, bg.b),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shd(pattern: ShadingPattern, color: Color, fill: Color) -> Shading {
        Shading {
            fill,
            pattern,
            color,
        }
    }

    const BLUE: Color = Color::Rgb(0x0000FF);
    const WHITE: Color = Color::Rgb(0xFFFFFF);

    fn flat(s: Shading) -> RgbColor {
        match resolve_shading(&s) {
            Some(ResolvedShading::Flat(c)) => c,
            other => panic!("expected a flat colour, got {other:?}"),
        }
    }

    /// [MS-OI29500] §2.1.550: `nil` on a cell is equivalent to not
    /// specifying shading — and its point in the cascade is to *suppress*,
    /// so it resolves to `None` rather than to a white flat.
    #[test]
    fn nil_resolves_to_nothing() {
        let s = shd(
            ShadingPattern::Nil,
            Color::Rgb(0xAA0000),
            Color::Rgb(0xAA0000),
        );
        assert_eq!(resolve_shading(&s), None);
    }

    /// `clear` paints the fill alone — the pre-#149 behaviour, unchanged —
    /// but a `clear` over an `auto` fill is transparent, not a white box:
    /// LibreOffice maps that pair to `FillStyle_NONE`.
    #[test]
    fn clear_is_the_fill_and_auto_fill_is_transparent() {
        let s = shd(ShadingPattern::Clear, BLUE, Color::Rgb(0xFFCC00));
        assert_eq!(
            flat(s),
            RgbColor {
                r: 0xFF,
                g: 0xCC,
                b: 0
            }
        );
        let transparent = shd(ShadingPattern::Clear, BLUE, Color::Auto);
        assert_eq!(resolve_shading(&transparent), None);
    }

    /// `solid` is a 100% pattern: the *pattern* colour shows, the fill is
    /// covered. Painting the fill was the defect for this value.
    #[test]
    fn solid_is_the_pattern_color_not_the_fill() {
        let s = shd(
            ShadingPattern::Solid,
            Color::Rgb(0xCC0000),
            Color::Rgb(0x00CC00),
        );
        assert_eq!(
            flat(s),
            RgbColor {
                r: 0xCC,
                g: 0,
                b: 0
            }
        );
    }

    /// `blend`'s `tenths` is documented ≤ 1000 and every call site today
    /// passes a hardcoded literal from the closed `ShadingPattern` match, so
    /// this can't happen through `resolve_shading` — but the bound was only
    /// a `debug_assert!`, which compiles to nothing in release, and
    /// `1000 - tenths` (`u32`) wraps rather than panics if that guard is
    /// gone. Calls `blend` directly (bypassing `resolve_shading`, which
    /// cannot produce an out-of-range `tenths`) to pin that an over-range
    /// value saturates instead of wrapping, in every build profile.
    #[test]
    fn blend_saturates_a_tenths_value_above_the_documented_bound() {
        let black = RgbColor { r: 0, g: 0, b: 0 };
        let white = RgbColor {
            r: 255,
            g: 255,
            b: 255,
        };
        assert_eq!(blend(black, white, 1500), blend(black, white, 1000));
    }

    /// The blend interpolates each channel: 25% of blue over white leaves
    /// red/green at 75% of 255 = 191.25, truncated to 191.
    #[test]
    fn pct25_blends_toward_the_pattern_color() {
        let s = shd(ShadingPattern::Pct25, BLUE, WHITE);
        assert_eq!(
            flat(s),
            RgbColor {
                r: 191,
                g: 191,
                b: 255
            }
        );
    }

    /// `auto` is ink over paper: black pattern colour, white fill — so a
    /// bare `pct50` is the mid grey. 127, not 128: the blend truncates
    /// 127.5 the way both reference implementations do.
    #[test]
    fn auto_colors_blend_black_over_white_truncating() {
        let s = shd(ShadingPattern::Pct50, Color::Auto, Color::Auto);
        assert_eq!(
            flat(s),
            RgbColor {
                r: 127,
                g: 127,
                b: 127
            }
        );
    }

    /// §17.18.78 names them `pct12`/`pct37`/`pct62`/`pct87` but defines the
    /// eighths — 12.5% of black over white is 223.125, which distinguishes
    /// 12.5 (→ 223) from a literal 12 (→ 224).
    #[test]
    fn the_eighth_percentages_are_halves_not_their_names() {
        let s = shd(ShadingPattern::Pct12, Color::Auto, Color::Auto);
        assert_eq!(
            flat(s),
            RgbColor {
                r: 223,
                g: 223,
                b: 223
            }
        );
        let s = shd(ShadingPattern::Pct87, Color::Auto, Color::Auto);
        assert_eq!(
            flat(s),
            RgbColor {
                r: 31,
                g: 31,
                b: 31
            }
        );
    }

    /// Every `pctN` blends monotonically from the fill (0%) toward the
    /// pattern colour (100%) — pinned over the whole family so a transposed
    /// table entry cannot hide between two spot checks.
    #[test]
    fn the_tint_family_is_monotonic_in_its_percentage() {
        use ShadingPattern::*;
        let tints = [
            Pct5, Pct10, Pct12, Pct15, Pct20, Pct25, Pct30, Pct35, Pct37, Pct40, Pct45, Pct50,
            Pct55, Pct60, Pct62, Pct65, Pct70, Pct75, Pct80, Pct85, Pct87, Pct90, Pct95,
        ];
        let greys: Vec<u8> = tints
            .iter()
            .map(|&p| flat(shd(p, Color::Auto, Color::Auto)).r)
            .collect();
        assert!(
            greys.windows(2).all(|w| w[0] > w[1]),
            "each step darker than the last: {greys:?}"
        );
        assert!(*greys.first().unwrap() > 240, "pct5 is nearly the fill");
        assert!(*greys.last().unwrap() < 16, "pct95 is nearly the ink");
    }

    /// The twelve geometric values map onto six families × thick/thin, with
    /// `w:color` as the stripe ink and `w:fill` behind — and an `auto` fill
    /// resolving to no background at all, like `clear`'s.
    #[test]
    fn geometric_values_resolve_to_their_family_and_weight() {
        use PatternFamily::*;
        for (pattern, family, thin) in [
            (ShadingPattern::HorzStripe, Horz, false),
            (ShadingPattern::ThinHorzStripe, Horz, true),
            (ShadingPattern::VertStripe, Vert, false),
            (ShadingPattern::ThinVertStripe, Vert, true),
            (ShadingPattern::DiagStripe, Diag, false),
            (ShadingPattern::ThinDiagStripe, Diag, true),
            (ShadingPattern::ReverseDiagStripe, ReverseDiag, false),
            (ShadingPattern::ThinReverseDiagStripe, ReverseDiag, true),
            (ShadingPattern::HorzCross, HorzCross, false),
            (ShadingPattern::ThinHorzCross, HorzCross, true),
            (ShadingPattern::DiagCross, DiagCross, false),
            (ShadingPattern::ThinDiagCross, DiagCross, true),
        ] {
            let s = shd(pattern, Color::Rgb(0x112233), Color::Rgb(0xAABBCC));
            match resolve_shading(&s) {
                Some(ResolvedShading::Pattern {
                    geometry,
                    foreground,
                    background,
                }) => {
                    assert_eq!(geometry, PatternGeometry { family, thin }, "{pattern:?}");
                    assert_eq!(
                        foreground,
                        RgbColor {
                            r: 0x11,
                            g: 0x22,
                            b: 0x33
                        },
                        "{pattern:?}: stripes are w:color"
                    );
                    assert_eq!(
                        background,
                        Some(RgbColor {
                            r: 0xAA,
                            g: 0xBB,
                            b: 0xCC
                        }),
                        "{pattern:?}: the fill sits behind"
                    );
                }
                other => panic!("{pattern:?}: expected a pattern, got {other:?}"),
            }

            let auto_fill = shd(pattern, Color::Rgb(0x112233), Color::Auto);
            match resolve_shading(&auto_fill) {
                Some(ResolvedShading::Pattern { background, .. }) => {
                    assert_eq!(background, None, "{pattern:?}: auto fill paints nothing");
                }
                other => panic!("{pattern:?}: expected a pattern, got {other:?}"),
            }
        }
    }
}
