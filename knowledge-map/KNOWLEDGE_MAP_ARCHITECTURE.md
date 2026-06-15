# Knowledge Map (KM) — a derived, question-keyed retrieval layer

A portable, **harness-agnostic** extension to the Durable Memory Architecture that makes
sure an AI/LLM (or a human) **never has to do archaeology** — re-deriving a fact from code
or runtime — to rediscover something that was *already logged once*.

This file is intentionally **project-agnostic**: it names no nouns from any one codebase.
Drop the `knowledge-map/` bundle into any repository that already uses **task-trees** +
`MEMORY_ARCHITECTURE.md` and it applies as-is.

> One-line thesis: **durable facts become findable when each is a small, self-describing,
> front-mattered file, and a machine derives a question-keyed index from them — so
> retrieval is one lookup, not an excavation.**

---

## 0. The problem this solves (and the one it does not)

Teams that already write things down still watch a fresh agent **re-derive facts from
code**. That is *archaeology*, and it is a **retrieval** failure, not a recording failure:
the fact existed (in a task leaf, a commit message, a decision record) but the next session
could not *find* it, so it dug it out of the source again and — worse — sometimes dressed
the re-derivation up as diligence.

Be honest about the ceiling. There are **two** kinds of "finding a fact":

- **Re-deriving a fact that was already established** (a root cause, an architectural
  decision, a gotcha). This is the archaeology the KM **eliminates**.
- **Measuring the current state of a living system for the first time** (today's metric,
  which case fails right now). You *cannot* pre-write a measurement you have not taken;
  this is legitimate **diagnostics**, not a failure. The KM's job there is narrow: once
  measured, its durable **conclusion** becomes a fact card so nobody measures it twice.

Target: **zero archaeology for structural/causal facts; diagnostics only for genuinely-new
state, whose conclusions then become facts.**

---

## 1. Why a derived index (and not a hand-curated one)

