//! §17.4.38 / §17.4.66: the square where two table borders join.
//!
//! ECMA-376 says which edges a cell paints and [MS-OI29500] §17.4.66 says which
//! of two facing cells wins a shared one, but neither says anything about the
//! square where a vertical border crosses a horizontal one. That is this
//! engine's own convention, and it has now been reported wrong three times, in
//! three different shapes, always as the same symptom: a 1–2px notch at a cell
//! corner. Each report was a square that the two edges of the cell owning it had
//! both been emptied of, while the borders that actually meet there belonged to
//! the neighbouring row and the neighbouring column.
//!
//! So this file asserts the property rather than any one shape of it: over a
//! whole rendered document, **every junction square is ink**. A junction is
//! where a vertical border rect and a horizontal one touch or overlap; the
//! square they join in is the vertical's x-band crossed with the horizontal's
//! y-band. Nothing here knows which cell a rect came from, which is the point —
//! that knowledge is exactly what each of the three defects had too little of.
//!
//! The fixture is the reporter's own document. `test-cases/` is untracked
//! (private customer documents), so the test is gated on its presence and is a
//! no-op in CI; the same invariant is asserted on in-memory tables by
//! `render::layout::table::emit`'s own tests, which do run there.

use dxpdf::render::layout::draw_command::{DrawCommand, LayoutedPage};
use dxpdf::render::resolve_and_layout;

/// A rect as `(x0, x1, y0, y1)`. Only thin ones are borders; a shading rect or
/// an image would swamp the junction search with squares that no border meets.
const MAX_BORDER_THICKNESS: f32 = 3.0;
const EPS: f32 = 0.001;

type Rect = (f32, f32, f32, f32);

fn border_rects(page: &LayoutedPage) -> Vec<Rect> {
    page.commands
        .iter()
        .filter_map(|c| match c {
            DrawCommand::Rect { rect, .. } => {
                let (w, h) = (rect.size.width.raw(), rect.size.height.raw());
                (w.min(h) <= MAX_BORDER_THICKNESS && w.min(h) > 0.0).then(|| {
                    (
                        rect.origin.x.raw(),
                        rect.origin.x.raw() + w,
                        rect.origin.y.raw(),
                        rect.origin.y.raw() + h,
                    )
                })
            }
            _ => None,
        })
        .collect()
}

/// Whether `square` is entirely painted by `rects` — by their **union**, not by
/// any one of them.
///
/// The union matters: two tables whose grids differ by a tenth of a point leave
/// junction squares straddling the seam between two abutting horizontals, which
/// a single-rect test reports as holes that are not there. Exact rather than
/// sampled — rect edges are the only discontinuities, so testing one x inside
/// each slab between them decides the whole slab.
fn covered(square: Rect, rects: &[Rect]) -> bool {
    let (sx0, sx1, sy0, sy1) = square;
    let mut xs = vec![sx0, sx1];
    for (x0, x1, ..) in rects {
        for x in [*x0, *x1] {
            if x > sx0 && x < sx1 {
                xs.push(x);
            }
        }
    }
    xs.sort_by(f32::total_cmp);

    for pair in xs.windows(2) {
        let (a, b) = (pair[0], pair[1]);
        if b - a <= EPS {
            continue;
        }
        let mid = (a + b) * 0.5;
        let mut spans: Vec<(f32, f32)> = rects
            .iter()
            .filter(|(x0, x1, ..)| *x0 <= mid && mid <= *x1)
            .map(|(_, _, y0, y1)| (*y0, *y1))
            .collect();
        spans.sort_by(|p, q| p.0.total_cmp(&q.0));
        let mut reached = sy0;
        for (y0, y1) in spans {
            if y0 > reached + EPS {
                break;
            }
            reached = reached.max(y1);
        }
        if reached < sy1 - EPS {
            return false;
        }
    }
    true
}

/// `(junctions_checked, unpainted)` for one page.
fn junctions(rects: &[Rect]) -> (usize, Vec<Rect>) {
    let (vertical, horizontal): (Vec<_>, Vec<_>) = rects
        .iter()
        .copied()
        .partition(|(x0, x1, y0, y1)| x1 - x0 < y1 - y0);

    let mut checked = 0usize;
    let mut missing: Vec<Rect> = Vec::new();
    for (vx0, vx1, vy0, vy1) in vertical {
        for (hx0, hx1, hy0, hy1) in horizontal.iter().copied() {
            if vx1 < hx0 - EPS || vx0 > hx1 + EPS || hy1 < vy0 - EPS || hy0 > vy1 + EPS {
                continue;
            }
            checked += 1;
            let square = (vx0, vx1, hy0, hy1);
            if !covered(square, rects) && !missing.contains(&square) {
                missing.push(square);
            }
        }
    }
    (checked, missing)
}

