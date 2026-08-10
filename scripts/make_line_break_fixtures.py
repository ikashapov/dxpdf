#!/usr/bin/env python3
"""Build the UAX #14 line-breaking fixtures under test-files/ (issue #130).

Two documents in scripts that are written without spaces between words, which
is what made them the hard case:

  line-break-thai.docx   Thai (UAX #14 class SA — the line breaking algorithm
                         hands these scripts to "complex context analysis",
                         which is what the LSTM models in
                         src/i18n/data/icu_data.blob perform).
  line-break-cjk.docx    Japanese, written to put closing punctuation
                         (`。`, `」`) and opening punctuation (`「`) near the
                         line edges where rules LB13 and LB14 apply.

Both are exercised by tests/line_breaking.rs, and both are also useful by hand:

    cargo build --release
    ./target/release/dxpdf test-files/line-break-thai.docx -o output/thai.pdf

Each paragraph is a *single* `<w:r>` except the last, which is deliberately
split across several runs with no formatting difference between them — Word
does that routinely (spell-check state, revision marks, `rsid` churn), and a
line breaker that segments per run instead of per paragraph gets that
paragraph wrong while getting the others right.

Regenerate and commit the result if the text changes; the build is
deterministic. Requires no third-party packages.

    scripts/make_line_break_fixtures.py
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


def run(text, font, lang):
    return (
        "<w:r><w:rPr>"
        f'<w:rFonts w:ascii="{font}" w:hAnsi="{font}" w:eastAsia="{font}" w:cs="{font}"/>'
        '<w:sz w:val="24"/><w:szCs w:val="24"/>'
        f'<w:lang w:val="{lang}" w:eastAsia="{lang}" w:bidi="{lang}"/>'
        "</w:rPr>"
        f'<w:t xml:space="preserve">{text}</w:t></w:r>'
    )


def paragraph(chunks, font, lang):
    return "<w:p>" + "".join(run(c, font, lang) for c in chunks) + "</w:p>"


def document(paragraphs, font, lang):
    body = "".join(paragraph(chunks, font, lang) for chunks in paragraphs)
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


# "Thai is a language without spaces between words, so word segmentation needs
#  a dictionary or a model in order to start a new line."
THAI = (
    "ภาษาไทยเป็นภาษาที่ไม่มีการเว้นวรรคระหว่างคำ"
    "จึงต้องใช้พจนานุกรมหรือแบบจำลองในการตัดคำเพื่อขึ้นบรรทัดใหม่"
)

# Sentences ending in `。` and containing a `「…」` quotation, repeated so that
# the punctuation sweeps across every column position on the line.
JA = "日本語の文書における行分割の規則を検証する段落です。「引用」も含みます。"

build(
    "line-break-thai.docx",
    [[THAI * 3]] + [[("ก" * n) + THAI * 2] for n in range(1, 13)] + [[THAI, THAI, THAI]],
    "Thonburi",
    "th-TH",
)

build(
    "line-break-cjk.docx",
    [[JA * 3]] + [[("あ" * n) + JA * 2] for n in range(1, 13)] + [[JA, JA, JA]],
    "Hiragino Sans",
    "ja-JP",
)
