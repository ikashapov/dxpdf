# AGENTS.md

Instructions for AI coding agents working in this repository. `AGENTS.md` is the
tool-neutral filename, so every agent reads the same source of truth — `CLAUDE.md`
is a pointer to this file, not a second copy. Add guidance here, never there.

## Project

**dxpdf** — a fast DOCX-to-PDF converter in Rust, powered by Skia. Three interfaces: CLI tool, Rust library, and Python package (via PyO3/maturin).

## Build & Test Commands

```bash
cargo build                    # Debug build
cargo build --release          # Release build
cargo test --all               # Run all tests
cargo test <test_name>         # Run a single test by name
cargo bench                    # Run Criterion benchmarks
cargo clippy --all-targets -- -D warnings   # Lint (CI enforces zero warnings)
RUSTDOCFLAGS="-D warnings" cargo doc --no-deps   # Doc links (CI enforces zero warnings)
cargo fmt --all -- --check     # Format check
cargo fmt --all                # Auto-format
```

System dependencies (Linux): `libfontconfig1-dev`, `libfreetype-dev`. Requires `clang` for Skia. Toolchain is pinned to 1.95.0 via `rust-toolchain.toml`.

**Cargo features**: `subset-fonts` (default, via `fontcull`) runs the font-subsetting pass; `python` gates the PyO3 bindings. Build with `--no-default-features` to skip subsetting (the pass is `#[cfg(feature = "subset-fonts")]`-gated in `render_with_font_mgr`).

Benchmarking: `cargo bench` for Criterion benchmarks (`benches/convert_bench.rs`, `benches/parse_bench.rs`). `RUST_LOG=debug` for per-phase timing from CLI; `RUST_LOG=warn` surfaces unsupported-feature warnings logged by parse/layout.

CLI usage: `cargo run -- input.docx [-o output.pdf] [--image-dpi 300]` (release binary: `dxpdf`). `--image-dpi` sets the resolution embedded raster images are downsampled to — default 220 (matching Word), valid range 1–2400.

Python bindings: `maturin develop --features python` builds and installs into the active venv. The `python` feature is gated in `Cargo.toml`.

Note: `Cargo.toml` excludes dev-only paths — `test-files/`, `scripts/`, `output/` — from the published crate. Any local-only scratch corpora are excluded there too, so nothing local can be published by accident.

## Architecture

The converter follows a **parse → resolve → layout → (subset) → paint** pipeline, orchestrated in `src/lib.rs::convert()` (parse + render) and `src/render/mod.rs::render_with_font_mgr()` (resolve → layout → subset → paint).

1. **Parse** (`src/docx/`) — Declarative XML parsing of DOCX (ZIP of XML) via serde schemas on `quick_xml::de`. `zip.rs` handles ZIP extraction; `relationships.rs` parses rels. `parse/primitives/` holds shared schema atoms (unit wrappers, `HexColor`, `OnOff`, `ST_*` enum catalog). `parse/properties/` holds `PPr`/`RPr`/`TblPr`/`SectPr` schemas shared across body, styles, and numbering. Part-specific schemas live under `body.rs`+`body_schema.rs`, `drawing/` (DrawingML — `anchor`, `inline`, `picture`, `shape`, `fill`, `stroke`, `geometry`), `styles.rs`, `numbering.rs`, `theme/`, `notes.rs`, `settings.rs`, `vml/`. Each schema type is `pub(crate)` and suffixed `Xml`; `From<XxxXml> for ModelType` is the XML→domain seam. Outputs an immutable `Document` model.

2. **Model** (`src/model/`) — Pure data types with no parsing logic. `types/` contains the ADT: `Document` → `Vec<Block>` (`Paragraph | Table | SectionBreak`) → `Vec<Inline>`. `Inline` has 17 variants — text and drawing (`TextRun`, `Image`, `Pict`, `Symbol`, `AlternateContent`), fields (`Field`, `FieldChar`, `InstrText`), notes (`FootnoteRef`, `EndnoteRef`, `FootnoteRefMark`, `EndnoteRefMark`, `Separator`, `ContinuationSeparator`), and navigation (`Hyperlink`, `BookmarkStart`, `BookmarkEnd`). `dimension.rs` and `geometry.rs` provide the type-safe unit system. `src/field/` contains the OOXML field instruction parser (PAGE, TOC, HYPERLINK, etc.).

