# Knowledge Map (KM) bundle

A self-contained, **project-agnostic**, git-tracked deliverable that gives an AI/LLM (or a
human) a **question-keyed, machine-derived index** over a repo's durable facts — so a future
session **never has to do archaeology** (re-derive a fact from code or runtime) to find
something that was already logged once.

It is an **additive extension** to the Durable Memory Architecture
(`MEMORY_ARCHITECTURE.md`) + task-trees. It **replaces nothing** and requires **no
conversion** of existing docs — see `FAQ.md`.

## Adopt it in another project (copy the bundle)

```bash
cp -r knowledge-map /path/to/other-repo/        # copy the whole self-contained bundle
cd /path/to/other-repo
bash knowledge-map/install.sh                    # create fact dir, gen first map, print wiring
git config core.hooksPath .githooks             # if not already set
# then apply the hook + CI snippets the installer prints
```

The only per-project knobs (env, or a repo-root `.knowledge_map.conf`):
`KM_SCAN_DIRS` (default `docs/knowledge docs/decisions`), `KM_OUTPUT`
(default `KNOWLEDGE_MAP.md`), `KM_TITLE`.

## What's in the bundle

| Path | What it is |
|---|---|
| `KNOWLEDGE_MAP_ARCHITECTURE.md` | the standard: the model, the fact format, what to index / what NEVER to convert, enforcement, the read path |
| `FAQ.md` | plain-language explainer (no conversion, the ceiling, costs, sizing) |
| `scripts/gen_knowledge_map.sh` | derives the deterministic map from fact front-matter |
| `scripts/check_knowledge_map.sh` | validates facts + asserts the map is in sync (hook + CI) |
| `scripts/knowledge_map.conf` | bundle-default config (`:=`, so env / repo override win) |
| `templates/FACT_TEMPLATE.md` | copy this to author a fact |
| `hooks/pre-commit.snippet` | lines to add to `.githooks/pre-commit` |
| `ci/knowledge-map-gate.yml` | the CI backstop |
| `install.sh` | idempotent adoption helper |

## The model in one breath

A **fact** is one `.md` with a non-empty `answers:` list in its YAML front-matter (the
questions an agent would search for) plus `id`/`title`/`date` and `evidence`/`reverify`. The
generator harvests those into a deterministic `Questions → fact` map. The pre-commit hook
regenerates + stages it (zero agent cost), CI checks it (no drift). A fact card is a
**signpost** that points at the canonical home (book/code/decision record) — it does not copy
it. Facts are added **lazily**, one at a time, whenever you establish a durable fact or catch
yourself re-deriving one. **No migration project.**

## How a future agent uses it

1. Open `KNOWLEDGE_MAP.md`. 2. Find the question. 3. Follow the one pointer; trust the dated
fact or run its `reverify` command. 4. Only investigate if the fact genuinely is not there —
then write a card so it never has to be investigated again.

> Read `KNOWLEDGE_MAP_ARCHITECTURE.md` and `FAQ.md` before authoring facts.
