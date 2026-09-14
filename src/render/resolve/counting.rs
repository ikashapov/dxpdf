//! §17.18.59 counting systems — the number read aloud in a language's own
//! numerals (issue #152).
//!
//! These are the values `StNumberFormat::Other` used to classify as "spellout
//! in a script that looks like digits": `chineseCounting` writes 12 as 十二 —
//! twelve read aloud — not two positional digits. The format itself names the
//! language, so unlike §17.9.27's three formats none of these consults
//! `w:lang`; and like [`super::spellout`], every table is hand-written rather
//! than pulled from CLDR — the dependency arithmetic recorded there applies
//! here unchanged.
//!
//! # Sources, and how much to trust each rendering
//!
//! No Microsoft document specifies these renderings past two or three
//! examples. The rules below were reconstructed from, in order: the ECMA-376
//! 5th-edition prose and its normative character sets; [MS-OI29500]'s
//! implementation notes (range caps, character substitutions); LibreOffice's
//! DOCX `writerfilter` mappings and `i18npool` numeral data — including its
//! Korean tables, which were validated against Word 2019 (tdf#143526); and
//! ONLYOFFICE's independent Word-compatible renderer, executed directly.
//! Where the sources disagree the discrepancy is recorded on the function it
//! affects, and a **Word reference render** is what settles it.
//!
//! Two formats carry a deliberate deviation from Word itself: [MS-OI29500]
//! §2.1.545 notes (d)/(t) record that Word renders `bahtText` and
//! `dollarText` *list labels* as plain decimal, implementing the spellout
//! only in field switches and Excel. This engine renders the spellout the
//! spec mandates — issue #152's ask — and the note on each function is what
//! a maintainer needs to flip to Word's behavior instead.

use super::spellout;

// ─────────────────────────────────────────────────────────────────────────────
// Positional digit transliteration
// ─────────────────────────────────────────────────────────────────────────────

/// Each decimal digit replaced by a character, most significant first.
fn positional(n: u32, zero: char, digits: [char; 9]) -> String {
    if n == 0 {
        return zero.to_string();
    }
    let mut out = String::new();
    for b in n.to_string().bytes() {
        match b {
            b'0' => out.push(zero),
            d => out.push(digits[(d - b'1') as usize]),
        }
    }
    out
}

const CJK_DIGITS: [char; 9] = ['一', '二', '三', '四', '五', '六', '七', '八', '九'];

/// `taiwaneseDigital` — positional 一二三 with `○` (U+25CB WHITE CIRCLE) for
/// zero. Identical to `ideographDigital` except for the zero glyph, which the
/// 5th-edition spec assigns per format: U+3007 there, U+25CB here.
pub fn taiwanese_digital(n: u32) -> String {
    positional(n, '○', CJK_DIGITS)
}

/// `japaneseDigitalTenThousand` — positional 一二三 with `〇` (U+3007). "Ten
/// thousand" names the range cap, not a grouping: Word renders an **empty**
/// label from 10000 up ([MS-OI29500] §2.1.545), and no 万 grouping exists
/// despite the name.
pub fn japanese_digital_ten_thousand(n: u32) -> String {
    if n >= 10_000 {
        return String::new();
    }
    positional(n, '〇', CJK_DIGITS)
}

/// `koreanDigital` — positional Hangul digits, zero `영`.
pub fn korean_digital(n: u32) -> String {
    positional(
        n,
        '영',
        ['일', '이', '삼', '사', '오', '육', '칠', '팔', '구'],
    )
}

/// `koreanDigital2` — positional plain Hanja digits, zero `零` (U+96F6).
/// Word 2019-verified via LibreOffice's tdf#143526: the zero is 零, not the
/// ideographic circle 〇, and the digits are 一二三, not the financial 壹貳參.
pub fn korean_digital2(n: u32) -> String {
    positional(n, '零', CJK_DIGITS)
}

// ─────────────────────────────────────────────────────────────────────────────
// The chineseCounting hybrid
// ─────────────────────────────────────────────────────────────────────────────

/// `chineseCounting` / `taiwaneseCounting` — a hybrid the 5th-edition spec
/// gives identical steps for: a proper 十-reading for 1–99 (十, 十二, 二十一),
/// then **digit-by-digit** transliteration from 100 up with `○` U+25CB for
/// zero (100 = 一○○, 1234 = 一二三四) — never 百/千/万. That regime switch is
/// the whole difference from the `-Thousand` formats.
///
/// Open-Xml-PowerTools instead falls back to Arabic digits at ≥10000 where
/// the spec's algorithm simply continues digit-wise; the spec is followed —
/// **Word reference render**.
fn counting_hybrid(n: u32) -> String {
    match n {
        0 => '○'.to_string(),
        1..=9 => CJK_DIGITS[(n - 1) as usize].to_string(),
        10..=19 => {
            let mut out = String::from("十");
            if !n.is_multiple_of(10) {
                out.push(CJK_DIGITS[(n % 10 - 1) as usize]);
            }
            out
        }
        20..=99 => {
            let mut out = String::new();
            out.push(CJK_DIGITS[(n / 10 - 1) as usize]);
            out.push('十');
            if !n.is_multiple_of(10) {
                out.push(CJK_DIGITS[(n % 10 - 1) as usize]);
            }
            out
        }
        _ => positional(n, '○', CJK_DIGITS),
    }
}

/// The `chineseCounting` hybrid — see the shared implementation above.
pub fn chinese_counting(n: u32) -> String {
    counting_hybrid(n)
}

/// The `taiwaneseCounting` hybrid — spec-identical to [`chinese_counting`].
pub fn taiwanese_counting(n: u32) -> String {
    counting_hybrid(n)
}

