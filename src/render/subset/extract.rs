//! Byte extraction — pull subsettable SFNT bytes for a typeface, regardless of
//! whether it came from an embedded DOCX font or a system `FontMgr`.
//!
//! Steps:
//! 1. Source bytes from the typeface's [`TypefaceOrigin`] —
//!    `Embedded` reads from the registry (already deobfuscated per
//!    ECMA-376 §17.8.1.4); `System` calls `Typeface::to_font_data`, which also
//!    reports which face of a collection the typeface came from.
//! 2. Classify with [`FontFormat::detect`].
//! 3. Direct SFNT → return as-is. WOFF2 → decompress via `fontcull`. TTC →
//!    carve out the selected face. WOFF1 → return
//!    [`ExtractionError::UnsupportedFormat`], a documented capability boundary:
//!    ECMA-376 forbids WOFF in DOCX.
//!
//! # Collections
//!
//! A TrueType Collection is a table directory per face over one shared pool of
//! tables — several faces routinely point at the *same* `glyf`, differing only
//! in `cmap` and `name`. `fontcull` takes a single-face SFNT, so the selected
//! face's directory is resolved into a standalone font whose tables are copies
//! of the ones it referenced.
//!
//! Which face is "selected" is not something OOXML can say: `fontTable.xml`
//! offers four style slots per `w:font/@w:name` and no index. It comes from the
//! catalogue, which enumerated the collection when the font was registered, and
//! travels on [`TypefaceOrigin::Embedded::collection_index`].

use crate::render::fonts::opentype::{FontFormat, FormatError, SfntFlavor};
use crate::render::fonts::{FontRegistry, TypefaceEntry, TypefaceOrigin};

/// SFNT bytes verified ready for `fontcull::subset_font_data_unicode`. The
/// newtype prevents accidental mixing with the various wrapper formats.
#[derive(Debug, Clone)]
pub struct ExtractedSfnt {
    pub bytes: Vec<u8>,
    pub flavor: SfntFlavor,
}

