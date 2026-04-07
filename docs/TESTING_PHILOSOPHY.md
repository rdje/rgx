# TESTING PHILOSOPHY

The engine is guilty until proven innocent at every corner.

## Mindset

Hostile skepticism. Every test exists to prove a claim wrong, not to confirm it works. If a test always passes, it's not testing hard enough. The goal is to shake the engine until it breaks — then fix what broke, add a regression test, and shake harder.

## What matters: behavioral categories, not test count

### Every opcode must be tested on its failure path, not just success
- The `Char` opcode should be tested with a character that doesn't match
- The `Split` opcode should be tested where both paths fail
- The `Backref` opcode should be tested with a reference to an unmatched group
- Every opcode that advances position should be tested at end-of-text

### Every backtracking scenario must be tested with maximum-backtracking input
- Greedy `a+` followed by literal `b` on `"aaaa"` (no `b` — forces full backtrack)
- Nested quantifiers `(a+)+b` on `"aaa"` — catastrophic case
- Alternation where every branch fails before the last succeeds
- Possessive quantifiers that refuse to backtrack

### Every feature combination must be tested together
- Steering + recursion + callbacks + backtracking in one pattern
- Events during async suspension
- File scanning with code blocks
- Conditionals inside lookaheads
- Backtracking verbs inside lookaheads
- Variables changing between find_all matches

### Every boundary must be tested
- Position 0, position end, empty input, single byte
- Maximum unicode codepoint (U+10FFFF)
- UTF-8 boundary: match starting mid-codepoint
- Zero-width matches at every position
- Matches that span the entire input

### Every error path must be tested
- Callbacks that return garbage/errors
- Files that don't exist or can't be read
- Variables that are missing when accessed
- Invalid patterns (verify Err, not panic)
- Resume with wrong result type

## Claims to prove right or wrong

The engine makes these claims. Each must have a test that could disprove it:

### "Trail-based backtracking restores state correctly"
Test with a pattern that backtracks 50 times through a callback. Verify captures are correct at each step. If trail replay has an off-by-one, this catches it.

### "Zero overhead when no observer"
Benchmark with and without observer registration. The numbers should match within noise. If event dispatch adds overhead when no observer is set, this catches it.

### "Continuations are Send+Sync"
Actually send one through a channel, resume on a different thread pool, under load. Not just `assert_send_sync::<T>()` — actually move it across threads and use it.

### "Prefix filter skips impossible positions"
Create a pattern where the filter should skip 999 positions and the match is at position 999. Verify it finds the match. If the filter has an off-by-one that skips the last position, this catches it.

### "memmem fast path produces identical results to VM path"
Compile the same literal pattern. Run it through both the memmem fast path AND the VM (by temporarily disabling the fast path). Compare results on 10,000 inputs. Any divergence is a bug.

### "Events don't affect match behavior"
Run the exact same match with and without an event observer. Compare results. If events introduce side effects, this catches it.

### "Async suspension preserves full VM state"
Suspend mid-match, resume, verify captures are identical to what they would be without suspension. If the continuation misses a field, this catches it.

## What's still untested (known gaps)

These are combinations that could hide bugs but don't have dedicated tests yet:

- **Recursion + steering**: what happens if a callback steers inside a recursive subroutine?
- **Events during async suspension**: do events fire before suspension? After resume? Both?
- **File scanning + async callbacks**: can you suspend during a file scan?
- **Variable mutation between find_all matches**: does the second match see updated variables?
- **Capture groups across `\K` boundaries**: are captures correct when match start is reset?
- **Backtracking verbs inside lookaheads**: does `(*COMMIT)` inside `(?=...)` affect the outer match?
- **Steering + zero-width matches**: what if Accept fires at a zero-width position in find_all?
- **Deep recursion + trail backtracking**: does the trail correctly restore captures after deep recursive calls?
- **Concurrent set_variable + matching**: is it safe to update variables from one thread while another thread is matching?

## Process

1. **Every bug fix must come with a regression test** that would have caught the bug.
2. **Every new feature must come with adversarial tests** that try to break it in combination with existing features.
3. **Testing is never done.** The test suite grows with the engine.
4. **Property-based tests** (via `proptest`) generate random inputs to find edge cases no human would think of. Every invariant the engine claims should be a property test.
5. **Stress tests** run thousands of iterations to catch non-determinism and resource leaks.
6. **Adversarial tests** simulate hostile users who push every feature to its limits.

## Test suites

| Suite | File | Purpose | Count |
|-------|------|---------|-------|
| Unit tests | `rgx-core/src/lib.rs` | Core API behavior | ~343 |
| Integration | `rgx-core/tests/host_integration.rs` | All 6 layers, cross-layer | 55 |
| Adversarial | `rgx-core/tests/adversarial.rs` | Try to break it | 34 |
| Property-based | `rgx-core/tests/property_tests.rs` | Random inputs, invariants | 11 (256+ cases each) |
| Stress/soak/fuzz | `rgx-core/tests/stress_tests.rs` | Sustained load, random patterns | 21 |
| PCRE2 parity | `rgx-bench/tests/pcre2_parity.rs` | Differential accuracy | ~250 |
| CLI | `rgx-cli/src/main.rs` | Command-line interface | 10 |