// ─────────────────────────────────────────────────────────────────────────────
// CJK place-value readings
// ─────────────────────────────────────────────────────────────────────────────

/// When the coefficient 1 is written before a unit character.
#[derive(Clone, Copy, PartialEq)]
enum OnePolicy {
    /// 10–19 read 十X only when the number stands alone below 100; the 1
    /// returns everywhere else (110 = 一百一十, 100000 = 一十萬). The
    /// Chinese reading formats.
    ElideTeensOnly,
    /// 1 is dropped before 十/百/千 at every magnitude but kept before the
    /// myriad (Japanese: 千 but 一万).
    ElideBeforeSubMyriad,
    /// 1 is dropped before every unit including the myriad (informal Korean:
    /// 만, not 일만).
    ElideAll,
    /// Always written — the anti-fraud legal formats (壹拾, 壱拾).
    Never,
}

/// One CJK place-value reading. `myriads` are the 10⁴ⁿ group characters in
/// ascending order (万, 億); groups render most significant first.
struct CjkReading {
    digits: [char; 9],
    ten: char,
    hundred: char,
    thousand: char,
    myriads: [char; 2],
    /// Interior-zero connector (零 or 〇); `None` omits interior zeros
    /// entirely — the Japanese and Korean readings.
    zero: Option<char>,
    /// What a value of exactly 0 renders as (a list may start at 0).
    zero_label: &'static str,
    one_policy: OnePolicy,
    /// Whether the connector is suppressed right after a myriad character —
    /// Word's documented deviation for `chineseCountingThousand`
    /// ([MS-OI29500] note e: 10005 = 一万五, not 一万零五).
    zero_after_myriad_omitted: bool,
    /// Values from here up render as an **empty** label ([MS-OI29500] note j
    /// documents the 10⁶ cap for the counting-thousand and legal formats).
    cap: Option<u32>,
}

impl CjkReading {
    /// Append the reading of one 10⁴ group (1–9999). `standalone` is true
    /// only for a units group that is also the whole number — the one place
    /// `ElideTeensOnly` drops the 1 of 10–19.
    fn push_group(&self, out: &mut String, n: u32, standalone: bool) {
        debug_assert!((1..10_000).contains(&n));
        let elide_sub = matches!(
            self.one_policy,
            OnePolicy::ElideBeforeSubMyriad | OnePolicy::ElideAll
        );
        let elide_ten = match self.one_policy {
            OnePolicy::ElideTeensOnly => standalone && n < 100,
            OnePolicy::ElideBeforeSubMyriad | OnePolicy::ElideAll => true,
            OnePolicy::Never => false,
        };

        let mut wrote_any = false;
        let mut pending_zero = false;
        let places = [
            (n / 1000, Some(self.thousand), elide_sub),
            (n / 100 % 10, Some(self.hundred), elide_sub),
            (n / 10 % 10, Some(self.ten), elide_ten),
            (n % 10, None, false),
        ];
        for (coeff, unit, elide_one) in places {
            if coeff == 0 {
                pending_zero |= wrote_any && unit.is_some();
                continue;
            }
            if pending_zero {
                if let Some(z) = self.zero {
                    out.push(z);
                }
                pending_zero = false;
            }
            if !(coeff == 1 && elide_one) {
                out.push(self.digits[(coeff - 1) as usize]);
            }
            if let Some(u) = unit {
                out.push(u);
            }
            wrote_any = true;
        }
    }

    fn read(&self, n: u32) -> String {
        if let Some(cap) = self.cap {
            if n >= cap {
                return String::new();
            }
        }
        if n == 0 {
            return self.zero_label.to_string();
        }

        let mut out = String::new();
        // 10⁴ groups, most significant first: (value, myriad char, remainder
        // after this group).
        let groups = [
            (n / 100_000_000, Some(self.myriads[1]), n % 100_000_000),
            (n / 10_000 % 10_000, Some(self.myriads[0]), n % 10_000),
            (n % 10_000, None, 0),
        ];
        let mut leading = true;
        for (value, myriad, rest) in groups {
            if value == 0 {
                continue;
            }
            let standalone = leading && myriad.is_none();
            let elide_myriad_one =
                value == 1 && myriad.is_some() && self.one_policy == OnePolicy::ElideAll;
            if !elide_myriad_one {
                self.push_group(&mut out, value, standalone);
            }
            if let Some(m) = myriad {
                out.push(m);
                // §: an interior zero run straddling the myriad (10005 →
                // 一萬零五) takes one connector — unless this reading omits
                // it there ([MS-OI29500] note e) or has no connector at all.
                if rest != 0 && rest < 1000 && !self.zero_after_myriad_omitted {
                    if let Some(z) = self.zero {
                        out.push(z);
                    }
                }
            }
            leading = false;
        }
        out
    }
}

/// `chineseCountingThousand` — the full Simplified-Chinese reading: 十/百/千/万
/// with `〇` (U+3007) as the interior-zero connector. The connector codepoint
/// follows the two Word-replication codebases (Open-Xml-PowerTools,
/// ONLYOFFICE); the ISO text prints 零 — **Word reference render**. Word
/// omits the connector straight after 万 (10005 = 一万五, [MS-OI29500] note
/// e) and blanks the label from 10⁶ up (note j).
pub fn chinese_counting_thousand(n: u32) -> String {
    CjkReading {
        digits: CJK_DIGITS,
        ten: '十',
        hundred: '百',
        thousand: '千',
        myriads: ['万', '亿'],
        zero: Some('〇'),
        zero_label: "〇",
        one_policy: OnePolicy::ElideTeensOnly,
        zero_after_myriad_omitted: true,
        cap: Some(1_000_000),
    }
    .read(n)
}