3. **Resolve** (`src/render/resolve/`) — Flattens style inheritance, splits sections, extracts font families, pre-loads images, resolves conditional formatting and colors. `shape_geometry/` generates DrawingML preset/custom shape paths (guide-formula evaluation under `guides.rs`) — see `docs/shape-geometry.md`. Produces a `ResolvedDocument` with fully-resolved styles and sections.

4. **Layout** (`src/render/layout/`) — Measures text with Skia font metrics (`measurer.rs`) and fits content into pages. `build/` orchestrates the constraint cascade: page → section → table → cell → paragraph (`block.rs`, `table.rs`, `floating.rs`, `convert.rs`, `list_label.rs`). `fragment/` breaks inline content into measurable units for line fitting, using the `unicode-*` crates for grapheme and script handling; emoji clusters are shaped GSUB-aware through Skia's own HarfBuzz (`render/emoji/shape.rs`). `paragraph/` handles line emission and paragraph borders. `table/` handles 3-pass table layout (`measure.rs` → `grid.rs` → `emit.rs`, with `borders.rs` for border resolution and `split.rs` for row splitting across pages — see `docs/table-layout.md`). `section/` stacks blocks into pages: `stacker.rs` is the shared vertical-flow core used by *both* page and table-cell layout, while `layout.rs` owns the page-level algorithm (`layout_section`, keepNext chains, paragraph splitting, columns, footnotes) — see `docs/section-stacking.md`. `float.rs` handles text wrapping around floating images. `header_footer.rs` renders headers/footers in a second pass (after total page count is known). Outputs `Vec<LayoutedPage>` of positioned `DrawCommand`s.

5. **Subset** (`src/render/subset/`, default `subset-fonts` feature) — Between layout and paint: `collect.rs` walks draw commands recording **codepoint** usage per resolved typeface (keyed by `TypefaceId`, so substituted and direct requests for the same face merge), `apply.rs` subsets each typeface via `fontcull`, splices the original `name` table back in, validates that every kept codepoint still shapes to a non-`.notdef` glyph, and swaps the bytes into the `FontRegistry`. Every failure mode is an explicit `SubsetOutcome` variant; a typeface that can't be subsetted keeps its original bytes. See `docs/font-subsetting.md`.

