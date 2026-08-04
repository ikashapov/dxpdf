# Font Subsetting — §17.8 / ISO 32000-1 §9.6

Between layout and paint, every typeface the document actually paints is
replaced by a subset containing only the glyphs it needs. A DOCX that uses 60
characters of a 400 KB font should not embed 400 KB in the PDF.

Gated by the default-on `subset-fonts` Cargo feature. Source:
`src/render/subset/`. Driven from `render_with_font_mgr` (`src/render/mod.rs:155`):

```rust
let usage = subset::collect(&pages, &registry);
let report = subset::apply(usage, &mut registry);
log::info!("font subset: {report}");
```

The pass runs *after* layout because it needs the final `Vec<LayoutedPage>` —
you cannot know which glyphs are used until pagination, headers/footers, and
field evaluation are done.

## Why codepoints, not glyph ids

`collect.rs` records **Unicode codepoints** per typeface, not glyph ids. Two
reasons (`src/render/subset/collect.rs:9-15`):

1. `fontcull` is codepoint-driven. It walks the font's own `cmap` to derive the
   glyph closure, then keeps `GSUB` substitutions reachable from those glyphs —
   ligatures and contextual alternates survive. Feeding it glyph ids observed
   during layout would drop any glyph the runtime *might* shape into later.
2. Codepoints are the source of truth in DOCX text, independent of whichever
   font happens to render them.

`Codepoint(u32)` is a newtype specifically so it cannot be confused with a glyph
id (`u16`).

## Pass 1 — collect

`collect(pages, registry) -> CodepointUsage` walks every `DrawCommand::Text`
and accumulates `BTreeMap<TypefaceId, BTreeSet<Codepoint>>`.

