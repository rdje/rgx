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

## Cluster 1A — Recursive / self-referencing captures across quantifier iterations (16 cases)

**Difficulty**: high (architectural). **Expected payoff**: ~16 cases, plus indirect wins in Clusters 2A/3.

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

## Cluster 1B — Returned-capture subroutines `(?N(grouplist))` — A12 follow-up (13 cases)

**Difficulty**: medium-high. **Expected payoff**: 13 direct cases plus Cluster 2G (2 cases) = ~15.

PCRE2 10.47+ added `(?N(grouplist))` syntax — a subroutine call that returns specified captures to the caller. A12 shipped parsing and `Call` opcode lowering (2026-04-09); what remains is **VM-side capture-return semantics**.

Cases:

| File:line | Pattern | Subject |
|---|---|---|
| testinput2:6277 | `^(?\|(\*)(*napla:\S*_(\2?+.+))\|(\w)(?=\S*_(\2?+\1)))+_\2$` | `*abc_12345abc` |
| testinput2:6280 | same family with extra nesting | `*abc_12345abc` |
| testinput2:8067 | `^(?1(2))\2(?(DEFINE)(a(.)b(.)c))` | `axbycx` |
| testinput2:8099 | `^(?1)(?(DEFINE)(<(?2(3,4))><\4\3>)((..)(..)))` | `<abcd><cdab>` |
| testinput2:8119 | `(?:(?1(<prefix>))#){4}(?(DEFINE)((?(<prefix>)\2)(?<prefix>.{3})))$` | 3 cascading-prefix subjects |
| testinput2:8142 | `(?(R)(Sat)urday\|(?R(1)),\1)` | `Saturday,Sat` |
| testinput2:8145 | `(?(DEFINE)((Sat)urday))(?1(2)),\2` | `Saturday,Sat` |
| testinput2:8148 | `(?(DEFINE)((Sat)urday))(?-2(-1)),\2` | `Saturday,Sat` |
| testinput2:8151 | `(?+1(+2)),\2(?(DEFINE)((Sat)urday))` | `Saturday,Sat` |
| testinput2:8154 | `(?(DEFINE)(?<fn>(?<ret>Sat)urday))(?&fn('ret')),\k<ret>` | `Saturday,Sat` |
| testinput2:8157 | `(?(DEFINE)(?<fn>(?<ret>Sat)urday))(?P>fn(<ret>)),\k<ret>` | `Saturday,Sat` |
| testinput2:8168 | `(?(DEFINE)((Sat)(urday)))(?1(2,3)),\2,\3` | `Saturday,Sat,urday` |
| testinput2:8109 | `<(?:[^<>]*?(?:(AB)[^<>]*\|)(?:\|(?R(1))))+>` | 6 nested-bracket subjects |

**What to change**: extend `invoke_subroutine` to accept an optional "return capture list" (currently the parser produces the AST but the VM ignores it). After the callee succeeds, merge the specified capture slots into the caller's capture state. Explicit backlog item **A12 capture-return VM semantics follow-up** in `docs/BACKLOG.md` and `RUST_CODEBASE_ANALYSIS.md`.

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

## Cluster 1D — Complex backtracking-verb interactions (7 cases)

**Difficulty**: medium (each case needs targeted analysis). **Expected payoff**: 7 direct + related SM in Bucket 2.