6. **Paint** (`src/render/painter.rs`) — Iterates draw commands and emits PDF bytes via `skia_safe::pdf`. This is the only f32/Skia boundary. `skia_conv.rs` handles Pt-to-Skia conversions. `emf.rs` handles EMF (Enhanced Metafile) image rendering. `emoji/` is a separate color-emoji pipeline (UAX #29 / UTS #51 cluster classification in `cluster.rs`, host-OS color-typeface resolution in `resolve.rs`, GSUB shaping via Skia's HarfBuzz in `shape.rs`, Skia raster rasterization with a per-render cache in `raster.rs`).

### Key Design Patterns

- **Type-safe dimensions** (`src/model/dimension.rs`): Generic `Dimension<U>` parameterized by a unit marker (grep `impl Unit` for the full list — `Twips`, `Emu`, `HalfPoints`, `Pt`, etc.). `i64` storage for lossless OOXML round-tripping; `Pt` is the `f32` rendering unit. Prevents accidental unit mixing at compile time.

- **Generic geometry** (`src/model/geometry.rs`): `Offset<U>`, `Size<U>`, `Rect<U>`, `EdgeInsets<U>` parameterized over dimension units.

- **Spec-faithful ADT modeling**: All parsed values use typed enums/structs per OOXML spec sections. No raw strings for enumerated attributes — each gets a Rust enum. Typed identifiers (`RelId`, `StyleId`, `VmlShapeId`, `BookmarkId`) prevent mixing. Catch-all branches log warnings for unparsed elements; invalid enum values produce parse errors.

- **Two-pass rendering**: Layout runs first to determine total page count, then headers/footers are rendered in a second pass so PAGE/NUMPAGES fields resolve correctly.

- **Font resolution** (`src/render/fonts/`): A request is a name plus two **tri-state** §17.7.2 toggles (`Toggle::{Absent, Off, On}`), not a name plus a `FontStyle` — `Absent` asks for no weight, which is what lets a face name keep its own. `catalog.rs` turns the host font system and the DOCX's embedded fonts into one list of `FaceRecord`s, reading each face's own `name`/`OS/2`/`fvar`/`STAT` through the hand-written readers in `opentype/` (one table at a time via `copy_table_data`, never `to_font_data`). `resolve.rs` is a **pure function** over a request and a catalogue, running eight steps in order: embedded face, embedded family, host family, host face name, other metadata alias, parsed family+weight-word, metric-compatible substitute, host default. Everything down to step 5 is evidence the font asserts about itself; step 6 is the first guess. Ambiguous names are reported, not guessed. `resolve_exact`/`resolve_system_only` are the narrow variants the emoji pipeline needs. `FontRegistry` is the single source of truth for typeface bytes and is owned **per render** — the subset pass mutates it in place after layout, so a process-wide (`thread_local!`) typeface cache would leak subsetted faces across documents and must not be reintroduced; the same rule binds the catalogue. See `docs/font-substitution.md`.

- **Text shaping & emoji**: Grapheme and script handling uses `unicode-segmentation`/`unicode-properties`/`unicode-normalization`; emoji clusters are shaped through **Skia's HarfBuzz** (`skia-safe`'s `textlayout` feature) driven by a `Typeface`, never by extracted font bytes — `Typeface::to_font_data()` on a 183 MB emoji font costs ~549 MB of unreleasable RSS, which is why the pure-Rust shaper it replaced is gone; color emoji is handled by the dedicated `render/emoji/` pipeline via typed ADTs (no font-name allowlists, no bundled emoji fonts — it resolves the host OS color-emoji typeface at render time).

## OOXML Reference

**`docs/` — behavior.** How the engine works today, and WHY it makes those choices, which is generally not re-derivable from the source. Consult the relevant page before changing layout behavior, and update it in the same change when you change behavior it describes. A page here stays valid as long as the behavior does.

`docs/` is the only reference directory in the repo. Working notes — designs, profiling analyses, branch reviews — are kept **local and uncommitted** (`/plans/`, gitignored), because they describe a point in time rather than current behaviour. Nothing tracked may link to them, and no code comment may cite them: a fresh clone does not have them. Anything that must outlive the work belongs in `docs/`, or in the code it describes.

### `docs/` — current behavior

