#!/usr/bin/env python3
"""Build the issue #165 probe fixtures — the documents that ask Word the three
questions ECMA-376 leaves open.

Each is authored so the answer is a *measurement off the rendered page*, and so
that every candidate reading predicts a visibly different number. The
predictions are recorded in `plans/issue-165-word-reference-renders.md` and in
the comment above each builder below; they are written before rendering on
purpose, because with a PDF in hand it is easy to decide after the fact which
reading the output "obviously" supports.

    python3 scripts/make_issue165_fixtures.py

Deterministic: fixed ZIP dates, no timestamps, hand-built PNG. Re-running
produces byte-identical archives. Regenerate rather than hand-edit.
"""

import pathlib
import struct
import zlib
import zipfile

OUT = pathlib.Path(__file__).resolve().parent.parent / "test-files"

W = 'xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"'
NS_R = "http://schemas.openxmlformats.org/officeDocument/2006/relationships"

# ── package plumbing ─────────────────────────────────────────────────────────


def content_types(*, settings=False, image=False):
    extra = ""
    if settings:
        extra += (
            '<Override PartName="/word/settings.xml" ContentType="application/'
            'vnd.openxmlformats-officedocument.wordprocessingml.settings+xml"/>'
        )
    if image:
        extra += '<Default Extension="png" ContentType="image/png"/>'
    return (
        '<?xml version="1.0" encoding="UTF-8" standalone="yes"?>\n'
        '<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">'
        '<Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>'
        '<Default Extension="xml" ContentType="application/xml"/>'
        f"{extra}"
        '<Override PartName="/word/document.xml" ContentType="application/'
        'vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/>'
        "</Types>"
    )


ROOT_RELS = (
    '<?xml version="1.0" encoding="UTF-8" standalone="yes"?>\n'
    f'<Relationships xmlns="{NS_R}">'
    f'<Relationship Id="rId1" Type="{NS_R}/officeDocument" Target="word/document.xml"/>'
    "</Relationships>"
)


def doc_rels(*, settings=False, image=False):
    rels = ""
    if settings:
        rels += f'<Relationship Id="rIdS" Type="{NS_R}/settings" Target="settings.xml"/>'
    if image:
        rels += f'<Relationship Id="rIdI" Type="{NS_R}/image" Target="media/dot.png"/>'
    return (
        '<?xml version="1.0" encoding="UTF-8" standalone="yes"?>\n'
        f'<Relationships xmlns="{NS_R}">{rels}</Relationships>'
    )


def solid_png(width, height, rgb):
    """A minimal solid-colour PNG, built from stdlib so the fixture is
    reproducible without Pillow."""

    def chunk(tag, data):
        c = tag + data
        return struct.pack(">I", len(data)) + c + struct.pack(">I", zlib.crc32(c))

    ihdr = struct.pack(">IIBBBBB", width, height, 8, 2, 0, 0, 0)
    raw = b"".join(b"\x00" + bytes(rgb) * width for _ in range(height))
    return (
        b"\x89PNG\r\n\x1a\n"
        + chunk(b"IHDR", ihdr)
        + chunk(b"IDAT", zlib.compress(raw, 9))
        + chunk(b"IEND", b"")
    )


def write(name, parts):
    path = OUT / name
    with zipfile.ZipFile(path, "w", zipfile.ZIP_DEFLATED) as zf:
        for member, body in parts:
            info = zipfile.ZipInfo(member, date_time=(1980, 1, 1, 0, 0, 0))
            info.compress_type = zipfile.ZIP_DEFLATED
            info.external_attr = 0o600 << 16
            zf.writestr(info, body)
    print(f"wrote {path.name} ({path.stat().st_size} bytes)")


SECT_LETTER = (
    "<w:sectPr><w:pgSz w:w=\"12240\" w:h=\"15840\"/>"
    '<w:pgMar w:top="1440" w:right="1440" w:bottom="1440" w:left="1440" '
    'w:header="720" w:footer="720" w:gutter="0"/></w:sectPr>'
)

