#!/usr/bin/env python3
"""Build the font fixtures in ``test-files/fonts/``.

Issue #85's last acceptance criterion is that *committed fixtures make face
selection equivalent across macOS, Linux and Windows*. Host fonts cannot do
that — every platform ships a different set, and the same family differs
between them — so the resolver's tests run against fonts committed to the
repository.

Those fonts are built here rather than taken from a font foundry, for three
reasons:

* **Independence.** The reader under test is ours; the writer is `fontTools`,
  which is a separate implementation of the same specification. A bug the two
  share would have to be a bug `fontTools` also has.
* **Precision.** Each fixture exists to exercise one thing — a localized name
  record, a family that ends in a style word, a two-face collection — and a
  shipping font carries all of that entangled with ten thousand glyphs.
* **Provenance.** Nothing here is derived from a third-party font, so the
  repository takes on no font licence at all.

The outputs are real SFNT files: the engine reads them through exactly the same
path as any font on the host, with no test-only parsing anywhere.

Usage::

    pip install fonttools
    python3 scripts/make_font_fixtures.py

Regenerate and commit the results whenever a fixture needs to change. The build
is deterministic — running it twice produces byte-identical files — so a diff
in ``git status`` means a real change.
"""

from __future__ import annotations

import os
from pathlib import Path

from fontTools.fontBuilder import FontBuilder
from fontTools.misc.timeTools import timestampSinceEpoch
from fontTools.pens.ttGlyphPen import TTGlyphPen
from fontTools.ttLib import TTFont, newTable
from fontTools.ttLib.tables.TupleVariation import TupleVariation
from fontTools.ttLib.ttCollection import TTCollection

OUT = Path(__file__).resolve().parent.parent / "test-files" / "fonts"

# A fixed timestamp keeps the build reproducible; `head` otherwise records
# "now" and every regeneration would show as a diff.
EPOCH = timestampSinceEpoch(0)

UPM = 1000
GLYPHS = [".notdef", "space", "A", "B", "a", "b"]
CMAP = {0x20: "space", 0x41: "A", 0x42: "B", 0x61: "a", 0x62: "b"}

# Windows platform (3), Unicode BMP encoding (1). English and two others, so
# the localized-name path is exercised by records a real producer would write.
EN_US, RU_RU, JA_JP = 0x0409, 0x0419, 0x0411


def _outlines() -> dict:
    """One rectangle per glyph — the resolver never looks at outlines, but a
    font with none is rejected by some rasterizers."""
    glyphs = {}
    for name in GLYPHS:
        pen = TTGlyphPen(None)
        if name not in (".notdef", "space"):
            pen.moveTo((50, 0))
            pen.lineTo((50, 700))
            pen.lineTo((450, 700))
            pen.lineTo((450, 0))
            pen.closePath()
        glyphs[name] = pen.glyph()
    return glyphs


def build(
    *,
    names: dict,
    weight: int = 400,
    width_class: int = 5,
    fs_selection: int = 0b1000000,  # REGULAR
    italic: bool = False,
) -> TTFont:
    """A minimal but complete static TrueType font carrying `names`."""
    fb = FontBuilder(UPM, isTTF=True)
    fb.setupGlyphOrder(GLYPHS)
    fb.setupCharacterMap(CMAP)
    fb.setupGlyf(_outlines())
    fb.setupHorizontalMetrics({name: (500, 50) for name in GLYPHS})
    fb.setupHorizontalHeader(ascent=800, descent=-200)
    fb.setupNameTable(names)
    fb.setupOS2(
        sTypoAscender=800,
        sTypoDescender=-200,
        usWeightClass=weight,
        usWidthClass=width_class,
        fsSelection=fs_selection | (0b1 if italic else 0),
        achVendID="DXPD",
    )
    fb.setupPost()
    fb.font["head"].created = EPOCH
    fb.font["head"].modified = EPOCH
    # `head.macStyle` and `OS/2.fsSelection` must agree, or fontTools warns and
    # some consumers disagree with each other about the face's style.
    mac_style = 0
    if fs_selection & 0b100000:  # BOLD
        mac_style |= 0b1
    if italic:
        mac_style |= 0b10
    fb.font["head"].macStyle = mac_style
    return fb.font


