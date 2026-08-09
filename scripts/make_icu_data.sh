#!/usr/bin/env bash
# Build src/i18n/data/icu_data.blob, the ICU4X locale data this engine bakes
# into the compiled library (issue #124/#127).
#
# ICU4X's default `compiled_data` Cargo feature bakes in every CLDR locale —
# megabytes landing on the Python wheel, per #124. This script generates a
# trimmed blob instead, loaded at runtime through `icu_provider_blob`
# (`src/i18n/mod.rs`); every `icu_*` leaf crate that ships locale data is
# added to Cargo.toml with `default-features = false` so the trimmed blob is
# the *only* copy of the data, not an addition on top of the default one.
#
# LOCALES is the single source of truth for this engine's baked locale set —
# don't duplicate it elsewhere. The base set is exactly the region tags
# present in test-files/ + test-cases/ today (grepped from the real XML),
# plus `und` (root, needed for ICU4X's locale-fallback chain). #127 left the
# region-divergent locales #124/#128 care about out on purpose, with nothing
# in this repo exercising them yet — that changed with #128: de-CH now has a
# fixture (tests/document_locale.rs) and en-ZA/es-MX are covered by
# `decimal_separator_for_tag_resolves_region_divergence` (src/i18n/mod.rs), so
# baking them explicitly is no longer the speculative addition AGENTS.md
# warns against — it's tested. (it-CH is not: nothing here calls it yet, and
# it happens to resolve correctly via ICU4X's own fallback from this same
# set without being baked at all — verified, not relied on blindly, but left
# out until something needs it explicitly.) Extend LOCALES (and MARKERS, as
# later phases add icu_segmenter/icu_datetime/etc.) here, in one place, when
# a fixture and a real caller justify it.
#
# MARKERS names the data types to bake in, read directly out of each icu_*
# crate's `provider.rs` (`icu_provider::data_marker!` sites) rather than
# derived via `--markers-for-bin`: that flag introspects a compiled binary
# for the markers it actually references, which only works if something
# outside `#[cfg(test)]` calls the API — true from #128 onward, not true for
# #127's proof-of-plumbing test alone.
#
# Usage:
#
#     cargo install icu4x-datagen
#     scripts/make_icu_data.sh
#
# Regenerate and commit the result whenever LOCALES or MARKERS changes. The
# build is deterministic — commit the diff, don't hand-edit the blob.
set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/.."

LOCALES="und ca-ES de-AT de-CH de-DE en-CA en-GB en-US en-ZA es-MX fr-FR it-IT pl-PL ru-RU"

# icu_decimal (#127): DecimalSymbolsV1 (separators/affixes), DecimalDigitsV1
# (per-numbering-system digit glyphs) — icu_decimal-*/src/provider.rs's own
# `MARKERS` const names these as its minimal required set.
MARKERS="DecimalSymbolsV1 DecimalDigitsV1"

icu4x-datagen \
  --locales $LOCALES \
  --markers $MARKERS \
  --format blob \
  --out src/i18n/data/icu_data.blob \
  --overwrite
