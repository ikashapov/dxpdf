//! §17.4.33 `w:shd` where two shaded cells meet — the seam an abutting pair of
//! fills leaves behind, and the property that keeps one from existing.
//!
//! ECMA-376 has nothing to say here: a cell's shading fills its cell, adjacent
//! cells abut, and the ideal geometry of two abutting rects is the same as that
//! of the one rect covering both. The difference is entirely in the raster.
//! A viewer that anti-aliases each fill independently gives the shared boundary
//! pixel partial coverage from each side and composites the two in sequence, so
//! it never reaches full coverage and a pale hairline is left along the join.
//! Whether that happens depends on the rasterizer and on where the boundary
//! falls: CoreGraphics — macOS Preview, Quick Look, Safari — shows it at every
//! zoom for a boundary at a fractional device pixel, while poppler composites
//! the same pair cleanly, which is why `pdftoppm` and the pixel-diff loop in
//! `AGENTS.md` cannot see this class of defect at all.
//!
//! It was reported against the `MediumShading2-Accent5` header row of
//! `sample-docx-files-sample1.docx`: four `4BACC6` cells at x = 66.6, 186.3,
//! 306.0 and 425.7, each 119.7pt wide, and a visible seam at 186.3 and 425.7
//! (fractional at every scale) but never at 306.0 (always integral).
//!
//! So the property is not about the spec, it is about the command stream:
//!
//! > two **consecutive** rects of the same colour never share an edge.
//!
//! Consecutive is the load-bearing word. Merging a pair with nothing painted
//! between them cannot change what reaches the page — the union is the same
//! region, in the same place in the paint order — so any such pair is a seam
//! that costs nothing to remove, and `coalesce_abutting_rects` removes it.
//!
//! A pair with commands *between* them is a different question, and this file
//! deliberately does not ask it. Merging there would move the later fill earlier
//! in the paint order, which is not sound in general: the border layer paints
//! overlapping junction squares (`tests/table_border_corners.rs`), and a
//! reordering across those would reopen exactly the class those guard.
//!
//! **What that leaves open, measured rather than assumed.** Widening the audit
//! to consecutive *fills* — ignoring what lies between them — finds three more
//! pairs in the committed corpus, in `sample-docx-files-sample1.docx` and
//! `sample-emoji.docx`, and a few dozen in the untracked one. Every one is
//! §17.3.2.32 run shading, which `paragraph::line_emit` emits per fragment
//! immediately before that fragment's own text, so adjacent runs of one colour
//! interleave as fill/text/fill/text. Those pairs are side by side and each
//! text sits inside its own fill, so fusing them would in fact paint the same
//! page — but establishing that in general needs horizontal bounds for a text
//! command, which `DrawCommand` does not carry. It is a real seam of the same
//! family and a separate fix; what would settle it is bounds on `DrawCommand`,
//! not a wider merge rule here.

use dxpdf::render::layout::draw_command::{DrawCommand, LayoutedPage};
use dxpdf::render::resolve_and_layout;
use std::io::Write;

fn make_docx(document_xml: &str) -> Vec<u8> {
    let mut buf = Vec::new();
    {
        let mut zip = zip::ZipWriter::new(std::io::Cursor::new(&mut buf));
        let o = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated);

        zip.start_file("[Content_Types].xml", o).unwrap();
        zip.write_all(
            br#"<?xml version="1.0" encoding="UTF-8"?>
<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">
  <Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>
  <Default Extension="xml" ContentType="application/xml"/>
  <Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/>
</Types>"#,
        )
        .unwrap();

        zip.start_file("_rels/.rels", o).unwrap();
        zip.write_all(
            br#"<?xml version="1.0" encoding="UTF-8"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
  <Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/>
</Relationships>"#,
        )
        .unwrap();

        zip.start_file("word/document.xml", o).unwrap();
        zip.write_all(document_xml.as_bytes()).unwrap();
        zip.finish().unwrap();
    }
    buf
}

fn layout(body: &str) -> Vec<LayoutedPage> {
    let document_xml = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
  <w:body>
    {body}
    <w:sectPr><w:pgSz w:w="11906" w:h="16838"/></w:sectPr>
  </w:body>
</w:document>"#
    );
    let doc = dxpdf::docx::parse(&make_docx(&document_xml)).expect("parse");
    resolve_and_layout(doc).1
}

