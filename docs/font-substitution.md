# Font Resolution & Substitution — §17.8

A DOCX names fonts it does not carry. `Calibri` and `Cambria` are Microsoft
fonts that are absent on most Linux hosts and on many Macs. Resolution turns a
requested **face** into a concrete Skia `Typeface`, preferring fidelity and
degrading predictably.

Source: `src/render/fonts/`.

## What a request is

A request is a name and two **tri-state** toggles, not a name and a
`FontStyle`:

```rust
FaceRequest { name: &str, bold: Toggle, italic: Toggle }
enum Toggle { Absent, Off, On }
```

§17.3.2.1 (`w:b`) and §17.3.2.16 (`w:i`) are toggle properties, and §17.7.2
gives them three states: absent from the entire cascade, explicitly off,
explicitly on. The model has always preserved all three as `Option<bool>`; the
render side used to collapse them to `bool` at one line in
`fragment::font_props_from_run`, two layers above resolution.

That collapse is why resolution needed a special case. With a `bool`, "not
bold" reaches the resolver as *a request for weight 400* — indistinguishable
from a document that genuinely wants Regular. A face name carrying its own
weight (`"Calibri Light"` at 342) was overruled by a default that meant nothing.

With the tri-state, `Absent` asks for no weight at all, so the matched face's
own weight stands; only `On` asks for one:

| Toggle | Weight | Slant |
|---|---|---|
| `Absent` / `Off` | the matched face's own | the matched face's own |
| `On` | `max(face weight, BOLD)` | Italic |

`Absent` and `Off` select the same face today. The distinction is kept because
it is real during the cascade — an explicit `w:val="0"` must override an
inherited `w:b` — and because the one behaviour that would separate them at
selection, *synthetic* emboldening of an already-bold face, is something this
engine does not do and would otherwise have no way to express. See the
`request` module doc.

## `FontRegistry` — one owner per render

`FontRegistry` is the single source of truth for typeface data within one
render. It owns:

- the document's embedded-font bytes (deobfuscated upstream by the parser per
  §17.8.3.3),
- a `FaceCatalog` of everything selectable, host and embedded alike,
- a cache of resolved typefaces keyed by the **request** (`FaceRequestKey`),
- a cache of opened typefaces keyed by the **face**.

The second cache is not an optimisation. `TypefaceId` is how the
[subsetting](font-subsetting.md) pass deduplicates, so building a fresh
typeface per request embeds one font several times over — observed on
`sample-docx-files-sample1.docx`, where `<w:b/>` and `<w:b/><w:i w:val="0"/>`
select the same embedded Ubuntu Bold and the PDF grew a second copy of it.

### Why not a thread-local cache

An earlier implementation cached typefaces in a `thread_local!` map. That is a
**correctness bug**, not a design preference: the [subsetting](font-subsetting.md)
pass mutates typefaces in place after layout. A process-lifetime cache would
leak a subsetted typeface — one whose glyph coverage is limited to *the previous
document* — into the next render, producing missing glyphs that depend on
conversion order.

