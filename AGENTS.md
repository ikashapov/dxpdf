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

6. **Paint** (`src/render/painter.rs`) — Iterates draw commands and emits PDF bytes via `skia_safe::pdf`. This is the only f32/Skia boundary. `skia_conv.rs` handles Pt-to-Skia conversions. `emf.rs` handles EMF (Enhanced Metafile) image rendering. `emoji/` is a separate color-emoji pipeline (UAX #29 / UTS #51 cluster classification in `cluster.rs`, host-OS color-typeface resolution in `resolve.rs`, Skia raster rasterization with a per-render cache in `raster.rs`).

### Key Design Patterns

- **Type-safe dimensions** (`src/model/dimension.rs`): Generic `Dimension<U>` parameterized by a unit marker (grep `impl Unit` for the full list — `Twips`, `Emu`, `HalfPoints`, `Pt`, etc.). `i64` storage for lossless OOXML round-tripping; `Pt` is the `f32` rendering unit. Prevents accidental unit mixing at compile time.

- **Generic geometry** (`src/model/geometry.rs`): `Offset<U>`, `Size<U>`, `Rect<U>`, `EdgeInsets<U>` parameterized over dimension units.

- **Spec-faithful ADT modeling**: All parsed values use typed enums/structs per OOXML spec sections. No raw strings for enumerated attributes — each gets a Rust enum. Typed identifiers (`RelId`, `StyleId`, `VmlShapeId`, `BookmarkId`) prevent mixing. Catch-all branches log warnings for unparsed elements; invalid enum values produce parse errors.

- **Two-pass rendering**: Layout runs first to determine total page count, then headers/footers are rendered in a second pass so PAGE/NUMPAGES fields resolve correctly.

- **Font resolution** (`src/render/fonts.rs`): Five steps, in order — embedded DOCX font, exact system match, face-alias index (PostScript/style names → family), metric-compatible substitutes (Calibri→Carlito, Cambria→Caladea, …), then the system default. `resolve_exact`/`resolve_system_only` are the narrow variants the emoji pipeline needs. `FontRegistry` is the single source of truth for typeface bytes and is owned **per render** — the subset pass mutates it in place after layout, so a process-wide (`thread_local!`) typeface cache would leak subsetted faces across documents and must not be reintroduced. See `docs/font-substitution.md`.

- **Text shaping & emoji**: Grapheme and script handling uses `unicode-segmentation`/`unicode-properties`/`unicode-normalization`; emoji clusters are shaped through **Skia's HarfBuzz** (`skia-safe`'s `textlayout` feature) driven by a `Typeface`, never by extracted font bytes — `Typeface::to_font_data()` on a 183 MB emoji font costs ~549 MB of unreleasable RSS, which is why the pure-Rust shaper it replaced is gone; color emoji is handled by the dedicated `render/emoji/` pipeline via typed ADTs (no font-name allowlists, no bundled emoji fonts — it resolves the host OS color-emoji typeface at render time).

## OOXML Reference

`docs/README.md` is the index — start there. It is grouped into **Rendering Reference** (current behavior), **Implementation Plans** (designs with items outstanding), and **Branch Code Reviews** (historical snapshots of merged branches, *not* descriptions of current behavior). It also lists which subsystems have no doc yet.

Consult the relevant page before changing layout behavior — these capture WHY the current code makes the choices it does, which is generally not re-derivable from the source. When you change behavior a doc describes, update the doc in the same change.

## Test Organization

- **Unit tests**: `#[cfg(test)]` modules within source files.
- **Integration tests** (`tests/`): `integration.rs` (in-memory DOCX build + parse), `parse_test_files.rs` (parse real DOCX files from `test-files/`), `render_integration.rs` (layout + rendering validation), `emoji_e2e.rs` (color-emoji pipeline end-to-end), `header_footer_selection.rs` and `header_part_rels.rs` (header/footer resolution), `serde_spike.rs` (mixed-content parsing).
- **Test helpers**: `make_docx()` and `simple_docx()` in `tests/integration.rs` build minimal in-memory DOCX archives.
- **Visual diffing**: `scripts/compare_pdfs.py` diffs generated PDFs against references. `scripts/verify_wheel.py` checks that FreeType is embedded in built wheels (run by the CI wheel job).

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

**Logging**: `RUST_LOG=debug` gives per-phase timing plus the font-resolution decision for every requested family; `RUST_LOG=warn` surfaces unsupported-feature warnings from parse and layout.