/// One filled rect: `(x, y, w, h, colour)`.
type Fill = (f32, f32, f32, f32, (u8, u8, u8));

fn fills(page: &LayoutedPage) -> Vec<Fill> {
    page.commands.iter().filter_map(as_fill).collect()
}

/// Whether `a` and `b` are the same colour and share a full edge — so the two
/// of them paint exactly the region one rect would.
fn share_an_edge(a: &Fill, b: &Fill) -> bool {
    let (ax, ay, aw, ah, ac) = *a;
    let (bx, by, bw, bh, bc) = *b;
    if ac != bc {
        return false;
    }
    let side_by_side = ay == by && ah == bh && (ax + aw == bx || bx + bw == ax);
    let stacked = ax == bx && aw == bw && (ay + ah == by || by + bh == ay);
    side_by_side || stacked
}

/// Every pair of **adjacent commands** that are both rects, share an edge and
/// share a colour — as a report line each. Empty is the passing answer.
///
/// Adjacency is over the whole command stream, not over the rects in it: a pair
/// with a text command between them is the case the module doc explains this
/// does not ask.
fn seams(pages: &[LayoutedPage]) -> Vec<String> {
    let mut out = Vec::new();
    for (i, page) in pages.iter().enumerate() {
        for w in page.commands.windows(2) {
            let (Some(a), Some(b)) = (as_fill(&w[0]), as_fill(&w[1])) else {
                continue;
            };
            if share_an_edge(&a, &b) {
                out.push(format!("page {}: {a:?} abuts {b:?}", i + 1));
            }
        }
    }
    out
}

fn as_fill(c: &DrawCommand) -> Option<Fill> {
    match c {
        DrawCommand::Rect { rect, color } => Some((
            rect.origin.x.raw(),
            rect.origin.y.raw(),
            rect.size.width.raw(),
            rect.size.height.raw(),
            (color.r, color.g, color.b),
        )),
        _ => None,
    }
}

/// A row of `n` cells, each shaded `fill.get(i)`, in a fixed-layout table with
/// no borders at all — so the only rects on the page are the shadings.
fn shaded_row(fills: &[&str]) -> String {
    let cols: String = fills
        .iter()
        .map(|_| r#"<w:gridCol w:w="1200"/>"#.to_string())
        .collect();
    let cells: String = fills
        .iter()
        .map(|f| {
            format!(
                r#"<w:tc>
  <w:tcPr>
    <w:tcW w:w="1200" w:type="dxa"/>
    <w:shd w:val="clear" w:color="auto" w:fill="{f}"/>
  </w:tcPr>
  <w:p><w:r><w:t>X</w:t></w:r></w:p>
</w:tc>"#
            )
        })
        .collect();
    format!(
        r#"<w:tbl>
  <w:tblPr>
    <w:tblW w:w="{}" w:type="dxa"/>
    <w:tblLayout w:type="fixed"/>
  </w:tblPr>
  <w:tblGrid>{cols}</w:tblGrid>
  <w:tr>{cells}</w:tr>
</w:tbl>"#,
        1200 * fills.len()
    )
}

/// Four identically shaded cells are one fill, not four abutting ones.
///
/// The reported case, reduced. Asserted as a count *and* a width so that a
/// merge which dropped a cell would not pass: the survivor has to span the whole
/// row.
#[test]
fn a_row_of_identically_shaded_cells_is_painted_as_one_rect() {
    let pages = layout(&shaded_row(&["4BACC6"; 4]));
    let f = fills(&pages[0]);
    let shaded: Vec<_> = f
        .iter()
        .filter(|(_, _, _, _, c)| *c == (0x4B, 0xAC, 0xC6))
        .collect();

    assert_eq!(shaded.len(), 1, "four cells, one fill; got {shaded:?}");
    // 4 × 1200 twips is 240pt.
    assert_eq!(shaded[0].2, 240.0, "the survivor spans the whole row");
}

/// The control: cells that are *not* the same colour must stay separate rects.
///
/// Without this, a "fix" that merged every rect in a row regardless of colour
/// would satisfy the test above.
#[test]
fn cells_with_different_fills_are_not_merged() {
    let pages = layout(&shaded_row(&["4BACC6", "4BACC6", "FFCC00", "FFCC00"]));
    let f = fills(&pages[0]);

    let of =
        |c: (u8, u8, u8)| -> Vec<&Fill> { f.iter().filter(|(_, _, _, _, k)| *k == c).collect() };
    let blue = of((0x4B, 0xAC, 0xC6));
    let gold = of((0xFF, 0xCC, 0x00));

    assert_eq!(blue.len(), 1, "the two blue cells merge: {blue:?}");
    assert_eq!(gold.len(), 1, "and the two gold ones: {gold:?}");
    assert_eq!(blue[0].2, 120.0, "each run is half the row");
    assert_eq!(gold[0].2, 120.0);
    assert_eq!(
        blue[0].0 + blue[0].2,
        gold[0].0,
        "the two runs still abut — different colours have no seam to remove"
    );
}

/// A run broken by an unshaded cell does not jump the gap.
///
/// The other way a merge can be too eager: cells 1 and 3 are the same colour but
/// cell 2 paints nothing between them, so merging would flood a cell the author
/// left clear.
#[test]
fn a_run_interrupted_by_an_unshaded_cell_stays_two_rects() {
    let pages = layout(&shaded_row(&["4BACC6", "auto", "4BACC6"]));
    let shaded: Vec<_> = fills(&pages[0])
        .into_iter()
        .filter(|(_, _, _, _, c)| *c == (0x4B, 0xAC, 0xC6))
        .collect();

    assert_eq!(
        shaded.len(),
        2,
        "the clear cell separates the two runs: {shaded:?}"
    );
}

// ── the corpus audit ────────────────────────────────────────────────────────

fn corpus(dir: &str) -> Vec<std::path::PathBuf> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut v: Vec<_> = entries
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|e| e == "docx"))
        // Word writes a `~$`-prefixed owner file beside any document it has
        // open — same extension, no ZIP inside — so anyone comparing a fixture
        // against Word drops one into the corpus directory.
        .filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| !n.starts_with("~$"))
        })
        .collect();
    v.sort();
    v
}

