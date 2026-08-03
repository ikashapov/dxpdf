# DrawingML Shape Geometry — §20.1.9

DrawingML shapes are not stored as paths. A `<a:prstGeom>` names one of ~200
preset shapes, and a `<a:custGeom>` stores path commands whose coordinates are
*expressions* evaluated against the shape's own size. Both must be resolved
into concrete drawable paths before painting.

Source: `src/render/resolve/shape_geometry/`. This runs in the **resolve**
phase — geometry depends only on the shape's extent, not on text layout.

## Pipeline

```
parse           →  ShapeGeometry (Preset | Custom)
build_geometry(&geom, extent)
   ├─ Preset → presets::build_preset   → ShapePath
   └─ Custom → custom::build_custom    → ShapePath
layout          →  places the ShapePath in the document
painter         →  PathVerb → Skia path ops
```

`build_geometry` returns `None` when the preset has no generator registered, or
when the extent is zero in **both** dimensions. It deliberately does not reject
a single zero axis: lines are commonly authored as `cx=0, cy=N` (vertical) or
`cx=N, cy=0` (horizontal), and both must render.

## Coordinate and angle conventions

All `ShapePath` coordinates are **shape-local points** — origin at the shape's
top-left, positive x right, positive y down.

Angles stay in OOXML's native unit: **60000ths of a degree**, 0° pointing right,
positive swing clockwise (§20.1.10.3). They are deliberately *not* converted at
build time, because Skia's arc operations use the same clockwise-from-3-o'clock
convention — the painter applies them directly, and no call site needs a unit
conversion. `custom.rs` likewise does not scale angles by the size transform.

## Types

- **`ShapePath`** — `Vec<SubPath>` plus an optional `text_rect` for `<wps:txbx>`
  body layout. Presets may leave `text_rect` as `None`; `custGeom` copies the
  `<a:rect>` child when present.
- **`SubPath`** — verbs, §20.1.10.45 path-level `fill_mode`, and whether the
  path takes the shape's outline stroke. A preset typically yields one subpath;
  `custGeom` preserves the source `<a:path>` grouping.
- **`PathVerb`** — `MoveTo`, `LineTo`, `QuadTo`, `CubicTo`, `ArcTo`, `Close`.
  `ArcTo`'s start point is implicit (the prior path cursor) and it does **not**
  implicitly line-to its start.

## Guide expressions (`guides.rs`, §20.1.9.11)

The heart of the subsystem. A guide formula is a single operator followed by
one to three operands, each a decimal literal, a reference to another guide, or
a spec-defined named constant. All **17 operators** in `ST_GeomGuideFormula`
are implemented.

Evaluation is pure: given a `GuideContext` and a slice of `GeomGuide`s in
document order, `evaluate_guides` returns a `GuideValues` map keyed by name.
Document order matters — a guide may reference any guide defined before it.

`GuideContext` is built from just `(w, h)`; every named constant derives from
those two values:

| Constants | Meaning |
|---|---|
| `w`, `h` | Path extent |
| `ss`, `ls` | Shortest / longest side |
| `hc`, `vc` | Horizontal / vertical centre |
| `t`, `b`, `l`, `r` | Edges |
| `wd2`…`wd32`, `hd2`…`hd32` | Width / height divisions |
| `cd2`, `cd4`, `cd8` | 360°/N angle divisions, in 60000ths of a degree |
| `3cd4`, `3cd8`, `5cd8`, `7cd8` | Compound angle constants |

Operands and results are `f64`; dimensions are in the path's local EMU space.

> **Constant-set gap (Tier-0):** the built-in table currently covers the
> `wd2…wd32` / `hd2…hd32` power-of-two divisions but **not** the spec's
> shortest-side family (`ssd2`, `ssd4`, `ssd6`, `ssd8`, `ssd16`, `ssd32`) or the
> odd divisions (`wd3`/`wd5`/`wd6`/`wd10`/`wd12`, `hd3`/`hd5`/`hd6`) that Tier-1+
> preset `gdLst` formulas rely on. Unknown constants resolve to `0.0`. Since only
> `line`/`rect` presets render today this is latent; expand the table alongside
> the Tier-1 preset generators. A `custGeom` that references these directly
> (uncommon — most declare their own guides) is the one live case.

## Custom geometry (`custom.rs`, §20.1.9.8)

Each `<a:path>` declares its **own** coordinate space via `w`/`h`, so:

1. Build a `GuideContext` from the path's `w`/`h` (EMU as `f64`). Per §20.1.9.8,
   both shape-level and path-level guides evaluate against this context.
2. Evaluate `av_list` (adjust values), then `gd_list`, in document order.
3. Walk the `PathCommand` list, resolving each `AdjPoint` / `AdjCoord` /
   `AdjAngle` to a concrete path-local `f64`.
4. Scale from path-local units into the shape's `extent` in Pt, emitting
   `PathVerb`s.

## Preset coverage — tiered

`build_preset` dispatches by `PresetShapeType`. Unimplemented presets log a
warning and return `None`; the call site falls back to the shape's bounding box
or skips the shape. **Nothing panics on an unknown preset.**

Current state is **Tier 0: `line` and `rect` only** — the minimum needed to
validate the pipeline end-to-end. The intended progression is Tier 1 (~20 common
shapes), Tier 2 (~60 more), Tier 3 (the spec's full ~200).

Adding a preset is a self-contained change: a pure `PtSize → ShapePath`
function in `presets/`, plus one match arm in `build_preset`.

## Spec references

- **§20.1.9.8** `CT_CustomGeometry2D` — `custGeom` structure and guide scoping.
- **§20.1.9.11** `CT_GeomGuide` / `ST_GeomGuideFormula` — the 18 operators.
- **§20.1.9.18** `ST_ShapeType` — the preset catalog.
- **§20.1.10.3** `ST_PositiveFixedAngle` — the 60000ths-of-a-degree unit.
- **§20.1.10.45** `ST_PathFillMode` — per-subpath fill mode.
