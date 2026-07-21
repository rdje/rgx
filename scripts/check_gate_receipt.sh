#!/usr/bin/env bash
# GATE-RECEIPT doctrine check (oracle-binding; commit-time scope).
#
# Doctrine (`COMMIT.md` non-negotiable invariants): the mandatory quality gate is
# "green" only when `./scripts/run-local-ci.sh` exited 0 for the EXACT tree being
# committed — proven by a fresh matching receipt, never by eyeballing filtered
# output. A red gate must never be self-reported green.
#
# This is the leg that binds RGX's heavyweight deterministic ORACLES (the full
# rgx-core/cli/bench/wasm/capi suites, the feature matrix, clippy, the capi ABI
# gate, the book-example ratchet, and — with RGX_RUN_CONFORMANCE=1 — the PCRE2
# conformance ratchet) to the commit: `run-local-ci.sh` re-executes them and, only
# on a real green, stamps a receipt keyed to the gate-affecting content hash. A
# fabricated "gate passed" claim does not produce a matching receipt.
#
# Why it exists (`DOCTRINE_ENFORCEMENT.md` §6.1 — a box is EARNED, not ticked):
# from 2026-04-07 to 2026-05-18 the mandatory `cargo test -p rgx-core` step was
# reported green for six weeks while it actually SIGABRT'd — satisfied by
# filtered/`--lib` runs and pipe-masked exit codes. The receipt makes "commit an
# unvalidated or red gate-affecting tree" impossible without an explicit, loud
# `--no-verify`.
#
# Extracted verbatim (2026-07-21, leaf DOCTRINE-ADOPT.1) from the pre-commit hook
# into a registered check so the driver owns it like every other doctrine.
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"; cd "$ROOT"

# shellcheck source=scripts/lib-gate-receipt.sh
. "$ROOT/scripts/lib-gate-receipt.sh"

gate_pathspec=(
  '--' '*.rs' '*/Cargo.toml' 'Cargo.toml' 'Cargo.lock'
  '*/build.rs' '.github/workflows/*' 'scripts/*'
  'rgx-capi/cbindgen.toml' 'rgx-capi/include/*'
  ':(exclude)subs/pgen' ':(exclude)git_message_brief.txt'
)

# Does this commit touch any gate-affecting path?
staged_gate_files="$(git diff --cached --name-only "${gate_pathspec[@]}")"
if [ -z "$staged_gate_files" ]; then
  # Pure docs / pgen-issues / non-gate commit — nothing the local Rust gate
  # could catch. Allow.
  exit 0
fi

receipt="$(rgx_receipt_path)"
if [ ! -f "$receipt" ]; then
  cat >&2 <<'EOF'
✗ GATE-RECEIPT: no green gate receipt.

This commit changes gate-affecting files (Rust / Cargo / CI / scripts) but
./scripts/run-local-ci.sh has not been run green for this content. The mandatory
gate is the FULL suite on the default thread stack (lib + integration incl.
tests/stress_tests.rs + doc) — a filtered/`--lib`/`| tail`-masked run is NOT the
gate (see COMMIT.md, and the 2026-05-18 deep-nesting incident).

Fix: ./scripts/run-local-ci.sh   (then re-commit)
Last resort, justified in the commit body: git commit --no-verify
EOF
  exit 1
fi

want="$(cat "$receipt")"
have="$(rgx_gate_state_id)"
if [ "$want" != "$have" ]; then
  cat >&2 <<'EOF'
✗ GATE-RECEIPT: stale gate receipt.

Gate-affecting files changed since ./scripts/run-local-ci.sh last passed, so the
green you have does not certify what you are about to commit. Re-run the gate
against the current tree.

Fix: ./scripts/run-local-ci.sh   (then re-commit)
Last resort, justified in the commit body: git commit --no-verify
EOF
  exit 1
fi

exit 0
