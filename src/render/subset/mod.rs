//! Font subsetting — collect codepoint usage, subset typefaces, replace.
//!
//! The pass runs between layout and paint. Driven by the `subset-fonts` Cargo
//! feature (default-on). See `docs/font-subsetting.md` for the full design.
//!
//! Design invariants:
//! - **Single source of truth** for typeface bytes ([`crate::render::fonts::FontRegistry`])
//!   and for usage tracking ([`collect::CodepointUsage`]). Each piece of state
//!   lives in exactly one place.
//! - **Codepoint-driven subsetting.** `fontcull` walks the font's own `cmap` to
//!   derive the glyph closure and keeps `GSUB` substitutions reachable from it,
//!   so ligatures and contextual alternates survive and paint's
//!   `text_to_glyphs`/`cmap` lookups stay valid — no re-shaping needed.
//! - **Shapeability is validated, not assumed.** A structurally valid subset
//!   can still ship a broken `cmap`; [`apply()`] rejects any subset whose kept
//!   codepoints shape to `.notdef` and keeps the original bytes instead.
//! - **Spec touchpoints.** ECMA-376 §17.8 (DOCX font embedding,
//!   deobfuscation) is enforced upstream by the parser. ISO 32000-1 §9.6.4
//!   subset prefixes (`AAAAAA+`-style) are emitted by Skia's PDF backend at
//!   write time — we only feed it smaller bytes.

pub mod apply;
pub mod collect;
pub mod extract;
pub mod format;
#[cfg(feature = "subset-fonts")]
pub mod name_splice;

pub use apply::{apply, SubsetOutcome, SubsetReport};
pub use collect::{collect, Codepoint, CodepointUsage};
pub use extract::{extract, ExtractedSfnt, ExtractionError};
pub use format::{FontFormat, FormatError, SfntFlavor, WoffVersion};
