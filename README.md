# dxpdf — Fast DOCX to PDF Converter in Rust

**Convert Microsoft Word DOCX files to PDF without Microsoft Office, LibreOffice, or any cloud API.**

dxpdf is an open-source, standalone DOCX-to-PDF conversion engine written in Rust and powered by [Skia](https://skia.org). It reads `.docx` files and produces high-fidelity PDF output — preserving text formatting, tables, images, headers, footers, hyperlinks, and page layout. Available as a CLI tool, a Rust library, and a Python package.

[![Crates.io](https://img.shields.io/crates/v/dxpdf)](https://crates.io/crates/dxpdf)
[![Documentation](https://img.shields.io/docsrs/dxpdf)](https://docs.rs/dxpdf)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)

Built by [nerdy.pro](https://nerdy.pro).

---

## Key Features

- **Blazing fast** — converts typical business documents in well under a second, most in tens of milliseconds
- **High fidelity** — parse → resolve → layout → paint pipeline with pixel-accurate baseline positioning
- **Compact output** — embedded fonts are subsetted to the glyphs actually used, so PDFs stay small
- **Type-safe** — compile-time dimensional type system (`Twips`, `Pt`, `Emu`) prevents unit mixing bugs
- **Standalone** — no Office installation, no LibreOffice, no external services needed
- **Cross-platform** — runs natively on macOS, Linux, and Windows
- **Three interfaces** — use as a CLI tool, Rust library (`use dxpdf;`), or Python package (`import dxpdf`)
- **Unicode-aware** — complex-script shaping via HarfBuzz, plus full-color emoji including ZWJ, skin-tone, keycap and flag sequences
- **ISO 29500 compliant** — validated against the Office Open XML specification

## Installation

### Command-Line Tool

```bash
cargo install dxpdf
```

### Rust Library

Add to your `Cargo.toml`:

```toml
[dependencies]
dxpdf = "0.3.1"
```

### Python Package

```bash
pip install dxpdf
```

## Usage

### CLI — Convert DOCX to PDF from the Terminal

```bash
dxpdf input.docx                  # produces input.pdf
dxpdf input.docx -o output.pdf    # specify output path
dxpdf input.docx --image-dpi 300  # embed images at 300 DPI (default 220; range 1–2400)
```

Embedded raster images are downsampled to `--image-dpi` pixels per inch
(default **220**, matching Word). Raise it for print-quality output (e.g. `300`)
or lower it for smaller files (e.g. `96`); images are never upsampled past their
source resolution.

### Rust — Convert DOCX to PDF Programmatically

```rust
let docx_bytes = std::fs::read("document.docx")?;
let pdf_bytes = dxpdf::convert(&docx_bytes)?;
std::fs::write("output.pdf", &pdf_bytes)?;
```

To customize rendering — e.g. the embedded-image resolution (default 220 DPI) —
use `convert_with_options`:

```rust
use dxpdf::RenderOptions;

let options = RenderOptions::default().with_image_dpi(300.0);
let pdf_bytes = dxpdf::convert_with_options(&docx_bytes, &options)?;
```

You can also inspect or transform the parsed document model before conversion:

```rust
use dxpdf::{docx, model, render};

let document = docx::parse(&std::fs::read("document.docx")?)?;

for block in &document.body {
    match block {
        model::Block::Paragraph(p) => { /* inspect paragraph content */ }
        model::Block::Table(t) => { /* inspect table structure */ }
        model::Block::SectionBreak(props) => { /* inspect section properties */ }
    }
}

let pdf_bytes = render::render(&document, &dxpdf::RenderOptions::default())?;
```

### Python — Convert DOCX to PDF in Python

```python
import dxpdf

# Bytes in, bytes out
pdf_bytes = dxpdf.convert(open("input.docx", "rb").read())

# File path to file path
dxpdf.convert_file("input.docx", "output.pdf")

# Customize embedded-image resolution (default 220 DPI)
pdf_bytes = dxpdf.convert(open("input.docx", "rb").read(), image_dpi=300)
dxpdf.convert_file("input.docx", "output.pdf", image_dpi=300)
```

## Supported DOCX Features

dxpdf handles the most common DOCX features found in real-world business documents, reports, and forms:

| Category | Features |
|---|---|
| **Text formatting** | Bold, italic, underline, highlighting, font size/family/color, character spacing, superscript/subscript, run shading |
| **Paragraphs** | Alignment (left/center/right/justify/distribute), spacing (before/after/line with auto/exact/atLeast), indentation, tab stops incl. absolute-position tabs, paragraph borders, paragraph shading |
| **Tables** | Column widths, cell margins (3-level cascade), merged cells (gridSpan + vMerge), row heights, borders, cell shading, nested tables, row splitting across pages |
| **Images** | Inline images (PNG, JPEG, BMP, WebP, EMF), floating/anchored images with alignment, wrapping and percentage-based positioning |
| **Styles** | Paragraph and character styles, `basedOn` inheritance, document defaults, theme fonts |
| **Fonts** | Embedded DOCX fonts, metric-compatible substitution, and subsetting so only used glyphs are embedded |
| **Text & emoji** | HarfBuzz shaping for complex scripts; full-color emoji including ZWJ, modifier, keycap and flag sequences |
| **Headers & footers** | Text, images, page numbers via PAGE/NUMPAGES field codes |
| **Lists** | Bullets, decimal, lower/upper letter, lower/upper roman numbering with counter tracking |
| **Hyperlinks** | Clickable PDF link annotations with URL resolution |
| **Page layout** | Multiple page sizes/margins, section breaks, multi-column sections, portrait and landscape orientation |
| **Pagination** | Automatic page breaking, paragraph splitting across pages with keep-lines and widow/orphan control, word wrapping, line spacing modes, footnotes, floating image text flow |

## Performance Benchmarks

Measured on Apple M3 Max with `hyperfine` (20 runs, 3 warmup) at **v0.1.5**:

| Document type | Pages | Conversion time | Memory usage |
|---|---|---|---|
| Short form with tables and images | 2 | **48 ms** | 20 MB |
| Multi-page report | 7 | **52 ms** | 24 MB |
| Image-heavy document (60+ images) | 24 | **353 ms** | 76 MB |

These figures predate later pipeline work (font subsetting, emoji rendering,
paragraph splitting) and have not been re-measured since. Treat them as an
order-of-magnitude guide rather than current numbers. To measure your own
workload, run `cargo bench` for the Criterion suites, or use the release binary
with `RUST_LOG=debug` for a per-phase breakdown of parse, resolve, layout,
subset and paint.

dxpdf is designed for batch processing, server-side conversion, and CI/CD
pipelines.

## Building from Source

### Prerequisites

- Rust 1.95.0 — pinned via `rust-toolchain.toml`, so `rustup` selects it automatically
- `clang` (required by `skia-safe` for building Skia bindings)
- **Linux only**: `libfontconfig1-dev` and `libfreetype-dev`

  ```bash
  sudo apt-get install -y libfontconfig1-dev libfreetype-dev
  ```

### Build

```bash
cargo build --release
```

The release binary will be at `target/release/dxpdf`.

The `subset-fonts` feature (font subsetting) is on by default; build with
`--no-default-features` to skip it.

### Run Tests

```bash
cargo test --all
```

## Architecture

dxpdf follows a **parse → resolve → layout → subset → paint** pipeline, with a measure-then-position model inspired by Flutter's rendering approach:

```
DOCX (ZIP) → Parse → Document Model → Resolve → Layout → Subset → Paint → PDF
             Twips/Emu/HalfPoints        ←──── Pt throughout ────→      Skia
```

Type-safe dimensions flow through the entire pipeline: OOXML units (`Twips`, `Emu`, `HalfPoints`) in the parsed model, `Pt` (typographic points) in layout, and `f32` only at the Skia rendering boundary.

1. **Parse** — declarative serde schemas over the DOCX XML parts, producing an immutable document model
2. **Resolve** — flatten the style cascade, split sections, pre-load images, generate shape geometry
3. **Layout** — measure text, fit lines, and position content into pages; runs first so total page count is known before headers/footers resolve PAGE/NUMPAGES
4. **Subset** — reduce each embedded typeface to the glyphs actually painted
5. **Paint** — emit draw commands in order (shading → content → borders) through Skia's PDF backend

### Module Overview

| Module | Purpose |
|---|---|
| `model::dimension` | Type-safe dimensional units (`Twips`, `HalfPoints`, `EighthPoints`, `Emu`, `Pt`) with compile-time unit safety |
| `model::geometry` | Spatial types (`Offset`, `Size`, `Rect`, `EdgeInsets`, `LineSegment`) — generic over unit, with Skia interop |
| `model` | Algebraic data types representing the full document tree (`Document`, `Block`, `Inline`, etc.) |
| `docx` | DOCX ZIP extraction, declarative serde-based XML parser for document, styles, numbering, theme, VML and DrawingML parts |
| `field` | OOXML field instruction parser (PAGE, NUMPAGES, HYPERLINK, TOC, …) |
| `render/resolve` | Style-cascade flattening, section splitting, image pre-loading, DrawingML shape geometry |
| `render/layout` | Fragment-based line fitting, paragraph layout, three-pass table layout, section stacking and pagination, header/footer handling |
| `render/subset` | Codepoint collection and per-typeface font subsetting before paint |
| `render/emoji` | Color-emoji pipeline — cluster classification, host typeface resolution, rasterization |
| `render/fonts` | Font resolution with embedded-font priority and metric-compatible substitution (e.g., Calibri → Carlito, Cambria → Caladea) |
| `render/painter` | Skia canvas operations for PDF output |

## OOXML Feature Coverage

Validated against ISO 29500 (Office Open XML). **53 entries fully implemented, 10 partial, 10 not yet supported.**

<details>
<summary>Full feature matrix (click to expand)</summary>

### Text Formatting (w:rPr)

| Feature | Status |
|---|---|
| Bold, italic | ✅ with toggle support |
| Underline | ✅ font-proportional stroke width |
| Font size, family, color | ✅ |
| Superscript/subscript | ✅ |
| Character spacing | ✅ |
| Character scaling (`w:w` horizontal compression/expansion) | ✅ |
| Run shading | ✅ |
| Strikethrough | ⚠️ parsed, not yet rendered |
| Highlighting | ✅ full ST_HighlightColor palette |
| Caps, smallCaps | ⚠️ parsed, not applied at layout |
| Shadow, outline, emboss, imprint | ❌ |
| Hidden text | ❌ |

### Paragraph Properties (w:pPr)

| Feature | Status |
|---|---|
| Alignment (left, center, right) | ✅ |
| Alignment (justify) | ✅ |
| Alignment (distribute) | ⚠️ scalar-based: combining marks and contextual scripts such as Arabic and Indic are not shaping-safe |
| Spacing before/after, line spacing | ✅ auto/exact/atLeast |
| Indentation (left, right, first-line, hanging) | ✅ |
| Tab stops (left) | ✅ |
| Tab stops (center, right) | ✅ |
| Tab stops (decimal) | ⚠️ rendered as left-aligned |
| Absolute position tabs (`w:ptab`) | ✅ §17.3.1.30 left/center/right, margin-relative |
| Paragraph shading | ✅ |
| Paragraph borders | ✅ with adjacent border merging, `w:space` offset |
| Keep with next | ✅ incl. chain pre-flight and page-fill |
| Keep lines together | ✅ §17.3.1.14 |
| Widow/orphan control | ✅ §17.3.1.44 |
| Paragraph splitting across pages | ✅ per-page re-fit around floats, per-segment borders |

### Styles

| Feature | Status |
|---|---|
| Paragraph styles, character styles | ✅ |
| `basedOn` inheritance | ✅ |
| Document defaults, theme fonts | ✅ |

### Tables

| Feature | Status |
|---|---|
| Grid columns, cell widths (dxa) | ✅ |
| Cell widths (pct, auto) | ⚠️ fall back to grid |
| Cell margins (3-level cascade) | ✅ |
| Merged cells (gridSpan, vMerge) | ✅ |
| Row heights | ✅ min / ⚠️ exact as min |
| Table borders (per-cell, per-table) | ✅ |
| Border styles (single) | ✅ |
| Border styles (double, dashed, dotted) | ⚠️ render as single |
| Cell shading (solid) | ✅ |
| Cell shading (patterns) | ❌ |
| Vertical alignment (top / center / bottom) | ✅ incl. vMerge-aware bottom alignment |
| Row splitting across page breaks | ✅ §17.4.1 row content split at legal cut points; `cantSplit` honored |
| Repeating header rows | ✅ §17.4.49 |
| Nested tables | ✅ |

### Images

| Feature | Status |
|---|---|
| Inline images | ✅ PNG, JPEG, BMP, WebP |
| Floating images | ✅ offset, align, wp14:pctPos |
| Wrap modes | ✅ none/square/tight/through |
| VML images | ❌ |

### Page Layout

| Feature | Status |
|---|---|
| Page size and orientation | ✅ |
| Page margins (all 6) | ✅ |
| Section breaks (nextPage) | ✅ |
| Section breaks (continuous) | ✅ continues on current page |
| Section breaks (even, odd) | ⚠️ treated as nextPage |
| Multi-column sections | ✅ incl. splitting across unequal-width columns |
| Page borders, doc grid | ❌ doc grid parsed, not applied |

### Headers & Footers

| Feature | Status |
|---|---|
| Default header/footer | ✅ |
| First page, even/odd, per-section | ✅ |

### Lists

| Feature | Status |
|---|---|
| Bullet, decimal, letter, roman | ✅ |
| Multi-level lists | ⚠️ levels parsed, nesting limited |

### Fields

| Feature | Status |
|---|---|
| PAGE, NUMPAGES | ✅ |
| Hyperlinks | ✅ clickable PDF annotations |
| Unknown fields | ✅ cached value fallback |
| TOC, MERGEFIELD, DATE | ❌ |

### Other

| Feature | Status |
|---|---|
| Footnotes | ✅ §17.11.23 separator, per-page reservation, split-aware |
| Endnotes | ❌ |
| Color emoji (ZWJ, modifier, keycap, flag sequences) | ✅ host-resolved color typeface, cross-run cluster reassembly |
| Complex-script shaping | ✅ via HarfBuzz (`rustybuzz`) |
| Font subsetting | ✅ codepoint-driven, with shapeability validation |
| Comments, tracked changes | ❌ / ⚠️ |
| DrawingML fills, strokes, outer shadow | ⚠️ Tier 0-1 coverage |
| DrawingML preset geometry | ⚠️ `line` and `rect`; `custGeom` fully evaluated incl. guide formulas |
| Text boxes, SmartArt, charts | ❌ |
| RTL text, automatic hyphenation | ❌ |

</details>

## Dependencies

| Crate | Purpose |
|---|---|
| [`quick-xml`](https://crates.io/crates/quick-xml) + [`serde`](https://crates.io/crates/serde) | Declarative XML parsing via serde deserializers |
| [`zip`](https://crates.io/crates/zip) | DOCX ZIP archive reading |
| [`skia-safe`](https://crates.io/crates/skia-safe) | PDF rendering, text measurement, link annotations |
| [`rustybuzz`](https://crates.io/crates/rustybuzz) | HarfBuzz-port text shaping for complex scripts |
| [`unicode-segmentation`](https://crates.io/crates/unicode-segmentation), [`unicode-properties`](https://crates.io/crates/unicode-properties), [`unicode-normalization`](https://crates.io/crates/unicode-normalization) | Grapheme clusters, emoji properties, NFC normalization |
| [`fontcull`](https://crates.io/crates/fontcull) (optional) | Font subsetting — `subset-fonts` feature, on by default |
| [`clap`](https://crates.io/crates/clap) | CLI argument parsing |
| [`thiserror`](https://crates.io/crates/thiserror) | Error types |
| [`log`](https://crates.io/crates/log) + [`env_logger`](https://crates.io/crates/env_logger) | Logging for unsupported features (`RUST_LOG=warn`) |
| [`rustc-hash`](https://crates.io/crates/rustc-hash) | Fast hasher for the per-render measurement cache |
| [`bitflags`](https://crates.io/crates/bitflags) | Compact flag sets in the document model |
| [`pyo3`](https://crates.io/crates/pyo3) (optional) | Python bindings via maturin |

## Frequently Asked Questions

### How do I convert a DOCX file to PDF?

Install dxpdf with `cargo install dxpdf`, then run `dxpdf input.docx`. The PDF will be created in the same directory. You can also specify an output path with `-o output.pdf`.

### Does dxpdf require Microsoft Office or LibreOffice?

No. dxpdf is a standalone converter that reads DOCX files directly and renders PDF output using Skia. No Office installation or external service is needed.

### Can I use dxpdf as a library in my Rust or Python project?

Yes. In Rust, add `dxpdf` as a dependency and call `dxpdf::convert(&docx_bytes)`. In Python, install with `pip install dxpdf` and call `dxpdf.convert(bytes)` or `dxpdf.convert_file("input.docx", "output.pdf")`.

### What DOCX features are supported?

dxpdf supports text formatting, paragraphs, tables (including nested and merged cells), inline and floating images, styles with inheritance, headers/footers, lists, hyperlinks, section breaks, and automatic pagination. See the full [feature matrix](#ooxml-feature-coverage) above.

### How fast is dxpdf?

Fast enough that conversion is rarely the bottleneck: on an Apple M3 Max a typical multi-page business document converts in tens of milliseconds, and a 24-page image-heavy document in a few hundred. See [Performance Benchmarks](#performance-benchmarks) for measured figures and how to benchmark your own workload.

### What platforms does dxpdf support?

dxpdf runs on macOS, Linux, and Windows. On Linux, you need `libfontconfig1-dev` and `libfreetype-dev` installed.

## Used By

- <img src="https://www.google.com/s2/favicons?domain=nerdy.pro&sz=32" width="16" height="16" alt=""> [nerdy.pro](https://nerdy.pro)
- <img src="https://www.google.com/s2/favicons?domain=formtastic.de&sz=32" width="16" height="16" alt=""> [formtastic.de](https://formtastic.de)

## Contributing

Contributions are welcome. Please open an issue before submitting large PRs.

Build commands and project conventions are in [`AGENTS.md`](AGENTS.md).

Before opening a PR, run what CI runs:

```bash
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
cargo test --all
```

Built by [nerdy.pro](https://nerdy.pro).

## License

MIT
