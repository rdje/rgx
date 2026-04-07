# The RGX Guide

A practical guide to building with rgx — the programmable regex engine.

## Who this guide is for

You know what regular expressions are. You've used them to find patterns in text. Maybe you've wished they could do more — validate data against a database, call your application's functions, or process files reactively. That's what rgx does.

This guide doesn't assume you know rgx's internals. Each chapter introduces one concept, explains why it matters, shows you how to use it, and gives you enough examples to build real things.

## How to read this guide

Start with **Chapter 0** if you're new to rgx. Then pick any chapter that solves your problem — they're designed to be read independently, though each one builds on the ideas before it.

## Chapters

### Part I — Foundations
- [Chapter 0: Your First Match](00-first-match.md) — The basics: compile a pattern, find matches, understand the result
- [Chapter 1: Passing Data In and Out](01-data-exchange.md) — Host variables, result values, and branch identification

### Part II — Code Inside Patterns
- [Chapter 2: Predicate Callbacks](02-predicate-callbacks.md) — Run code during matching, validate on the fly, four language options
- [Chapter 3: Steering the Match](03-match-steering.md) — Accept, reject, skip, or abort from a callback

### Part III — Observability and I/O
- [Chapter 4: Watching the Engine](04-structured-events.md) — Debug, profile, and monitor matching with zero-overhead events
- [Chapter 5: Async Callbacks](05-async-io.md) — Suspend a match, do I/O, resume — works with any async runtime
- [Chapter 6: Working with Files](06-file-matching.md) — Match against files, scan line by line, trigger callbacks per match

### Part IV — Putting It Together
- [Chapter 7: Real-World Patterns](07-real-world.md) — Complete examples: log monitor, tokenizer, data pipeline, WAF rules

### Reference
- [Quick Reference](quick-reference.md) — One-page cheat sheet for common tasks
- [Execution Modes](execution-modes.md) — Pure, Safe, Full — when to use each
- [Context Reference](context-reference.md) — Everything available inside a callback