SIDES = ("top", "left", "bottom", "right", "insideH", "insideV")
TBL_BORDERS = "<w:tblBorders>" + "".join(
    f'<w:{s} w:val="single" w:sz="8" w:space="0" w:color="000000"/>' for s in SIDES
) + "</w:tblBorders>"
TC_BORDERS = "<w:tcBorders>" + "".join(
    f'<w:{s} w:val="single" w:sz="8" w:space="0" w:color="000000"/>'
    for s in ("top", "left", "bottom", "right")
) + "</w:tcBorders>"


# ── A. vMerge overflow distribution ──────────────────────────────────────────
#
# Column 1 is a restart+continue pair holding ten lines; column 2 holds one
# short line per row, so the row boundary is a visible rule whose y can be
# measured. NO w:trHeight anywhere — the question is what the auto sizer does,
# and an authored height would answer a different one.
#
#   even distribution (dxpdf today) → boundary at H/2
#   last row absorbs                → boundary at h, near the top
#   restart row absorbs             → boundary at H-h, near the bottom
def build_vmerge():
    tall = "".join(
        f"<w:p><w:r><w:t>merged line {i:02d}</w:t></w:r></w:p>" for i in range(1, 11)
    )
    doc = f"""<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:document {W}><w:body>
<w:p><w:r><w:t>A: vMerge overflow distribution</w:t></w:r></w:p>
<w:tbl>
<w:tblPr><w:tblW w:w="8640" w:type="dxa"/>{TBL_BORDERS}<w:tblLayout w:type="fixed"/></w:tblPr>
<w:tblGrid><w:gridCol w:w="4320"/><w:gridCol w:w="4320"/></w:tblGrid>
<w:tr>
  <w:tc><w:tcPr><w:tcW w:w="4320" w:type="dxa"/><w:vMerge w:val="restart"/>{TC_BORDERS}</w:tcPr>{tall}</w:tc>
  <w:tc><w:tcPr><w:tcW w:w="4320" w:type="dxa"/>{TC_BORDERS}</w:tcPr><w:p><w:r><w:t>R1</w:t></w:r></w:p></w:tc>
</w:tr>
<w:tr>
  <w:tc><w:tcPr><w:tcW w:w="4320" w:type="dxa"/><w:vMerge/>{TC_BORDERS}</w:tcPr><w:p/></w:tc>
  <w:tc><w:tcPr><w:tcW w:w="4320" w:type="dxa"/>{TC_BORDERS}</w:tcPr><w:p><w:r><w:t>R2</w:t></w:r></w:p></w:tc>
</w:tr>
</w:tbl>
<w:p/>{SECT_LETTER}</w:body></w:document>"""
    write(
        "issue-165-vmerge.docx",
        [
            ("[Content_Types].xml", content_types()),
            ("_rels/.rels", ROOT_RELS),
            ("word/document.xml", doc),
        ],
    )


# ── B. tblCellSpacing at the table's own edges ───────────────────────────────
#
# 20pt spacing, every border drawn (per [MS-OI29500] §17.4.66 a non-zero
# spacing means all borders display, which is what makes the gaps measurable).
#
#   one full spacing everywhere (dxpdf today) → edge 20pt, inner 20pt
#   half at the edges                         → edge 10pt, inner 20pt
def build_cellspacing():
    cells = "".join(
        f'<w:tc><w:tcPr><w:tcW w:w="2400" w:type="dxa"/>{TC_BORDERS}</w:tcPr>'
        f"<w:p><w:r><w:t>C{i}</w:t></w:r></w:p></w:tc>"
        for i in (1, 2, 3)
    )
    doc = f"""<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:document {W}><w:body>
<w:p><w:r><w:t>B: tblCellSpacing at the table edges</w:t></w:r></w:p>
<w:tbl>
<w:tblPr><w:tblW w:w="7200" w:type="dxa"/>
<w:tblCellSpacing w:w="400" w:type="dxa"/>
{TBL_BORDERS}<w:tblLayout w:type="fixed"/></w:tblPr>
<w:tblGrid><w:gridCol w:w="2400"/><w:gridCol w:w="2400"/><w:gridCol w:w="2400"/></w:tblGrid>
<w:tr><w:trPr><w:tblCellSpacing w:w="400" w:type="dxa"/></w:trPr>{cells}</w:tr>
</w:tbl>
<w:p/>{SECT_LETTER}</w:body></w:document>"""
    write(
        "issue-165-cellspacing.docx",
        [
            ("[Content_Types].xml", content_types()),
            ("_rels/.rels", ROOT_RELS),
            ("word/document.xml", doc),
        ],
    )


