//! §17.18.59 counting systems, end to end (issue #152): each `counting-*.docx`
//! fixture (built by `scripts/make_issue152_fixtures.py`) carries two lists
//! per format — items 1‑2‑3 and 10‑11‑12, crossing the tens boundary where a
//! counting system first diverges from positional digits (十二, not 一二).
//!
//! The fixtures' `w:lvlText` is a bare `%1`, so every expected string below
//! is a full label draw command. The per-format construction rules and their
//! sources live in `src/render/resolve/counting.rs`; these tests pin the
//! pipeline — `w:numFmt` value → parse → resolve → layout — not the tables.

use std::path::Path;

const TEST_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/test-files");

/// Every text draw command in the laid-out fixture, trimmed.
fn drawn_texts(fixture: &str) -> Vec<String> {
    use dxpdf::render::layout::draw_command::DrawCommand;

    let path = Path::new(TEST_DIR).join(fixture);
    let bytes =
        std::fs::read(&path).unwrap_or_else(|e| panic!("failed to read {}: {e}", path.display()));
    let doc = dxpdf::docx::parse(&bytes)
        .unwrap_or_else(|e| panic!("failed to parse {}: {e}", path.display()));
    let (_, pages) = dxpdf::render::resolve_and_layout(doc);
    pages
        .iter()
        .flat_map(|p| &p.commands)
        .filter_map(|c| match c {
            DrawCommand::Text { text, .. } => Some(text.trim().to_string()),
            _ => None,
        })
        .collect()
}

/// Assert every `(format, label)` pair was drawn verbatim.
fn assert_labels(fixture: &str, expected: &[(&str, &str)]) {
    let texts = drawn_texts(fixture);
    for (format, label) in expected {
        assert!(
            texts.iter().any(|t| t == label),
            "{fixture}: `{format}` must draw the label {label:?}; drawn texts: {texts:?}"
        );
    }
}

// Expected labels are (numFmt value, label) for n = 1, 2, 3, 10, 11, 12 —
// filled per format below.

#[test]
fn chinese_simplified_labels() {
    assert_labels("counting-zh-hans.docx", &EXPECTED_ZH_HANS);
}

#[test]
fn chinese_traditional_labels() {
    assert_labels("counting-zh-hant.docx", &EXPECTED_ZH_HANT);
}

#[test]
fn japanese_labels() {
    assert_labels("counting-ja.docx", &EXPECTED_JA);
}

#[test]
fn korean_labels() {
    assert_labels("counting-ko.docx", &EXPECTED_KO);
}

#[test]
fn vietnamese_labels() {
    assert_labels("counting-vi.docx", &EXPECTED_VI);
}

#[test]
fn hindi_labels() {
    assert_labels("counting-hi.docx", &EXPECTED_HI);
}

#[test]
fn thai_labels() {
    assert_labels("counting-th.docx", &EXPECTED_TH);
}

#[test]
fn dollar_text_labels() {
    assert_labels("counting-en-dollar.docx", &EXPECTED_DOLLAR);
}

const EXPECTED_ZH_HANS: [(&str, &str); 6] = [
    ("chineseCounting", "一"),
    ("chineseCounting", "十"),
    ("chineseCounting", "十二"),
    ("chineseCountingThousand", "一"),
    ("chineseCountingThousand", "十二"),
    ("chineseLegalSimplified", "壹"),
];

const EXPECTED_ZH_HANT: [(&str, &str); 6] = [
    ("taiwaneseCounting", "一"),
    ("taiwaneseCounting", "十二"),
    ("taiwaneseCountingThousand", "十二"),
    ("taiwaneseDigital", "一二"),
    ("ideographLegalTraditional", "壹"),
    ("ideographLegalTraditional", "壹拾貳"),
];

const EXPECTED_JA: [(&str, &str); 5] = [
    ("japaneseCounting", "一"),
    ("japaneseCounting", "十二"),
    ("japaneseLegal", "壱"),
    ("japaneseLegal", "壱拾弐"),
    ("japaneseDigitalTenThousand", "一二"),
];

const EXPECTED_KO: [(&str, &str); 7] = [
    ("koreanCounting", "일"),
    ("koreanCounting", "십이"),
    ("koreanLegal", "하나"),
    ("koreanLegal", "열둘"),
    ("koreanDigital", "일이"),
    ("koreanDigital2", "一"),
    ("koreanDigital2", "一二"),
];

const EXPECTED_VI: [(&str, &str); 3] = [
    ("vietnameseCounting", "một"),
    ("vietnameseCounting", "mười"),
    ("vietnameseCounting", "mười hai"),
];

const EXPECTED_HI: [(&str, &str); 3] = [
    ("hindiCounting", "एक"),
    ("hindiCounting", "दस"),
    ("hindiCounting", "बारह"),
];

const EXPECTED_TH: [(&str, &str); 4] = [
    ("thaiCounting", "หนึ่ง"),
    ("thaiCounting", "สิบสอง"),
    ("bahtText", "หนึ่งบาทถ้วน"),
    ("bahtText", "สิบสองบาทถ้วน"),
];

const EXPECTED_DOLLAR: [(&str, &str); 3] = [
    ("dollarText", "One and 00/100"),
    ("dollarText", "Ten and 00/100"),
    ("dollarText", "Twelve and 00/100"),
];