#[derive(thiserror::Error, Debug)]
pub enum ExtractionError {
    #[error("system typeface returned no font data via Typeface::to_font_data")]
    NoBytesAvailable,
    #[error("font format detection failed: {0}")]
    Format(#[from] FormatError),
    #[error("unsupported font format for subsetting: {0:?}")]
    UnsupportedFormat(FontFormat),
    #[error("WOFF2 decompression failed: {0}")]
    Woff2DecompressionFailed(String),
    #[error("post-decompression bytes are not SFNT (got {0:?})")]
    PostUnwrapNotSfnt(FontFormat),
    #[error("collection has {face_count} face(s); index {index} is out of range")]
    CollectionIndexOutOfRange { index: u32, face_count: u32 },
    #[error("could not carve face {index} out of the collection: {reason}")]
    CollectionFaceUnreadable { index: u32, reason: String },
}

/// Pull SFNT bytes for a typeface, unwrapping WOFF2 if necessary.
pub fn extract(
    entry: &TypefaceEntry,
    registry: &FontRegistry,
) -> Result<ExtractedSfnt, ExtractionError> {
    // The collection index travels with the bytes. For an embedded font it is
    // whatever the catalogue enumerated; for a system font `to_font_data`
    // reports it, and Skia is not in fact guaranteed to have stripped the
    // collection down to one face.
    let (raw, face) = match entry.origin {
        TypefaceOrigin::Embedded {
            id,
            collection_index,
        } => (registry.embedded_bytes(id).to_vec(), collection_index),
        TypefaceOrigin::System { .. } => {
            let (bytes, ttc_index) = entry
                .typeface
                .to_font_data()
                .ok_or(ExtractionError::NoBytesAvailable)?;
            (bytes, ttc_index as u32)
        }
    };

    classify_and_unwrap(raw, face)
}

fn classify_and_unwrap(bytes: Vec<u8>, face: u32) -> Result<ExtractedSfnt, ExtractionError> {
    let format = FontFormat::detect(&bytes)?;
    match format {
        FontFormat::Sfnt(flavor) => Ok(ExtractedSfnt { bytes, flavor }),
        FontFormat::Ttc { face_count } => carve_collection_face(&bytes, face, face_count),
        FontFormat::Woff(crate::render::subset::WoffVersion::V2) => unwrap_woff2(&bytes),
        // WOFF1 is a documented capability boundary — see the module doc.
        FontFormat::Woff(crate::render::subset::WoffVersion::V1) => {
            Err(ExtractionError::UnsupportedFormat(format))
        }
    }
}

/// Resolve one face of a collection into a standalone SFNT.
///
/// The collection header is a `numFonts`-long array of offsets, each to an
/// ordinary table directory. Copying the tables that directory names — rather
/// than slicing a byte range — is what makes this correct: faces share table
/// data, so the selected face's tables are scattered through the file and are
/// not contiguous.
fn carve_collection_face(
    bytes: &[u8],
    index: u32,
    face_count: u32,
) -> Result<ExtractedSfnt, ExtractionError> {
    if index >= face_count {
        return Err(ExtractionError::CollectionIndexOutOfRange { index, face_count });
    }
    let fail = |reason: String| ExtractionError::CollectionFaceUnreadable { index, reason };

    // TTC header: 'ttcf', major u16, minor u16, numFonts u32, then numFonts × Offset32.
    let offset_pos = TTC_HEADER_SIZE + index as usize * 4;
    let directory = read_u32(bytes, offset_pos)
        .ok_or_else(|| fail("collection offset table is truncated".into()))?
        as usize;

    let version = bytes
        .get(directory..directory + 4)
        .ok_or_else(|| fail("face directory is past the end of the file".into()))?;
    let flavor = match FontFormat::detect(version) {
        Ok(FontFormat::Sfnt(flavor)) => flavor,
        Ok(other) => return Err(fail(format!("face directory is not an SFNT ({other:?})"))),
        Err(err) => return Err(fail(err.to_string())),
    };

    let num_tables = read_u16(bytes, directory + 4)
        .ok_or_else(|| fail("face directory header is truncated".into()))?
        as usize;
    let mut tables = Vec::with_capacity(num_tables);
    for i in 0..num_tables {
        let rec = directory + SFNT_HEADER_SIZE + i * TABLE_RECORD_SIZE;
        let tag = bytes
            .get(rec..rec + 4)
            .ok_or_else(|| fail(format!("table record {i} is truncated")))?;
        let offset = read_u32(bytes, rec + 8)
            .ok_or_else(|| fail(format!("table record {i} has no offset")))?
            as usize;
        let length = read_u32(bytes, rec + 12)
            .ok_or_else(|| fail(format!("table record {i} has no length")))?
            as usize;
        let data = offset
            .checked_add(length)
            .and_then(|end| bytes.get(offset..end))
            .ok_or_else(|| fail(format!("table {i} runs past the end of the file")))?;
        let mut t = [0u8; 4];
        t.copy_from_slice(tag);
        tables.push((t, data.to_vec()));
    }

    let rebuilt = super::name_splice::rebuild_sfnt(version, tables).map_err(fail)?;
    log::debug!(
        "[subset] carved face {index} of {face_count} out of a collection \
         ({} bytes → {} bytes)",
        bytes.len(),
        rebuilt.len()
    );
    Ok(ExtractedSfnt {
        bytes: rebuilt,
        flavor,
    })
}

const TTC_HEADER_SIZE: usize = 12;
const SFNT_HEADER_SIZE: usize = 12;
const TABLE_RECORD_SIZE: usize = 16;

fn read_u16(bytes: &[u8], offset: usize) -> Option<u16> {
    let end = offset.checked_add(2)?;
    let slice = bytes.get(offset..end)?;
    Some(u16::from_be_bytes([slice[0], slice[1]]))
}

fn read_u32(bytes: &[u8], offset: usize) -> Option<u32> {
    let end = offset.checked_add(4)?;
    let slice = bytes.get(offset..end)?;
    Some(u32::from_be_bytes([slice[0], slice[1], slice[2], slice[3]]))
}

fn unwrap_woff2(bytes: &[u8]) -> Result<ExtractedSfnt, ExtractionError> {
    let unwrapped = fontcull::decompress_font(bytes)
        .map_err(|e| ExtractionError::Woff2DecompressionFailed(e.to_string()))?;
    let format = FontFormat::detect(&unwrapped)?;
    match format {
        FontFormat::Sfnt(flavor) => Ok(ExtractedSfnt {
            bytes: unwrapped,
            flavor,
        }),
        other => Err(ExtractionError::PostUnwrapNotSfnt(other)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::EmbeddedFontVariant;
    use crate::render::fonts::FaceRequest;
    use crate::render::fonts::FontRegistry;
    use crate::render::subset::WoffVersion;
    use skia_safe::{FontMgr, FontStyle};

    fn registry() -> FontRegistry {
        FontRegistry::new(FontMgr::new())
    }

    /// Pull bytes from any guaranteed-available system typeface.
    fn arbitrary_system_font_bytes() -> Vec<u8> {
        let mgr = FontMgr::new();
        let tf = mgr
            .legacy_make_typeface(None::<&str>, FontStyle::normal())
            .expect("system has no default typeface");
        tf.to_font_data().expect("legacy default lacks bytes").0
    }

    #[test]
    fn extract_from_embedded_origin_returns_raw_docx_bytes() {
        let bytes = arbitrary_system_font_bytes();
        let mut r = registry();
        r.register_embedded(
            "EmbeddedExtractProbe",
            EmbeddedFontVariant::Regular,
            bytes.clone(),
        )
        .unwrap();
        let entry = r.resolve(&FaceRequest::plain("EmbeddedExtractProbe"));
        assert!(matches!(entry.origin, TypefaceOrigin::Embedded { .. }));

        let extracted = extract(&entry, &r).expect("embedded extraction must succeed");
        assert_eq!(
            extracted.bytes, bytes,
            "embedded extraction must hand back exactly the registered bytes"
        );
    }

    #[test]
    fn extract_from_system_origin_calls_to_font_data() {
        let r = registry();
        let entry = r.resolve(&FaceRequest::plain("UnknownExtractProbe"));
        assert!(matches!(entry.origin, TypefaceOrigin::System { .. }));
        let extracted = extract(&entry, &r).expect("system extraction must succeed");
        // Phase 0 verified to_font_data returns clean SFNT on macOS+Linux.
        assert!(matches!(
            extracted.flavor,
            SfntFlavor::TrueTypeStandard | SfntFlavor::TrueTypeApple | SfntFlavor::OpenTypeCff,
        ));
        assert!(!extracted.bytes.is_empty());
    }

    #[test]
    fn extract_returns_unsupported_for_woff1_input() {
        // Build a synthetic WOFF1 magic — we don't actually decompress, just
        // assert that classification routes WOFF1 to the documented boundary.
        let mut woff1 = b"wOFF".to_vec();
        woff1.extend_from_slice(&[0u8; 40]); // pad to a plausible length
        let result = classify_and_unwrap(woff1, 0);
        assert!(matches!(
            result,
            Err(ExtractionError::UnsupportedFormat(FontFormat::Woff(
                WoffVersion::V1
            )))
        ));
    }

    /// Build a two-face collection whose faces share every table but `name`,
    /// which is how real collections are laid out — and the reason a face
    /// cannot be extracted by slicing a byte range.
    fn collection_of_two(sfnt: &[u8]) -> Vec<u8> {
        let num_tables = u16::from_be_bytes([sfnt[4], sfnt[5]]) as usize;
        let mut tables = Vec::new();
        for i in 0..num_tables {
            let rec = 12 + i * 16;
            let mut tag = [0u8; 4];
            tag.copy_from_slice(&sfnt[rec..rec + 4]);
            let off = u32::from_be_bytes(sfnt[rec + 8..rec + 12].try_into().unwrap()) as usize;
            let len = u32::from_be_bytes(sfnt[rec + 12..rec + 16].try_into().unwrap()) as usize;
            tables.push((tag, sfnt[off..off + len].to_vec()));
        }

        // Two identical directories over one shared table pool.
        let header = 12 + 2 * 4;
        let dir_size = 12 + num_tables * 16;
        let pool_start = header + 2 * dir_size;

        let mut pool = Vec::new();
        let mut offsets = Vec::new();
        for (_, data) in &tables {
            offsets.push(pool_start + pool.len());
            pool.extend_from_slice(data);
            while !pool.len().is_multiple_of(4) {
                pool.push(0);
            }
        }

        let mut out = Vec::new();
        out.extend_from_slice(b"ttcf");
        out.extend_from_slice(&[0x00, 0x01, 0x00, 0x00]);
        out.extend_from_slice(&2u32.to_be_bytes());
        out.extend_from_slice(&(header as u32).to_be_bytes());
        out.extend_from_slice(&((header + dir_size) as u32).to_be_bytes());
        for _ in 0..2 {
            out.extend_from_slice(&sfnt[0..4]);
            out.extend_from_slice(&sfnt[4..12]);
            for (i, (tag, data)) in tables.iter().enumerate() {
                out.extend_from_slice(tag);
                out.extend_from_slice(&[0u8; 4]); // checksum — rebuilt on carve
                out.extend_from_slice(&(offsets[i] as u32).to_be_bytes());
                out.extend_from_slice(&(data.len() as u32).to_be_bytes());
            }
        }
        out.extend_from_slice(&pool);
        out
    }

    /// A collection face is carved into a standalone SFNT rather than refused.
    /// Skia accepting the result is the real assertion: it means the rebuilt
    /// directory, checksums and `head.checksumAdjustment` are all right.
    #[test]
    fn a_collection_face_is_carved_into_a_standalone_font() {
        let sfnt = arbitrary_system_font_bytes();
        let ttc = collection_of_two(&sfnt);
        assert!(matches!(
            FontFormat::detect(&ttc),
            Ok(FontFormat::Ttc { face_count: 2 })
        ));

        for index in 0..2 {
            let carved = classify_and_unwrap(ttc.clone(), index)
                .unwrap_or_else(|e| panic!("face {index} must carve: {e}"));
            assert!(matches!(
                FontFormat::detect(&carved.bytes),
                Ok(FontFormat::Sfnt(_))
            ));
            assert!(
                skia_safe::FontMgr::new()
                    .new_from_data(&carved.bytes[..], 0)
                    .is_some(),
                "Skia must accept the carved face {index}"
            );
        }
    }

    #[test]
    fn a_collection_index_past_the_end_is_reported() {
        let ttc = collection_of_two(&arbitrary_system_font_bytes());
        assert!(matches!(
            classify_and_unwrap(ttc, 7),
            Err(ExtractionError::CollectionIndexOutOfRange {
                index: 7,
                face_count: 2
            })
        ));
    }

    /// A header that promises faces it does not describe must produce a typed
    /// error, not a panic — these bytes come from documents.
    #[test]
    fn a_truncated_collection_is_reported_not_fatal() {
        let mut ttc = b"ttcf".to_vec();
        ttc.extend_from_slice(&[0x00, 0x01, 0x00, 0x00]);
        ttc.extend_from_slice(&[0x00, 0x00, 0x00, 0x02]); // numFonts = 2, no offsets
        assert!(matches!(
            classify_and_unwrap(ttc, 0),
            Err(ExtractionError::CollectionFaceUnreadable { index: 0, .. })
        ));
    }

    #[test]
    fn extract_returns_format_error_for_garbage() {
        let garbage = vec![0xab, 0xcd, 0xef, 0x00, 1, 2, 3];
        let result = classify_and_unwrap(garbage, 0);
        assert!(matches!(result, Err(ExtractionError::Format(_))));
    }

    #[test]
    fn extract_unwraps_woff2_to_sfnt() {
        // Synthesize a WOFF2 from a real SFNT via fontcull's compress_to_woff2,
        // then verify our extract path round-trips it back to SFNT.
        let sfnt = arbitrary_system_font_bytes();
        let woff2 = match fontcull::compress_to_woff2(&sfnt) {
            Ok(b) => b,
            Err(e) => {
                // Some system fonts don't survive a WOFF2 round-trip in the
                // current fontcull/woofwoof — skip rather than fail. The
                // unwrap path itself is still exercised by garbage / format
                // tests; this test is the integration smoke.
                eprintln!("skipping: WOFF2 compression of system font failed: {e}");
                return;
            }
        };
        assert_eq!(&woff2[..4], b"wOF2", "fixture must actually be WOFF2");

        let unwrapped = classify_and_unwrap(woff2, 0).expect("WOFF2 must unwrap to SFNT");
        assert!(matches!(
            unwrapped.flavor,
            SfntFlavor::TrueTypeStandard | SfntFlavor::TrueTypeApple | SfntFlavor::OpenTypeCff,
        ));
        assert!(
            FontFormat::detect(&unwrapped.bytes)
                .unwrap()
                .is_directly_subsettable(),
            "post-unwrap bytes must be directly subsettable"
        );
    }
}
