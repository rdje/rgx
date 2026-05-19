# Summary

[Introduction](./introduction.md)
[Beyond regex: what rgx adds](./why-rgx.md)

---

# Part I: Getting Started

- [Installation & First Match](./getting-started/first-match.md)
- [Finding Matches](./getting-started/finding-matches.md)
- [Capture Groups](./getting-started/capture-groups.md)
- [Replace & Split](./getting-started/replace-and-split.md)
- [RegexBuilder & Configuration](./getting-started/regex-builder.md)

# Part II: Core API

- [The Match Type](./core-api/match-type.md)
- [Iterators](./core-api/iterators.md)
- [Position-Aware Matching](./core-api/position-aware.md)
- [RegexSet](./core-api/regex-set.md)
- [RegexCache](./core-api/regex-cache.md)
- [BytesRegex](./core-api/bytes-regex.md)
- [Safety Limits](./core-api/safety-limits.md)
- [Error Diagnostics](./core-api/error-diagnostics.md)

# Part III: Advanced Features

- [Unicode](./advanced/unicode.md)
- [Match Semantics](./advanced/match-semantics.md)
- [Partial Matching](./advanced/partial-matching.md)
- [CaptureLocations](./advanced/capture-locations.md)
- [The Replacer Trait](./advanced/replacer-trait.md)

# Part IV: Host Integration

- [Data Exchange](./host-integration/data-exchange.md)
- [Predicate Callbacks](./host-integration/predicate-callbacks.md)
- [Match Steering](./host-integration/match-steering.md)
- [Structured Events](./host-integration/structured-events.md)
- [Async I/O](./host-integration/async-io.md)
- [File Matching & tail_file](./host-integration/file-matching.md)
- [Using rgx from Other Languages (C ABI)](./host-integration/c-abi.md)

# Part V: Real World

- [Log Monitor](./real-world/log-monitor.md)
- [Tokenizer / Lexer](./real-world/tokenizer.md)
- [HTTP Router](./real-world/http-router.md)
- [Data Pipeline](./real-world/data-pipeline.md)
- [WAF Rule Engine](./real-world/waf-engine.md)

# Part VI: Internals & Project

- [Architecture Overview](./internals/architecture.md)
- [Compilation Pipeline](./internals/compilation-pipeline.md)
- [The VM](./internals/the-vm.md)
- [The NFA/DFA Hybrid Engine](./internals/nfa-dfa-engine.md)
- [The JIT Compiler](./internals/jit-compiler.md)
- [PGEN Integration](./internals/pgen-integration.md)
- [Performance](./internals/performance.md)
- [Performance Measurement Methodology](./internals/measurement-methodology.md)
- [Sandboxing & Security](./internals/sandboxing.md)
- [Testing Philosophy](./internals/testing-philosophy.md)
- [Project Status & Roadmap](./internals/project-status.md)
- [PCRE2 Conformance Residual](./internals/pcre2-conformance-residual.md)
- [PCRE2 Conformance Audit](./internals/pcre2-conformance-audit.md)
- [Contributing](./internals/contributing.md)

---

# Appendices

- [Pattern Syntax](./appendices/pattern-syntax.md)
- [PCRE2 Compatibility](./appendices/pcre2-compatibility.md)
- [Context Reference](./appendices/context-reference.md)
- [Execution Modes](./appendices/execution-modes.md)
- [CLI Guide](./appendices/cli-guide.md)