# ── C. vertical inside/outside for floats ────────────────────────────────────
#
# Mirrored margins with ASYMMETRIC top and bottom (1in / 2in) so a vertical
# mirror is visible at all, and one anchor per page so each y is unambiguous.
# Six pages: {margin+align, insideMargin+offset, margin+outside} x {odd, even}.
#
#   aligns to region top (dxpdf today) → identical y on odd and even
#   mirrors vertically                 → y differs between odd and even
#
# That w:mirrorMargins mirrors left and right, not top and bottom, is the
# question — not an objection to the probe.
def build_floatv():
    def anchor(idx, rel_from, body):
        return f"""<w:r><w:drawing>
<wp:anchor xmlns:wp="http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing"
 distT="0" distB="0" distL="0" distR="0" simplePos="0" relativeHeight="{idx}"
 behindDoc="0" locked="0" layoutInCell="1" allowOverlap="1">
<wp:simplePos x="0" y="0"/>
<wp:positionH relativeFrom="margin"><wp:posOffset>0</wp:posOffset></wp:positionH>
<wp:positionV relativeFrom="{rel_from}">{body}</wp:positionV>
<wp:extent cx="457200" cy="457200"/><wp:effectExtent l="0" t="0" r="0" b="0"/>
<wp:wrapNone/><wp:docPr id="{idx}" name="Probe{idx}"/>
<a:graphic xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main">
<a:graphicData uri="http://schemas.openxmlformats.org/drawingml/2006/picture">
<pic:pic xmlns:pic="http://schemas.openxmlformats.org/drawingml/2006/picture">
<pic:nvPicPr><pic:cNvPr id="{idx}" name="dot.png"/><pic:cNvPicPr/></pic:nvPicPr>
<pic:blipFill><a:blip r:embed="rIdI" xmlns:r="{NS_R}"/><a:stretch><a:fillRect/></a:stretch></pic:blipFill>
<pic:spPr><a:xfrm><a:off x="0" y="0"/><a:ext cx="457200" cy="457200"/></a:xfrm>
<a:prstGeom prst="rect"><a:avLst/></a:prstGeom></pic:spPr>
</pic:pic></a:graphicData></a:graphic></wp:anchor></w:drawing></w:r>"""

    cases = [
        ("margin", "<wp:align>inside</wp:align>", "margin+align=inside"),
        ("insideMargin", "<wp:posOffset>0</wp:posOffset>", "insideMargin+offset=0"),
        ("margin", "<wp:align>outside</wp:align>", "margin+align=outside"),
    ]
    pages = []
    idx = 1
    for rel_from, body, label in cases:
        for parity in ("odd", "even"):
            pages.append(
                f"<w:p><w:r><w:t>{label} / {parity}</w:t></w:r>"
                f"{anchor(idx, rel_from, body)}</w:p>"
            )
            idx += 1
    # A page break between pages, but not after the last one.
    brk = '<w:p><w:r><w:br w:type="page"/></w:r></w:p>'
    body = brk.join(pages)

    settings = f"""<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:settings {W}><w:mirrorMargins/></w:settings>"""

    # Asymmetric top/bottom is what makes a vertical mirror detectable.
    sect = (
        '<w:sectPr><w:pgSz w:w="12240" w:h="15840"/>'
        '<w:pgMar w:top="1440" w:right="1440" w:bottom="2880" w:left="1440" '
        'w:header="720" w:footer="720" w:gutter="0"/></w:sectPr>'
    )
    doc = f"""<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:document {W}><w:body>{body}{sect}</w:body></w:document>"""

    write(
        "issue-165-floatv.docx",
        [
            ("[Content_Types].xml", content_types(settings=True, image=True)),
            ("_rels/.rels", ROOT_RELS),
            ("word/_rels/document.xml.rels", doc_rels(settings=True, image=True)),
            ("word/settings.xml", settings),
            ("word/media/dot.png", solid_png(64, 64, (200, 30, 30))),
            ("word/document.xml", doc),
        ],
    )


if __name__ == "__main__":
    build_vmerge()
    build_cellspacing()
    build_floatv()
