# PROJECT VISION
This document describes long-term direction, not a statement that all features are currently shipped.

## Mission
Build a fast, safe regex engine with a VM core that targets practical parity with PCRE2 for features, speed, and matching accuracy, plus controlled embedded code execution.

## Principles
- Performance-first core for pure regex workloads
- Safety-first design for any embedded execution features
- Clear separation of syntax parsing, compilation, and execution
- Pragmatic delivery: ship verified capabilities, avoid overstating status

## Long-term goals
- Practical feature parity with PCRE2 across supported regex constructs
- Competitive performance with PCRE2 on representative benchmark workloads
- High-confidence matching accuracy validated against differential and integration tests
- Mature multi-language code-block execution story where sandbox guarantees are explicit
- Deep host integration: match steering, structured events, and async I/O callbacks that make the engine a programmable matching substrate, not just a pattern matcher (see `docs/HOST_INTEGRATION_ARCHITECTURE.md`)
- Robust language bindings and production-focused tooling

## Non-goals for the near term
- Claiming full compatibility with every Perl/PCRE edge case before verified test coverage
- Shipping broad plugin ecosystems before core parser/VM completeness is stable

## Success criteria
- A documented compatibility matrix versus PCRE2, including explicit exceptions/gaps
- A documented and tested capability matrix that matches real behavior
- Sustained benchmark improvements validated in `rgx-bench`, including PCRE2 comparisons
- High-confidence API behavior backed by integration tests
- Documentation that cleanly separates current status from future aspirations
