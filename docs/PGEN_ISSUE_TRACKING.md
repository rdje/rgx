# PGEN ISSUE TRACKING
Git-tracked local issue workflow for PGEN parser bugs and misbehavior observed from `rgx`.

## Purpose
- Give `rgx` a stable local record for every suspected PGEN parser issue.
- Preserve RGX-side context even before an upstream PGEN bug report exists.
- Keep local IDs, reproduction details, and upstream handoff metadata in version control.

## Storage model
- Local records live in `pgen-issues/`.
- Each issue gets exactly one YAML file named `PGEN-RGX-0001.yaml`, `PGEN-RGX-0002.yaml`, and so on.
- The filename is the canonical RGX-local issue ID.
- IDs are never reused, even if an issue is later closed as non-PGEN or non-reproducible.

## Canonical tools/files
- `scripts/new-pgen-issue.sh` creates the next numbered issue stub.
- `pgen-issues/TEMPLATE.yaml` is the canonical schema/template for issue records.
- `docs/PARSER_CONTRACT.md` defines the parser-boundary expectations that these records support.

## Required record content
Each issue record must capture at least:
- local ID
- summary
- status
- `opened_at`, `first_seen_at`, and `last_updated_at`
- parser backend identity/version information
- current `rgx` commit
- precise RGX-side context:
  - feature flag / parser path
  - command or test context
  - pattern
  - input
  - any additional notes needed to understand when the issue manifests
- expected behavior
- actual behavior
- minimal reproduction
- impact on RGX integration or downstream behavior
- upstream report metadata once reported
- resolution/verification notes when closed

## Status vocabulary
Use one of these status values in local issue files:
- `open`
- `triaged`
- `reported-upstream`
- `blocked-upstream`
- `fixed-upstream`
- `fixed-local-workaround`
- `closed-not-repro`
- `closed-not-pgen`

## Workflow
1. Create a new local issue stub:
   - `./scripts/new-pgen-issue.sh --summary "short summary"`
2. Fill in the placeholders immediately while the failure context is still fresh.
3. Update the same issue file as triage progresses:
   - add reproduction refinements
   - add upstream issue ID/URL once reported
   - update `last_updated_at`
   - record local workaround or upstream-fix status
4. Close the issue only after writing validation evidence in the `resolution` block.

## Example commands
```bash
./scripts/new-pgen-issue.sh --summary "Lookbehind parse tree mismatch in conditional branch" --dry-run
./scripts/new-pgen-issue.sh --summary "Lookbehind parse tree mismatch in conditional branch"
```

## Closing discipline
- If the bug is reported upstream, keep the local RGX issue open until RGX validates the upstream fix or an RGX-local workaround lands.
- If the behavior turns out to be an RGX misuse, contract mismatch, or non-reproducible report, keep the issue file and close it with `closed-not-pgen` or `closed-not-repro` plus notes explaining why.
