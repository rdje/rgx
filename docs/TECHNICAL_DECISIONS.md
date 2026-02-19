# Technical Decisions
Concise record of key engineering decisions and tradeoffs.

## 1) VM-centric execution
- Decision: execute regex through a bytecode VM backend
- Why: clean separation between syntax/frontend and execution backend, plus optimization flexibility
- Tradeoff: greater implementation complexity vs direct interpreter approaches

## 2) Layered compile pipeline
- Decision: keep explicit lexer -> parser -> AST -> compiler -> VM stages
- Why: easier debugging, targeted testing, and future parser evolution
- Tradeoff: more interfaces to maintain

## 3) Public API stability over internal churn
- Decision: keep `Regex` API surface small and predictable
- Why: user code should not depend on internal opcode/compiler refactors
- Tradeoff: advanced features may arrive incrementally

## 4) Documentation must follow verified behavior
- Decision: separate shipped status from vision/plans
- Why: prevent contributor confusion and false assumptions
- Tradeoff: requires regular doc maintenance discipline

## 5) Benchmark-driven optimization
- Decision: use `rgx-bench` baselines to evaluate performance changes
- Why: avoids anecdotal optimization and regressions
- Tradeoff: benchmark maintenance and interpretation overhead

## 6) PCRE2 parity as north-star target
- Decision: treat practical PCRE2 parity (features, speed, accuracy) as a primary strategic target.
- Why: gives a concrete external quality/performance bar and reduces ambiguity in prioritization.
- Tradeoff: parity work requires substantial compatibility testing and can slow down speculative feature work.

## 7) Multi-language code-block expansion with shared safety contracts
- Decision: expand embedded code-block language support beyond initial runtimes (e.g., JavaScript and Lua) to additional runtimes such as Julia, under shared execution/sandbox constraints.
- Why: expands expressiveness while preserving predictable safety boundaries.
- Tradeoff: each new runtime increases integration complexity, security review scope, and maintenance burden.

## Deferred decisions
- Final shape of advanced parser support (named groups/lookaround/code blocks)
- Full integration path for embedded execution features in end-user regex patterns
- Scope and timeline for broader language/runtime bindings and code-block runtime sequence
