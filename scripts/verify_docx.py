#!/usr/bin/env python3
"""Check that a .docx is a well-formed OPC package — the things Word refuses to
open over, which this engine's own parser is far too tolerant to notice.

    python3 scripts/verify_docx.py test-files/*.docx

Written after three hand-built fixtures (`test-files/issue-165-*.docx`) were
rejected by Word with "unreadable content" while dxpdf parsed them happily,
`textutil` read them, and nothing in the test suite complained. The cause was a
single wrong namespace on the `.rels` parts — `.../officeDocument/2006/
relationships`, which is the URI relationship *types* are built from, where a
relationships *part* must be `.../package/2006/relationships`. Word could not
resolve the officeDocument relationship, so it could not find `document.xml`,
so the whole file was unreadable. Nothing else in this repo would have caught
it.

The checks are deliberately about the **package**, not about WordprocessingML:
schema-validating `document.xml` needs the ECMA XSDs, which are not vendored
here. These are the structural invariants that can be checked from the file
alone, and they are the ones a hand-built fixture gets wrong.

Exit status is 0 when every file passes, 1 otherwise.
"""

import posixpath
import re
import sys
import xml.etree.ElementTree as ET
import zipfile

CT = "{http://schemas.openxmlformats.org/package/2006/content-types}"
PKG_REL = "{http://schemas.openxmlformats.org/package/2006/relationships}"
OFFICE_REL = "http://schemas.openxmlformats.org/officeDocument/2006/relationships"


def check(path):
    """Return a list of problems; empty means the package is sound."""
    problems = []
    try:
        z = zipfile.ZipFile(path)
    except zipfile.BadZipFile as e:
        return [f"not a zip archive: {e}"]

    names = set(z.namelist())

    if "[Content_Types].xml" not in names:
        return ["no [Content_Types].xml — not an OPC package at all"]

    # Every XML part has to parse before anything else can be said about it.
    for n in sorted(names):
        if n.endswith((".xml", ".rels")):
            try:
                ET.fromstring(z.read(n))
            except ET.ParseError as e:
                problems.append(f"{n}: not well-formed XML ({e})")
    if problems:
        return problems

    ct = ET.fromstring(z.read("[Content_Types].xml"))
    defaults = {d.get("Extension", "").lower() for d in ct.findall(CT + "Default")}
    overrides = {
        o.get("PartName", "").lstrip("/") for o in ct.findall(CT + "Override")
    }

    for o in sorted(overrides):
        if o not in names:
            problems.append(f"[Content_Types].xml: Override for missing part /{o}")

    for n in sorted(names):
        if n == "[Content_Types].xml" or n.endswith("/"):
            continue
        ext = n.rsplit(".", 1)[-1].lower() if "." in n else ""
        if n not in overrides and ext not in defaults:
            problems.append(f"{n}: no content type (no Default for .{ext}, no Override)")

    # Relationship parts: right namespace, and every target resolves.
    for n in sorted(names):
        if not n.endswith(".rels"):
            continue
        root = ET.fromstring(z.read(n))
        rels = root.findall(PKG_REL + "Relationship")
        if not rels:
            # The failure this script exists for. Distinguish "empty" from
            # "wrong namespace", because they look identical from a distance
            # and only one of them breaks the file.
            if len(list(root)) > 0:
                problems.append(
                    f"{n}: has children but no Relationship in the package "
                    f"namespace — wrong xmlns? found {root.tag!r}, child "
                    f"{list(root)[0].tag!r}"
                )
            continue
        base = posixpath.dirname(posixpath.dirname(n))
        for r in rels:
            if r.get("TargetMode") == "External":
                continue
            target = posixpath.normpath(posixpath.join(base, r.get("Target", "")))
            if target not in names:
                problems.append(f"{n}: relationship target {target} does not exist")

    # A relationship id used in a part must be declared by that part's rels.
    for n in sorted(names):
        if not n.endswith(".xml") or n.startswith("docProps/"):
            continue
        rels_path = posixpath.join(
            posixpath.dirname(n), "_rels", posixpath.basename(n) + ".rels"
        )
        used = set(re.findall(rb'r:(?:embed|id|link)="([^"]+)"', z.read(n)))
        if not used:
            continue
        declared = set()
        if rels_path in names:
            declared = {
                r.get("Id", "").encode()
                for r in ET.fromstring(z.read(rels_path)).findall(
                    PKG_REL + "Relationship"
                )
            }
        for rid in sorted(used - declared):
            problems.append(
                f"{n}: uses relationship {rid.decode()} not declared in {rels_path}"
            )

    # The package must actually point at a main document part.
    root_rels = "_rels/.rels"
    if root_rels not in names:
        problems.append("no _rels/.rels — nothing identifies the main document part")
    else:
        types = {
            r.get("Type")
            for r in ET.fromstring(z.read(root_rels)).findall(PKG_REL + "Relationship")
        }
        if f"{OFFICE_REL}/officeDocument" not in types:
            problems.append(
                "_rels/.rels declares no officeDocument relationship — Word cannot "
                "find the main document part"
            )

    return problems


def main(argv):
    paths = argv[1:]
    if not paths:
        print(__doc__.strip().splitlines()[0])
        print("usage: verify_docx.py FILE.docx [FILE.docx ...]")
        return 2
    failed = 0
    for p in paths:
        problems = check(p)
        if problems:
            failed += 1
            print(f"FAIL {p}")
            for problem in problems:
                print(f"       {problem}")
        else:
            print(f"ok   {p}")
    if failed:
        print(f"\n{failed} of {len(paths)} file(s) failed")
    return 1 if failed else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
