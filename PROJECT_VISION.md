# PROJECT VISION
This document describes long-term direction, not a statement that all features are currently shipped.

## Mission
Build a fast, safe regex engine with a VM core and a path to controlled embedded code execution.

## Principles
- Performance-first core for pure regex workloads
- Safety-first design for any embedded execution features
- Clear separation of syntax parsing, compilation, and execution
- Pragmatic delivery: ship verified capabilities, avoid overstating status

## Long-term goals
- Competitive performance against established engines for targeted workloads
- Broader regex feature coverage (including advanced assertions/group types)
- Mature multi-language execution story where sandbox guarantees are explicit
- Robust language bindings and production-focused tooling

## Non-goals for the near term
- Claiming full compatibility with every Perl/PCRE edge case before verified test coverage
- Shipping broad plugin ecosystems before core parser/VM completeness is stable

## Success criteria
- A documented and tested capability matrix that matches real behavior
- Sustained benchmark improvements validated in `rgx-bench`
- High-confidence API behavior backed by integration tests
- Documentation that cleanly separates current status from future aspirations