Single-verb behaviours are shipped (engine fixes #9, #18, #24, #25, #27, #28, #36, #37). What remains is **multi-verb interactions** and **scanner start-optimization parity**.

| File:line | Pattern | Subject | Note |
|---|---|---|---|
| testinput1:5429 | `aaaaa(*COMMIT)(*SKIP)b\|a+c` | `aaaaaac` | COMMIT then SKIP pair — PCRE2 advances scan to SKIP position after COMMIT abort |
| testinput1:5457 | `aaaaa(*COMMIT)(*THEN)b\|a+c` | `aaaaaac` | COMMIT then THEN |
| testinput1:5486 | `a(*:m)a(*COMMIT)(*SKIP:m)b\|a+c/mark` | `aaaaaac` | MARK + COMMIT + named SKIP |
| testinput1:6355 | `a+(*:Z)b(*COMMIT:X)(*SKIP:Z)c\|.*` | `aaaabd` | named COMMIT + named SKIP with different names |
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

**What to change**: the VM's conditional-with-lookahead dispatch routes through `evaluate_conditional_operand` → `execute_assertion_subexpr`. The failing path is probably when the conditional's test *fails* but the alternation should fall through to the `|` branch. Needs targeted dispatch investigation.

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
| testinput2:6244 | `\A\s*(a\|(?:[^\`]{28500}){4})/I` | `a` | Start-optimization pathological alternation — trivial alt should be tried first |
| testinput2:6249 | `\A\s*((?:[^\`]{28500}){4}\|a)/I` | `a` | same |
| testinput2:6592 | `\G(?:(?=(\1.\|)(.))){1,13}?(?!.*\2.*\2)\1\K\2/g` | `aaabcccdeee` | `\G` + lazy outer + nested lookaheads + `\K` — deep interaction |
| ~~testinput5:53~~ | ~~`^A\s+Z/utf,ucp`~~ | ~~`A\x{85}\x{180e}\x{2005}Z`~~ | ✅ CLOSED 2026-05-03. U+180E is no longer in `White_Space` (since Unicode 6.3) but PCRE2 retains the pre-6.3 classification as `\s`/`[:space:]` for backward compatibility. Unioned in at `parsing.rs::ucp_posix_class_ranges("space")` to match. Same shape as the existing `[:blank:]` / `[:print:]` MVS special-cases. |
| testinput4:1448 | `\p{katakana}/utf` | `、` (U+3001) | Script vs Script_Extensions: U+3001 categorised differently in RGX's Unicode tables vs PCRE2's |
| testinput4:1452 | `\p{scx:katakana}/utf` | `、` | `scx` (Script_Extensions) treatment |
| testinput4:2383 | `A‎‏  B/x` | `AB` | Bi-di formatting chars U+200E/U+200F inside `/x` pattern — PCRE2 treats as ignorable; RGX treats as literals |

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

**What to change**: per case. `(*NUL)` newline convention (first case testinput2:2357) probably needs `(*NUL)` threaded into the `.`/`\N`/`^`/`$` handling similar to how `(*CRLF)` etc. are. The `(*:N)` + `(*SKIP:N)` + atomic-group interactions (testinput1:6318/6326/6329) are PCRE2-specific mark-registry lifecycle semantics.

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

| testinput1:6797 | `[\Qabc\E-z]+` | `abcdwxyz` | `abcdwxyz` | `abc` |

PCRE2 reads this as literal `a`, literal `b`, range `c-z` (last char of quote + dash + literal z). RGX reads the whole quote as literal set `{a,b,c}`.

**What to change**: class-body parser/adapter needs to treat the final char of `\Q...\E` as a potential range-endpoint. See `parsing.rs::convert_class_item` for the quoted-class-literal handling.

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

**PCRE2 rejects the pattern at compile time; RGX accepts it.** Each case is a single compile-time validation that RGX is missing.

| File:line | Pattern | Subject | Note |
|---|---|---|---|
| testinput10:447 | `/abc/utf` | `abc` | PCRE2 rejects — some UTF-mode precondition |
| testinput2:4959 | `(a)\|(b)/replace` | `b` | PCRE2 rejects — top-level alternation with replace may be invalid |
| testinput2:5047 | `/abc/replace` | `abc` | PCRE2 rejects — missing/invalid template requirement |
| testinput2:6462 | `X*/g` | `>X<` | PCRE2 rejects — `X*` with `/g` on non-empty match may trigger zero-width-infinite-loop guard |

**What to change**: for each, identify the exact PCRE2 compile error via `pcre2test -v` or by reading the paired `testoutputN` file's rejection message. Then add the matching compile-time rejection in `rgx-core/src/compiler.rs::feature_validation_message` or the appropriate validation layer.

Each fix is a one-or-two-line compile-check addition. The value is small per case but the pattern is clean and closes the bucket entirely with ~4 focused commits.

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
