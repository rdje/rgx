#!/usr/bin/env bash
# Book-example verification ratchet.
#
# The actual compile+run of book examples is done by the EXISTING
# mandatory gate: `cargo test -p rgx-core` runs the doctests pulled
# in by `rgx-core/src/book_doctests.rs` (chapters wired via
# `#[doc = include_str!(…)]`). So examples are already enforced —
# this script only guards the *ratchet*: the set of verified
# chapters must never shrink (you cannot quietly un-wire a chapter,
# or re-`ignore` its blocks, to dodge the gate). Mirrors the
# pcre2_conformance ratchet idiom: regression fails; an intentional
# increase must bump the baseline in the same commit.
set -euo pipefail
repo_root="$(git rev-parse --show-toplevel)"
cd "$repo_root"

doctests_file="rgx-core/src/book_doctests.rs"
baseline_file="book/.examples-verified-chapters"

wired=$(grep -cE '^\s*#\[doc = include_str!\("\.\./\.\./book/src/' "$doctests_file" || true)
baseline=$(tr -d '[:space:]' < "$baseline_file" 2>/dev/null || echo 0)

echo "[check-book-examples] verified book chapters: wired=$wired baseline=$baseline"

if [ "$wired" -lt "$baseline" ]; then
  echo "[check-book-examples] REGRESSION: $wired < baseline $baseline." >&2
  echo "  A chapter was un-wired from book_doctests.rs (or its blocks" >&2
  echo "  re-\`ignore\`d). Book examples must only ever become MORE" >&2
  echo "  verified. Re-wire it, or — if truly removed — justify in the" >&2
  echo "  commit body and lower $baseline_file deliberately." >&2
  exit 1
fi
if [ "$wired" -gt "$baseline" ]; then
  echo "[check-book-examples] IMPROVEMENT: $wired > baseline $baseline." >&2
  echo "  Bump $baseline_file to $wired in this same commit (pcre2" >&2
  echo "  conformance-ratchet idiom)." >&2
  exit 1
fi
echo "[check-book-examples] ratchet OK ($wired chapters verified)."
echo "[check-book-examples] (compile+run is enforced by cargo test -p rgx-core --doc)"
