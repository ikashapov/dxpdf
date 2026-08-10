#!/usr/bin/env python3
"""Build the UAX #9 bidi fixtures under test-files/ (issue #131).

Two documents in right-to-left scripts, written so that every case the
reordering has to get right is present and separable:

  bidi-hebrew.docx   Hebrew — the script #131 completes on its own, because
                     Hebrew letters have no positional forms and so need no
                     shaper. What must be right here is *order*, and nothing
                     else.
  bidi-arabic.docx   Arabic — the same reordering, plus the joining that
                     `render::shape` adds. Also carries the Western-digit case,
                     which is where an embedding level of 2 shows up.

Both are exercised by tests/bidi.rs. Both are also useful by hand:

    cargo build --release
    ./target/release/dxpdf test-files/bidi-hebrew.docx -o output/hebrew.pdf

Each document repeats its text three ways, and the repetition is the point:

  * once as a single `<w:r>`,
  * once split across several identically-formatted runs — Word splits runs for
    reasons that have nothing to do with language (spell-check state, revision
    ids), and levels resolved per run instead of per paragraph get a different
    answer for two documents that read identically,
  * once with `<w:rtl/>` set explicitly on the runs (§17.3.2.30).

Regenerate and commit the result if the text changes; the build is
deterministic. Requires no third-party packages.

    scripts/make_bidi_fixtures.py
"""

import pathlib
import zipfile

ROOT = pathlib.Path(__file__).resolve().parent.parent
OUT = ROOT / "test-files"

W = "http://schemas.openxmlformats.org/wordprocessingml/2006/main"

CONTENT_TYPES = """<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">
  <Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>
  <Default Extension="xml" ContentType="application/xml"/>
  <Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/>
</Types>
"""

RELS = """<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
  <Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/>
</Relationships>
"""

DOC_RELS = """<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"/>
"""


def run(text, font, lang, rtl):
    return (
        "<w:r><w:rPr>"
        f'<w:rFonts w:ascii="{font}" w:hAnsi="{font}" w:cs="{font}"/>'
        '<w:sz w:val="24"/><w:szCs w:val="24"/>'
        + ("<w:rtl/>" if rtl else "")
        + f'<w:lang w:val="en-US" w:bidi="{lang}"/>'
        "</w:rPr>"
        f'<w:t xml:space="preserve">{text}</w:t></w:r>'
    )


def paragraph(chunks, font, lang, rtl, bidi):
    # §17.3.1.6: `w:bidi` states the paragraph's base direction. No `w:jc`
    # anywhere in these fixtures, deliberately — absent alignment is
    # `Alignment::Start`, so what the layout does with it is the
    # right-alignment decision `line_emit::align_offset` records.
    props = "<w:pPr><w:bidi/></w:pPr>" if bidi else ""
    return (
        "<w:p>" + props + "".join(run(c, font, lang, rtl) for c in chunks) + "</w:p>"
    )


def document(paragraphs, font, lang):
    body = "".join(
        paragraph(chunks, font, lang, rtl, bidi) for chunks, rtl, bidi in paragraphs
    )
    return (
        '<?xml version="1.0" encoding="UTF-8" standalone="yes"?>'
        f'<w:document xmlns:w="{W}"><w:body>{body}'
        '<w:sectPr><w:pgSz w:w="11906" w:h="16838"/>'
        '<w:pgMar w:top="1134" w:right="1134" w:bottom="1134" w:left="1134"'
        ' w:header="709" w:footer="709" w:gutter="0"/></w:sectPr>'
        "</w:body></w:document>"
    )


def build(name, paragraphs, font, lang):
    path = OUT / name
    with zipfile.ZipFile(path, "w", zipfile.ZIP_DEFLATED) as z:
        # Fixed timestamps so regenerating an unchanged fixture is a no-op diff.
        for entry, data in (
            ("[Content_Types].xml", CONTENT_TYPES),
            ("_rels/.rels", RELS),
            ("word/_rels/document.xml.rels", DOC_RELS),
            ("word/document.xml", document(paragraphs, font, lang)),
        ):
            info = zipfile.ZipInfo(entry, date_time=(1980, 1, 1, 0, 0, 0))
            info.compress_type = zipfile.ZIP_DEFLATED
            z.writestr(info, data)
    print(f"{path.relative_to(ROOT)}  {path.stat().st_size} bytes")


# "Hello world, this is a test of Hebrew text." — plain, strongly right-to-left,
# nothing weak or neutral in the middle.
HE = "שלום עולם זהו מבחן של טקסט בעברית"

# The same with a Latin phrase inside it: the level-1/level-2 case, where the
# embedded run must move as a block while keeping its own words in order.
HE_MIXED = "שלום עולם the quick brown fox זהו מבחן"

# A mirrored pair (rule L4) around Hebrew, plus one around Latin so the two can
# be told apart in the output.
HE_BRACKETS = "שלום (עולם) זהו (test) מבחן"

# "Hello world, this is a test of Arabic text."
AR = "مرحبا بالعالم هذا اختبار للنص العربي"

# Western digits inside Arabic — rule I1 puts them at level 2, so the number
# keeps its own left-to-right order while the words around it reverse.
AR_DIGITS = "الصفحة 12 من 345 في هذا المستند"

build(
    "bidi-hebrew.docx",
    [
        ([HE], False, True),
        (["שלום ", "עולם ", "זהו מבחן ", "של טקסט בעברית"], False, True),
        ([HE], True, True),
        ([HE_MIXED], False, True),
        ([HE_MIXED], False, False),
        ([HE_BRACKETS], False, True),
        # A right-to-left phrase inside a left-to-right paragraph: the mirror
        # image of the case above, and the one most real documents actually
        # contain.
        (["Quoted: ", "שלום עולם", " — end."], False, False),
    ],
    "Arial",
    "he-IL",
)

build(
    "bidi-arabic.docx",
    [
        ([AR], False, True),
        (["مرحبا ", "بالعالم ", "هذا اختبار ", "للنص العربي"], False, True),
        ([AR], True, True),
        ([AR_DIGITS], False, True),
        ([AR_DIGITS], False, False),
        (["Quoted: ", AR, " — end."], False, False),
    ],
    "Arial",
    "ar-SA",
)
