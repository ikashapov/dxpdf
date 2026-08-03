# Tabs — §17.3.1.30 position tabs, §17.18.85 `bar` and `decimal` stops

## Absolute Position Tabs — §17.3.1.30

### Element

```xml
<w:r>
  <w:ptab w:relativeTo="margin" w:alignment="center" w:leader="none"/>
</w:r>
```

Unlike a regular tab (§17.3.3.29, `<w:tab/>`), a position tab (`<w:ptab>`, `CT_PTab`)
carries **no `pos` attribute**. Its target position is *derived* from `relativeTo`
and `alignment` at layout time. Its canonical use is the Word three-region
header/footer:

```
[left text] <ptab center margin> [center text] <ptab right margin> [right text]
```

### Attributes

Each is a distinct ST enum (all required by the schema; we default leniently so a
malformed run does not abort the whole parse — see `body_schema::PTabXml`).

| Attribute | Enum (§) | Values | Default |
|-----------|----------|--------|---------|
| `alignment` | ST_PTabAlignment (§17.18.59) | `left` `center` `right` | `left` |
| `relativeTo` | ST_PTabRelativeTo (§17.18.61) | `margin` `indent` | `indent` |
| `leader` | ST_PTabLeader (§17.18.60) | `none` `dot` `hyphen` `underscore` `middleDot` | `none` |

`ST_PTabLeader` is a strict subset of the tab-stop leader enum `ST_TabTlc`
(§17.18.86) — it lacks `heavy`. The model keeps a dedicated `PTabLeader` enum for
spec fidelity, with `From<PTabLeader> for TabLeader` so layout reuses the existing
tab-leader painter (`emit_tab_leader`).

### Positioning

Position tabs are resolved during line emission (`paragraph/line_emit.rs`), in the
paragraph's x-space where `x = 0` is the constraint's left edge (the page/cell text
margin) and `content_width` is **already net of paragraph indents**.

1. **Reference span** from `relativeTo`:
   - `margin` → `[0, max_width]` (full text area between page margins; ignores
     indents, and ignores floats — a float does not move a page margin)
   - `indent` → the indented region **as it is on this line**:
     `[indent_left + float_left, indent_left + content_width − float_right]`.
     A float overlapping the line narrows that region, and the span follows it;
     using the paragraph-level width instead right-aligned a zone on top of the
     float. Exact Word behaviour here is unconfirmed — the line-local reading is
     chosen because it is the only one that cannot overlap the float.
2. **Anchor** `P` from `alignment`: `left` → span start, `center` → midpoint,
   `right` → span end.
3. **Zone alignment** — the content from this ptab up to the next tab/ptab or
   line end (the "zone", width `w`) is placed so that its anchor point lands on
   `P`: `left` → zone start at `P`, `center` → `P − w/2`, `right` → `P − w`.

This is the same right/center zone math the regular tab-stop path uses; only the
source of `P` differs (derived, not from a `<w:tabs>` stop). Leader characters
fill the gap `[x, P]`.

**When the anchor is already behind the pen (§17.3.1.30).** The result is a
`PTabPlacement`, not a coordinate: `Placed(x)` when the zone fits at or after
the pen, `AdvancesToNextLine` when it does not. The spec advances the tab to its
alignment point on the *next* line, so the decision belongs to line **fitting** —
emission cannot create a line break. `fit_lines_with_first` therefore tracks a
second pen (`pen_x`) that follows ptab jumps, alongside the width sum it already
accumulates; the width sum, and so every paragraph without a ptab, is untouched.

Two consequences worth knowing:

- A tab that is **already first on its line** places instead of advancing.
  Advancing could not bring the anchor any closer, and acting on a condition the
  action cannot change is how this engine's pagination bugs have historically
  become infinite loops. In that position the floor is the line's own left edge,
  so a `relativeTo="margin"` tab correctly *outdents* past `indent_left` — which
  is what makes `left` + `margin` reach the page margin rather than being the
  silent no-op the old `0.max(x)` produced.
- Fitting bounds a zone by the next tab or line break in the whole fragment
  list, since lines do not exist yet; emission bounds it by the line. Where they
  differ — a zone fitting then splits for width — emission is authoritative for
  the final x, and fitting only decided whether the tab could be honoured here.