/// `taiwaneseCountingThousand` — the Traditional-Chinese reading: 十/百/千/萬
/// with `零` (U+96F6). Unlike its simplified sibling the connector *does*
/// follow 萬 (10005 = 一萬零五 is the spec's own worked example). The 10⁶
/// blank mirrors ONLYOFFICE; the spec instead continues to 億 — **Word
/// reference render**.
pub fn taiwanese_counting_thousand(n: u32) -> String {
    CjkReading {
        digits: CJK_DIGITS,
        ten: '十',
        hundred: '百',
        thousand: '千',
        myriads: ['萬', '億'],
        zero: Some('零'),
        zero_label: "零",
        one_policy: OnePolicy::ElideTeensOnly,
        zero_after_myriad_omitted: false,
        cap: Some(1_000_000),
    }
    .read(n)
}

/// `chineseLegalSimplified` — the Simplified-Chinese banker's numerals
/// 壹贰叁 with 拾/佰/仟, interior `零`, and — per [MS-OI29500] note q — the
/// **traditional** 萬 for ten-thousands. Coefficients are never elided
/// (10 = 壹拾), and the label blanks from 10⁶ (note j).
pub fn chinese_legal_simplified(n: u32) -> String {
    CjkReading {
        digits: ['壹', '贰', '叁', '肆', '伍', '陆', '柒', '捌', '玖'],
        ten: '拾',
        hundred: '佰',
        thousand: '仟',
        myriads: ['萬', '億'],
        zero: Some('零'),
        zero_label: "零",
        one_policy: OnePolicy::Never,
        zero_after_myriad_omitted: false,
        cap: Some(1_000_000),
    }
    .read(n)
}

/// `ideographLegalTraditional` — the Traditional-Chinese banker's numerals
/// 壹貳參 (參 is U+53C3) with 拾/佰/仟/萬 and interior `零`. Coefficients are
/// never elided; the spec's own example runs 壹拾, 壹拾壹, 壹拾貳. The 10⁶
/// blank mirrors ONLYOFFICE — **Word reference render**.
pub fn ideograph_legal_traditional(n: u32) -> String {
    CjkReading {
        digits: ['壹', '貳', '參', '肆', '伍', '陸', '柒', '捌', '玖'],
        ten: '拾',
        hundred: '佰',
        thousand: '仟',
        myriads: ['萬', '億'],
        zero: Some('零'),
        zero_label: "零",
        one_policy: OnePolicy::Never,
        zero_after_myriad_omitted: false,
        cap: Some(1_000_000),
    }
    .read(n)
}

/// `japaneseCounting` — the kanji reading: 一 is elided before 十/百/千 but
/// written before 万 (12 = 十二, 1000 = 千, 10000 = 一万), interior zeros
/// vanish without a connector (105 = 百五), and the label blanks from 10⁶
/// ([MS-OI29500] note j). The 千-elision at exactly 1000 is the one
/// cross-source wobble — **Word reference render**.
pub fn japanese_counting(n: u32) -> String {
    CjkReading {
        digits: CJK_DIGITS,
        ten: '十',
        hundred: '百',
        thousand: '千',
        myriads: ['万', '億'],
        zero: None,
        zero_label: "〇",
        one_policy: OnePolicy::ElideBeforeSubMyriad,
        zero_after_myriad_omitted: true,
        cap: Some(1_000_000),
    }
    .read(n)
}

/// `japaneseLegal` — the daiji anti-fraud reading: 壱弐参 (only 1, 2, 3, 5
/// take daiji forms; 4, 6–9 stay ordinary) with 拾, plain 百, daiji 阡 and
/// 萬/億. Every coefficient is written (10 = 壱拾, 1000 = 壱阡); no interior
/// connector; no documented cap below the general display limit.
pub fn japanese_legal(n: u32) -> String {
    CjkReading {
        digits: ['壱', '弐', '参', '四', '伍', '六', '七', '八', '九'],
        ten: '拾',
        hundred: '百',
        thousand: '阡',
        myriads: ['萬', '億'],
        zero: None,
        zero_label: "〇",
        one_policy: OnePolicy::Never,
        zero_after_myriad_omitted: true,
        cap: None,
    }
    .read(n)
}

/// `koreanCounting` — the informal Sino-Korean reading in Hangul: 일 is
/// elided before every unit including 만 (12 = 십이, 10000 = 만), no interior
/// connector, no spaces. Validated against Word 2019 through LibreOffice's
/// tdf#143526 test set; the bare 만 at exactly 10000 rests on the
/// LibreOffice + ONLYOFFICE consensus — **Word reference render**.
pub fn korean_counting(n: u32) -> String {
    CjkReading {
        digits: ['일', '이', '삼', '사', '오', '육', '칠', '팔', '구'],
        ten: '십',
        hundred: '백',
        thousand: '천',
        myriads: ['만', '억'],
        zero: None,
        zero_label: "영",
        one_policy: OnePolicy::ElideAll,
        zero_after_myriad_omitted: true,
        cap: None,
    }
    .read(n)
}