def add_localized(font: TTFont, records: list[tuple[int, int, str]]) -> None:
    """Add `(nameID, languageID, text)` records on the Windows platform.

    Localized names are why the `name` table is read at all rather than trusting
    the platform: a Russian or Japanese copy of Word writes the spelling *its*
    UI showed into ``w:rFonts``, and that spelling exists only here.
    """
    for name_id, language, text in records:
        font["name"].setName(text, name_id, 3, 1, language)


def write(font: TTFont, filename: str) -> None:
    path = OUT / filename
    font.save(path)
    print(f"  {filename:28} {path.stat().st_size:>7} bytes")


def main() -> None:
    OUT.mkdir(parents=True, exist_ok=True)
    print(f"writing fixtures to {OUT}")

    # ── Dx Sans: a family split across the legacy four-slot model ─────────
    #
    # The case the whole metadata model exists for. `SemiBold` cannot live in
    # the four legacy slots, so its *legacy* family (name ID 1) is
    # "Dx Sans SemiBold" while its *typographic* family (ID 16) is "Dx Sans" —
    # and a document may name it either way, or by its full or PostScript name.
    write(
        build(
            names={
                "familyName": "Dx Sans",
                "styleName": "Regular",
                "fullName": "Dx Sans Regular",
                "psName": "DxSans-Regular",
                "typographicFamily": "Dx Sans",
                "typographicSubfamily": "Regular",
                "version": "Version 1.000",
            }
        ),
        "DxSans-Regular.ttf",
    )

    write(
        build(
            names={
                "familyName": "Dx Sans",
                "styleName": "Bold",
                "fullName": "Dx Sans Bold",
                "psName": "DxSans-Bold",
                "typographicFamily": "Dx Sans",
                "typographicSubfamily": "Bold",
                "version": "Version 1.000",
            },
            weight=700,
            fs_selection=0b100000,  # BOLD
        ),
        "DxSans-Bold.ttf",
    )

    semibold = build(
        names={
            "familyName": "Dx Sans SemiBold",
            "styleName": "Regular",
            "uniqueFontIdentifier": "1.000;DXPD;DxSans-SemiBold",
            "fullName": "Dx Sans SemiBold",
            "psName": "DxSans-SemiBold",
            "compatibleFullName": "Dx Sans SemiBold",
            "typographicFamily": "Dx Sans",
            "typographicSubfamily": "SemiBold",
            "wwsFamilyName": "Dx Sans",
            "wwsSubfamilyName": "SemiBold",
            "version": "Version 1.000",
        },
        weight=600,
    )
    add_localized(
        semibold,
        [
            (1, RU_RU, "Дх Санс СемиБолд"),
            (4, RU_RU, "Дх Санс СемиБолд"),
            (1, JA_JP, "Dx サンズ セミボールド"),
            (4, JA_JP, "Dx サンズ セミボールド"),
        ],
    )
    write(semibold, "DxSans-SemiBold.ttf")

    # ── Dx Medium: a family whose *name* ends in a style word ─────────────
    #
    # "Exact family names win even when they end in style-like words such as
    # Medium, Display, or Narrow." Nothing about this font is medium-weight; the
    # word is part of its name, and the old resolver would have chopped it off
    # and gone looking for a family called "Dx".
    write(
        build(
            names={
                "familyName": "Dx Medium",
                "styleName": "Regular",
                "fullName": "Dx Medium Regular",
                "psName": "DxMedium-Regular",
                "version": "Version 1.000",
            }
        ),
        "DxMedium-Regular.ttf",
    )

    # A family the parsing step *would* reach if it ever ran first, so a test
    # can prove that "Dx Medium" did not resolve through it.
    write(
        build(
            names={
                "familyName": "Dx",
                "styleName": "Regular",
                "fullName": "Dx Regular",
                "psName": "Dx-Regular",
                "version": "Version 1.000",
            }
        ),
        "Dx-Regular.ttf",
    )

    # ── A two-face collection ────────────────────────────────────────────
    #
    # OOXML has no way to say which face of a collection a `w:font` means, so
    # the index is discovered by reading the bytes. Two faces with different
    # names is the minimum that makes "which one?" a real question.
    collection = TTCollection()
    collection.fonts = [
        build(
            names={
                "familyName": "Dx Collection One",
                "styleName": "Regular",
                "fullName": "Dx Collection One Regular",
                "psName": "DxCollectionOne-Regular",
                "version": "Version 1.000",
            }
        ),
        build(
            names={
                "familyName": "Dx Collection Two",
                "styleName": "Bold",
                "fullName": "Dx Collection Two Bold",
                "psName": "DxCollectionTwo-Bold",
                "version": "Version 1.000",
            },
            weight=700,
            fs_selection=0b100000,
        ),
    ]
    path = OUT / "DxCollection.ttc"
    collection.save(path)
    print(f"  {'DxCollection.ttc':28} {path.stat().st_size:>7} bytes")

    # ── A variable font ──────────────────────────────────────────────────
    #
    # Two axes and four named instances, plus the `STAT` records that give each
    # axis value a word. That pair is what lets "Dx Variable Condensed SemiBold"
    # be *read* rather than parsed.
    write(build_variable(), "DxVariable.ttf")

    print("done — commit the results")


