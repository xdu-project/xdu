# REVIEW — {Title}

> Adversarial QA by `xdu-review`, run in an isolated/clean context. The correctness pass grades the
> branch diff against [`GOAL.md`](GOAL.md) + the AGENTS.md invariants **only** — it does not see
> `PLAN.md`/`TECH.md` (avoids grading-its-own-homework / plan-sycophancy). Every finding cites an
> **executed** command, not an assertion.

- **Reviewed commit:** {sha}  ·  **Base:** {base}  ·  **Date:** {YYYY-MM-DD}
- **Verdict:** approved | changes-requested
- **Cycle:** {n} of ≤3 — mirrors `review.cycle` in `TECH.md` (escalate to human on non-convergence)

## Verification run

Commands actually executed and their outcomes (the spine of the review):

- `cargo test` → <result>
- `.agents/factory/bin/temp_index.sh sh -c 'xdu-find -i "$XDU_INDEX" -u alice --count && xdu-rm -i "$XDU_INDEX" --dry-run -p "\.log$" --force'` → <observed behavior>
- <man-page (scdoc) build / other CLI drives when relevant>

## Requirement → evidence matrix

Bidirectional traceability. Flag requirements with no implementing change **and** changes that map
to no requirement (scope creep).

| R-ID | Implemented by (file/commit) | Verified how | Status |
|------|------------------------------|--------------|--------|
| R1   | <…>                          | <command>    | ✅ / ❌ |

Unmapped changes (possible scope creep): <list or "none">.

## Findings

Severity: **CRITICAL** (any `invariants.md` §1–§12 violation is auto-CRITICAL, **including lettered
subsections** such as §2b/§2c; a §13 project-conventions violation is **HIGH**) · **HIGH** · **MEDIUM** · **LOW**. Verdict: **CONFIRMED**
(reproduced) vs **PLAUSIBLE** (suspected, needs human triage). Only CONFIRMED findings auto-loop to
`xdu-build`.

### [CRITICAL/CONFIRMED] <one-line defect>
- **Where:** `file:line`
- **Failure scenario:** <concrete inputs/state → wrong output/crash>
- **Evidence:** <the command run and what it showed>
- **Touches invariant / requirement:** <R-ID or invariant name>

## Human-gate triggers

Set if any CONFIRMED finding touches the high-blast-radius core (`src/bin/xdu-rm.rs`,
`src/bin/xdu.rs`, `src/crawl.rs`, `src/lib.rs`, `src/cli.rs`, `src/s3.rs`, `src/view_query.rs`,
`src/engine.rs`) or a destructive-rm / schema-stability /
atomic-write / SQL-injection invariant — these **always** require human sign-off before
`xdu-publish`, regardless of auto-loop. (`invariants.md`'s *High-blast-radius files* header is the
authoritative path list; this copy may only ever **widen** to match it.)

- <triggered? which finding?>

## Optional completeness sub-pass (separate reviewer; may see TECH.md)

- Was every planned phase actually shipped? Did scope balloon beyond the appetite? <notes>