Keyed by `TypefaceId` (Skia's `Typeface::unique_id`), **not** by requested
family name. This matters: a document requesting both `Calibri` (substituted to
Carlito) and `Carlito` directly resolves to one underlying typeface, and their
usage must merge — otherwise the second subset would drop the first's glyphs.

Resolution is memoized on the whole request — the family plus both §17.7.2
toggles — because that is what `registry.resolve` takes. It folds the name,
hashes, and clones a `TypefaceEntry` on every call, and the same request recurs
across a document's 100k+ text commands.

## Pass 2 — apply

`apply(usage, &mut registry) -> SubsetReport` processes each used typeface:

0. **Bail on a variable instance** — see below. The check comes first because a
   subset carved at the font's *default* location would be smaller *and* wrong,
   which would hide the problem behind an apparent success.
1. **Extract** (`extract.rs`) — get subsettable SFNT bytes. `Embedded` origin
   reads from the registry (already deobfuscated per §17.8.1.4); `System` calls
   `Typeface::to_font_data`, which also reports which face of a collection the
   typeface came from. WOFF2 is decompressed via `fontcull`; a **TrueType
   Collection** has its selected face carved out (below); **WOFF1 remains a
   documented capability boundary** — ECMA-376 forbids WOFF in DOCX.
2. **Subset** — `fontcull::subset_font_data_unicode(bytes, unicodes, &[])`.
3. **Splice the `name` table** (`name_splice.rs`) — see below.
4. **Reject non-shrinking output** — if `bytes_after >= bytes_before`, keep the
   original (`UnchangedNoSavings`).
5. **Rebuild** as a Skia `Typeface` via `FontMgr::new_from_data`.
6. **Post-validate shapeability** — see below.
7. **Swap into the registry** via `replace_typeface_by_id`.

A typeface reachable from multiple cache keys is subsetted once; the replace
step covers every key pointing at it. That deduplication is on `TypefaceId`, so
it depends on the registry opening one typeface per *face* rather than per
request — see the face cache in
[font resolution](font-substitution.md#fontregistry--one-owner-per-render).

### Carving a face out of a collection

A TrueType Collection is a table directory per face over one shared pool of
tables: several faces routinely point at the *same* `glyf`, differing only in
`cmap` and `name`. `fontcull` takes a single-face SFNT, so the selected face's
directory is resolved into a standalone font whose tables are copies of the ones
it referenced — copying, not slicing a byte range, because the tables a face
names are scattered through the file.

Which face is selected comes from `TypefaceOrigin::Embedded::collection_index`,
discovered when the font was registered. OOXML cannot express it: `fontTable.xml`
offers four style slots per `w:font/@w:name` and no index.

The assembler is `rebuild_sfnt` in `name_splice.rs` — the same directory,
checksum and `head.checksumAdjustment` code the `name` splice needs, shared
rather than written twice.

### Variable instances cannot be baked

When resolution selects an `fvar` **named instance**, the coordinates are
applied through `Typeface::clone_with_arguments`, so measurement and Skia
painting draw the right weight. The bytes available to *embed*, however, are the
font's default location: `fontcull` 2.0.1 exposes no instancing API — its
`klippa` still carries `TODO: instancing` — and neither `to_font_data` nor a
table copy can bake a design-space location into `glyf`/`gvar`.

A conforming PDF viewer therefore draws the default instance's outlines against
advances taken from the instanced font. That is the one outcome where the PDF is
*visually* wrong rather than merely larger than it could be, so it gets its own
variant and its own `log::warn!` naming the axes that were dropped.

Selection avoids it wherever it can: where a static face fits a request as well
as an instance does, ranking prefers the static one precisely because it survives
the round trip. `VariableInstanceNotBaked` is what remains when a family ships
*only* as a variable font.

Baking the location is issue #113.

### The `name` table splice

`fontcull` (via `klippa`) drops every record from the `name` table — its public
API hard-codes an empty `name_ids` set. The subset then has no family, full, or
PostScript name, so Skia falls back to a synthetic `font<hex>` identifier and
the PDF embeds *that*.

`splice_original_name` copies the original `name` table back in. This is safe
because the `name` table stores font metadata, not per-glyph data — it is
independent of glyph order and cannot affect shaping. Skia still adds the
standard `ABCDEF+` subset prefix at PDF write time (ISO 32000-1 §9.6.4).

The splice is best-effort: on failure the unnamed subset is used rather than
aborting.

### Shapeability post-validation

A structurally valid SFNT can still ship a broken `cmap`. Observed with macOS
Apple-vendored fonts (Helvetica Neue, Arial Unicode MS), where klippa's cmap
reconstruction silently drops mappings — Skia accepts the bytes, but every
glyph shapes to `.notdef` and the PDF renders blank text.

`check_shapeability` therefore compares the rebuilt typeface **against the
original**, per codepoint via `Typeface::unichar_to_glyph`, and rejects the
subset only where coverage was *lost*: the codepoint shaped before and is
`.notdef` after.

**The baseline is the whole point.** Asking only "does this codepoint shape in
the subset?" cannot distinguish a subsetter that destroyed the cmap from a font
that never had the glyph — and the second is ordinary. Measured before the
baseline existed: a single U+25AA bullet falsely rejected
`sample-docx-files-sample1`, and one SOFT HYPHEN took a document from 13 KB to
272 KB because the full 606 KB face was embedded instead of its subset. Both now
subset correctly, while the two genuine cmap failures in the corpus
(`sample2`, `sample-emoji`, `0/N → N/N`) are still rejected.

There is deliberately **no whitespace or control exemption**. The old code
skipped those because their `.notdef`-ness is font-dependent — which is true of
every codepoint, and is exactly what the baseline handles. The exemption also
hid real regressions: a subset that dropped a space the original had went
uncaught.

**This validation is the single most important invariant in the pass.** Without
it, subsetting silently destroys text on affected hosts. The regression tests are
`apply_never_installs_unshapeable_subset` and
`losing_coverage_the_original_had_is_a_regression`.

## Failure handling

Every terminal state is an explicit `SubsetOutcome` variant — nothing is
silently swallowed:

| Variant | Meaning |
|---|---|
| `Subsetted` | Strictly smaller bytes; new typeface installed |
| `UnchangedNoSavings` | Nothing to drop — every glyph is referenced |
| `UnsupportedFormat` | WOFF1 — capability boundary |
| `NoBytesAvailable` | `to_font_data` returned `None` for a system font |
| `SubsetterError` | `fontcull` rejected the input |
| `SkiaRebuildFailed` | Skia would not build a typeface from the output |
| `UnshapeableSubset` | Rebuilt, but coverage the original had was lost |
| `VariableInstanceNotBaked` | A variable instance; the embedded bytes are the default location |

The pass is **best-effort per typeface**: a failure leaves that entry's original
in place (PDF gets bigger, text still renders) while other typefaces keep their
savings. Add a state → add a variant.

`SubsetReport::Display` emits the one-line `log::info!` summary.

## Interaction with the font registry

The subset pass **mutates `FontRegistry` in place** after layout. This is why
the typeface cache is per-render and owned by the registry rather than
`thread_local!` — a cross-render cache would leak subsetted typefaces into
later renders, where their glyph coverage is wrong. See
[Font Substitution](font-substitution.md).

Ordering is load-bearing: layout must measure against the **full** typeface,
because subsetting is derived from what layout produced. Subsetting before
layout would be circular.

## Spec references

- **ECMA-376 §17.8** — DOCX font embedding.
- **ECMA-376 §17.8.1.4** — embedded-font obfuscation; deobfuscation happens
  upstream in the parser, so this pass sees plain SFNT bytes.
- **ISO 32000-1 §9.6.4** — PDF subset name prefixes (`ABCDEF+`), emitted by
  Skia's PDF backend, not by us.
- **OpenType** — offset-table signatures used by `format.rs` for detection.