- Fitting seeds `pen_x` at the line's own start — `indent_left`, plus the
  first-line indent on line 0 — because that is where emission starts. Seeding
  it at zero instead makes the two classify the same tab differently: fitting
  sees the anchor ahead of its pen and declines to break, emission sees it
  behind and, unable to break, clamps.
- Once a `relativeTo="margin"` tab has placed content, the line's right edge for
  fitting becomes the **margin**, not `content_width`, and the test runs against
  the pen rather than the accumulated width sum. Without that, a paragraph with a
  right indent wrapped a line whose content fits the margin region, stranding a
  leader on the short line. (`relativeTo="indent"` needs no such widening: its
  span ends at `indent_left + content_width`, so its zone cannot reach past the
  indented area.)

**Zone anchoring (§17.18.85).** A regular tab stop positions its *zone* — the
content from the tab to the next tab or the line end. Which point of the zone
lands on the stop is a `ZoneAnchor` (`line_emit.rs`): `Start` for `left` (and
for `bar`/`clear`, neither of which repositions content), `End` for `right`,
`Middle` for `center`, and `At(offset)` for `decimal`. `decimal` is why this is
an enum rather than a fraction of the zone width — its anchor is a property of
the zone's *text*, being the offset of the first decimal separator.

A decimal zone with **no** separator anchors at `End`, i.e. right-aligns. That
is what keeps a column of figures flush when one entry is a whole number;
left-aligning it would make that entry stick out by its full width. Which
character counts as the separator is the paragraph's language, not a constant —
see "Which separator, and whose language" below.

**Leader formatting (§17.3.1.38).** A leader carries no formatting of its own —
it is drawn in the formatting in effect at the tab, i.e. the `<w:rPr>` of the
run holding the `<w:tab/>` or `<w:ptab/>`. Both tab fragments therefore carry
their run's `FontProps` and colour, and `emit_tab_leader` measures *and* draws
with them. Two consequences worth knowing: the leader repeat count depends on
the run's glyph advance, so it changes with the font; and a document whose
leaders are its only use of some family no longer drags that family into the
PDF. (Leaders were previously hardcoded to 12pt Times New Roman in black,
which embedded Times New Roman in documents that never referenced it.)
Character spacing and text scaling are deliberately *not* applied — they are
run effects on text, not on a fill pattern.

Worked example — three-region header, `max_width = 100`, no indents:

| Step | Anchor `P` | Zone width | New `x` |
|------|-----------|-----------|---------|
| left text `L` | — | — | 0 |
| `ptab center margin`, then `C` | 50 | 20 | 40 (`50 − 20/2`) |
| `ptab right margin`, then `R` | 100 | 30 | 70 (`100 − 30`) |

→ `C` centered on the page center, `R` ending at the right margin.

### Interaction with paragraph alignment

Like a line that contains regular tabs (§17.3.1.37 rationale), a line containing a
position tab is placed explicitly by the tab, so paragraph alignment
(`center`/`end`/`both`) is suppressed for that line (`is_tab_like` gate in
`line_emit.rs`).

### Limitations

A ptab contributes only a nominal 1pt to the accumulated line *width* (as a
regular tab does), but fitting also simulates the **pen** through ptab jumps —
which is what lets it decide the §17.3.1.30 next-line advance and honour the
margin span. This is correct for
the single-line header/footer case where ptabs are overwhelmingly used. Paragraph
`relativeTo="margin"` uses the constraint box as the "margin" reference, which is
the page text margin for body/header/footer paragraphs.

## Bar tab stops — §17.18.85 `ST_TabJc` = `bar`

```xml
<w:pPr><w:tabs><w:tab w:val="bar" w:pos="4320"/></w:tabs></w:pPr>
```

**A `bar` entry in `w:tabs` is not a tab stop.** It names a column of the
paragraph where a vertical rule is drawn, and a tab character passes straight
over it to the next real stop. Word's tab dialog says the same thing: a bar tab
does not position text. Both halves follow from that one fact, and this renderer
had neither — `bar` shared an arm with `left`, so a tab landed on it *and* no
rule was ever drawn.