/// Run the audit over a corpus, one line per document so a `--nocapture` run is
/// the corpus-wide report rather than only a pass/fail.
fn audit_corpus(dir: &str) -> Vec<String> {
    let mut failures = Vec::new();
    let mut rects_total = 0usize;
    for path in corpus(dir) {
        let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
        let doc =
            dxpdf::docx::parse(&bytes).unwrap_or_else(|e| panic!("parse {}: {e}", path.display()));
        let (_, pages) = resolve_and_layout(doc);
        let n: usize = pages.iter().map(|p| fills(p).len()).sum();
        rects_total += n;
        let mut bad = seams(&pages);
        println!(
            "{:>5} rects {:>3} pages  {:>2} seams  {}",
            n,
            pages.len(),
            bad.len(),
            path.file_name().unwrap_or_default().to_string_lossy()
        );
        failures.append(&mut bad);
    }
    println!(
        "{dir}: {rects_total} rects checked, {} seams",
        failures.len()
    );
    failures
}

/// The whole committed corpus, every page: no two consecutive same-colour rects
/// share an edge.
///
/// This is the check that would have caught the reported defect, and it is here
/// rather than in a scratch script for that reason — the class stays closed only
/// while something asks the question on every render. It is also the only kind
/// of check that can: the artifact is invisible to poppler, so no pixel diff of
/// ours would ever have shown it.
#[test]
fn no_committed_fixture_paints_a_seam() {
    let failures = audit_corpus(concat!(env!("CARGO_MANIFEST_DIR"), "/test-files"));
    assert!(
        failures.is_empty(),
        "consecutive same-colour rects sharing an edge:\n{}",
        failures.join("\n")
    );
}

/// The same over the untracked local corpus. A no-op without it; the loop above
/// still runs in CI.
#[test]
fn no_local_corpus_document_paints_a_seam() {
    if corpus("test-cases").is_empty() {
        eprintln!("SKIPPED: test-cases/ not present");
        return;
    }
    let failures = audit_corpus("test-cases");
    assert!(
        failures.is_empty(),
        "consecutive same-colour rects sharing an edge:\n{}",
        failures.join("\n")
    );
}