/// `koreanLegal` — native Korean numerals for 1–99 (하나, 둘 … 아흔아홉; the
/// native system genuinely ends there), then the [`korean_counting`] reading
/// — both Word-interop suites fall back to exactly it, not to a hybrid like
/// 백하나. The fallback is the least-attested part — **Word reference
/// render**.
pub fn korean_legal(n: u32) -> String {
    const ONES: [&str; 9] = [
        "하나", "둘", "셋", "넷", "다섯", "여섯", "일곱", "여덟", "아홉",
    ];
    const TENS: [&str; 9] = [
        "열", "스물", "서른", "마흔", "쉰", "예순", "일흔", "여든", "아흔",
    ];
    match n {
        1..=99 => {
            let mut out = String::new();
            if n / 10 > 0 {
                out.push_str(TENS[(n / 10 - 1) as usize]);
            }
            if !n.is_multiple_of(10) {
                out.push_str(ONES[(n % 10 - 1) as usize]);
            }
            out
        }
        _ => korean_counting(n),
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Thai
// ─────────────────────────────────────────────────────────────────────────────

const THAI_DIGITS: [&str; 9] = ["หนึ่ง", "สอง", "สาม", "สี่", "ห้า", "หก", "เจ็ด", "แปด", "เก้า"];

/// The shared Thai reading. `ed_needs_tens` selects the trailing-one rule:
/// `thaiCounting` writes เอ็ด whenever anything precedes the final 1
/// (101 = หนึ่งร้อยเอ็ด), while the BAHTTEXT engine requires a non-zero tens
/// digit (101 = หนึ่งร้อยหนึ่ง) — the two documented behaviors diverge at
/// exactly this point, so it is a parameter rather than a fork.
fn thai_reading(n: u32, ed_needs_tens: bool) -> String {
    if n == 0 {
        return "ศูนย์".to_string();
    }
    let mut out = String::new();
    // 10⁶ blocks joined by ล้าน — u32 needs at most two.
    let millions = n / 1_000_000;
    let rest = n % 1_000_000;
    if millions > 0 {
        thai_block(&mut out, millions);
        out.push_str("ล้าน");
    }
    if rest > 0 {
        thai_block(&mut out, rest);
    }
    thai_apply_ed(n, out, ed_needs_tens)
}

/// One block below 10⁶: scale digits each spelled (including 1: หนึ่งร้อย),
/// bare สิบ for a tens digit of 1, ยี่ for 2, interior zeros silent.
fn thai_block(out: &mut String, block: u32) {
    let scales: [(u32, &str); 3] = [(100_000, "แสน"), (10_000, "หมื่น"), (1_000, "พัน")];
    let mut rest = block;
    for (value, word) in scales {
        let digit = rest / value;
        if digit > 0 {
            out.push_str(THAI_DIGITS[(digit - 1) as usize]);
            out.push_str(word);
        }
        rest %= value;
    }
    if rest >= 100 {
        out.push_str(THAI_DIGITS[(rest / 100 - 1) as usize]);
        out.push_str("ร้อย");
        rest %= 100;
    }
    if rest == 0 {
        return;
    }
    match rest / 10 {
        0 => {}
        1 => out.push_str("สิบ"),
        2 => out.push_str("ยี่สิบ"),
        t => {
            out.push_str(THAI_DIGITS[(t - 1) as usize]);
            out.push_str("สิบ");
        }
    }
    if !rest.is_multiple_of(10) {
        out.push_str(THAI_DIGITS[(rest % 10 - 1) as usize]);
    }
}

/// Rewrite the final unit "หนึ่ง" as "เอ็ด" where the selected rule calls
/// for it.
fn thai_apply_ed(n: u32, rendered: String, ed_needs_tens: bool) -> String {
    if n % 10 != 1 || n < 11 {
        return rendered;
    }
    let use_ed = if ed_needs_tens {
        !(n / 10).is_multiple_of(10)
    } else {
        true
    };
    if !use_ed {
        return rendered;
    }
    match rendered.strip_suffix("หนึ่ง") {
        Some(head) if !head.is_empty() => format!("{head}เอ็ด"),
        _ => rendered,
    }
}

/// `thaiCounting` — Thai number words: สิบเอ็ด for 11, ยี่สิบ for 20, scale
/// words ร้อย/พัน/หมื่น/แสน/ล้าน each with a spelled coefficient (100 =
/// หนึ่งร้อย, never bare ร้อย), interior zeros silent, no separators.
pub fn thai_counting(n: u32) -> String {
    thai_reading(n, false)
}

/// `bahtText` — the Thai check-writing text: the BAHTTEXT-engine reading with
/// `บาทถ้วน` appended (labels are integers, so the สตางค์ branch never
/// fires). That engine's trailing-one rule differs from `thaiCounting`:
/// เอ็ด only after a non-zero tens digit, so 101 = หนึ่งร้อยหนึ่งบาทถ้วน.
///
/// [MS-OI29500] §2.1.545(d) records that Word renders this *list label* as
/// plain decimal and keeps the spellout for field switches and Excel; the
/// spellout is rendered here because it is what §17.18.59 mandates and what
/// issue #152 asks for. To match Word instead, replace the body with
/// `n.to_string()`.
pub fn baht_text(n: u32) -> String {
    let mut out = thai_reading(n, true);
    out.push_str("บาทถ้วน");
    out
}

// ─────────────────────────────────────────────────────────────────────────────
// Vietnamese
// ─────────────────────────────────────────────────────────────────────────────

/// `vietnameseCounting` — Vietnamese number words with the orthographic
/// alternations no grammar lets a renderer skip: `mươi` for tens from 20
/// (hai mươi), `mốt` for a final 1 after mươi (hai mươi mốt), `lăm` for a
/// final 5 after mười/mươi (mười lăm), and the Southern `lẻ`/`ngàn` lexemes
/// (một trăm lẻ năm, một ngàn).
///
/// The only Word-adjacent source (ONLYOFFICE's renderer) instead emits
/// naive, non-alternating forms — "hai mười một" — and wraps the sequence
/// modulo 1000, both of which contradict every Vietnamese orthography
/// including the one LibreOffice ships (`libnumbertext` vi). Its lexeme
/// *choices* (lẻ over linh, ngàn over nghìn) are kept as the closest thing
/// to Word evidence; its grammar is not — **Word reference render**.
pub fn vietnamese_counting(n: u32) -> String {
    if n == 0 {
        return "không".to_string();
    }

    fn under_hundred(n: u32, out: &mut Vec<&'static str>) {
        const DIGITS: [&str; 9] = [
            "một", "hai", "ba", "bốn", "năm", "sáu", "bảy", "tám", "chín",
        ];
        debug_assert!((1..100).contains(&n));
        if n < 10 {
            out.push(DIGITS[(n - 1) as usize]);
            return;
        }
        if n < 20 {
            out.push("mười");
            match n % 10 {
                0 => {}
                5 => out.push("lăm"),
                u => out.push(DIGITS[(u - 1) as usize]),
            }
            return;
        }
        out.push(DIGITS[(n / 10 - 1) as usize]);
        out.push("mươi");
        match n % 10 {
            0 => {}
            1 => out.push("mốt"),
            5 => out.push("lăm"),
            u => out.push(DIGITS[(u - 1) as usize]),
        }
    }

    fn under_thousand(n: u32, out: &mut Vec<&'static str>) {
        const DIGITS: [&str; 9] = [
            "một", "hai", "ba", "bốn", "năm", "sáu", "bảy", "tám", "chín",
        ];
        debug_assert!((1..1000).contains(&n));
        if n < 100 {
            under_hundred(n, out);
            return;
        }
        out.push(DIGITS[(n / 100 - 1) as usize]);
        out.push("trăm");
        match n % 100 {
            0 => {}
            u @ 1..=9 => {
                // Interior zero: một trăm lẻ năm.
                out.push("lẻ");
                out.push(DIGITS[(u - 1) as usize]);
            }
            u => under_hundred(u, out),
        }
    }

    let mut words: Vec<&'static str> = Vec::new();
    let groups: [(u32, Option<&'static str>); 4] = [
        (n / 1_000_000_000, Some("tỷ")),
        (n / 1_000_000 % 1000, Some("triệu")),
        (n / 1000 % 1000, Some("ngàn")),
        (n % 1000, None),
    ];
    let mut leading = true;
    for (value, scale) in groups {
        if value == 0 {
            continue;
        }
        // A sub-ten remainder after a scale word takes the connector:
        // một ngàn lẻ năm. (The formal "không trăm linh" padding of
        // check-writing style is deliberately not emitted — undocumented for
        // Word, and the proxy does not write it either.)
        if !leading && value < 10 {
            words.push("lẻ");
        }
        under_thousand(value, &mut words);
        if let Some(s) = scale {
            words.push(s);
        }
        leading = false;
    }
    words.join(" ")
}

// ─────────────────────────────────────────────────────────────────────────────
// Hindi
// ─────────────────────────────────────────────────────────────────────────────

/// The 1–99 table `hindiCounting` needs whole: Hindi decades are idiosyncratic
/// enough that no rule generates them. The spellings are the Word-proxy's
/// (ONLYOFFICE), several of which differ from dictionary forms in ways that
/// smell of transcription from Word output (इकीस for 21, छयालिस for 46) —
/// kept verbatim, flagged for the **Word reference render**.
const HINDI_WORDS: [&str; 99] = [
    "एक",
    "दो",
    "तीन",
    "चार",
    "पांच",
    "छह",
    "सात",
    "आठ",
    "नौ",
    "दस",
    "ग्यारह",
    "बारह",
    "तेरह",
    "चौदह",
    "पंद्रह",
    "सोलह",
    "सत्रह",
    "अठारह",
    "उन्नीस",
    "बीस",
    "इकीस",
    "बाईस",
    "तेइस",
    "चौबीस",
    "पच्चीस",
    "छब्बीस",
    "सताइस",
    "अट्ठाइस",
    "उनतीस",
    "तीस",
    "इकतीस",
    "बतीस",
    "तैंतीस",
    "चौंतीस",
    "पैंतीस",
    "छतीस",
    "सैंतीस",
    "अड़तीस",
    "उनतालीस",
    "चालीस",
    "इकतालीस",
    "बयालीस",
    "तैतालीस",
    "चवालीस",
    "पैंतालीस",
    "छयालिस",
    "सैंतालीस",
    "अड़तालीस",
    "उनचास",
    "पचास",
    "इक्यावन",
    "बावन",
    "तिरपन",
    "चौवन",
    "पचपन",
    "छप्पन",
    "सतावन",
    "अठावन",
    "उनसठ",
    "साठ",
    "इकसठ",
    "बासठ",
    "तिरसठ",
    "चौंसठ",
    "पैंसठ",
    "छियासठ",
    "सड़सठ",
    "अड़सठ",
    "उनहतर",
    "सत्तर",
    "इकहतर",
    "बहतर",
    "तिहतर",
    "चौहतर",
    "पचहतर",
    "छिहतर",
    "सतहतर",
    "अठहतर",
    "उन्नासी",
    "अस्सी",
    "इक्यासी",
    "बयासी",
    "तिरासी",
    "चौरासी",
    "पचासी",
    "छियासी",
    "सतासी",
    "अट्ठासी",
    "नवासी",
    "नब्बे",
    "इक्यानवे",
    "बानवे",
    "तिरानवे",
    "चौरानवे",
    "पचानवे",
    "छियानवे",
    "सतानवे",
    "अट्ठानवे",
    "निन्यानवे",
];

/// `hindiCounting` — Hindi number words in Devanagari over the standard
/// Indian 2-2-3 grouping: करोड़ (10⁷), लाख (10⁵), हज़ार (10³), सौ (10²),
/// then the 1–99 table; zero groups are silent (2024 = दो हज़ार चौबीस).
///
/// The Word-proxy wraps the sequence modulo 9999, which no orthography or
/// Microsoft document corroborates; the wrap is not reproduced — **Word
/// reference render**.
pub fn hindi_counting(n: u32) -> String {
    if n == 0 {
        return "शून्य".to_string();
    }
    let mut words: Vec<String> = Vec::new();
    let mut push = |value: u32, scale: Option<&str>| {
        if value == 0 {
            return;
        }
        words.push(HINDI_WORDS[(value - 1) as usize].to_string());
        if let Some(s) = scale {
            words.push(s.to_string());
        }
    };
    push(n / 10_000_000, Some("करोड़"));
    push(n / 100_000 % 100, Some("लाख"));
    push(n / 1000 % 100, Some("हज़ार"));
    push(n / 100 % 10, Some("सौ"));
    push(n % 100, None);
    words.join(" ")
}

// ─────────────────────────────────────────────────────────────────────────────
// dollarText
// ─────────────────────────────────────────────────────────────────────────────

/// `dollarText` — the English check-writing text §17.18.59 mandates: the
/// `cardinalText` value with a fixed ` and 00/100` appended ("the latter text
/// is fixed because values in numbering sequences are integer-based" — the
/// spec's own note). Composes with the house English speller, so the words
/// are title-cased ("Twelve and 00/100") the way Word's cardinalText labels
/// are; the spec's lowercase example is the field-switch house style.
///
/// [MS-OI29500] §2.1.545(t) records that Word renders this *list label* as
/// plain decimal; see [`baht_text`] for the same trade-off and the one-line
/// change that matches Word instead.
pub fn dollar_text(n: u32) -> String {
    let mut out =
        spellout::cardinal(n, super::locale::Locale::English).unwrap_or_else(|| n.to_string());
    out.push_str(" and 00/100");
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The shared sample set every researched format was tabled against.
    const SAMPLES: [u32; 25] = [
        1, 2, 3, 4, 5, 10, 11, 12, 15, 20, 21, 42, 99, 100, 101, 105, 110, 111, 999, 1000, 1005,
        1234, 2024, 10000, 12345,
    ];

    #[track_caller]
    fn check(f: fn(u32) -> String, expected: [&str; 25]) {
        for (n, want) in SAMPLES.into_iter().zip(expected) {
            assert_eq!(f(n), want, "n = {n}");
        }
    }

    #[test]
    fn chinese_counting_reads_tens_then_goes_positional() {
        check(
            chinese_counting,
            [
                "一",
                "二",
                "三",
                "四",
                "五",
                "十",
                "十一",
                "十二",
                "十五",
                "二十",
                "二十一",
                "四十二",
                "九十九",
                "一○○",
                "一○一",
                "一○五",
                "一一○",
                "一一一",
                "九九九",
                "一○○○",
                "一○○五",
                "一二三四",
                "二○二四",
                "一○○○○",
                "一二三四五",
            ],
        );
    }

    #[test]
    fn chinese_counting_thousand_full_reading() {
        check(
            chinese_counting_thousand,
            [
                "一",
                "二",
                "三",
                "四",
                "五",
                "十",
                "十一",
                "十二",
                "十五",
                "二十",
                "二十一",
                "四十二",
                "九十九",
                "一百",
                "一百〇一",
                "一百〇五",
                "一百一十",
                "一百一十一",
                "九百九十九",
                "一千",
                "一千〇五",
                "一千二百三十四",
                "二千〇二十四",
                "一万",
                "一万二千三百四十五",
            ],
        );
    }

    /// [MS-OI29500] note e: the connector is omitted straight after 万.
    #[test]
    fn chinese_counting_thousand_omits_zero_after_myriad() {
        assert_eq!(chinese_counting_thousand(10_005), "一万五");
        assert_eq!(chinese_counting_thousand(10_000), "一万");
    }

    /// [MS-OI29500] note j: the label blanks from a million up.
    #[test]
    fn counting_thousand_formats_blank_at_a_million() {
        assert_eq!(chinese_counting_thousand(1_000_000), "");
        assert_eq!(taiwanese_counting_thousand(1_000_000), "");
        assert_eq!(chinese_legal_simplified(1_000_000), "");
        assert_eq!(ideograph_legal_traditional(1_000_000), "");
        assert_eq!(japanese_counting(1_000_000), "");
        assert_ne!(japanese_legal(1_000_000), "", "no documented cap");
    }

    #[test]
    fn chinese_legal_simplified_bankers_numerals() {
        check(
            chinese_legal_simplified,
            [
                "壹",
                "贰",
                "叁",
                "肆",
                "伍",
                "壹拾",
                "壹拾壹",
                "壹拾贰",
                "壹拾伍",
                "贰拾",
                "贰拾壹",
                "肆拾贰",
                "玖拾玖",
                "壹佰",
                "壹佰零壹",
                "壹佰零伍",
                "壹佰壹拾",
                "壹佰壹拾壹",
                "玖佰玖拾玖",
                "壹仟",
                "壹仟零伍",
                "壹仟贰佰叁拾肆",
                "贰仟零贰拾肆",
                "壹萬",
                "壹萬贰仟叁佰肆拾伍",
            ],
        );
    }

    #[test]
    fn taiwanese_counting_matches_chinese_counting() {
        for n in SAMPLES {
            assert_eq!(taiwanese_counting(n), chinese_counting(n), "n = {n}");
        }
    }

    #[test]
    fn taiwanese_counting_thousand_uses_traditional_zero_and_myriad() {
        check(
            taiwanese_counting_thousand,
            [
                "一",
                "二",
                "三",
                "四",
                "五",
                "十",
                "十一",
                "十二",
                "十五",
                "二十",
                "二十一",
                "四十二",
                "九十九",
                "一百",
                "一百零一",
                "一百零五",
                "一百一十",
                "一百一十一",
                "九百九十九",
                "一千",
                "一千零五",
                "一千二百三十四",
                "二千零二十四",
                "一萬",
                "一萬二千三百四十五",
            ],
        );
        // Unlike the simplified sibling, the connector follows 萬 — the
        // spec's own worked example.
        assert_eq!(taiwanese_counting_thousand(10_005), "一萬零五");
    }

    #[test]
    fn taiwanese_digital_is_positional_with_white_circle() {
        check(
            taiwanese_digital,
            [
                "一",
                "二",
                "三",
                "四",
                "五",
                "一○",
                "一一",
                "一二",
                "一五",
                "二○",
                "二一",
                "四二",
                "九九",
                "一○○",
                "一○一",
                "一○五",
                "一一○",
                "一一一",
                "九九九",
                "一○○○",
                "一○○五",
                "一二三四",
                "二○二四",
                "一○○○○",
                "一二三四五",
            ],
        );
    }

    #[test]
    fn ideograph_legal_traditional_bankers_numerals() {
        check(
            ideograph_legal_traditional,
            [
                "壹",
                "貳",
                "參",
                "肆",
                "伍",
                "壹拾",
                "壹拾壹",
                "壹拾貳",
                "壹拾伍",
                "貳拾",
                "貳拾壹",
                "肆拾貳",
                "玖拾玖",
                "壹佰",
                "壹佰零壹",
                "壹佰零伍",
                "壹佰壹拾",
                "壹佰壹拾壹",
                "玖佰玖拾玖",
                "壹仟",
                "壹仟零伍",
                "壹仟貳佰參拾肆",
                "貳仟零貳拾肆",
                "壹萬",
                "壹萬貳仟參佰肆拾伍",
            ],
        );
    }

    #[test]
    fn japanese_counting_elides_one_below_the_myriad() {
        check(
            japanese_counting,
            [
                "一",
                "二",
                "三",
                "四",
                "五",
                "十",
                "十一",
                "十二",
                "十五",
                "二十",
                "二十一",
                "四十二",
                "九十九",
                "百",
                "百一",
                "百五",
                "百十",
                "百十一",
                "九百九十九",
                "千",
                "千五",
                "千二百三十四",
                "二千二十四",
                "一万",
                "一万二千三百四十五",
            ],
        );
        // Elision continues after 万.
        assert_eq!(japanese_counting(11_111), "一万千百十一");
    }

    #[test]
    fn japanese_legal_daiji_always_writes_the_coefficient() {
        check(
            japanese_legal,
            [
                "壱",
                "弐",
                "参",
                "四",
                "伍",
                "壱拾",
                "壱拾壱",
                "壱拾弐",
                "壱拾伍",
                "弐拾",
                "弐拾壱",
                "四拾弐",
                "九拾九",
                "壱百",
                "壱百壱",
                "壱百伍",
                "壱百壱拾",
                "壱百壱拾壱",
                "九百九拾九",
                "壱阡",
                "壱阡伍",
                "壱阡弐百参拾四",
                "弐阡弐拾四",
                "壱萬",
                "壱萬弐阡参百四拾伍",
            ],
        );
    }

    #[test]
    fn japanese_digital_ten_thousand_blanks_at_its_cap() {
        check(
            japanese_digital_ten_thousand,
            [
                "一",
                "二",
                "三",
                "四",
                "五",
                "一〇",
                "一一",
                "一二",
                "一五",
                "二〇",
                "二一",
                "四二",
                "九九",
                "一〇〇",
                "一〇一",
                "一〇五",
                "一一〇",
                "一一一",
                "九九九",
                "一〇〇〇",
                "一〇〇五",
                "一二三四",
                "二〇二四",
                "",
                "",
            ],
        );
    }

    #[test]
    fn korean_counting_informal_sino_korean() {
        check(
            korean_counting,
            [
                "일",
                "이",
                "삼",
                "사",
                "오",
                "십",
                "십일",
                "십이",
                "십오",
                "이십",
                "이십일",
                "사십이",
                "구십구",
                "백",
                "백일",
                "백오",
                "백십",
                "백십일",
                "구백구십구",
                "천",
                "천오",
                "천이백삼십사",
                "이천이십사",
                "만",
                "만이천삼백사십오",
            ],
        );
    }

    #[test]
    fn korean_legal_native_below_hundred_then_counting() {
        check(
            korean_legal,
            [
                "하나",
                "둘",
                "셋",
                "넷",
                "다섯",
                "열",
                "열하나",
                "열둘",
                "열다섯",
                "스물",
                "스물하나",
                "마흔둘",
                "아흔아홉",
                "백",
                "백일",
                "백오",
                "백십",
                "백십일",
                "구백구십구",
                "천",
                "천오",
                "천이백삼십사",
                "이천이십사",
                "만",
                "만이천삼백사십오",
            ],
        );
    }

    #[test]
    fn korean_digital_positional_hangul() {
        check(
            korean_digital,
            [
                "일",
                "이",
                "삼",
                "사",
                "오",
                "일영",
                "일일",
                "일이",
                "일오",
                "이영",
                "이일",
                "사이",
                "구구",
                "일영영",
                "일영일",
                "일영오",
                "일일영",
                "일일일",
                "구구구",
                "일영영영",
                "일영영오",
                "일이삼사",
                "이영이사",
                "일영영영영",
                "일이삼사오",
            ],
        );
    }

    #[test]
    fn korean_digital2_positional_hanja_with_u96f6_zero() {
        check(
            korean_digital2,
            [
                "一",
                "二",
                "三",
                "四",
                "五",
                "一零",
                "一一",
                "一二",
                "一五",
                "二零",
                "二一",
                "四二",
                "九九",
                "一零零",
                "一零一",
                "一零五",
                "一一零",
                "一一一",
                "九九九",
                "一零零零",
                "一零零五",
                "一二三四",
                "二零二四",
                "一零零零零",
                "一二三四五",
            ],
        );
        // The zero is 零 U+96F6 (Word 2019-verified), not 〇 U+3007.
        assert!(korean_digital2(10).contains('\u{96F6}'));
    }

    #[test]
    fn vietnamese_counting_standard_orthography() {
        check(
            vietnamese_counting,
            [
                "một",
                "hai",
                "ba",
                "bốn",
                "năm",
                "mười",
                "mười một",
                "mười hai",
                "mười lăm",
                "hai mươi",
                "hai mươi mốt",
                "bốn mươi hai",
                "chín mươi chín",
                "một trăm",
                "một trăm lẻ một",
                "một trăm lẻ năm",
                "một trăm mười",
                "một trăm mười một",
                "chín trăm chín mươi chín",
                "một ngàn",
                "một ngàn lẻ năm",
                "một ngàn hai trăm ba mươi bốn",
                "hai ngàn hai mươi bốn",
                "mười ngàn",
                "mười hai ngàn ba trăm bốn mươi lăm",
            ],
        );
    }

    #[test]
    fn hindi_counting_irregular_decades_and_indian_grouping() {
        check(
            hindi_counting,
            [
                "एक",
                "दो",
                "तीन",
                "चार",
                "पांच",
                "दस",
                "ग्यारह",
                "बारह",
                "पंद्रह",
                "बीस",
                "इकीस",
                "बयालीस",
                "निन्यानवे",
                "एक सौ",
                "एक सौ एक",
                "एक सौ पांच",
                "एक सौ दस",
                "एक सौ ग्यारह",
                "नौ सौ निन्यानवे",
                "एक हज़ार",
                "एक हज़ार पांच",
                "एक हज़ार दो सौ चौंतीस",
                "दो हज़ार चौबीस",
                "दस हज़ार",
                "बारह हज़ार तीन सौ पैंतालीस",
            ],
        );
    }

    #[test]
    fn thai_counting_ed_and_yi_rules() {
        check(
            thai_counting,
            [
                "หนึ่ง",
                "สอง",
                "สาม",
                "สี่",
                "ห้า",
                "สิบ",
                "สิบเอ็ด",
                "สิบสอง",
                "สิบห้า",
                "ยี่สิบ",
                "ยี่สิบเอ็ด",
                "สี่สิบสอง",
                "เก้าสิบเก้า",
                "หนึ่งร้อย",
                "หนึ่งร้อยเอ็ด",
                "หนึ่งร้อยห้า",
                "หนึ่งร้อยสิบ",
                "หนึ่งร้อยสิบเอ็ด",
                "เก้าร้อยเก้าสิบเก้า",
                "หนึ่งพัน",
                "หนึ่งพันห้า",
                "หนึ่งพันสองร้อยสามสิบสี่",
                "สองพันยี่สิบสี่",
                "หนึ่งหมื่น",
                "หนึ่งหมื่นสองพันสามร้อยสี่สิบห้า",
            ],
        );
    }

    /// The BAHTTEXT engine's trailing-one rule needs a non-zero tens digit,
    /// so 101 differs from `thaiCounting`.
    #[test]
    fn baht_text_appends_the_currency_and_keeps_the_excel_ed_rule() {
        assert_eq!(baht_text(1), "หนึ่งบาทถ้วน");
        assert_eq!(baht_text(11), "สิบเอ็ดบาทถ้วน");
        assert_eq!(baht_text(12), "สิบสองบาทถ้วน");
        assert_eq!(baht_text(101), "หนึ่งร้อยหนึ่งบาทถ้วน");
        assert_eq!(thai_counting(101), "หนึ่งร้อยเอ็ด");
        assert_eq!(baht_text(0), "ศูนย์บาทถ้วน");
        assert_eq!(baht_text(12345), "หนึ่งหมื่นสองพันสามร้อยสี่สิบห้าบาทถ้วน");
    }

    #[test]
    fn dollar_text_composes_the_house_cardinal() {
        assert_eq!(dollar_text(1), "One and 00/100");
        assert_eq!(dollar_text(12), "Twelve and 00/100");
        assert_eq!(dollar_text(21), "Twenty-One and 00/100");
        assert_eq!(
            dollar_text(1234),
            "One Thousand Two Hundred Thirty-Four and 00/100"
        );
    }

    /// Zero labels (a list can start at 0): each system's own zero.
    #[test]
    fn zero_renders_each_systems_zero() {
        assert_eq!(chinese_counting(0), "○");
        assert_eq!(taiwanese_digital(0), "○");
        assert_eq!(japanese_digital_ten_thousand(0), "〇");
        assert_eq!(korean_digital(0), "영");
        assert_eq!(korean_digital2(0), "零");
        assert_eq!(chinese_legal_simplified(0), "零");
        assert_eq!(thai_counting(0), "ศูนย์");
        assert_eq!(vietnamese_counting(0), "không");
        assert_eq!(hindi_counting(0), "शून्य");
    }
}
