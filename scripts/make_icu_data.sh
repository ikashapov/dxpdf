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
#
# icu_datetime (#129): §17.16.4.2 date-picture month and weekday names.
# Month-name data is per calendar system (there is a separate marker for
# Buddhist, Chinese, Hebrew, …); OOXML DATE fields are Gregorian, so only the
# Gregorian one is baked. Weekday names are calendar-agnostic — one marker
# covers every system. Both names are the `data_marker!` identifiers from
# icu_datetime-*/src/provider/names.rs (`WeekdayNamesV1` there is an alias of
# DatetimeNamesWeekdayV1; datagen wants the canonical name).
#
# These two markers are the bulk of this blob: adding them took it from 7.7 KB
# to 189 KB, and the wheel from 11,688,534 to 11,845,682 bytes (+1.34%).
# `--attribute-filter datetime_month_length=/^(3s|5s)$/` (and the weekday
# equivalent) would drop the name lengths src/i18n/mod.rs never asks for and
# bring the blob to 160 KB — measured, and deliberately *not* done: 29 KB is
# 0.24% of the wheel, and the filter would have to be kept in step with the
# exact `MonthNameLength`/`WeekdayNameLength` constants that module picks.
# Those constants encode the stand-alone-vs-format choice its doc flags as
# unverified against Word, so they are the likeliest thing here to change —
# and a filter left stale behind such a change removes the data silently,
# degrading every locale to the English fallback.
#
# icu_segmenter (#130): UAX #14 line breaking. All three markers are
# locale-independent — LOCALES above does not apply to them, and dropping a
# locale from that list does not shrink them.
#
#   SegmenterBreakLineV1            the UAX #14 property + pair tables
#   SegmenterBreakGraphemeClusterV1 required by every LineSegmenter
#                                   constructor, complex-script or not
#   SegmenterLstmAutoV1             the four class-SA models (Thai, Lao,
#                                   Khmer, Burmese)
#
# CJK is deliberately absent from that list: UAX #14's own rules break between
# ideographs (class ID), so `SegmenterDictionaryAutoV1`'s `cjdict` buys line
# breaking nothing — icu_segmenter's own `load_lstm_unstable` says so in a
# comment. Measured, since the difference is large: line + grapheme alone is
# 30,140 bytes; adding the four LSTM models makes the blob 529,383 (Thai
# 72,089, Lao 71,921, Khmer 74,428, Burmese 91,132 standalone). Taking the
# *dictionary* route for those same four scripts instead — swapping
# SegmenterLstmAutoV1 for SegmenterDictionaryAutoV1, which also drags in
# cjdict — measured 2,037,269 bytes, 6x the LSTM, and was rejected on that.
# LSTM weights are floats and barely deflate (339,656 -> 299,106), so unlike
# the CLDR name tables above this lands on the wheel at close to full size:
# 11,845,784 -> 12,177,618 bytes, +331,834 (+2.80%), measured before and after
# with `maturin build --release --features python`.
MARKERS="DecimalSymbolsV1 DecimalDigitsV1 DatetimeNamesMonthGregorianV1 DatetimeNamesWeekdayV1 SegmenterBreakLineV1 SegmenterBreakGraphemeClusterV1 SegmenterLstmAutoV1"

icu4x-datagen \
  --locales $LOCALES \
  --markers $MARKERS \
  --format blob \
  --out src/i18n/data/icu_data.blob \
  --overwrite