def build_variable() -> TTFont:
    font = build(
        names={
            "familyName": "Dx Variable",
            "styleName": "Regular",
            "fullName": "Dx Variable Regular",
            "psName": "DxVariable-Regular",
            "typographicFamily": "Dx Variable",
            "typographicSubfamily": "Regular",
            "version": "Version 1.000",
        }
    )
    name = font["name"]

    def add(text: str) -> int:
        """Append a name record and return its id, starting at 256 as the spec
        reserves everything below for the standard names."""
        return name.addName(text, platforms=((3, 1, EN_US),), minNameID=255)

    wght_name = add("Weight")
    wdth_name = add("Width")

    fvar = newTable("fvar")
    from fontTools.ttLib.tables._f_v_a_r import Axis, NamedInstance

    wght = Axis()
    wght.axisTag, wght.minValue, wght.defaultValue, wght.maxValue = "wght", 100, 400, 900
    wght.axisNameID = wght_name
    wdth = Axis()
    wdth.axisTag, wdth.minValue, wdth.defaultValue, wdth.maxValue = "wdth", 75, 100, 125
    wdth.axisNameID = wdth_name
    fvar.axes = [wght, wdth]

    fvar.instances = []
    for subfamily, coords in [
        ("Regular", {"wght": 400, "wdth": 100}),
        ("SemiBold", {"wght": 600, "wdth": 100}),
        ("Bold", {"wght": 700, "wdth": 100}),
        ("Condensed SemiBold", {"wght": 600, "wdth": 75}),
    ]:
        instance = NamedInstance()
        instance.subfamilyNameID = add(subfamily)
        instance.postscriptNameID = add(f"DxVariable-{subfamily.replace(' ', '')}")
        instance.coordinates = coords
        fvar.instances.append(instance)
    font["fvar"] = fvar

    # `gvar`: issue #113 (baking an instance's coordinates into embedded PDF
    # bytes) needs instances that actually draw different outlines — a
    # variable font with a design space but no deltas, which is all the
    # *catalogue* reads, was sufficient before that but isn't anymore.
    #
    # Every real point of every non-empty glyph (`A`, `B`, `a`, `b`, all four
    # corners of the same rectangle — see `_outlines`) gets an explicit delta
    # in both tuples, rather than leaving some unset and relying on `gvar`'s
    # IUP inference to fill them in: the glyph shape stays a rectangle (only
    # taller or narrower), and every consumer of this fixture reads deltas it
    # wrote explicitly, not ones a reader-side IUP implementation invented.
    # `wght` stretches the top edge upward from a pinned bottom edge; `wdth`
    # pulls the right edge inward from a pinned left edge — independent axes,
    # independently checkable. Phantom points (indices 4-7, appended after
    # each glyph's real points) are left with no delta: this fixture is about
    # outlines, not advance widths.
    gvar = newTable("gvar")
    gvar.version = 1
    gvar.reserved = 0
    wght_tv = TupleVariation(
        {"wght": (0.0, 1.0, 1.0)},
        [(0, 0), (0, 100), (0, 100), (0, 0), None, None, None, None],
    )
    wdth_tv = TupleVariation(
        {"wdth": (-1.0, -1.0, 0.0)},
        [(0, 0), (0, 0), (-50, 0), (-50, 0), None, None, None, None],
    )
    gvar.variations = {g: [wght_tv, wdth_tv] for g in ("A", "B", "a", "b")}
    font["gvar"] = gvar

    # `MVAR`: baking (issue #113) doesn't just rebuild `glyf`/`hmtx` for the
    # pinned location — it also applies MVAR deltas to the metric tables that
    # carry them (`hhea`, `post`, `OS/2`), rather than embedding the default
    # location's stale values into a font that no longer varies. That needs
    # deltas that actually differ from zero, hence a real MVAR table, built
    # through the same `OnlineVarStoreBuilder`/`VariationModel` pair `varLib`
    # itself uses to compile MVAR when merging masters — an item variation
    # store is a different structure from `gvar`'s tuple variations above, so
    # it isn't buildable by hand the same way.
    #
    # `hasc`/`hdsc` (hhea ascender/descender) and `undo`/`unds` (post
    # underline offset/thickness) are enough to exercise what baking patches
    # without also touching every OS/2 field the same mechanism covers. A
    # single tent from the default (`{}`) to `wght`'s maximum (normalized
    # `1.0`) reuses the shape of the `gvar` tuples above, so `Regular` (400,
    # the default) again reads as an exact match to the unbaked font's
    # values, and `SemiBold`/`Bold` (600/700) fall at different fractions of
    # the tent and so must differ from each other too.
    from fontTools.ttLib.tables import otTables as ot
    from fontTools.varLib import varStore
    from fontTools.varLib.models import VariationModel

    axis_tags = [axis.axisTag for axis in fvar.axes]
    model = VariationModel([{}, {"wght": 1.0}], axisOrder=axis_tags)
    store_builder = varStore.OnlineVarStoreBuilder(axis_tags)
    store_builder.setModel(model)

    mvar_records = []

    def mvar_entry(tag: str, default_value: int, peak_value: int) -> int:
        """Store one MVAR value record; return the default-location value,
        which must equal what the corresponding table field already carries
        — MVAR only ever contributes a delta on top of it."""
        base, var_idx = store_builder.storeMasters([default_value, peak_value])
        record = ot.MetricsValueRecord()
        record.ValueTag = tag
        record.VarIdx = var_idx
        mvar_records.append(record)
        return base

    font["hhea"].ascender = mvar_entry("hasc", font["hhea"].ascender, font["hhea"].ascender + 100)
    font["hhea"].descender = mvar_entry("hdsc", font["hhea"].descender, font["hhea"].descender - 40)
    font["post"].underlinePosition = mvar_entry(
        "undo", font["post"].underlinePosition, font["post"].underlinePosition - 20
    )
    font["post"].underlineThickness = mvar_entry(
        "unds", font["post"].underlineThickness, font["post"].underlineThickness + 40
    )

    mvar = newTable("MVAR")
    mvar.table = ot.MVAR()
    mvar.table.Version = 0x00010000
    mvar.table.Reserved = 0
    mvar.table.ValueRecordSize = 8
    mvar_records.sort(key=lambda r: r.ValueTag)
    mvar.table.ValueRecord = mvar_records
    mvar.table.ValueRecordCount = len(mvar_records)
    mvar.table.VarStore = store_builder.finish()
    font["MVAR"] = mvar

    stat = newTable("STAT")
    from fontTools.otlLib.builder import buildStatTable

    buildStatTable(
        font,
        axes=[
            {
                "tag": "wdth",
                "name": "Width",
                "ordering": 0,
                "values": [
                    {"value": 75, "name": "Condensed"},
                    {"value": 100, "name": "Normal", "flags": 0x2},  # ELIDABLE
                ],
            },
            {
                "tag": "wght",
                "name": "Weight",
                "ordering": 1,
                "values": [
                    {"value": 400, "name": "Regular", "flags": 0x2, "linkedValue": 700},
                    {"value": 600, "name": "SemiBold"},
                    {"value": 700, "name": "Bold"},
                ],
            },
        ],
        elidedFallbackName="Regular",
    )
    _ = stat
    font["head"].created = EPOCH
    font["head"].modified = EPOCH
    return font


if __name__ == "__main__":
    os.environ.setdefault("SOURCE_DATE_EPOCH", "0")
    main()