`TabStopRole` (`line_emit.rs`) is the one place the distinction lives, total over
`TabAlignment` so a new alignment has to state its role rather than inherit
`left`'s:

| Role | Alignments | Consumed by |
|---|---|---|
| `PositionsContent` | `left`, `center`, `right`, `decimal`, `clear` | `find_next_tab_stop` → `ZoneAnchor` |
| `DrawsRule` | `bar` | `emit_bar_rules`, once per line |

`clear` is a positioning stop only nominally: §17.3.1.38 deletes it during style
merge, so one reaching layout is inert. Leaving it where it was keeps its
long-standing `left` placement; giving it a rule would invent a mark the
document never asked for.

### Where the rule is drawn

Not from the `Fragment::Tab` arm — a bar draws on lines holding no tab, and on
paragraphs holding none at all, including empty ones. `emit_line_commands`
therefore emits it per *line*, before that line's content so text wins wherever
a glyph and the rule overlap.

The x is the stop's own `w:pos` and owes nothing to the line: not its content
extent, not its alignment offset, not its float indent. The vertical span is the
line's band (`cursor_y` to `cursor_y + line_height`), so successive lines' rules
abut and read as one continuous rule down the paragraph — including across a
page split, where each page's lines are emitted by a separate call.

### Colour and weight, neither of which the file supplies

`w:tab` has no attribute for either.

**Colour** follows §17.3.1.38's rule for tab leaders — a decoration has no
formatting of its own, it takes the formatting in effect — but a bar rule has no
owning run to read it from. Word keys it off the paragraph mark's run
properties, which do not reach layout; the paragraph's **first run** is the
closest thing that does. Taking it from the paragraph rather than from each line
is what keeps one rule one colour when the paragraph wraps or splits across
pages. Falls back to black for a paragraph with no text at all.

**Weight** is 0.75pt — a hairline, and the same default this renderer already
applies to an unspecified DrawingML outline, so the two agree instead of each
inventing a number.

Both are approximations of a value Word derives from data this layer does not
have; they are recorded here rather than in a TODO because there is nothing
further to implement without threading paragraph-mark run properties into
`ParagraphStyle`.

## Decimal tab stops — §17.18.85 `ST_TabJc` = `decimal`

```xml
<w:pPr><w:tabs><w:tab w:val="decimal" w:pos="4320"/></w:tabs></w:pPr>
```

A `decimal` stop anchors its zone on the zone's **decimal separator**, so a
column of figures lines up on its decimal points rather than on either edge.
`decimal_anchor` (`line_emit.rs`) scans the zone's text fragments for the first
occurrence and returns a `ZoneAnchor::At` offset; a zone with no separator falls
through to `ZoneAnchor::End`, i.e. right-aligned, which keeps a whole number
flush with the column instead of stranding it left.

### Which separator, and whose language

The separator is a property of the *language*, and §17.3.2.20 `w:lang` is where
a document states one. It resolves through
`build::convert::paragraph_locale`, whose cascade is:

1. the paragraph **mark**'s `w:rPr` (§17.3.1.29),
2. the paragraph style's run defaults,
3. `docDefaults`,

— §17.7.2's order for any other run property, **minus the runs themselves**.

Leaving the runs out is the one judgement call here, and it is deliberate. A
decimal stop is declared in `w:pPr/w:tabs`; its zone may span several runs that
each declare a different `w:lang`, and there is no principled way to pick one of
them. The paragraph mark is the nearest thing the file offers to "the language
of this paragraph", and it is already what a §17.9.22 list label reads its
formatting from, so the two agree rather than each inventing a rule.

**Unverified against Word, and recorded as such.** Word is reported to take a
decimal tab's separator from the *host's regional settings* rather than from the
document at all — which is why a Word file can align differently on two
machines. A converter cannot reproduce that and should not want to: the same
input must produce the same output. Keying it to the document is the
deterministic reading. If a Word reference render ever settles the question
differently, the cascade above is the one place to change.

The separator itself is a property of the document's language; how that is
resolved, and everything else this engine does or does not do with `w:lang`, is
in [Internationalisation](i18n.md).
