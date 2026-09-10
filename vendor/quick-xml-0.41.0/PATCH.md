# dxpdf patch: namespace-qualified field matching

This is a vendored copy of [quick-xml 0.41.0](https://crates.io/crates/quick-xml/0.41.0)
(MIT-licensed; see `LICENSE-MIT.md`), patched for `dxpdf` and pinned in the
workspace root's `Cargo.toml` via:

```toml
[patch.crates-io]
quick-xml = { path = "vendor/quick-xml-0.41.0" }
```

## Why

`quick_xml::de`'s serde support matches a struct field against an XML
element by **local name only** — the part after `:` — discarding the
namespace entirely (`de::key::QNameDeserializer::from_elem` /
`de::map::not_in`, both keyed off `BytesStart::local_name()`). That's fine
for the overwhelming majority of `dxpdf`'s `XxxXml` schemas, where every
child genuinely lives in one namespace (`w:`) and the producer's own choice
of prefix for it is irrelevant. It breaks the moment two elements from
**different** namespaces share a local name and a struct wants to
distinguish them.

That happened for real:
[`test-cases/joern.hendrich@vdwbayern.de.docx`](../../test-cases) carries a
numbering level's `<w:rPr>` with both the ordinary `<w:shadow w:val="0"/>`
boolean toggle (§17.3.2.37) and, non-adjacent to it, a Word-2010
`<w14:shadow>` — a WordArt-style text-effect extension `dxpdf` doesn't model
at the run level. Both matched `RPrXml`'s `shadow` field by local name, and
because they weren't adjacent, quick-xml reported the second as a duplicate
rather than folding it into the `Vec` — the whole document failed to parse.

We looked for a way to solve this **without** patching quick-xml first:

- A custom `Deserialize` for the colliding struct only ever sees a generic
  `D: serde::Deserializer<'de>` (it's reached through the parent's ordinary
  derive composition) — no raw XML text, no reader, nothing to build a
  namespace-aware view from. Confirmed for both quick-xml 0.41 and the
  latest 0.42 (which added `Deserializer::resolver()`/`resolver_mut()` —
  inherent methods on the *concrete* `Deserializer<'de, R, E>` struct, so
  unreachable from generic composed code either way).
- quick-xml's own `$value`/enum catch-all, empirically checked, *also*
  collapses to local name only — no escape hatch there either.
- quick-xml has no built-in way to capture a child element's raw XML text
  for a later, separate top-level parse that could hold the concrete
  `Deserializer` and use its namespace resolver.

So within `quick_xml::de`'s "one document-wide `from_str`/`from_reader`
call, composed via derive" architecture, there is no way to make any nested
type's parsing namespace-aware without changing something inside
`quick_xml::de` itself — this patch is not a workaround chosen for
convenience, it's the only point where a namespace check can happen at all.

## What changed

Both `SliceReader` (backing `Deserializer::from_str`) and `IoReader`
(backing `Deserializer::from_reader`) already wrap a real `NsReader`
internally — used today only for `xsi:nil` resolution (`has_nil_attr`). The
patch:

1. **`src/de/mod.rs`** — adds `resolve_element_namespace` to the `XmlRead`
   trait, implemented for both readers by delegating to the same
   `NsReader::resolver()` that `has_nil_attr` already uses, now via
   `resolve_element`.
2. **`src/de/map.rs`** — in `ElementMapAccess::next_key_seed`, before
   falling through to the ordinary local-name-only
   `QNameDeserializer::from_elem`, resolves the current element's
   namespace-qualified name in Clark notation (`{uri}local`) and checks
   whether it's one of the struct's own declared field names. If so, that
   qualified string is handed to serde as the key instead of the bare local
   name — so a field opts in to namespace-qualified matching purely by
   writing its `#[serde(rename = "...")]` in Clark notation.

Every other field, in every other `XxxXml` schema in this crate, is
completely unaffected: its rename is a plain local name, which the
qualified string is never equal to, so `next_key_seed` falls through to the
exact upstream behavior for it. `dxpdf`'s only use of this today is
`RPrXml::w14_shadow` (`src/docx/parse/properties/schema/run.rs`):

```rust
#[serde(
    rename = "{http://schemas.microsoft.com/office/word/2010/wordml}shadow",
    default
)]
w14_shadow: Vec<serde::de::IgnoredAny>,
```

Matching is by **resolved namespace URI**, not by literal prefix text — a
producer binding the Word-2010 namespace to some prefix other than the
conventional `w14` still routes correctly (pinned by
`w14_shadow_extension_matches_by_namespace_not_by_prefix_spelling` in
`run.rs`'s own test module). This is a real capability upgrade over prefix
string-matching (which is what `crate::docx::parse::body::mc_requires`
does, for comparison, for `mc:Choice`'s `Requires` list — a narrower,
already-accepted simplification elsewhere in this codebase that this patch
does not need to make).

## What this patch does *not* fix

`cargo publish` strips `[patch]` sections entirely — a published crate
always resolves its dependencies from the real registry, never from a local
or git-path patch. Concretely:

- **Covered**: every build rooted at *this* repository's own `Cargo.toml` —
  the `dxpdf` CLI binary, `cargo test`, CI, the Python wheels (built by CI
  running inside this repo), the `.deb` package, and the Go bindings'
  `staticlib` (`cargo build --features capi`).
- **Not covered**: `cargo install dxpdf` (installs the published,
  patch-stripped package) and, more importantly, anyone depending on
  `dxpdf = "…"` as a **Rust library** from their own `Cargo.toml` — their
  build resolves the real, unpatched quick-xml 0.41.0, `w14:shadow` goes
  back to colliding with the bare `shadow` field, and this exact document
  fails to parse for them too, with no indication anything here was ever
  patched.

This gap was weighed and accepted, not missed — see the project's own
review history for `w14_shadow` (`git log -- vendor/,
src/docx/parse/properties/schema/run.rs`) for the alternatives considered
(a general or narrow preprocessing elision pass, a try-then-fallback hybrid)
and why this patch was chosen as the operative fix over them despite the
gap.

## Maintaining this fork

This is pinned to exactly 0.41.0 and does **not** track upstream releases
automatically. Upgrading `quick-xml`'s version here means re-applying this
same patch (the diff is small and self-contained — `git diff` against a
fresh copy of the target version's `src/de/mod.rs` and `src/de/map.rs`
should make the rebase mechanical) or upstreaming it as a real PR against
[quick-xml#218](https://github.com/tafia/quick-xml/issues/218) — an already
open, unaddressed feature request for exactly this capability — after which
this vendored copy could be dropped in favor of a stock upstream release.

`tests` and `benches` were removed from this vendored copy (never part of
the crates.io package's `include` list anyway — irrelevant to building
`dxpdf`, which only uses this as a build dependency), so don't expect
`cargo test -p quick-xml` to do anything useful from inside this directory.
`cargo check --features serialize` (or `clippy`) does work and is how this
patch itself was verified in isolation before being wired into the main
project.
