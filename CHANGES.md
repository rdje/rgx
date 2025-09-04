# Technical Change History

- 2025-09-02: Initialized fresh rgx workspace (Cargo workspace, rgx-core, rgx-cli). Added scaffolding modules and CLI. Set repository to https://github.com/rdje/rgx.
- 2025-09-02: Finalized project vision - Lua + JavaScript code execution in regex patterns, Apache 2.0 license, beat PCRE2 performance goal.
- 2025-09-02: Implemented comprehensive lexer and AST system. Created complete token set for all Perl regex features including code blocks (?{lua:...}), (?{js:...}). All lexer tests passing.
- 2025-09-03: Built recursive descent parser and basic VM execution engine. Implemented complete AST-to-bytecode compilation pipeline.
- 2025-09-04: **MAJOR**: Created zero-cost parser abstraction for seamless PGEN integration. Fixed critical VM quantifier bug - \d+ patterns now compile correctly (DigitAscii vs Any). Test improvements: 29 passed, 2 failed (was 3 failed). Parser selection via compile-time feature flags with zero runtime overhead.

