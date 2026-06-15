---
# Copy this file into a scanned dir (default: docs/knowledge/) and rename it <id>.md.
# A file becomes a Knowledge Map fact ONLY if `answers:` is a non-empty list.
# Format = constrained YAML: scalars `key: value`, inline lists `key: [a, b]`, or block
# lists (key: then "  - item"). Plain/double-quoted scalars only; no tabs in values;
# questions containing a comma must use the block-list form.

id: my-fact-stable-kebab-id            # REQUIRED: unique, stable, kebab-case
title: One-line human title of the fact # REQUIRED
answers:                                # REQUIRED: the questions an agent will SEARCH for,
  - "the primary question this answers" #           in THEIR words; list synonyms + any
  - "an alternate phrasing or synonym"  #           literal error string
  - "the literal error message if any"
date: 2026-01-01                        # REQUIRED: when established (absolute YYYY-MM-DD)
status: current                         # optional: current | superseded | deprecated
supersedes:                             # optional: id of the fact this replaces
tags: [topic-a, topic-b]                # optional
evidence: path/to/file.rs:123; docs/...  # REQUIRED (this OR reverify): what proves it
reverify: grep -n "Symbol" path/to/file # REQUIRED (this OR evidence): ONE cheap re-check
---

State the fact itself here, concisely — the distilled claim, not a narrative. This card is a
**signpost**: point at the canonical home where the full story lives (a book chapter, the
code, a decision record) rather than copying it. Link related facts with [[other-fact-id]].

Keep it to the durable, structural/causal fact — the kind that would otherwise force a
future session to re-derive it from source. Do NOT paste volatile metrics or narrative here.
