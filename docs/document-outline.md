# Document Outline — §17.3.1.19 `w:outlineLvl` → PDF `/Outlines`

A DOCX heading becomes an entry in the PDF's outline — the tree a viewer shows
in its Bookmarks or Contents sidebar. Word calls the same structure the
Navigation Pane; issue #90 called it "the document index".

## What counts as a heading

§17.3.1.19 `w:outlineLvl`, and nothing else. It is a **paragraph** property, so:

* a paragraph using a Heading style inherits the style's level through the
  §17.7.2 cascade and is a heading;
* a paragraph with a direct `w:outlineLvl` and no style is *also* a heading;
* a style *named* "Heading" that sets no level is **not**.

Value 9 is `ST_DecimalNumber`'s "body text" — an explicit statement that the
paragraph has no outline level, which is what Word writes when a heading's level
is reset. `OutlineLevel::from_ooxml` returns `None` for it, so "not a heading"
covers both the absent attribute and the present-and-9 case with one answer.
This is not theoretical: `sample-emoji.docx` in the corpus declares
`w:outlineLvl w:val="9"` twice.

A heading with no text produces no entry. The title is the only thing a reader
can navigate by, and an untitled row points somewhere unnameable.

### Where headings are *not* collected

| Context | Why |
|---|---|
| Headers and footers | Page furniture repeated on every page, not a position in the document — and would otherwise enter the outline once per page |
| Footnote and endnote bodies | Not the document's main story |
| Shape text boxes | Same, and Word's navigation pane leaves them out too |

All three share one mechanism. `build_header_footer_content` (headers, footers
*and* shape text bodies) and `build_note_content` each **suspend** the
`OutlineCollector` for the whole call and restore it after — save-and-restore
rather than a fresh flag, because the calls nest: a shape text body inside a
header goes through the first of them twice.

Suspending the whole call, rather than passing `None` at the paragraph, is what
makes it correct. Both of those builders hand a `Block::Table` to the **body**
table builder, so a heading in a header table reaches the ordinary paragraph
builder with the document's state. Guarding only the paragraph arm let such a
heading into the outline *once per page the header was drawn on* — three copies
on a three-page document. That is a real bug this design had until a mutation
test asked why the guard was a call-site convention rather than a rule.

Every paragraph path therefore asks the same question, `paragraph_outline`, and
the answer depends on where it is asked from rather than on each call site
remembering.

## The title

The paragraph's own text: runs joined, hyperlink and field *results* walked
into. A title cut at the first run boundary would be wrong more often than not,
since that is exactly where formatting changes.

The §17.9.22 numbering label is **excluded**. It is injected at layout and is
not part of the paragraph, and both Word and LibreOffice title a numbered
heading with its text alone — measured from the reference PDF attached to issue
#90, whose fixture's headings all carry `w:numPr` and render numbered on the
page while their outline entries do not.

## How it reaches the PDF

Three pieces, in pipeline order.

**Layout** assigns each heading a PDF structure node ID — 1-based, because
`skia_safe::pdf::node_id` reserves 0 for "no node" and every negative value for
artifacts — and brackets the paragraph's draw commands with
`DrawCommand::Outline(Begin/End)`.

The bracket is a *pair* because Skia derives an entry's destination from the
union of the marks under its node: leaving the ID set past the heading would
drag its destination onto the body text below. It is the same shape as the
`BDC`/`EMC` marked-content pair it ultimately becomes. Balance is held by the
single emitter rather than by the type — a variant owning a `Vec<DrawCommand>`
would need an arm in each of the twenty files that match on `DrawCommand`, for a
nesting the flat per-page command list does not otherwise have — and is asserted
directly in `tests/document_outline.rs`.

Only a paragraph's **first segment** emits the pair. `emit_line_commands` runs
once per page-slice of a split paragraph (§17.3.1.14), so a heading spanning a
page break passes through it twice; the entry belongs to the page the heading
starts on. It rides the same `is_first_segment` flag that already decides
`space_before` and the drop cap. This cannot be inferred from the line range: a
continuation is re-fitted against its own page and starts again at line 0.

**Paint** walks the pages twice. The first pass builds the structure tree, which
has to exist before `pdf::new_document` — the metadata holds it by reference for
the document's lifetime. The second sets and clears the node ID as it meets the
markers. Both read the `node_id` the marker carries rather than each keeping a
counter, so they cannot disagree.

### The Skia contract, which is not what the upstream source suggests

Measured against Skia **m145**, the pinned build. `SkPDFTag.cpp` on `main`
describes different behaviour; do not "simplify" toward it.

`pdf::Metadata::outline` has three values and only one is this feature:

| `Outline` | m145 behaviour |
|---|---|
| `None` | no `/Outlines` at all |
| `StructureElements` | mirrors the whole structure tree, titles are the *type strings* (`"Document"`, `"H1"`, …) |
| `StructureElementHeaders` | H1–H6 only, titles from `fAlt` — what we use |

Two traps:

1. **`fAlt` is mandatory.** m145's public node has no `fTitle` field; accumulating
   drawn text into a title is a `main`-branch feature. Without an alt, m145 emits
   no `/Outlines` element whatsoever — not an untitled entry, nothing.

2. **The tree must be FLAT.** Skia derives the outline hierarchy from the heading
   level *digit*, not from tree shape. Nesting an H2 node inside an H1 node does
   not nest the entry — it **concatenates the two titles into one**. A flat
   sibling list of H1/H2/H1 yields the correct nested outline with correct
   per-page destinations. "Mirror the heading hierarchy" is the obvious design
   and it silently merges every subheading into its parent.

### Levels 7–9

ISO 32000-1 defines heading structure types `H1`–`H6` only and Skia enforces
that, while §17.3.1.19 allows nine levels. Levels 7–9 clamp to `H6`, with a
warning: the entry survives — dropping a heading outright is worse than a
flattened one — and the outline stops distinguishing depth below six.

### What the `End` marker is for, and what it is not

`End` restores "no node", so a heading's marked content stops at the heading.
Its effect is **not** observable through `/Outlines` alone: a destination is the
uppermost point on the *earliest* page of a node's marks, and content following
a heading is neither higher nor earlier, so leaving the ID set produces the same
destinations. It is kept because the scoping is what the structure tree
describes — without it every heading's node would swallow the rest of the
document — and because a following heading would otherwise be the only thing
that ever ended one. Recorded here because a mutation that drops it survives the
outline tests, and that is a property of what `/Outlines` can express, not a
gap in them.

## Coverage

`tests/document_outline.rs` reads the emitted PDF with `lopdf` rather than the
layout, because nothing below the PDF proves the structure arrived.
`test-files/document_outline.docx` is the reporter's own file from issue #90,
and the titles asserted against it are the reference PDF's.

Three corpus documents gained an outline: `sample-docx-files-sample1` (17
entries), `sample3` (9) and `sample2` (2) — the only three whose paragraphs
actually *use* heading styles. All three are pixel-identical with the same page
count; the bytes differ only by the added catalog structure. The remaining
`w:outlineLvl` documents in the corpus define Heading1–9 in template boilerplate
that no paragraph uses.
