# PCRE2 Conformance Residual — the 107 Remaining Failures

At the time of writing the ratchet sits at **12,709 pass / 101 fail / 0 panic / 0 skip** against the full `testinput1..29` corpus — approximately **99.2%**. Initial draft of this chapter was at 12,703 / 107; closed since: Cluster 3B (`(*CRLF)` `.` lookbehind, +2), Cluster 1F conditional (+3), Cluster 1F substitute follow-up (+1). Total −6 since the catalogue landed.

This chapter is a surgical, per-case map of those 107 remaining failures. Its purpose is to let a contributor walk in cold and immediately start fixing, without having to re-discover what has already been analysed.

---

# For a new session: how to read this page

The harness sorts the 107 failures into **5 buckets**. Each bucket is a different kind of divergence between RGX and PCRE2 and each demands a different diagnostic approach. Pick a bucket that fits your budget, then go to its section below — each bucket lists every case inside it along with the root-cause cluster it belongs to.

| Bucket | Count | Meaning | Where to start |
|---|---:|---|---|
| [**1. false negative**](#bucket-1--false-negatives-65-cases) | 65 | PCRE2 matches; RGX returns no match | Cluster 1A (recursive captures) is the architectural prize; Cluster 1C (`(*napla:...)`) is a bounded compiler change. |
| [**2. span mismatch**](#bucket-2--span-mismatches-27-cases) | 27 | Both match; spans differ | Cluster 2A (balanced-bracket greedy recursion) is entangled with Bucket 1's recursive captures; Cluster 2C (`\K` inside `{0}`) needs `\K` propagation from lookarounds — non-trivial. |
| [**3. false positive**](#bucket-3--false-positives-6-cases) | 6 | PCRE2 says no match; RGX matches | Cluster 3B (`.+` under `/newline`) is a newline-convention edge case. Cluster 3C reclassified into Cluster 2C (degenerate match handling, not FP). |
| [**4. other (substitute-mode output)**](#bucket-4--substitute-mode-output-divergence-5-cases) | 5 | `/replace=TEMPLATE` tests where `replace_all` output disagrees | Harness dispatch for the first case (2 vs 1 replacements), engine for the rest. |
| [**5. RGX too permissive**](#bucket-5--rgx-too-permissive-4-cases) | 4 | PCRE2 rejects at compile; RGX accepts | Add compile-time rejection in `compiler.rs::feature_validation_message`. |

**Total**: 107. Pass rate 99.2%. The number moves with every engine fix — the authoritative source is `cargo test --release -p rgx-core --test pcre2_conformance -- --ignored --nocapture` against the baselines in `rgx-core/tests/pcre2_conformance.rs`. When counts in this chapter go stale, the **cluster shape** is still the useful durable thing; individual case lists decay.

If you have to pick one strategy to start with, the highest leverage per hour is:

1. **Sweep through Bucket 3** (6 FPs) — small targeted fixes each, 3 clusters totalling ~6 cases.
2. **Pick off Bucket 5** (4 too-permissive) — each is a one-line compile-time rejection.
3. ~~**Attack Cluster 2C**~~ — superseded; the single-codegen-fix prescription was wrong. Real fix is `\K` propagation from inside lookarounds, scoped as a multi-session engine change. See Cluster 2C below.
4. ~~**Attack Cluster 4-substitute-1**~~ — closed 2026-05-03. Per-subject `\=g` is now threaded through harness substitute dispatch.

That pass alone closes **~13 cases** in a single session and hands off a cleaner residual. After that, the architectural work (Cluster 1A, Cluster 1B) is the serious sprint that closes another ~30 cases but needs days, not hours.

## How to reproduce on demand

```bash
# Full histogram with first-example per bucket:
cargo test --release -p rgx-core --test pcre2_conformance -- --ignored --nocapture

# Dump every failing case in a specific category:
RGX_CONFORMANCE_DUMP_CAT="false negative" cargo test --release -p rgx-core \
    --test pcre2_conformance -- --ignored --nocapture 2>&1 | \
    grep -E "^(false|span|other|RGX)"

# Dump every failure (empty string matches every category):
RGX_CONFORMANCE_DUMP_CAT="" cargo test --release -p rgx-core \
    --test pcre2_conformance -- --ignored --nocapture 2>&1 | \
    grep -E "^(false|span|other|RGX)"
```

The harness writes each dumped case as `<category> <file>:<line>: /<pattern>/<mods> :: <detail>`.

---

# Bucket 1 — false negatives (65 cases)

**RGX returns "no match" where PCRE2 finds one.** This is by far the biggest bucket and splits cleanly into 7 root-cause clusters.

## Cluster 1A — Recursive / self-referencing captures across quantifier iterations (16 cases — **PARTIAL CLOSURE 2026-05-06**: 11 of 16 recovered)

**Difficulty**: high (architectural). **Expected payoff**: ~16 cases, plus indirect wins in Clusters 2A/3. **Status 2026-05-06**: 11 cases shipped via the prev-iter-capture-slot architecture (commit ad49523's successor — capture vector doubled, SaveStart promotes completed pair to upper-half "prev iter" slots, backref resolves current-first-then-prev-iter). Remaining: testinput1:3254 (`(a(?(1)\1)){4}` — conditional + self-backref), the 3 testinput1:5964 palindrome subjects, testinput1:5971, :5984 — these need the conditional `(?(N)...)` test or the lookahead-recursion path to also see prev-iter, plus the propagation isolation extends to subroutine calls.

Patterns where a backreference (`\1`) or subroutine call (`(?1)`) refers to a capture group that is *currently being iterated* by a surrounding quantifier. On iteration N+1, the reference must see the value captured during iteration N. RGX's capture machinery restores group values on backtrack but does not preserve the "last completed iteration's value" as a separate, readable slot. When the inner body re-enters the group, the capture is in-flight and the backreference sees either an empty value or the wrong span.

Cases:

| File:line | Pattern | Subject(s) |
|---|---|---|
| testinput1:2372 | `^(a\1?){4}$` | `aaaaa`, `aaaaaaa`, `aaaaaaaaaa` (3 cases) |
| testinput1:3247 | `^(a\1?){4}$` | `aaaaaaaaaa` |
| testinput1:3254 | `^(a(?(1)\1)){4}$` | `aaaaaaaaaa` |
| testinput1:6502 | `^(a\1?){4}$` | `aaaaaa` |
| testinput1:6506 | `^((\1+)\|\d)+133X$` | `111133X` |
| testinput2:325 | `^(xa\|=?\1a){2}$` | `xa=xaa` |
| testinput2:330 | `^(xa\|=?\1a)+$` | `xa=xaa` |
| testinput1:5964 | `^(.\|(.)(?1)\2)$` | `ababa`, `abcba`, `abcdcba` (3 cases) — palindrome family |
| testinput1:5971 | `^((.)(?1)\2\|.?)$` | `ababa` |
| testinput1:5984 | `^(.\|(.)(?1)?\2)$` | `abcba` |
| testinput2:3030 | `^(ab(c\1)d\|x){2}$` | `xabcxd` |

**Related engine fix history**: #29 (2026-04-23, +7) and #30 (2026-04-24, +3) added empty-match retry frames to the `Call` opcode dispatch (top-level + subexpr + continuation). That closed the easy palindrome cases where the subroutine legitimately matches empty. What remains requires **full subroutine-stack reification**: each `(?1)` / backref to a quantified group needs to preserve the caller's capture state and the "last completed iteration" value separately.

**What to change**: the VM's `invoke_subroutine` (`rgx-core/src/vm.rs`) currently runs the subroutine body in an isolated local backtrack stack. A full fix needs (1) an explicit subroutine-call frame that preserves all enclosing capture slots at entry; (2) proper replay of the callee's capture writes into the caller's frame; (3) a "previous iteration's completed capture" read-only slot that backref sees. Multi-day architectural change.

## Cluster 1B — Returned-capture subroutines `(?N(grouplist))` — A12 follow-up (13 cases — **PARTIAL CLOSURE 2026-05-06**: 10 of 13 recovered)

**Status 2026-05-06 (walker shipped, ratchet 12,747/63)**: typed walker in `parsing.rs::convert_typed_subroutine_call_object` now decodes `target.captures` (raw-token tree shape `["(", first_arg, [comma_tail], ")"]`) into `Regex::ReturnedCaptureSubroutine { target, returned_groups: Vec<RecursionTarget> }`. The compile path (`vm.rs::compile`) emits each via `recursion_target_to_id`; the `OpCode::CallReturning = 0x46` VM dispatch (commit 7105804) is now live. Closed 10 cases: testinput2:8067, 8099, 8142, 8145, 8148 (`(?-2(-1))` relative), 8151 (`(?+1(+2))` relative), 8154 (`(?&fn('ret'))` named), 8157 (`(?P>fn(<ret>))` named), 8168 (`(?1(2,3))` two-arg), plus the testinput2:8109 second nested-bracket subject. Remaining 3 ride the deeper subroutine-stack-reification work shared with Cluster 1A/2A:

**Earlier "blocked-on-PGEN / arg-list dropped" framing was wrong** — withdrawn 2026-05-06 along with the never-shipped PGEN-RGX-0083 draft. Empirical AST capture against PGEN pin 1.1.75 (12-pattern matrix via `pgen::embedding_api::parse_grammar_profile_ast_dump_named`) proved the data was always present; the walker just wasn't reading it. The named-form coverage is broader than first thought: `(?&fn('ret'))`, `(?P>fn(<ret>))`, `\g<fn('ret')>` all parse; only the bare-name `(?&fn(ret))` (which PCRE2 also rejects) does not.

**Difficulty**: medium-high. **Expected payoff**: 13 direct cases plus Cluster 2G (2 cases) = ~15.

PCRE2 10.47+ added `(?N(grouplist))` syntax — a subroutine call that returns specified captures to the caller. A12 shipped parsing and `Call` opcode lowering (2026-04-09); what remains is **VM-side capture-return semantics**.

Cases:

| File:line | Pattern | Subject |
|---|---|---|
| testinput2:6277 | `^(?\|(\*)(*napla:\S*_(\2?+.+))\|(\w)(?=\S*_(\2?+\1)))+_\2$` | `*abc_12345abc` |
| testinput2:6280 | same family with extra nesting | `*abc_12345abc` |
| ~~testinput2:8067~~ | ~~`^(?1(2))\2(?(DEFINE)(a(.)b(.)c))`~~ | ~~`axbycx`~~ | ✅ CLOSED 2026-05-06 (typed walker). |
| ~~testinput2:8099~~ | ~~`^(?1)(?(DEFINE)(<(?2(3,4))><\4\3>)((..)(..)))`~~ | ~~`<abcd><cdab>`~~ | ✅ CLOSED 2026-05-06 (typed walker). |
| testinput2:8119 | `(?:(?1(<prefix>))#){4}(?(DEFINE)((?(<prefix>)\2)(?<prefix>.{3})))$` | 3 cascading-prefix subjects | Still red — needs subroutine-stack reification (Cluster 1A capstone). |
| ~~testinput2:8142~~ | ~~`(?(R)(Sat)urday\|(?R(1)),\1)`~~ | ~~`Saturday,Sat`~~ | ✅ CLOSED 2026-05-06 (typed walker). |
| ~~testinput2:8145~~ | ~~`(?(DEFINE)((Sat)urday))(?1(2)),\2`~~ | ~~`Saturday,Sat`~~ | ✅ CLOSED 2026-05-06 (typed walker). |
| ~~testinput2:8148~~ | ~~`(?(DEFINE)((Sat)urday))(?-2(-1)),\2`~~ | ~~`Saturday,Sat`~~ | ✅ CLOSED 2026-05-06 (typed walker; relative arg). |
| ~~testinput2:8151~~ | ~~`(?+1(+2)),\2(?(DEFINE)((Sat)urday))`~~ | ~~`Saturday,Sat`~~ | ✅ CLOSED 2026-05-06 (typed walker; relative arg). |
| ~~testinput2:8154~~ | ~~`(?(DEFINE)(?<fn>(?<ret>Sat)urday))(?&fn('ret')),\k<ret>`~~ | ~~`Saturday,Sat`~~ | ✅ CLOSED 2026-05-06 (typed walker; named arg). |
| ~~testinput2:8157~~ | ~~`(?(DEFINE)(?<fn>(?<ret>Sat)urday))(?P>fn(<ret>)),\k<ret>`~~ | ~~`Saturday,Sat`~~ | ✅ CLOSED 2026-05-06 (typed walker; named arg). |
| ~~testinput2:8168~~ | ~~`(?(DEFINE)((Sat)(urday)))(?1(2,3)),\2,\3`~~ | ~~`Saturday,Sat,urday`~~ | ✅ CLOSED 2026-05-06 (typed walker; two-arg). |
| testinput2:8109 | `<(?:[^<>]*?(?:(AB)[^<>]*\|)(?:\|(?R(1))))+>` | 6 nested-bracket subjects | Walker closed the second subject (Cluster 2G); first subject still red — same subroutine-stack-reification dependency. |

**What still needs to change** (for the 3 residual): the cascading-prefix testinput2:8119 family and testinput2:8109's first nested-bracket subject share the same root cause as Cluster 1A's outstanding palindrome cases — recursive captures across quantifier iterations need full subroutine-stack reification, not just the prev-iter slot. See Cluster 1A "What to change" — the same change closes these.

## Cluster 1C — Non-atomic positive lookahead `(*napla:...)` (5 cases)

**Difficulty**: medium (compiler-level). **Expected payoff**: 5 cases direct (plus 1 FP + 1 SM = 7 total across buckets).

`(*napla:...)` is PCRE2's non-atomic positive lookahead — unlike `(?=...)` which is atomic, `napla` allows the engine to backtrack *into* the lookahead body from outside. RGX currently routes napla through ordinary positive-lookahead assertion code.

FN cases:

| File:line | Pattern | Subject |
|---|---|---|
| testinput2:6155 | `\A(*napla:.*\b(\w++))(?>.*?\b\1\b){3}` | `word1 word3 word1 word2 word3 word2 word2 word1 word3 word4` |
| testinput2:6158 | `\A(?*.*\b(\w++))(?>.*?\b\1\b){3}` | same — `(?*` short-form napla |
| testinput2:6195 | `(*napla:a\|(*COMMIT)(.))\1\1` | `aa` (also appears as FP on `abbc` — see Bucket 3) |
| testinput2:6200 | `(*napla:a\|(.))\1\1` | `aa` |
| testinput2:6538 | `^(?=.*(?=(([A-Z]).*(?(1)\1)))(?!.+\2)){26}/i` | 3 pangram subjects — nested lookaheads + conditional backref |

**What to change**: compile `(*napla:X)` to a **re-enterable** lookahead whose captures and backtrack state are visible to the outer match. Currently compiled as ordinary `(?=X)` which is atomic. A new opcode path that leaves the assertion's alternation frames on the outer stack.

## Cluster 1D — Complex backtracking-verb interactions (7 cases — **PARTIAL CLOSURE 2026-05-06**: testinput1:5429/5486/6355/5457 closed, testinput2:6604/6607 attempted but trade off)

**Status 2026-05-06**: testinput1:5429/5486/6355 closed by `4fb3980` (SKIP-overrides-COMMIT). testinput1:5457 closed by `ad49523` (Phase 2 deferred-COMMIT). testinput2:6604/6607 attempted: a "let local alt-frame run after COMMIT-fail" subexpr-macro tweak recovered them but introduced regressions in testinput1:5599/5603 (which expect COMMIT to abort outer-alt-2 at the same position) — net ratchet 0. The right fix needs distinguishing "alt2 = empty" from "alt2 = content" or a different scoping of COMMIT through positive lookahead body.

**Difficulty**: medium (each case needs targeted analysis). **Expected payoff**: 7 direct + related SM in Bucket 2.

Single-verb behaviours are shipped (engine fixes #9, #18, #24, #25, #27, #28, #36, #37). What remains is **multi-verb interactions** and **scanner start-optimization parity**.

| File:line | Pattern | Subject | Note |
|---|---|---|---|
| ~~testinput1:5429~~ | ~~`aaaaa(*COMMIT)(*SKIP)b\|a+c`~~ | ~~`aaaaaac`~~ | ✅ CLOSED 2026-05-05. SKIP-overrides-COMMIT precedence fix in `vm.rs` scanning loops. |
| testinput1:5457 | `aaaaa(*COMMIT)(*THEN)b\|a+c` | `aaaaaac` | COMMIT then THEN |
| ~~testinput1:5486~~ | ~~`a(*:m)a(*COMMIT)(*SKIP:m)b\|a+c/mark`~~ | ~~`aaaaaac`~~ | ✅ CLOSED 2026-05-05. Same root as testinput1:5429. |
| ~~testinput1:6355~~ | ~~`a+(*:Z)b(*COMMIT:X)(*SKIP:Z)c\|.*`~~ | ~~`aaaabd`~~ | ✅ CLOSED 2026-05-05. Same root as testinput1:5429. |
| testinput2:3350 | `^.*? (?1) c (?(DEFINE)(a(*THEN)b))/x` | `aabc` | THEN reached through DEFINE recursion |
| testinput2:6604 | `a?(?=b(*COMMIT)c\|)d/I` | `bd` | Start-optimization: RGX's literal-prefix scan can't look past leading `a?` |
| testinput2:6607 | `(?=b(*COMMIT)c\|)d/I` | `bd` | same family |

**What to change** (per sub-case):
- COMMIT+SKIP/THEN/PRUNE pairs: each needs trace-level investigation. Engine fix #36 closed `(*PRUNE)` clearing pending `(*COMMIT)`; the inverse combinations need symmetric treatment.
- `(*:m)` mark-registry lifecycle: check `ctx.marks.clear()` placement — should clear per match *attempt*, not per *clone*.
- Start-optimization past `a?`: the VM's literal-prefix scan bails on any leading optional quantifier. PCRE2's own start optimizer looks further. Residual from engine fix #28.

## Cluster 1E — Conditional lookahead inside repeated alternation (3 cases)

**Difficulty**: medium. **Expected payoff**: 3 cases.

Pattern family: `^QUOTE ((?(?=[X])NOT_X_CHAR) | B)* QUOTE $`. The conditional asserts "next char is X"; yes-branch is "match one non-X char"; no-branch is "match B."

| File:line | Pattern | Subject |
|---|---|---|
| testinput1:4110 | `^%((?(?=[a])[^%])\|b)*%$` | `%ab%` |
| testinput2:2601 | `^"((?(?=[a])[^"])\|b)*"$/auto` | `"ab"` |
| testinput2:2604 | `^"((?(?=[a])[^"])\|b)*"$` | `"ab"` |

**Investigation 2026-05-06 (codegen attempt rejected)**: tried the obvious "assertion-fail-no-else → emit Fail" codegen change. Closed testinput1:4110 / testinput2:2601 / 2604 but regressed testinput2:4128 (`(?(?=ab)ab)` on `ca`/`cd` — PCRE2 expects empty match) and testinput2:5915 (`(?(?=^))b`). PCRE2's actual semantic for assertion-fail-no-else is **match-empty**, not fail. Reverted.

**Investigation 2026-05-07 (root cause traced)**: PCRE2 and RGX agree on conditional semantics; the divergence is in the *lazy quantifier*'s alternation-backtrack contract. Trace for `(|ab)*?d` on `abd` (the canonical Cluster 2B repro, smaller than the 1E patterns but identical mechanism):

PCRE2 at scan-pos 0:
- Iter 0: `d` at pos 0 fails.
- Iter 1: body alternation pushes alt-2 (`ab`) frame. Body alt-1 (empty) succeeds zero-width. Continuation `d` at pos 0 fails. **Backtrack pops alt-2 frame**, runs `ab` at pos 0 → pos 2. Continuation `d` at pos 2 succeeds. Match 0..3.

RGX at scan-pos 0:
- StarLazy main dispatch (`vm.rs:3707`) calls `probe_subexpr` which clones the ctx, runs body on the clone, and returns the clone if matched.
- Clone runs body `(|ab)`: alternation pushes alt-2 frame **on the clone's `backtrack_stack`**. Alt-1 (empty) succeeds zero-width. Probe returns None (because `probe_ctx.pos == ctx.pos` and `!accept_forced` — see line 4181-4187).
- Clone is dropped. **Alt-2 frame is lost**.
- StarLazy advances `ip = expr_end`. Continuation `d` at pos 0 fails. No alt-2 frame to backtrack into. Scanner moves to pos 1, etc., eventually finds `d` at pos 2 → match 2..3 (just `d`).

For `(ab|)*?d` on `abd` (alt order swapped) RGX *does* match 0..3 because alt-1 is `ab` which advances; probe returns the clone with non-zero advance; the existing iter-retry frame handles further iterations. The bug is *specifically*: **lazy quantifier with body whose first alternative matches zero-width**.

**Why the fix is non-trivial**:
1. Body alt-frames live on the clone's stack and get discarded. Lifting them to the outer ctx requires their `trail_mark` and `call_stack_mark` to be valid in the outer ctx — possible only for frames pushed *before* any body-induced state change (alt-frames at body entry). For deeper alts inside body, the clone's trail entries would have to be propagated too, which conflicts with the lazy "0-iter preferred" semantic that requires outer captures to stay at original.
2. Even with alt-frame extraction, **multi-iter lazy** patterns (`(|ab)*?cd` on `ababcd` → expects 0..6) need the body to execute repeatedly. After alt-2 succeeds and body completes, there's no hook to push another iter-frame for the next iteration.
3. The clean architectural fix requires: a `SaveLazyPos` opcode at body entry, a `StarLazyContinue` opcode at body exit, an `ExecContext.lazy_iter_save: Vec<usize>` stack, a `BacktrackFrame.lazy_iter_save_len: usize` field for save/restore across backtrack, codegen changes for `Quantifier::ZeroOrMore { lazy: true }`, and matching dispatch in **5 execution loops** (`execute_at`, `execute_at_continuation`, `execute_subexpr_inner`, plus subexpr dispatchers at lines 5630/6280). 25 `BacktrackFrame` initialization sites need the new field. Bytecode layout becomes `[StarLazy][block_len][SaveLazyPos][body bytes][StarLazyContinue][back-offset to SaveLazyPos]`.

**Scope**: 3–5 hours of careful coding plus 5–10 conformance runs (~3:45 each) for verification. Closes Cluster 1E (3) + 2B (4) + 2H (1) = 8 cases. Not a PNT-sized increment; warrants a dedicated session.

## Cluster 1F — `(?J)` dupnames + conditional + substitute ✅ CLOSED 2026-04-24 (3 of 4 cases)

The 3 FN cases closed via engine fix #38. Introduced a parallel `named_groups_all: HashMap<String, Vec<u32>>` map on the compiler plus new opcode `CONDITIONAL_KIND_NAMED_GROUP_EXISTS_ANY` that tests "is any of these group ids set." The substitute case testinput2:4953 remains — requires the same treatment in `Regex::interpolate_replacement`.

Original cluster text (historical):

**Difficulty**: medium. **Expected payoff**: 3 direct + 1 substitute (Bucket 4) = 4.

`(?J)` enables duplicate named groups. `(?('name')yes|no)` and `$name` template interpolation must resolve "which instance?" PCRE2 picks the most-recently-*set*; RGX picks the first-*defined*.

| File:line | Pattern | Subject |
|---|---|---|
| testinput1:4673 | `(?:a(?<quote> (?<apostrophe>')\|(?<realquote>")) \|b(?<quote> (?<apostrophe>')\|(?<realquote>")) ) (?('quote')[a-z]+\|[0-9]+)/x` | `a"aaaaa` |
| testinput1:5715 | `^ (?:(?<A>A)\|(?'B'B)(?<A>A)) (?('A')x) (?(<B>)y)$/x` | `Ax` |
| testinput2:2962 | `(?:a(?<digit>[0-5])\|b(?<digit>[4-7]))c(?(<digit>)d\|e)/B` | `a4cd` |

(Plus testinput2:4953 in Bucket 4 — same root cause.)

**What to change**: track the last-set pointer per name during matching; use it at conditional-evaluate and substitute-interpolate time. `Captures::name(...)` and the VM's condition evaluator currently return the first duplicate-named slot. Harness-level "dupnames-backref" gates exist for plain backrefs (engine fix history 2026-04-22) but conditional and substitute paths are not gated.

## Cluster 1G — Misc FN edges (18 cases)

**Difficulty**: varies per case. **Expected payoff**: scattered.

Remaining FN cases that don't fit the above clusters. Each is its own investigation.

| File:line | Pattern | Subject | Note |
|---|---|---|---|
| testinput1:6450 | `(?(DEFINE) (?<word> \w+ ) ) ( (?&word)* )   \./xi` | `pokus.` | DEFINE reference + `*` quantifier over subroutine call — possibly a variant of Cluster 1A |
| ~~testinput1:6597~~ | ~~`(?<=(\d{1,255}))X`~~ | ~~`1234X`~~ | ✅ CLOSED 2026-05-03 — bytecode body-length prefix on lookahead/lookbehind opcodes was a single byte; bodies > 255 bytes (which `\d{1,N}` for N ≥ 64 produces because each optional iteration is 5 bytes of Split + class) silently truncated. Widened to u16 LE in `vm.rs`. |
| ~~testinput2:6509~~ | ~~`(?<=(\d{1,256}))X/max`~~ | ~~`12345XYZ`~~ | ✅ CLOSED 2026-05-03 — same root as :6597. |
| testinput1:6794 | `\Qab*\E{2,}` | `ab***z` | `\Q...\E` + `{2,}` quantifier applies to the last char of the quote (`*`), not the whole quote (verified 2026-05-03: RGX currently passes — catalogue stale here) |
| testinput1:6679 | `a{ 1 , 2 }` | `Xaaaaa` | Whitespace inside `{m,n}` quantifier between digits and comma. Filed as PGEN-RGX-0080 — PGEN's grammar accepts whitespace at outer boundaries (`{ 1,2 }`) but not abutting the comma. |
| testinput2:6244 | `\A\s*(a\|(?:[^\`]{28500}){4})/I` | `a` | **Root cause pinned 2026-05-05**: dispatch-chain trade-off, not a JIT bug. `Engine::should_dispatch_to_c2` (engine.rs:1770) explicitly skips Pike-VM when ANY runtime match limit is set (`max_steps`, `max_backtrack_frames`, or `max_recursion_depth`). The harness sets all three. Without limits → Pike-VM dispatches → linear-time NFA → matches `a` instantly. With limits → Pike-VM is gated off → falls through to JIT/interpreter → JIT compiles 114k unrolled char-class ops, step counter increments per visited block on the failure path, exceeds 1M before reaching success. **What to change**: either (a) remove the limit gate from `should_dispatch_to_c2` (Pike is linear-time by design — limits' raison d'être is catastrophic-backtracking protection that Pike doesn't need), violating the current documented contract that "patterns relying on [limits] continue to run on the existing backtracking VM"; or (b) thread `max_steps` through to Pike-VM and have it respect the budget as a state-transition counter. Either is a contract change worth a deliberate engine session; not a silent-shape sweep target. |
| testinput2:6249 | `\A\s*((?:[^\`]{28500}){4}\|a)/I` | `a` | same |
| testinput2:6592 | `\G(?:(?=(\1.\|)(.))){1,13}?(?!.*\2.*\2)\1\K\2/g` | `aaabcccdeee` | `\G` + lazy outer + nested lookaheads + `\K` — deep interaction |
| ~~testinput5:53~~ | ~~`^A\s+Z/utf,ucp`~~ | ~~`A\x{85}\x{180e}\x{2005}Z`~~ | ✅ CLOSED 2026-05-03. U+180E is no longer in `White_Space` (since Unicode 6.3) but PCRE2 retains the pre-6.3 classification as `\s`/`[:space:]` for backward compatibility. Unioned in at `parsing.rs::ucp_posix_class_ranges("space")` to match. Same shape as the existing `[:blank:]` / `[:print:]` MVS special-cases. |
| ~~testinput4:1448~~ | ~~`\p{katakana}/utf`~~ | ~~`、` (U+3001)~~ | ✅ CLOSED 2026-05-04. Bare `\p{<script>}` now resolves via `Script_Extensions=<script>` (PCRE2 default per pcre2pattern(3)). Fix in `parsing.rs::resolve_unicode_property_class` with a Common/Inherited carve-out per Unicode TR24 §5.2. |
| ~~testinput4:1452~~ | ~~`\p{scx:katakana}/utf`~~ | ~~`、`~~ | ✅ CLOSED 2026-05-04. Same root as testinput4:1448; `scx:` prefix now forces `Script_Extensions=` lookup explicitly. |
| ~~testinput4:2383~~ | ~~`A‎‏  B/x`~~ | ~~`AB`~~ | ✅ CLOSED 2026-05-04. Pattern_White_Space chars (NEL/LRM/RLM/LSEP/PSEP — the Unicode TR31 set) are now classified as `WhitespaceLiteral` in the typed walker so `(?x)` strips them under `/x,utf` per pcre2pattern(3). |

(The 18 total for this cluster includes the 11 above + 7 cases that belong arguably to overlapping clusters — I've listed the primary ones.)

**What to change**: per-case investigation. The Unicode-property cases may need a `regex-syntax` data refresh or our own `unicode_support.rs` table update. The bi-di chars need `/x`-mode lexer handling. The bounded-lookbehind cases need `analyze_lookbehind_width` inspection.

---

# Bucket 2 — span mismatches (27 cases)

**Both engines match, but produce different spans.** This bucket is where subtle semantic differences hide — greedy-loop winners, capture scope, backtracking-verb interactions, newline conventions.

## Cluster 2A — Balanced-bracket greedy recursion (8 cases)

**Difficulty**: high, intertwined with Cluster 1A. **Expected payoff**: 8 cases.

Classic HTML-like balanced-bracket pattern with recursive call and possessive quantifier. RGX finds an inner match but not the outermost.

| File:line | Pattern | Subject | PCRE2 span | RGX span |
|---|---|---|---|---|
| testinput1:4567 | `((< (?: (?(R) \d++  \| [^<>]*+) \| (?2)) * >))/x` | `<abc <123> hij>` | `<abc <123> hij>` | `<123>` |
| testinput1:4567 | same | `<abc<>def>` | `<abc<>def>` | `<>` |
| testinput2:1161 | same with `/Ix` | same 2 subjects | same divergence (2 cases) | |
| testinput2:955 | `< (?: (?(R) \d++  \| [^<>]*+) \| (?R)) * >/Ix` | same 2 subjects | same divergence (2 cases) | |
| testinput1:6823 | `\w(?R)*\w` | `abcdef` | `abcdef` | `ab` |
| testinput1:6823 | same | `grtg` | `grtg` | `gr` |

The outer quantified recursive call `(?R)*` or `(?2)*` is supposed to keep going until brackets balance; RGX's `Call` + quantifier-loop composition exits too early. Probably the same subroutine-stack reification as Cluster 1A.

## Cluster 2B — Empty-alternative lazy-quantifier span (4 cases)

**Difficulty**: medium. **Expected payoff**: 4 cases.

`(|ab)*?d` on `"abd"` — lazy quantifier over alternative where the empty branch is first — should pick span `abd` (the lazy iteration stays at 0, the `d` needs to be found, the engine backs off and tries `(ab)` to reach `d`). RGX stops too early at just `d`.

| File:line | Pattern | Subject | PCRE2 | RGX |
|---|---|---|---|---|
| testinput1:5825 | `(\|ab)*?d` | `abd` | `abd` | `d` |
| testinput2:4192 | `(\|ab)*?d/I` | `abd` | `abd` | `d` |
| testinput2:4196 | same | same | same | same |
| testinput1:4862 | `(?P<abn>(?P=abn)xxx\|)+` | (context) | `""` | `xxx` |

**What to change**: `OpCode::StarLazy` / `PlusLazy` in `vm.rs` — the empty-branch-first behaviour needs to match PCRE2's "try zero iterations first, back off to alternatives only if the rest fails" semantic. Engine fix "greedy-quantifier advancing retry" (2026-04-18) touched this for greedy; the lazy path is symmetric but not yet fixed.

## Cluster 2C — `\K` inside `{0}` zero-repetition (1 SM + 2 FP in Bucket 3)

**Difficulty**: high. Original prescription was incorrect — see analysis below. **Expected payoff**: 3 cases across buckets.

The 3 cases share the shape `(?=...(?1)...)<body>(\K){0}` — a forward lookahead invokes group 1 (`(\K)`) via subroutine call, while the lexical occurrence of group 1 is wrapped in `{0}`.

| File:line | Pattern | Subject | PCRE2 | RGX |
|---|---|---|---|---|
| testinput2:6439 | `(?=.{5}(?1))\d*(\K){0}` | `1234567890` | `67890` (start=5, end=10) | `1234567890` (start=0, end=10) |
| testinput2:6433 | `(?=.{10}(?1))x(\K){0}` | `x1234567890` | degenerate start>end (start=10, end=1; pcre2test renders `123456789`) | `x` (start=0, end=1) |
| testinput2:6439 | `(?=.{5}(?1))\d*(\K){0}` | `abcdefgh` | degenerate start>end (start=5, end=0; pcre2test renders `abcde`) | `""` (start=0, end=0) |

Empirically verified against PCRE2 10.46 (2025-08-27, 8-bit) on 2026-05-01.

**Original prescription (rejected)**: the chapter previously said "PCRE2 correctly never executes the inner body; RGX executes it anyway" and proposed eliding the body in subroutine compilation when the host group is `{0}`-quantified. This is wrong on both counts:

1. PCRE2 does execute the subroutine body when invoked via `(?1)` — the `{0}` only suppresses the lexical (main-flow) site, not the subroutine table.
2. RGX is already eliding the lexical site correctly (`Quantifier::Range { min: 0, max: Some(0) }` in `vm.rs::codegen_pass` emits no opcodes).

The actual divergence is **`\K` propagation from inside lookarounds**. PCRE2 propagates `\K` set inside a lookahead (here, via subroutine call) to the outer match start. RGX treats lookarounds as fully isolated, so the inner `\K` never reaches the outer `Match.start`.

Confirmed with a minimal reproducer:

```text
target/release/rgx 'ab(?1)c(\K){0}d' 'abcd'      → 2..4   # main-flow (?1) propagates \K
target/release/rgx 'ab(?=(?1))c(\K){0}d' 'abcd'   → 0..4   # lookahead-wrapped (?1) does NOT propagate
```

PCRE2's behaviour is symmetric across both call sites; RGX's diverges only inside lookarounds.

**What to change**: lookaround state propagation in `vm.rs` — when a lookahead succeeds, surface any `\K`-driven match-start adjustment from the inner thread to the outer thread. Today the outer position is restored verbatim from before the lookaround. Risk: lookbehind variants need the same treatment, and care is required to keep capture-isolation intact for non-`\K` writes.

Two of the three target cases are also degenerate-match cases (start > end), which `pcre2test` renders with the `Start of matched string is beyond its end` banner. The harness today treats the ` 0:` line that follows as an ordinary expected match, so chapter classification should read "1 SM + 2 SM" rather than "1 SM + 2 FP" — corrected here.

**Status**: prescription corrected 2026-05-02. Implementation deferred — the `\K`-from-lookaround propagation is a non-local engine change and warrants a dedicated session with a full conformance pass to bound regression risk.

## Cluster 2D — Backtracking-verb span divergences (7 cases)

**Difficulty**: medium. **Expected payoff**: 7 cases.

Multi-verb interactions where RGX picks a different span than PCRE2.

| File:line | Pattern | Subject | PCRE2 | RGX |
|---|---|---|---|---|
| testinput1:5447 | `aaaaa(*SKIP)(*THEN)b\|a+c` | `aaaaaac` | `aaaaaac` | `ac` |
| testinput1:5452 | `aaaaa(*PRUNE)(*THEN)b\|a+c` | `aaaaaac` | `aaaaaac` | `aaaac` |
| testinput1:6318 | `a(?>(*:X))(*SKIP:X)(*F)\|(.)` | `ab` | `a` | `b` |
| testinput1:6326 | `(?>a(*:1))(?>b(*:1))(*SKIP:1)x\|.*` | `abc` | `abc` | `c` |
| testinput1:6329 | `(?>a(*:1))(?>b)(*SKIP:1)x\|.*` | `abc` | `abc` | `bc` |
| testinput2:2357 | `(*NUL)^.*/s` | `a\nb\0ccc` | `a\nb\0ccc` | `a\nb` |
| testinput2:6189 | `(*napla:a\|(.)(*ACCEPT)zz)\1../` | `abc` | `abc` | `bcd` |

**What to change**: per case.

`(*NUL)` (testinput2:2357) — verified 2026-05-05: the directive IS threaded through (`NewlineMode::Nul` is recognised in `parsing.rs::dot_ast`), but the rewrite to a negated `\0` CharClass happens at parse time, before `/s` flag context is known. Under `/s`, `.` should match everything including `\0`; the static rewrite ignores the flag and incorrectly truncates `.*` at NUL. The fix is structural: defer the newline-mode rewrite to compile time so it can consult the dot-all flag. Same shape applies to the `(*CRLF)` rewrite — both produce CharClass / Lookaround structures at parse time that don't compose with `/s`. **Engine task**: introduce a `Regex::DotWithNewlineMode(NewlineMode)` AST node (or thread newline_mode through the existing `Regex::Dot`) so the compiler can pick the right semantic per `/s` scope.

The `(*:N)` + `(*SKIP:N)` + atomic-group interactions (testinput1:6318/6326/6329) are PCRE2-specific mark-registry lifecycle semantics.

## Cluster 2E — `(?0)` self-pattern recursion (3 cases)

**Difficulty**: medium. **Expected payoff**: 3 cases.

`(?0)` is an alias for `(?R)` — call the entire pattern recursively. Edge cases with empty alternatives and `/endanchored`.

| File:line | Pattern | Subject | PCRE2 | RGX |
|---|---|---|---|---|
| testinput2:6595 | `\|(?0)./endanchored` | `abcd` | `abcd` | `""` |
| testinput2:6601 | `(?:\|(?0).)(?(R)\|\z)` | `abcd` | `abcd` | `d` |
| testinput2:6439 | `(?=.{5}(?1))\d*(\K){0}` | `67890` | `67890` | `1234567890` (same as Cluster 2C) |

**What to change**: `Call(0)` should allow re-entry with empty alternative at start of pattern. Investigate `invoke_subroutine` behaviour at depth 1 with empty first alt.

## Cluster 2F — `\Q...\E` inside character-class range (1 case)

**Difficulty**: medium. **Expected payoff**: 1 case (+1 related FN in Cluster 1G).

| ~~testinput1:6797~~ | ~~`[\Qabc\E-z]+`~~ | ~~`abcdwxyz`~~ | ✅ CLOSED 2026-05-05. `convert_typed_char_class` body iteration peeks ahead for the `[<quoted_run>, "-", <atom>]` shape and splits the last char of the quoted run as a range start. |

## Cluster 2G — Returned-capture subroutine balanced-paren (2 cases)

**Difficulty**: same as Cluster 1B. **Expected payoff**: 2 cases.

| File:line | Pattern | Subject | PCRE2 | RGX |
|---|---|---|---|---|
| testinput2:8109 | `<(?:[^<>]*?(?:(AB)[^<>]*\|)(?:\|(?R(1))))+>` | `<aa<AB>cc<dd>ee>` | `<aa<AB>cc<dd>ee>` | `<AB>` |
| testinput2:8109 | same | `<aa<bb<ABcc>dd>ee<ff<gg>hh>ii>` | `<aa<bb<ABcc>dd>ee<ff<gg>hh>ii>` | `<ABcc>` |

Same root cause as Cluster 1B — returned-capture subroutine semantics.

## Cluster 2H — Lookahead-as-alternative in greedy star (1 case)

| testinput1:6481 | `(?:a\|(?=b)\|.)*\z` | `abc` | `abc` | `c` |

The zero-width `(?=b)` alternative in a `*` loop — RGX exits the loop too early.

## Cluster 2I — Conditional over empty capture with quantified tail (1 case)

| testinput1:3910 | `()()()()()()()()()(?:(?(10)\10a\|b)(X\|Y))+` | (subject implied by SM detail) | `bX` | `bXXaYYaY` |

9 empty groups, conditional on group 10 which is defined *inside* the repeated body. RGX's conditional sees the group's transient value on iteration N instead of "not yet set."

---

# Bucket 3 — false positives (6 cases)

**RGX matches where PCRE2 doesn't.** Small bucket, three clusters.

## Cluster 3A — `(*SKIP)` inside failing lookbehind (1 case)

| testinput1:6487 | `(?<=a(*SKIP)x)\|c` | `abcd` — PCRE2 no match, RGX matches `""` |

**Status**: attempted in this session as a parallel to engine fix #37. The straightforward aggregation approach (track any SKIP fired across failing starts, propagate after all starts fail, gated on `propagate_captures`) regressed 3 other cases in the corpus. Specifically, `(?<=(a(*COMMIT)b))c` on "xabcd" (expected match "c") became a false negative — the clone's `committed` state inherited from `ctx` interferes with the aggregation check.

A less aggressive variant (propagate `skip_position` without `committed=true`) didn't regress but also didn't close the target. The full fix likely needs to track **which clone** fired the verb (not just whether any clone had it in the state at the end).

See `MEMORY.md` 2026-04-24 "tighten assertion verb propagation" for the partial tightening that landed for the lookahead case (positive-only gate on `propagate_captures`). The lookbehind variant is a follow-up requiring per-iteration diagnosis.

## Cluster 3B — `.+` under `/newline=...` ✅ CLOSED 2026-04-24

Both cases (testinput2:2107 `.+foo` on `\r\nfoo`, testinput2:2296 `.+A` on `\r\nA`) closed. Engine fix #11 (2026-04-22) had handled the START of a CRLF pair via `(?!\r\n)<any>` but missed the END. Extended to `(?!\r\n|(?<=\r)\n)<any>` — the inner lookbehind in the second alternative scopes the prev-`\r` check to `\n`-only positions so bare `\r` followed by non-`\n` (e.g. `c\rd`) still matches. See CHANGES.md 2026-04-24 "(*CRLF) . rejects both ends of \r\n pair (+2 passes)".

## Cluster 3C — `\K` inside `{0}` (2 cases) — RECLASSIFIED into Cluster 2C as SM

The two cases listed below were classified as FP (PCRE2 no match) but PCRE2 actually produces a degenerate start>end match that the harness pairs against an ordinary ` 0:` expected line — i.e. SM, not FP. Treat as part of Cluster 2C above; its corrected analysis covers them.

Original entries (kept for cross-reference):

| testinput2:6433 | `(?=.{10}(?1))x(\K){0}` | `x1234567890` — PCRE2 no match, RGX matches |
| testinput2:6439 | `(?=.{5}(?1))\d*(\K){0}` | `abcdefgh` — PCRE2 no match, RGX matches |

Single compiler-level fix (counted-quantifier n=0 bypass) closes all 3 cases across Buckets 2 + 3.

## Cluster 3D — Non-atomic lookahead + COMMIT + backref (1 case)

| testinput2:6195 | `(*napla:a\|(*COMMIT)(.))\1\1` | `abbc` — PCRE2 no match, RGX matches |

Same root cause as Cluster 1C (napla compile path). Expected to close alongside the napla fix.

---

# Bucket 4 — substitute-mode output divergence (5 cases)

**pcre2test `/replace=TEMPLATE` tests where RGX's `replace`/`replace_all` output disagrees with PCRE2's `pcre2_substitute`.** Harness dispatch for substitute mode landed 2026-04-18 (+41 pass). These are the remaining genuine output divergences.

| File:line | Pattern | Subject | Template | PCRE2 output | RGX output |
|---|---|---|---|---|---|
| testinput2:4262 | `/abc/replace` | `123abc456abc789` | `xyz` | `123xyz456xyz789` | `123xyz456abc789` |
| testinput2:4268 | `(?<=abc)(\|def)/g` | `123abcxyzabcdef789abcpqr` | `<$0>` | `123abc<>xyzabc<><def>789abc<>pqr` | `123abc<>xyzabc<>def789abc<>pqr` |
| testinput2:4953 | `(?J)(?:(?<A>a)\|(?<A>b))/replace` | `[a]` | `<$A>` | `[<a>]` | `[<>]` |
| testinput2:5122 | `^$/gm` | `X\r\n\r\nY` | `-` | `X\r\n-\r\nY` | `X\r-\n-\r-\nY` |
| testinput5:1640 | `(?<=abc)(\|def)/g` | `123abcáyzabcdef789abcሴqr` | `<$0>` | `123abc<>áyzabc<><def>789abc<>ሴqr` | `123abc<>áyzabc<>def789abc<>ሴqr` |

**Root causes**:

1. **testinput2:4262** — ✅ CLOSED 2026-05-03. Diagnosis: per-subject `\=g` (`123abc456abc789\=g` is the affected fourth subject under the `/abc/replace=xyz` pattern) was not being threaded through to RGX's substitute-mode dispatch. Pcre2test ran `pcre2_substitute(...PCRE2_SUBSTITUTE_GLOBAL...)` and produced `123xyz456xyz789` (count=2); the harness called `re.replace(...)` (single) and produced `123xyz456abc789` (count=1). Fix landed in `tests/pcre2_conformance.rs` (helper `subject_carries_per_subject_global` + `case.per_subject_global` field ORed into `want_global`). Ratchet bumped 12,697 → 12,698.
2. **testinput2:4268** and **testinput5:1640** — `(?<=abc)(|def)/g` with lookbehind + empty-or-`def`. Empty-match vs `def`-capture-match overlap semantics under `replace_all`.
3. **testinput2:4953** — same root as Cluster 1F (dupnames: PCRE2 picks most-recently-set, RGX picks first-defined).
4. **testinput2:5122** — `/gm` substitute on CRLF lines. RGX treats `\r` as its own line terminator in multiline mode; PCRE2's `\r\n` convention treats the pair as a single line break. Cross-cutting with Cluster 3B.

**What to change**: fixes are template-interpolation (for case 3, shared with Cluster 1F) and engine-level (cases 2, 4, 5 in newline/substitute context). Case 1 was harness-level and landed 2026-05-03.

---

# Bucket 5 — RGX too permissive (4 cases)

**Re-classified 2026-05-06 (empirical probe)**: the bucket name is a misnomer for these specific 4 cases. PCRE2 does NOT reject the *pattern* at compile time — `pcre2_compile` succeeds on all four. PCRE2's `Failed:` line in the test output comes from `pcre2_substitute` rejecting the *substitute call* (template syntax, unset-reference, infinite-loop-on-empty-match guard, invalid UTF in template under `/utf`). The harness conflates substitute-side `Failed:` lines with pattern-compile rejection by setting `Expected::CompileError` for both. The underlying divergence is real — RGX's `replace`/`replace_all` is more lenient than PCRE2's default `pcre2_substitute` — but the "compile-time" framing is wrong.

| File:line | Pattern | Subject | Modifiers | PCRE2 substitute output | RGX `replace[_all]` output | Verdict |
|---|---|---|---|---|---|---|
| testinput2:5047 | `abc` | `abc` | `replace=A$3123456789Z` | `Failed: error 56: unknown group $N reference` | `"AZ"` (template parser greedily consumes all digits → group 3,123,456,789 → unset → silent empty; trailing `Z` survives) | RGX substitute bug — `$N` digit-run should bound by `num_capture_groups`, not infinite |
| testinput2:4959 | `(a)\|(b)` | `b` | `replace=<$1>` | `Failed: error 55: unknown group $N reference` (group 1 unset because `(b)` branch captured) | `"<>"` (silent empty for unset $1) | RGX substitute bug — default should error on unset; PCRE2 only goes lenient with `PCRE2_SUBSTITUTE_UNKNOWN_UNSET` |
| testinput2:6462 | `X*` | `>X<` | `g,replace=xy` | `Failed: error 54: bad substitution string` (zero-width infinite-loop guard) | `"xy>xy<xy"` (replaces every empty span between chars) | RGX substitute bug — `replace_all` should error or skip-and-advance on zero-width match instead of looping at the same position |
| testinput10:447 | `abc` | `abc` (under `/utf`) | `replace=<U+FFFD or similar>` | `Failed: invalid UTF in template` | RGX `&str` API can't represent invalid-UTF templates; the case is structurally untestable | API gap — RGX has no path to surface invalid-UTF template errors because the `Replacer` trait takes `&str` |

**What to change** (per case, all RGX-side, all in `rgx-core/src/lib.rs::interpolate_replacement_ext` or its callers):

1. **testinput2:5047** — in the digit-run scanner (line 2391-2400 area), bound the consumed digit count by `groups.len()` (number of capture groups). For `$3...` with 0 groups, treat as one-digit `$3` → unknown group → return error (or, in lenient mode, push empty and advance one digit).
2. **testinput2:4959** — when the resolved group exists but its capture is `None`, today RGX silently pushes empty. PCRE2's default is to return `PCRE2_ERROR_UNSET`. Change RGX's default to return an error from `replace`/`replace_all`; add a builder method `Regex::lenient_substitute()` (or a `Replacer` flag) for callers who want the existing silent-empty behaviour.
3. **testinput2:6462** — `replace_all` lacks a zero-width-loop guard. PCRE2 either errors or advances by one codepoint after a zero-width match. RGX currently retries at the same position, which is why `X*` produces a replacement at every position. The fix is in `replace_all` (or its inner loop): when the match is zero-width AND `pos == previous_match_end`, advance pos by one codepoint and continue; if the advance crosses end-of-input, stop. (PCRE2 calls this "no-empty-match-at-same-position".)
4. **testinput10:447** — substantive API change. The `Replacer` trait would need to accept `&[u8]` or a typed UTF-validation hook. Out of scope for an incremental fix.

**Path to closure** (proposed, not yet implemented):

- Each case 1–3 is one focused commit on `lib.rs::interpolate_replacement_ext` / `replace_all`.
- The harness needs a sibling change: introduce `Expected::SubstituteFailure` (distinct from `Expected::CompileError`) so the "Failed: inside substitute mode" path at `pcre2_conformance.rs` line 1888 maps onto a fall-through-to-RGX-substitute that compares RGX's `Result` against the expected error. Without this harness refinement the engine fixes won't move the ratchet because the harness short-circuits at compile time.
- Case 4 stays open as a documented API-surface gap.

**Status as of 2026-05-06**: analysis complete (RGX engine bugs identified per case, no PGEN involvement), but no engine commits yet — the RGX `Replacer` trait error-return refactor is the long-pole and warrants its own session. Closing this bucket honestly takes one substitute-API commit (Replacer → fallible) plus the three per-case validations plus the harness `Expected::SubstituteFailure` split. Total: ~5 small commits across one session, +3 ratchet (case 4 stays open).

**No fudging policy** (2026-05-06): the previous draft of this section suggested adding a harness pass-through when `replace=` was in modifiers and `Expected::CompileError` was set. That would have hidden the RGX/PCRE2 substitute divergence, not closed it. Per CLAUDE.md and the no-PGEN-workarounds policy applied here to RGX-itself: every divergence gets analyzed and addressed engine-side, not papered over in the harness.

---

# Prioritisation — a recommended session sequence

**For a session that has ~1–2 hours**, pick off the quick wins:

1. **Bucket 5** (4 cases) — one short commit per case. Each is a compile-time rejection rule.
2. **Cluster 2C** — `\K` inside `{0}` bypass. 3 cases across Buckets 2+3.
3. **Cluster 3B** — `/newline=` modifier propagation to `.`'s CRLF handling. 2 cases.

Together: **~9 cases** closed, ratchet moves ~12,703 → ~12,712 / ~107 → ~98.

**For a session with ~4–8 hours**, add:

4. **Cluster 1E** — conditional lookahead in repeated alt. 3 cases.
5. **Cluster 1F + Bucket 4 case 3** — dupnames last-set tracking. 4 cases.
6. **Bucket 4 case 1** — substitute-mode `/replace=` dispatch. 1 case.
7. **Cluster 1D** — backtracking-verb pairs (pick the easiest 2–3 subcases). ~3 cases.

Adds **~11 more cases**. Cumulative: ~20 closed, ratchet ~12,723 / ~87.

**For a multi-day architectural sprint**:

8. **Cluster 1C** — non-atomic positive lookahead `(*napla:...)`. Compiler change, 5 FN + 1 FP + 1 SM = 7 cases.
9. **Cluster 2B** — empty-alternative lazy-quantifier semantics. 4 cases.
10. **Cluster 1B** — A12 returned-capture VM semantics. 13 FN + 2 SM = 15 cases.

Adds **~26 more cases**. Cumulative: ~46 closed, ratchet ~12,749 / ~61.

**The architectural capstone**:

11. **Cluster 1A + 2A** — full subroutine-stack reification with "last completed iteration" capture slot. 16 FN + 8 SM = 24 cases.

Adds **~24 cases**. Cumulative: ~70 closed, ratchet ~12,773 / ~37.

After that the residual (~37 cases) is scattered misc edges where no single cluster dominates. At that point, per-case work is the only path forward.

---

# Cross-references

- **Conformance harness source**: `rgx-core/tests/pcre2_conformance.rs`. The `classify_failure` function near the bottom is the authoritative category definition. The `PASS_BASELINE` / `FAIL_BASELINE` constants are the ratchet gate.
- **Engine-fix history**: `CHANGES.md` entries dated 2026-04-17 → 2026-04-24, numbered fixes #1 through #37 (plus unnumbered harness refinements). Each CHANGES entry is a worked example of a cluster-close.
- **Roadmap-grounded Rust analysis**: `RUST_CODEBASE_ANALYSIS.md` "High-confidence next actions" section.
- **Backlog inventory**: `docs/BACKLOG.md` section C7 (PCRE2 conformance bug triage) — mirrors this chapter at a higher level.
- **PCRE2 source of truth**: `subs/pcre2/testdata/testinput1..29` (pinned to PCRE2 10.47 via `subs/pcre2` submodule) and paired `testoutput1..29` files. The harness reads both.
- **PGEN source of truth**: `subs/pgen` at commit `48a9f064` (PGEN 1.1.29). All 72 PGEN-RGX reports filed to date have been resolved either upstream or routed around via engine work — no RGX adapter workarounds exist.

When this chapter goes stale (a fix lands), the right move is:

1. Regenerate the dump with `RGX_CONFORMANCE_DUMP_CAT="" cargo test ...`.
2. Re-run `awk -F' testinput' '{print $1}' | sort | uniq -c | sort -rn` to get the new bucket counts.
3. Bump `PASS_BASELINE` / `FAIL_BASELINE` in the conformance harness source to match the new numbers.
4. Update the counts in the table at the top of this chapter and in the per-cluster tables.
5. Mark the closed cases in the cluster lists (or remove them entirely).
6. Mirror the updates to `CHANGES.md`, `MEMORY.md`, `docs/BACKLOG.md`, `RUST_CODEBASE_ANALYSIS.md`.

The cluster taxonomy should stay useful even as individual cases come and go.