/// `(pages, junctions checked, one report line per page that has an unpainted
/// junction)` for one document.
fn audit(path: &std::path::Path) -> (usize, usize, Vec<String>) {
    let bytes = std::fs::read(path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    let doc =
        dxpdf::docx::parse(&bytes).unwrap_or_else(|e| panic!("parse {}: {e}", path.display()));
    let (_, pages) = resolve_and_layout(doc);

    let mut total_checked = 0usize;
    let mut failures = Vec::new();
    for (i, page) in pages.iter().enumerate() {
        let (checked, missing) = junctions(&border_rects(page));
        total_checked += checked;
        if !missing.is_empty() {
            failures.push(format!(
                "{} page {}: {missing:?}",
                path.file_name().unwrap_or_default().to_string_lossy(),
                i + 1
            ));
        }
    }
    (pages.len(), total_checked, failures)
}

/// Every `.docx` in a directory, sorted, or none when the directory is absent.
fn corpus(dir: &str) -> Vec<std::path::PathBuf> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut v: Vec<_> = entries
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|e| e == "docx"))
        .filter(|p| !is_word_owner_file(p))
        .collect();
    v.sort();
    v
}

/// Word writes a `~$`-prefixed owner file beside any document it has open, with
/// the same `.docx` extension and no ZIP inside it. Anyone comparing a fixture
/// against Word therefore drops one into the corpus directory, and a scan that
/// picked it up would fail the audit with a parse error that has nothing to do
/// with borders.
fn is_word_owner_file(p: &std::path::Path) -> bool {
    p.file_name()
        .and_then(|n| n.to_str())
        .is_some_and(|n| n.starts_with("~$"))
}

/// Run the audit over a corpus and print one line per document, so a run with
/// `--nocapture` is the corpus-wide report and not only a pass/fail.
fn audit_corpus(dir: &str) -> Vec<String> {
    let mut failures = Vec::new();
    let mut checked_total = 0usize;
    for path in corpus(dir) {
        let (pages, checked, mut bad) = audit(&path);
        checked_total += checked;
        println!(
            "{:>5} junctions {:>3} pages  {:>2} unpainted  {}",
            checked,
            pages,
            bad.len(),
            path.file_name().unwrap_or_default().to_string_lossy()
        );
        failures.append(&mut bad);
    }
    println!(
        "{dir}: {checked_total} junctions checked, {} unpainted",
        failures.len()
    );
    failures
}

/// The whole committed corpus, every page: no junction is painted by nobody.
///
/// This is the check that would have caught all three reported corner defects at
/// once, and it is here rather than in a scratch script for that reason — the
/// class stays closed only while something asks the question on every render.
#[test]
fn no_committed_fixture_has_an_unpainted_border_junction() {
    let failures = audit_corpus(concat!(env!("CARGO_MANIFEST_DIR"), "/test-files"));
    assert!(
        failures.is_empty(),
        "border junctions painted by nobody:\n{}",
        failures.join("\n")
    );
}

/// The same over the untracked local corpus, which is where all three reports
/// came from. A no-op without it; the loop above still runs in CI.
#[test]
fn no_local_corpus_document_has_an_unpainted_border_junction() {
    if corpus("test-cases").is_empty() {
        eprintln!("SKIPPED: test-cases/ not present");
        return;
    }
    let failures = audit_corpus("test-cases");
    assert!(
        failures.is_empty(),
        "border junctions painted by nobody:\n{}",
        failures.join("\n")
    );
}

/// The reporter's document: "a still-missing cell corner on page 1, at the cell
/// labelled *Location GPS:*".
///
/// Measured off the rendered page-1 content stream, the notch is the square
/// x = 265.598…266.098, y = 206.152…206.652 — the right edge of the form's
/// spacer column crossed with the band under the short gutter row above it. The
/// x is grid-derived and fixed; the y is a sum of measured row heights and so is
/// a property of the host's fonts, which is why the assertion below is the
/// property over the whole document rather than that one square: on any host
/// where the notch exists at all, it is a junction, and the audit finds it.
#[test]
fn ip05_trenches_has_no_unpainted_border_junction() {
    let path = std::path::Path::new("test-cases/IP 05 Trenches_Bad Harzburg_03-06-2026.docx");
    if !path.exists() {
        eprintln!("SKIPPED: {} not present", path.display());
        return;
    }
    let bytes = std::fs::read(path).expect("read fixture");
    let doc = dxpdf::docx::parse(&bytes).expect("parse fixture");
    let (_, pages) = resolve_and_layout(doc);
    assert!(!pages.is_empty(), "expected at least one page");

    let mut total_checked = 0usize;
    let mut failures: Vec<String> = Vec::new();
    for (i, page) in pages.iter().enumerate() {
        let (checked, missing) = junctions(&border_rects(page));
        total_checked += checked;
        if !missing.is_empty() {
            failures.push(format!("page {}: {missing:?}", i + 1));
        }
    }

    // Non-vacuity: this document's tables are drawn with `Tabellenraster`, so
    // every page carries hundreds of junctions. A run that found none would
    // mean the rect filter above stopped matching, not that the borders are
    // sound.
    assert!(
        total_checked > 100,
        "expected the audit to find junctions to check, got {total_checked}"
    );
    assert!(
        failures.is_empty(),
        "border junctions painted by nobody:\n{}",
        failures.join("\n")
    );
}
