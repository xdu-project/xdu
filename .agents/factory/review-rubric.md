# Review rubric — `xdu-review`

The operating manual for the adversarial QA pass. The correctness reviewer runs in an **isolated
context** (fresh subagent) and grades the branch diff against `GOAL.md` + the AGENTS.md invariants
**only** — it is denied `PLAN.md`/`TECH.md` (showing the author's own rationale triggers
grading-its-own-homework and plan-sycophancy). Verification is by **executed command**, never by
assertion.

## What the reviewer sees

- ✅ `GOAL.md` (the locked contract — R-IDs)
- ✅ the branch diff **excluding `spec/`** (`git diff main...HEAD -- . ':(exclude)spec/'` — the
  spec artifacts are committed on the branch, so an unfiltered diff would hand the reviewer
  PLAN/TECH/research and any prior cycle's REVIEW.md) and the full runnable repo
- ✅ [`invariants.md`](invariants.md) (the footgun checklist) and `AGENTS.md`
- ❌ **NOT** `PLAN.md`, `TECH.md`, `research/`, or `META.md` (for the correctness pass — `META.md` is
  the harness self-improvement log and leaks author intent, same as PLAN/TECH)
- A **separate, later** completeness sub-pass *may* read `TECH.md` to ask "was every planned phase
  shipped? did scope balloon?" — kept isolated so the plan never contaminates the correctness verdict.

## Scope — flag ONLY

1. **Correctness bugs** — the code produces wrong behavior / crashes / data corruption.
2. **GOAL-requirement gaps** — an R-ID with no implementing change, or implemented incorrectly.
3. **AGENTS.md invariant violations** — auto-CRITICAL (see below).
4. **Scope creep** — changes that map to no R-ID (report, don't necessarily block).
5. **Operating-manual drift** — a symbol, module, flag, or on-disk artifact **this diff introduced or
   moved** that `AGENTS.md` or `invariants.md` still describes wrongly or not at all. `AGENTS.md`
   opens by declaring itself the map and the code ground truth ("fix this file"), so a diff that
   moves the code owns the map. Not documentation taste — `invariants.md` is the gate `xdu-plan` and
   the next `xdu-review` both draw from, so stale text there silently narrows what later cycles
   check. Mechanical on a `kind: refactor` diff: grep both files for every new or moved module,
   every new CLI flag, and every new on-disk artifact.

**Do NOT** report style nits (including prose wrapping and commit-message style), speculative
hardening, or "you could also…" gold-plating. A
gap-hunting reviewer manufactures gaps, which drives over-engineering. Silence on a clean diff is a
valid, valuable result.

## Reviewer conduct (the subagent)

- **Leave the tree clean — and `git status` is not the whole tree.** Make no edits to tracked files;
  if you must instrument to reproduce a finding (a probe, a print), revert it before returning —
  `git status --porcelain` must be empty when you hand back. **`target/` is git-ignored, so that check
  is blind to build-state poisoning.** If you mutate build state to construct a negative control
  (`cp /usr/bin/false target/release/xdu`, moving an artifact, editing an untracked fixture),
  restoring it is part of the control: assert the restore's post-condition and report it. Verify by
  driving the real thing — `cargo test` and CLI drives in a throwaway index
  (`.agents/factory/bin/temp_index.sh`), never the developer's real one.
- The **Verdict & loop** section below is the *orchestrator's* job, executed after you return — do
  not write `REVIEW.md`, call `ReportFindings`, or run `set_phase.py` yourself. Your deliverable is
  the structured findings list + requirement→evidence matrix you were asked for.

## Refutation protocol (mandatory)

For every candidate finding, **try to disprove it first**:

1. Reproduce it — run the exact command / construct the exact input that triggers it (`cargo test`
   or a real CLI drive through `temp_index.sh`).
2. If reproduced with observed wrong behavior → **CONFIRMED**.
3. If plausible by reading but not reproduced → **PLAUSIBLE** (needs human triage; does not auto-loop).
4. If it dissolves under scrutiny → drop it silently.

Default to dropping when uncertain. A single-model reviewer has self-preference bias even in a fresh
context, so lean on *executed evidence*, not opinion.

## Severity

| Severity | Meaning |
|---|---|
| **CRITICAL** | Data loss (a bad `xdu-rm` change) or index corruption (schema or atomic-write), security weakening (the DuckDB SQL-injection surface), or **any** xdu invariant violation (`invariants.md` §1–§12, lettered subsections such as §2b/§2c included; a §13 project-conventions violation is **HIGH**, not auto-CRITICAL). |
| **HIGH** | A GOAL R-ID unmet or wrong; a real bug on a common path; **operating-manual drift that degrades a gate** — stale text in `invariants.md`, in an `AGENTS.md` load-bearing invariant, or in any file-path-keyed gate list. |
| **MEDIUM** | A bug on an edge path; a partial/again-fragile requirement; operating-manual drift elsewhere (an `AGENTS.md` description merely wrong, absent, or misattributed). |
| **LOW** | Minor correctness risk; missing-but-non-blocking test coverage of an R-ID. |

## Verdict & loop (orchestrator only)

- Emit findings via `ReportFindings` (most-severe first) **and** write `REVIEW.md`.
- **CONFIRMED** findings → set `TECH.md` `status: blocked` + `review.verdict: changes-requested`
  (via `set_phase.py`) and loop back to `xdu-build`.
- **PLAUSIBLE** findings → surface to the human for triage, do not auto-loop.
- Clean pass → `review.verdict: approved`; proceed to `xdu-publish`.
- Cycle 2+ **appends** a dated `## Review cycle {n}` section to `REVIEW.md` — never overwrite an
  earlier cycle; the file is the cumulative record.
- **Bounded loop:** at most 2–3 review↔build cycles — graded against the durable `review.cycle`
  counter in `TECH.md` (auto-incremented by each `set_phase.py --verdict`). On non-convergence, STOP
  and escalate to the human (self-correction does not reliably converge).

## Mandatory human sign-off gate

Regardless of auto-loop, a human must approve before `xdu-publish` whenever a CONFIRMED finding
touches:

- the high-blast-radius core: `src/bin/xdu-rm.rs` (destructive), `src/bin/xdu.rs` (crawl concurrency
  scaffold + marker sequencing), `src/crawl.rs` (atomic finalize + stale-chunk prune +
  reserved-index-name collision rejection), `src/lib.rs` (schema + `QueryFilters`/SQL + `index_glob`
  + layout constants + `RESERVED_INDEX_NAMES`), `src/cli.rs` (the one CLI definition), `src/s3.rs`
  (S3 chunk/prune/marker sequencing + S3 partition-name refusal + the credential resolver and the
  reader's bound secret), `src/view_query.rs` (every `xdu-view` statement), or `src/engine.rs` (the
  one connection open, the extension load and `open_s3_connection`); **or**
- a destructive-`rm` / schema-stability / atomic-write / SQL-injection invariant
  (`invariants.md` §4 / §1 / §2 / §5).

**That path list is restated in `invariants.md`, `xdu-review`'s Safety Principles, and
`templates/REVIEW.md`, and the copies drift independently** — a pure code *move* has already disarmed
this gate once. Any diff that relocates code re-derives all of them in the same diff, and the list may
only ever **widen**: a path whose logic moved gets the new home added, never the old one removed.

## Optional debate variant (high-risk diffs)

For a diff touching the coupled core, run **two** independent fresh reviewers — one arguing "ship",
one arguing "block" — and reconcile. Independent instances beat single-model introspection. Reserve
for genuinely high-risk changes (cost).