- [Style Cascade](docs/style-cascade.md) — §17.7.2 property resolution, doc defaults, table style interaction
- [Paragraph Spacing](docs/paragraph-spacing.md) — §17.3.1.33 spacing, page-top suppression, collapse rules
- [Line Spacing](docs/line-spacing.md) — §17.3.1.33 line/lineRule, Auto/Exact/AtLeast modes
- [Tabs](docs/position-tabs.md) — §17.3.1.30 `w:ptab` alignment derived at layout time; §17.18.85 `bar` stops (a rule, invisible to a tab character) and `decimal` stops
- [Internationalisation](docs/i18n.md) — §17.3.2.20 `w:lang`, the `Locale` type and its language table, and the ICU-shaped gap it is a stopgap for (number spelling, UAX #14 line breaking, bidi, date pictures)
- [Section Stacking](docs/section-stacking.md) — §17.6 block stacking, page/column breaks, keepNext chains, §17.3.1.14/§17.3.1.44 across-page paragraph splitting with widow/orphan control, footnote reservation
- [Table Layout](docs/table-layout.md) — §17.4 3-pass column sizing, border conflict resolution, row splitting across pages
- [Floating Tables](docs/floating-tables.md) — §17.4.58 tblpPr positioning, vertical anchors
- [Floating Images](docs/floating-images.md) — §20.4.2 anchor positioning, text wrapping, forward-scan
- [Headers and Footers](docs/headers-footers.md) — §17.10.1 rendering, table support, per-page fields
- [Document Outline](docs/document-outline.md) — §17.3.1.19 `w:outlineLvl` → PDF `/Outlines`, the flat-structure-tree contract Skia actually implements, and where headings are deliberately not collected
- [Fields](docs/fields.md) — §17.16.18 complex/simple fields, PAGE/NUMPAGES evaluation
- [Font Resolution & Substitution](docs/font-substitution.md) — §17.8 the tri-state request, the face catalogue and its three cost tiers, the 8-step resolution chain, metric-compatible substitutes, per-render `FontRegistry` ownership
- [Font Subsetting](docs/font-subsetting.md) — §17.8 / ISO 32000-1 §9.6 codepoint collection, `fontcull` subsetting, name-table splice, TTC face carving, shapeability validation, the variable-instance boundary
- [Shape Geometry](docs/shape-geometry.md) — §20.1.9 `prstGeom`/`custGeom` → paths, guide-formula evaluation, preset tiering

### Known-unimplemented work

Open engineering units are tracked as GitHub issues, not here — this file goes stale the moment one closes. Everything that is *not* a tracked unit is recorded where it applies: each ambiguity ECMA-376 cannot settle is stated in a comment at the site that makes the choice, saying what the choice is, why the spec does not decide it, and what evidence would. Grep for "Word reference render" to find them. Where a capability boundary is deliberate, the code says so at the boundary rather than deferring to the tracker — `SubsetOutcome::VariableInstanceNotBaked` states why a variable instance cannot be baked into embedded PDF bytes and names its two candidate routes; `register_embedded` states which faces of an embedded collection a given platform will open; `src/render/fonts/request.rs` states why `Toggle::Off` and `Toggle::Absent` select the same face today. One larger question is a decision rather than a gap: whether to take on a CLDR/ICU dependency for the i18n gaps [Internationalisation](docs/i18n.md) scopes.

**No doc yet** — start from the module docs at these entry points: character spacing and distributed alignment (`src/render/spacing.rs` — §17.3.2.35 and §17.3.1.13 share one unit, the UAX #29 grapheme cluster; the module doc says why it is that and not a shaped cluster), color-emoji pipeline (`src/render/emoji/mod.rs`), parse/serde schemas (`src/docx/parse/`, the `XxxXml` → domain seam), text shaping & fragments (`src/render/layout/fragment/`), paint & PDF emission (`src/render/painter.rs`), EMF images (`src/render/emf.rs`), numbering & list labels (`src/docx/parse/numbering.rs`, `src/render/layout/build/list_label.rs`), VML fallback (`src/docx/parse/vml/`).

## Test Organization

- **Unit tests**: `#[cfg(test)]` modules within source files.
- **Integration tests** (`tests/`): `integration.rs` (in-memory DOCX build + parse), `parse_test_files.rs` (parse real DOCX files from `test-files/`), `render_integration.rs` (layout + rendering validation), `emoji_e2e.rs` (color-emoji pipeline end-to-end), `header_footer_selection.rs` and `header_part_rels.rs` (header/footer resolution), `serde_spike.rs` (mixed-content parsing), `table_border_conflict.rs` (§17.4.66 nil-vs-none, conflict resolution), `table_row_height.rs` (vMerge row heights), `table_style_whole_table.rs` (§17.7.6 `wholeTable` cascade), `floating_table_pagination.rs` (§17.4.58 anchor/spillover termination), `font_resolution.rs` (§17.8 face resolution against the committed fixture fonts).
- **Test helpers**: `make_docx()` and `simple_docx()` in `tests/integration.rs` build minimal in-memory DOCX archives.
- **Visual diffing**: `scripts/compare_pdfs.py` diffs generated PDFs against references. `scripts/verify_wheel.py` checks that FreeType is embedded in built wheels (run by the CI wheel job). `scripts/make_font_fixtures.py` rebuilds the font fixtures under `test-files/fonts/` (needs `fonttools`).

## Public API

- **Rust**: `convert(&[u8])` uses default options; `convert_with_options(&[u8], &RenderOptions)` is the full entry point. `RenderOptions` is a builder (`with_image_dpi`) with `DEFAULT_IMAGE_DPI = 220.0`; non-finite or non-positive requests are clamped up to `MIN_IMAGE_DPI`.
- **Python** (`--features python`, built with maturin via `pyproject.toml`): `convert(docx_bytes, image_dpi=220)` and `convert_file(input, output, image_dpi=220)`. Type stubs and the `py.typed` marker live in `python/dxpdf/`.

## Working in this repo

**Test corpus** — `test-files/` holds the committed DOCX fixtures, and is the corpus to use for reproductions and regression work:

| File | Exercises |
|---|---|
| `sample-docx-files-sample1`…`sample4` | General documents — text, tables, images, sections. `sample4` (14 MB) is the large-document/perf case |
| `sample-docx-files-sample-4`…`sample-6` | Small focused samples |
| `font_scaling.docx` | Font sizing and scaling |
| `sample-emoji.docx` | Color-emoji pipeline |
| `fonts/*.ttf`, `fonts/*.ttc` | §17.8 face resolution — built by `scripts/make_font_fixtures.py`, exercised by `tests/font_resolution.rs`. Regenerate rather than hand-edit; the build is deterministic |

`tests/parse_test_files.rs` parses these and validates the resulting `Document`, so anything added here becomes part of the test suite. Add a new fixture when reproducing a bug — a committed fixture is what makes a fix verifiable by anyone.

`output/` holds generated PDFs. Scratch only, gitignored; never commit generated PDFs.

**Render-and-verify loop.** Rendering changes need visual confirmation, not just green tests:

```bash
cargo build --release
./target/release/dxpdf test-files/sample-docx-files-sample1.docx -o output/sample1.pdf

# Targeted before/after on a single page:
pdftoppm -png -r 150 -f 1 -l 1 output/sample1.pdf /tmp/after
magick compare -metric AE /tmp/before-1.png /tmp/after-1.png null:
```

`scripts/compare_pdfs.py` batch-diffs rendered output against `*_real.pdf` reference files (needs poppler + Pillow). It reads a local reference corpus that is not part of the repo, so it reports "No test pairs found" unless you have those references locally.

For any paint or subset change, pixel-diff before vs after — a passing test suite does not prove the output is unchanged.

**Before handing work back**, run what CI runs (`.github/workflows/ci.yml`):

```bash
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings   # CI enforces zero warnings
cargo test --all
RUSTDOCFLAGS="-D warnings" cargo doc --no-deps   # CI enforces zero doc warnings
cargo build --no-default-features           # `subset-fonts` off still compiles
```

The doc check catches dangling `[`links`]`, links from public docs to private
items, and prose rustdoc reads as HTML (`Vec<Thing>` outside backticks). Link to
a private item with a plain code span, not `[`brackets`]`.

**Logging**: `RUST_LOG=debug` gives per-phase timing — parse/render/total from `convert`, then resolve, registry, layout, subset and paint from `render_with_font_mgr` — plus the font-resolution decision for every requested family; `RUST_LOG=warn` surfaces unsupported-feature warnings from parse and layout. Prefer these numbers to intuition. The registry build used to be the largest cost on an ordinary document — a fixed 78–95 ms on every render regardless of document size — because it indexed the whole host font system up front. It is now tiered and lazy: a document whose fonts are all present or embedded costs ~3 ms, and one that has to reach the metadata index costs ~105 ms, paid once. `docs/font-substitution.md` has the per-operation breakdown and says which operation dominates (`FontMgr::match_family`, at 28 ms across a 210-family host).
