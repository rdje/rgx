# MEMORY
Live continuity memory for `rgx` sessions.

## Why this file exists
- Preserve the actionable context needed to resume work after any interruption (session crash, machine crash, tool upgrade/reset, context loss).
- Allow a new LLM/AI instance to continue as if the previous session never stopped.
- Capture only high-signal context (decisions, constraints, current state, next actions), not verbatim transcript logs.

## Mandatory update policy
- Update this file after each completed task and before starting commit workflow.
- Record key user/agent exchange outcomes that affect implementation, process, or priorities.
- Keep entries compact, concrete, and execution-oriented.
- Prefer links/references to live docs for deep detail:
  - `CHANGES.md`
  - `DEVELOPMENT_NOTES.md`
  - `docs/USER_GUIDE.md`
  - `ROADMAP.md`
  - `docs/CAPABILITY_MATRIX.md`
  - `docs/PCRE2_COMPATIBILITY_MATRIX.md`
  - `docs/PARSER_CONTRACT.md`

## Fast resume checklist
1. Read this file top-to-bottom.
2. Check current working tree and branch state (`git --no-pager status --short`).
3. Read newest entries in `CHANGES.md` and `ROADMAP.md`.
4. Confirm current known gaps and active priorities from:
   - `DEVELOPMENT_NOTES.md`
   - `docs/PCRE2_COMPATIBILITY_MATRIX.md`
5. Continue with the next concrete task, then update this file before commit workflow.

## Persistent workflow agreements with user
- Always run `git --no-pager status` before every commit.
- Stage from that exact status output (no hidden extras).
- Use `git_message_brief.txt` with `git commit -F git_message_brief.txt`.
- Include `Co-Authored-By: Warp <agent@warp.dev>` in commit messages.
- After commit:
  - clear `git_message_brief.txt`
  - verify `git_message_brief.txt` stays untracked (`TRACKED:1` check).

## Current technical snapshot
- Parity program with PCRE2 differential tests is active and operational in `rgx-bench/tests/pcre2_parity.rs`.
- End-anchor (`$`) parity mismatch was fixed and reclassified as supported.
- `{n,m}` range-quantifier scanning/earliest-match parity gap has now been fixed and reclassified as supported.
- Capability and parser-boundary guardrails are actively enforced in:
  - `rgx-core/src/lib.rs`
  - `rgx-core/src/parsing.rs`
  - `docs/CAPABILITY_MATRIX.md`
  - `docs/PCRE2_COMPATIBILITY_MATRIX.md`

## Next likely tasks
- Continue expanding differential parity coverage for other backtracking-sensitive quantifier patterns beyond bounded-range cases.
- Continue closing remaining parsed-but-unintegrated parity gaps (backreferences, recursion, conditionals).
- Maintain strict compile-boundary explicit errors for parsed-but-unintegrated advanced features.

## Session memory entries (newest first)
### 2026-02-22
- Completed follow-up parity-hardening pass after closing `{n,m}` gap:
  - added supported-syntax PCRE2 differential cases for bounded-range suffix backtracking (`\\d{2,3}3`) in both first-match and find-all suites
  - added exact-range `{3}` find-all differential coverage
  - added parser-path API regressions for bounded-range suffix backtracking, greedy longest-valid suffix behavior, and stable `find_all` spans
  - expanded capability-matrix parser-path supported case table with bounded-range suffix positive/negative examples
- Validation for this increment:
  - `cargo test -p rgx-core parser_range_quantifier -- --nocapture`
  - `cargo test -p rgx-core capability_matrix_supported_parser_path_cases -- --nocapture`
  - `cargo test -p rgx-bench`
  - `cargo test -p rgx-core`
- Closed the previously tracked `{n,m}` PCRE2 parity gap:
  - root cause: range quantifier codegen forced exact-max behavior for bounded ranges and mismatch paths bypassed available backtrack frames
  - fix: compile bounded optional range tail with `Split`-based greedy optionals and make key opcode mismatch paths honor `try_backtrack`
  - validation: targeted and full `rgx-core` + `rgx-bench` test suites passed
- Updated parity/docs/test state after the fix:
  - reclassified range differential case to parity-supported in `rgx-bench/tests/pcre2_parity.rs`
  - updated `docs/PCRE2_COMPATIBILITY_MATRIX.md` to mark `{n,m}` scan behavior parity-verified
  - added parser-path regressions in `rgx-core/src/lib.rs` for earliest-scan and bounded-range suffix backtracking behavior
- User requested creation of `MEMORY.md` as critical live continuity infrastructure.
- Explicit requirement: keep this document continuously updated with key actionable exchange outcomes (not full transcript), and do it before commit workflow.
- This file was created and integrated into live documentation policy so future AI instances can resume quickly and safely.
