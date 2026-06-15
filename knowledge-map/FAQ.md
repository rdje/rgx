# Knowledge Map — FAQ / explainer

Plain-language answers to the questions people actually ask when they meet the KM. The
formal spec is `KNOWLEDGE_MAP_ARCHITECTURE.md`; this file is the "explain it to me like a
colleague" companion. (These came out of the design conversation that produced the bundle.)

---

## "Do we have to convert all our live docs, the book, and the task-trees into the KM?"

**No. You convert *nothing*. That is the whole point.**

If the KM required converting the book, live docs, and task-trees, it would be a disaster —
massive effort, destroyed narrative, and duplicated content (the #1 anti-pattern in
`MEMORY_ARCHITECTURE.md` §12: "re-narrating into prose docs = duplication that goes stale").
The KM is **not a replacement for any existing doc.** It is a thin **retrieval layer that
sits on top of them and points into them.**

Each layer keeps its own job; the KM just makes their durable facts *findable*:

| Layer | Role | What it is for | Does the KM change it? |
|---|---|---|---|
| **mdBook** | narrative / teaching | a human reads it top-to-bottom to *learn* the system | **No** — it's the *destination* a card points to |
| **live docs / status / changelog** | current state | "what's true right now," metrics | **No** — too volatile to be a durable fact |
| **task-trees** | work tracking (layer B) | the frontier, what's being built | **No** — a fact that *emerges* is promoted to a card on conclusion |
| **decision records** | durable facts (layer C) | ADR-style "we decided/learned X" | **No** (optionally fold one in by adding `answers:` front-matter *in place*) |
| **Knowledge Map** | retrieval index | a future agent's "where do I find X" lookup | it is **derived** from the cards |

A KM fact card is a **signpost, not a copy.** A good card is ~15 lines that (a) state the
distilled fact, (b) list the *questions* an agent would search for, and (c) **point** — via
`evidence:` and `[[links]]` — at the canonical home where the full story already lives (the
book chapter, the code, the decision record, the task leaf). **The full content stays where
it is.** The card just makes it findable and trustable (`date` + `reverify`).

### So how does the KM get populated? Lazily, on demand — never a big-bang.

You write a card in exactly two situations, both cheap:

1. **When you establish a new durable fact** — you were going to record it anyway. Spend
   five extra lines giving it `answers:` so it is findable.
2. **When you catch yourself doing archaeology** — re-deriving something that *should* have
   been written down. Write the card *then*, so it never happens again.

There is **no migration project.** The map grows one small card at a time, each card
permanently retiring one future excavation.

### The one optional, incremental bridge

Your existing decision records are already atomic, durable facts — perfect candidates. You
do **not** rewrite them. If/when you want one to be question-keyed, add an `answers:` block
to its front-matter *in place* (the generator already scans the decisions dir) and it joins
the map with zero duplication. Do it for the high-traffic ones, when convenient — never as
a sweep.

### What does NOT belong in the KM (equally important)

- Narrative / teaching prose → stays in the book.
- Current metrics / status / frontier → stays in live docs + task-trees.
- History → stays in git.
- A fresh **measurement** of changing runtime state → that is diagnostics; only its durable
  **conclusion** becomes a card.

So: the book teaches, the task-trees track, git remembers, and the KM is the ~30-line index
that tells the next agent which of those to open.

---

## "Is there even a solution to the archaeology problem?"

Yes — a good one, not a perfect one. Be honest about the ceiling by separating two things:

- **Re-deriving a fact that was already established** = archaeology = a documentation
  failure. **Fixable to ~zero** by the KM.
- **Measuring the current state of a living system for the first time** = diagnostics =
  legitimate, and it should not go away (you cannot pre-write a measurement you have not
  taken). The fix there is not "avoid measuring" — it is "log the **conclusion** so nobody
  measures it twice."

Target: zero archaeology for structural/causal facts; diagnostics only for genuinely-new
state, whose conclusions then become facts.

---

## "Do we have to invent a new *type* of documentation?"

No. A strong durability architecture (the four layers of `MEMORY_ARCHITECTURE.md`) already
nails *durability* and the *write path*. What was missing was a **retrieval** artifact: the
layers are organized by **lifecycle** (now / work / decisions / history), but retrieval is
organized by **topic/question**, and nothing told a future agent *which* layer a topical
fact lived in. The KM adds exactly that one cross-cut — and it is an extension of a pattern
you already use (a flat, hook-annotated index of atomic, front-mattered files), not a new
paradigm.

---

## "Doesn't maintaining an index cost time at every commit?"

Only if you hand-maintain it — which you must not. Separate the two costs:

- **Curation cost** (deciding what to index, phrasing keys, keeping it in sync) — expensive
  and rot-prone. The KM **eliminates** it: the keys live inside each fact file, authored
  once, when you write the fact you were recording anyway.
- **Generation cost** (a script scanning files) — milliseconds, paid by the **machine** in
  the pre-commit hook, never by the workflow or the agent.

The map is a derived build artifact: the hook regenerates and `git add`s it every commit, so
it is always in sync and you spend zero time on it.

---

## "Won't the map get huge and slow to read?"

It is a flat, sorted, greppable list — an agent jumps to the matching question line, it does
not read top-to-bottom. And because facts are atomic and front-mattered, an agent can skip
the map entirely and grep the fact files directly. The map is a convenience cache, not a
load-bearing dependency.