A hand-maintained index is the very anti-pattern `MEMORY_ARCHITECTURE.md` forbids
("prefer derived over hand-written … so it cannot drift"; "hand-maintained current-state
that drifts" is an explicit anti-pattern). It also costs time at every commit. So the KM
splits the two costs and keeps only the cheap one:

- **Curation cost** — a human/agent deciding what to index and phrasing the keys. This is
  the expensive, rot-prone part. The KM **eliminates** it: the keys live *inside* each
  fact file, authored once, when you write the fact you were recording anyway.
- **Generation cost** — a script scanning files and emitting the map. Milliseconds. The
  **machine** pays it (in the pre-commit hook), never the workflow, never the agent.

The map is a **build artifact**, never hand-edited, **deterministic** (sorted, no
timestamps) so "regenerate-and-diff" is a valid sync gate. Because the facts are atomic and
front-mattered, an agent can also **skip the map entirely** and grep the fact files
directly — the map is a convenience cache, not a load-bearing dependency. (Contrast a
hand-index, where staleness breaks everything.)

---

## 2. What a fact is

A **fact** is one `.md` file whose YAML front-matter has a **non-empty `answers:` list**.
The presence of `answers:` *is* the marker — no separate flag. Required and optional
fields:

| Field | Req? | Purpose |
|---|---|---|
| `id` | ✅ | stable, unique, kebab-case identifier (used as the map anchor) |
| `title` | ✅ | one-line human title |
| `answers` | ✅ | the **questions a future agent will search for**, in *their* words (this is the retrieval surface — list synonyms) |
| `date` | ✅ | when the fact was established (absolute, `YYYY-MM-DD`) |
| `evidence` *or* `reverify` | ✅ (≥1) | what proves it / the cheap command to re-confirm it |
| `reverify` | ⬚ | a single cheap command that re-establishes the fact (so an agent trusts it, or runs ONE command — never full archaeology) |
| `tags` | ⬚ | topical tags |
| `status` | ⬚ | `current` (default) · `superseded` · `deprecated` |
| `supersedes` | ⬚ | id of the fact this replaces |

Front-matter format is a **constrained YAML subset** (so it parses in portable awk):
scalars `key: value`; inline lists `key: [a, b]`; block lists (`key:` then `  - item`).
Use **plain or double-quoted** scalars; **no tabs** in values; questions containing a comma
must use the block-list form. Copy `templates/FACT_TEMPLATE.md` to start.

---

## 3. The six properties LLMs need (why the format is shaped this way)

Humans *browse and recognize*; LLMs *query (grep/semantic) and follow pointers*. So:

1. **Query-shaped keys.** `answers:` are phrased as the questions you'd actually grep —
   not topics a human would skim. Include synonyms and the literal error string.
2. **Canonical vocabulary.** One canonical term per concept; list aliases in `answers:` so
   any search term routes to the one home. (Grep is lexical — drift loses half the hits.)
3. **Atomic, titled units.** One fact per file; the `title` *is* the index entry. (A fact
   buried at line 480 of a 600-line chapter is grep-findable but expensive to read.)
4. **Self-dating + self-verifying.** `date` + `evidence` + `reverify` let an agent *trust*
   a fact, or re-confirm with **one** command — instead of re-deriving "to be safe."
5. **Stable anchors + cross-links.** `id` is a stable anchor; link related facts with
   `[[other-id]]` so one lookup leads to its neighbors.
6. **Pointer, not copy.** A fact card *points to* the canonical home (book chapter, code,
   decision record). The full story stays there; the card makes it findable.

---

## 4. What to index — and what NEVER to convert

**The KM is additive. You convert nothing.** It is a retrieval layer *on top of* your
existing docs, not a replacement. Each existing layer keeps its job; the KM points into it.

| Layer | Role | KM relationship |
|---|---|---|
| mdBook | narrative / teaching | **destination** — a card's body links to the chapter |
| live status / changelog | current state | not indexed — changes too fast (it's not a durable fact) |
| task-trees | work frontier (layer B) | not indexed — a fact that *emerges* from a task is promoted to a card on conclusion |
| decision records (layer C) | durable ADR facts | **optionally** add `answers:` front-matter in place to fold one into the map (zero duplication) |
| **Knowledge Map** | retrieval index | **derived** from the cards above |

**DO** write a card when:
- you establish a **durable, structural/causal fact** (a root cause, an architectural
  constraint, a gotcha, where-something-lives) — i.e. the kind that would otherwise need
  archaeology; or
- you **catch yourself re-deriving** a fact that should already have been written down
  (write the card *then*, so it never recurs).

**DON'T**:
- ❌ Convert prose docs/the book/task-trees into cards (destroys narrative, duplicates
  content, goes stale — the §12 anti-pattern).
- ❌ Hand-edit the generated map. It is derived.
- ❌ Card a **measurement that changes every run** (a metric, today's count). Card the
  durable **conclusion**, not the reading.
- ❌ Card narrative/teaching (book's job), current status (live-docs' job), history
  (git's job).
- ❌ Duplicate a fact across cards. One canonical card per fact; `supersedes` when it
  changes (don't silently rewrite).
- ❌ Omit `evidence`/`reverify` — without them an agent re-derives anyway, defeating the
  purpose.
- ❌ Put nondeterminism (timestamps) into the map — it breaks the sync gate.

There is **no migration project.** The map grows one cheap card at a time; each card
permanently retires one future archaeology.

---

## 5. The scripts

Two portable scripts (POSIX shell + awk; work on BSD/macOS and GNU/Linux):

- **`scripts/gen_knowledge_map.sh`** — scans `KM_SCAN_DIRS`, parses each fact's
  front-matter, emits the deterministic `KM_OUTPUT` map (a `Questions → fact` lookup plus a
  `Facts (by id)` catalog). `--print-map-path` prints the resolved output path.
- **`scripts/check_knowledge_map.sh`** — fails nonzero if any fact is missing a required
  field, if two facts share an `id`, or if the committed map differs from a fresh
  regeneration (derive-and-diff). This is what the hook and CI run.

Config precedence (`scripts/knowledge_map.conf`, all `:=` assignments): **environment >
repo-root `.knowledge_map.conf` > bundle default**. Knobs: `KM_SCAN_DIRS`, `KM_OUTPUT`,
`KM_TITLE`.

---

## 6. Enforcement — so the map cannot rot

Mirror the four-layer defense from `MEMORY_ARCHITECTURE.md` §9 (a rule nothing checks is a
rule nothing follows):

- **Pre-commit hook** regenerates the map, `git add`s it, then runs the check — so the
  agent spends **zero** time indexing and the map is always in sync. See
  `hooks/pre-commit.snippet`.
- **CI** runs `check_knowledge_map.sh` (same script) — catches a bypassed hook; a stale map
  or invalid fact **fails the build**. See `ci/knowledge-map-gate.yml`.

Because generation is deterministic, the hook's "regenerate + stage" makes drift
structurally impossible: the committed map always equals what the facts produce.

---

## 7. Read path — how a future agent uses it (no archaeology)

1. From the bootstrap entrypoint → open `KNOWLEDGE_MAP.md` (your window + the agent's
   index).
2. Scan `Questions → fact` for the question at hand → follow the one pointer to the
   canonical home.
3. Trust the fact (it is dated + evidenced), or run its one `reverify` command.
4. Only if the fact is genuinely **not** in the map is new investigation warranted — and
   its conclusion becomes a new card before the turn ends.

---

## 8. Adoption checklist (any repo already using task-trees + MEMORY_ARCHITECTURE)

1. Copy the `knowledge-map/` bundle into the repo root; commit it.
2. `bash knowledge-map/install.sh` (creates `docs/knowledge/`, generates the first map,
   prints the hook + CI wiring) — or do those steps by hand from §6.
3. Add the hook snippet (`hooks/pre-commit.snippet`) to `.githooks/pre-commit`; ensure
   `git config core.hooksPath .githooks` is set.
4. Add the CI step (`ci/knowledge-map-gate.yml`) to your pipeline.
5. Seed it: write a card for each fact you have recently had to re-derive. Going forward,
   add a card whenever you establish a durable fact or catch archaeology.
6. *(Optional)* fold high-traffic `docs/decisions/` records into the map by adding
   `answers:` front-matter in place.

---

## 9. Anti-patterns (quick reference)

- ❌ Hand-editing the generated map.
- ❌ A big-bang "convert all docs to the KM" project.
- ❌ Cards that duplicate book/prose instead of pointing to it.
- ❌ Carding volatile metrics instead of durable conclusions.
- ❌ Cards with no `evidence`/`reverify` (agent re-derives anyway).
- ❌ Nondeterministic map output (breaks the sync gate).
- ❌ Treating the map as load-bearing — facts must stay greppable without it.
- ❌ An index with no enforcement — it rots.

---

*This bundle is itself an instance of what it describes: small, structured, in-repo,
git-tracked, self-describing, and backed by a derive-and-diff gate so it is hard to let it
drift. It composes with — and does not replace — `MEMORY_ARCHITECTURE.md`.*