Per-render ownership makes that leakage impossible by construction. Do not
reintroduce a global typeface cache. The same rule binds the face catalogue
(issue #85, acceptance item 8), even though it holds no font bytes: a catalogue
built from one `FontMgr` is simply wrong for another, and `render_with_font_mgr`
accepts a caller-supplied manager.

## The face catalogue

`FaceCatalog` turns the host font system and the DOCX's embedded fonts into one
list of `FaceRecord`s, so the resolver matches names against records and never
has to know where a face came from.

A `FaceRecord` has two halves. `FaceIdentity` is how to *reopen* the face —
positionally, by family plus face index, or by embedded id plus collection
index. It holds no names, because reopening by `(family, style)` would re-run
the host's matching and could hand back a different face than the one
resolution chose. `Vec<FaceName>` is how to *match* it, and each name carries a
`NameKind` recording how the engine came to know it:

| `NameKind` | Source | Evidence? |
|---|---|---|
| `Table(NameId)` | the font's own `name` table | yes |
| `ManagerFamily` | the family the host reported | no |
| `ManagerFace` | the host's family + style name | no |
| `Instance` | an `fvar` named instance | derived |
| `ComposedStyle` | family + `STAT` axis-value names | derived |

That field is what the resolution chain keys on. Collapsing the two into a flat
alias list — which the previous `FaceAliasIndex` did — is precisely what let a
composed guess outrank a name the font stated outright.

A named instance of a variable font is a `FaceRecord` in its own right, not an
attribute of the variable face, because a document can ask for
`"Inter Display SemiBold"` and get exactly it.

### Three tiers, and what each costs

Measured on a host with 210 families / 677 faces, warm, release build:

| Operation | Cost |
|---|---|
| `FontMgr::family_names()` | 0.04 ms |
| `FontMgr::match_family()` × 210 | **28 ms** |
| `FontStyleSet::style(i)` × 677 | 0.4 ms |
| `FontStyleSet::new_typeface(i)` × 677, style sets retained | 7.6 ms |
| `copy_table_data` × 677 for `name`/`OS/2`/`fvar`/`STAT` | 1.7 ms |
| parsing 32 526 `name` records | 4.6 ms |

One line dominates. `match_family` is the whole cost of enumeration and
everything else is nearly free, which is what the tiering is shaped around:

- **Tier 0**, at construction: `family_names()` only.
- **Tier 1**, per family on first touch: `match_family` for *that* family.
- **Tier 2**, once per render at most: every face instantiated and its own
  tables read. Reached only by a request that missed every cheaper step.

Style sets are retained from tier 1, so tier 2 does not pay `match_family` a
second time — the difference between 7.6 ms and 37 ms for the same 677
`new_typeface` calls.

End to end, against the 35 ms the previous unconditional index cost **every**
render, read off the CLI's `RUST_LOG=debug` `registry:` line:

| Document | Before | After |
|---|---|---|
| `sample-docx-files-sample1` (embeds its fonts; everything resolves by family) | 86 ms | **2.9 ms** |
| `sample-docx-files-sample3` (names fonts the host lacks) | 91 ms | 105 ms |
| `sample-docx-files-sample-4` (likewise) | 91 ms | 105 ms |

So a document whose fonts are all present or embedded is roughly thirty times
cheaper, and one that has to reach the metadata index pays about 14 ms more for
it. This page used to record 78–95 ms as the fixed cost on *every* render; there
is no longer a fixed cost.

## The resolution chain

`resolve(request, catalog)` is a **pure function** — no Skia, no I/O. That is
the design goal, not a side effect: it is what lets the whole chain be tested
against the fonts committed under `test-files/fonts/` and behave identically on
macOS, Linux and Windows (`tests/font_resolution.rs`).

| Step | Matches | Evidence |
|---|---|---|
| 1 | an embedded font's full, compatible-full, PostScript or instance name | the document's own font, by face |
| 2 | an embedded font's family | the document's own font, by family |
| 3 | a host family, exactly | the host, by family |
| 4 | a host face's full, compatible-full or PostScript name | the font's `name` table |
| 5 | any other name the font sanctions — typographic, WWS, localized, a `STAT` style, an `fvar` instance | the font's own tables |
| 6 | a family plus a trailing weight word, parsed | a guess |
| 7 | a metric-compatible substitute | a curated table |
| 8 | the host default | nothing |

Everything down to step 5 is something a font *asserts about itself*. Step 6 is
the first guess, and demoting it is the point of the whole exercise: it used to
run before any metadata was read, so `"PT Sans Narrow"` and `"Foo Medium"` were
chopped into a base family plus a style word even when the host had a family by
exactly that name.

Steps 4 and 5 are what build tier 2. Everything above is cheap.

Step 3 also adopts a family the manager will *match* but did not *enumerate*:
`family_names()` is not guaranteed to list every name `match_family_style`
answers to. The answer is accepted only when the returned face genuinely carries
the requested family name, because the manager is permitted to substitute rather
than decline — fontconfig routinely does, CoreText does not — and without that
check every miss would be swallowed as a hit on Linux and the rest of the chain
would be unreachable.

### Ranking within a family

Steps 2, 3, 6 and 7 match a *family*, which every face in it carries, so the
faces are ranked. The score is lexicographic — each field a strict tie-break of
the last: slant, then width distance, then weight distance, then which family
slot the face declares (`OS/2` `fsSelection`), then static-over-instance, then
the candidate's position so an exact tie resolves the same way every run.

Weight distance is asymmetric when the request explicitly asked to be bold: a
document that says `<w:b/>` is better served by something heavier than the
request than by something lighter, so lighter candidates are penalised at double
rate.

A step that matched a *face* re-ranks within that face's family when the
toggles moved the style — which is how `"Calibri Light"` with `<w:b/>` lands on
Calibri Bold rather than returning Light and ignoring the bold.

### Ambiguity is an outcome, not a coin toss

When a name matches several genuinely different faces the chain does not pick
one. It records the ambiguity and continues; if nothing later matches either,
the ambiguity — not a bare "not found" — is what `FallbackReason` reports.

The same font installed twice, or reachable under two family names, is *not* an
ambiguity: every candidate would draw the same glyphs. Only faces that differ in
family, intrinsic style or design-space location are.

### Diagnostics

Every step logs at `debug`, so `RUST_LOG=debug` shows which arm fired for each
request and on what evidence — the fastest way to tell a correct metadata match
from a lucky substitution. A request that reaches step 8, or one abandoned as
ambiguous, additionally logs at `warn`, **once per family**: `preload` resolves
four toggle combinations of every family a document mentions, and without the
deduplication one missing font would produce four identical lines.

> **Read the debug log with care: most of its lines describe fonts nothing
> draws.** `FontRegistry::build` calls `preload`, which resolves all four
> combinations of every family the document mentions, whether or not any run
> uses them. Confirm against the combinations actually emitted as draw commands
> before chasing a decision line. This cost one investigation during the H2
> review.

### `FONT_SUBSTITUTIONS`

Metric-compatible means *same advance widths*, so line breaks and page counts
match Word even though glyph shapes differ. This is why Carlito/Caladea are
used rather than a generic sans/serif.

| Requested | Substitutes, in order |
|---|---|
| Calibri | Carlito, Liberation Sans, Noto Sans |
| Cambria | Caladea, Liberation Serif, Noto Serif |
| Arial | Liberation Sans, Noto Sans, Helvetica |
| Times New Roman | Liberation Serif, Noto Serif, Times |
| Courier New | Liberation Mono, Noto Sans Mono, Courier |
| Verdana | DejaVu Sans, Noto Sans |
| Georgia | DejaVu Serif, Noto Serif |
| Trebuchet MS | Ubuntu, Noto Sans |
| Consolas | Inconsolata, Liberation Mono, Noto Sans Mono |
| Segoe UI | Noto Sans, Liberation Sans |

The table is keyed by *family*, so a face-qualified name reaches it through its
base family: `"Segoe UI Light"` → strip the trailing weight word → `"Segoe UI"`.
The parsed weight carries over, so the substitute is asked for Light rather than
Regular.

Substitution is a *host-dependent* step: the same DOCX can paginate differently
on macOS and Linux depending on what is installed. When cross-platform output
differs, check this chain first.

## Reading the font's own tables

`src/render/fonts/opentype/` holds hand-written readers for `name`, `OS/2`,
`fvar` and `STAT`. Three constraints shape them:

- **Pure over bytes.** Nothing takes a Skia type, which is what makes the
  catalogue testable identically on every host.
- **One table at a time**, via `Typeface::copy_table_data(tag)` — never
  `to_font_data()`, which costs ~549 MB of unreleasable RSS on the 183 MB system
  emoji font and would be paid per face.
- **Never panic on hostile input.** Fonts arrive from documents; every offset is
  bounds-checked and every entry point returns a typed error. Shipping fonts are
  routinely a little malformed, so that is the common path, not the exception.

There is no font-parsing dependency. The decisive reason is
`--no-default-features`: `fontcull` is compiled out there, and the metadata
index still has to work.

## The two narrow resolvers

`resolve` always returns something. Two callers must not accept a fallback:

- **`resolve_exact`** — accepts only the evidence-backed steps (1–5). Used by
  the emoji pipeline, where substituting a non-emoji typeface for a missing
  color-emoji font is never correct; better to fail over to the monochrome path.
- **`resolve_system_only`** — bypasses the embedded index entirely. Word's font
  subsetter strips color glyph tables (`sbix`/`CBDT`/`COLR`/`SVG`) when it
  embeds an emoji font, so a DOCX-embedded "Segoe UI Emoji" carries the right
  family name but **no color glyphs**. It must not satisfy emoji resolution.

  This is a deliberate portability boundary, not an oversight: a document that
  ships its own emoji font does not get it back, and the same document renders
  with different emoji artwork on macOS, Windows and Linux. It applies only to
  the color-emoji path — ordinary embedded text fonts (§17.8) are honoured
  normally. See `src/render/emoji/resolve.rs`.

## Collections and variable fonts

**TrueType Collections.** OOXML cannot say which face of a collection a
`w:font` means — `fontTable.xml` offers four style slots per `w:font/@w:name`
and no index — so the index is discovered by reading the bytes, and travels on
`TypefaceOrigin::Embedded::collection_index`. How many faces are *reachable* is
platform-dependent: Skia's CoreText backend declines a non-zero collection index
outright, so a two-face collection contributes one face on macOS and both on a
backend that honours the index. Subsetting does not share the limitation — it
carves a face out of the collection's own bytes.

**Variable fonts.** `fvar` named instances and `STAT` axis-value combinations
each become their own `FaceRecord`, so `"Dx Variable Condensed SemiBold"` is
*read* rather than parsed. A composed name is attached only to the face whose
own location it describes; attaching it to whichever record came first made one
face answer to every style its family can take, and made the real named instance
look like an ambiguity.

Selection applies the coordinates through `Typeface::clone_with_arguments`, so
measurement and Skia painting are correct. Baking them into the *embedded* PDF
bytes is a boundary — see `SubsetOutcome::VariableInstanceNotBaked` in
[font subsetting](font-subsetting.md). Where a static face fits a request as
well as an instance does, ranking prefers the static one precisely because it
survives the round trip.

## Test fixtures

`tests/font_resolution.rs` runs against fonts committed under
`test-files/fonts/`, never the host's. That is what makes face selection
equivalent across platforms — a test that asks the host about "Arial" cannot
demonstrate it.

The fixtures are built by `scripts/make_font_fixtures.py` with `fontTools`,
which documents what each one exists to exercise: a family split across the
legacy four-slot model, localized `name` records, a family whose *name* ends in
a style word, a two-face collection, a variable font with `fvar` and `STAT`.
Building them rather than taking them from a foundry keeps the writer
independent of the reader under test, keeps each fixture about one thing, and
leaves the repository with no font licence to carry. Regenerate and commit; the
build is deterministic, so a diff means a real change.

## Spec references

- **ECMA-376 §17.8** — font embedding and the `w:fonts` part.
- **ECMA-376 §17.8.3.3** — embedded-font obfuscation; deobfuscated by the
  parser before the registry ever sees bytes.
- **ECMA-376 §17.3.2.1 / §17.3.2.16** — `w:b` and `w:i` as toggle properties.
- **ECMA-376 §17.7.2** — the property cascade the toggles' three states come
  from.
- **OpenType** — `name`, `OS/2`, `fvar`, `STAT`.
