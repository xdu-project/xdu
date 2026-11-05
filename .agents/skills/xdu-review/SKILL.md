---
name: xdu-review
description: >-
  Adversarial QA of a completed xdu feature branch. Delegates the correctness pass to a FRESH
  reviewer subagent that sees only GOAL.md + the branch diff + the AGENTS.md invariant checklist + the
  runnable repo — NOT PLAN.md/TECH.md (avoids grading-its-own-homework). The reviewer refutes each
  finding and cites executed commands; CONFIRMED findings loop back to /xdu-build; the coupled core
  forces a human gate. Fourth step of the software factory (see .agents/factory/review-rubric.md).
disable-model-invocation: true
argument-hint: "[debate] [completeness] [status]"
allowed-tools: Read, Grep, Glob, Write, Agent, ReportFindings, AskUserQuestion, Bash(git status *), Bash(git branch *), Bash(git log *), Bash(git diff *), Bash(git rev-parse *), Bash(git add *), Bash(git commit *), Bash(uv run --with pyyaml python .agents/factory/bin/*), Bash(cargo test *), Bash(.agents/factory/bin/temp_index.sh *), Bash(scdoc *), Bash(tail *)
---

# xdu-review — adversarial QA (clean context)

## When to Use

Invoke `/xdu-review` when a branch's `TECH.md` is fully built (`status: in_review`). **Best run in a
fresh session** — but the real guarantee comes from delegating scrutiny to freshly-spawned subagents
with curated inputs, so bias is removed even if this session is not clean. The reviewer grades the
diff against the **locked `GOAL.md`** and the **AGENTS.md invariants**, by **executed command**, not
opinion.

Operating manual: [`.agents/factory/review-rubric.md`](../../factory/review-rubric.md) and
[`.agents/factory/invariants.md`](../../factory/invariants.md). Read them before delegating.

**Harness portability.** Runs on any harness — see [`factory/portability.md`](../../factory/portability.md).
Fallbacks: run the *Current state* commands yourself if not auto-injected; ask in plain text and STOP if
`AskUserQuestion` is unavailable; **if subagents are unavailable, perform the correctness pass yourself
in a clean context** (you lose delegated blindness — compensate with executed evidence, per the rubric);
and skip `ReportFindings` (`REVIEW.md` is the durable record).

## User Instructions

Additional instructions provided with the invocation: $ARGUMENTS

## Current state (injected at load)

- Branch: !`git branch --show-current`
- Base: `main` (feature/fix branch base; confirm from `base:` in TECH.md during Step 1).
- Diffstat vs main: !`git diff --stat main...HEAD 2>/dev/null | tail -n 20`

## Argument Parsing

- `status` → report the current `review` verdict from `TECH.md` and any existing `REVIEW.md`; no work.
- `debate` → run the two-independent-reviewer variant (for high-risk / coupled-core diffs).
- `completeness` → also run the *separate* completeness sub-pass (may see `TECH.md`).

## Safety Principles

- **Blindness is the point.** The correctness reviewer subagent is given `GOAL.md`, the diff, the
  runnable repo, `invariants.md`, and `review-rubric.md` — and is **explicitly told NOT to read
  `PLAN.md`, `TECH.md`, `research/`, or `META.md`** (the last leaks author intent / harness notes, same
  reason as PLAN/TECH). Only this skill (the orchestrator) reads `TECH.md`, and only
  for the `base`/`slug`/`kind` metadata — it must not pass PLAN/TECH *content* into the reviewer prompt.
  **The diff must be blind too:** those artifacts are committed on the branch, so a plain
  `git diff main...HEAD` hands the reviewer PLAN/TECH/research (and any prior cycle's REVIEW.md) as
  added hunks — the `':(exclude)spec/'` pathspec below is load-bearing, not cosmetic.
- **External verification is the spine.** Every finding must cite an executed command
  (`cargo test`, real CLI in a throwaway index via `.agents/factory/bin/temp_index.sh`, the man-page
  render via `scdoc` when a `doc/*.scd` is touched). No assertion-only findings.
  **A render is evidence only when the published text was read** — and only when the literal is matched
  the way CI matches it. Exit 0 is the same "necessary but not sufficient" the build gate already
  rejects for code. Use **`AGENTS.md`'s Commands section verbatim**: the reading form, the
  whitespace-stripped presence form, and the **counting** form for a literal that appears more than
  once. Do not hand-roll a `grep` — `mandoc` breaks lines *inside* a token and your `scdoc` is probably
  not CI's, so an un-normalized check can be green here and red on the runner; a presence check on a
  duplicated literal cannot predict CI at all. Restating that pipeline here is what drifted last time —
  point, don't copy. Record in `REVIEW.md` which literals you confirmed **and by which form**, not just
  that it rendered.
  **When `scdoc` is absent**, do not silently drop the check and do not report an unearned pass. This
  session is read-only and does not install tooling — instead compare each affected binary's `--help`
  flag set against the `.scd` by inspection, **state plainly in `REVIEW.md` that the render was
  unavailable**, and flag it to the human as an unclosed gap (`AGENTS.md`'s Commands section documents
  the one-line install, so it is cheap for them to fix and re-run). This is not hypothetical: a
  `doc/*.scd` that had not compiled for six commits, through a full review cycle, survived precisely
  because no gate on that host could render it. An absent tool is a reported gap, never a pass.
- **Refute before reporting.** Try to disprove each candidate; classify `CONFIRMED` (reproduced) vs
  `PLAUSIBLE` (needs human triage). Default to dropping when uncertain.
- **Scope is narrow:** correctness bugs, GOAL R-ID gaps, AGENTS.md invariant violations
  (auto-CRITICAL), and scope creep (changes mapping to no R-ID). **No style nits, no speculative
  hardening** — a gap-hunting reviewer manufactures gaps.
- **Read-only session.** This skill makes no source edits; it writes `REVIEW.md` and updates the
  `TECH.md` `review` block via `set_phase.py`.
- **Mandatory human gate** when any CONFIRMED finding touches the high-blast-radius core
  (`src/bin/xdu-rm.rs`, `src/bin/xdu.rs`, `src/crawl.rs`, `src/lib.rs`, `src/cli.rs`, `src/s3.rs`,
  `src/view_query.rs`, `src/engine.rs`) or a
  destructive-rm / schema-stability / atomic-write / SQL-injection invariant (`invariants.md`
  §4 / §1 / §2 / §5, including their lettered subsections) — regardless of auto-loop.
  `invariants.md`'s *High-blast-radius files* header is the authoritative copy of that path list;
  this one may only ever **widen** to match it.
- **Bounded loop:** ≤ 2–3 review↔build cycles; escalate to the human on non-convergence.

## Procedure

### Step 1 — Pre-flight
Confirm a feature/fix branch; resolve `{slug}` from the branch, confirm `base` (defaults to `main`),
and read `kind` (the commit `{category}`) from TECH.md. Capture the head SHA (`git rev-parse HEAD`). If `TECH.md` `status`
is not `in_review`/`done`, note it (the build may be incomplete) and ask whether to proceed.

**Contract-drift check:** `git log --oneline main..HEAD -- spec/{slug}/GOAL.md` — anything beyond
the original shaping commit means the locked contract moved mid-build. Surface those commits to the
human and confirm before grading: post-shape clarifications happen legitimately, but a silently
drifted requirement would make this review grade the wrong contract.

### Step 2 — Delegate the correctness pass (fresh subagent)
Launch a fresh `general-purpose` reviewer via the `Agent` tool. Give it, inline, **only**:
- the full text of `spec/{slug}/GOAL.md` (the contract — R-IDs);
- the command to produce the diff: `git diff main...HEAD -- . ':(exclude)spec/'` (and
  `git log --oneline main..HEAD`) — never a bare `git diff main...HEAD`, which would leak the
  committed spec artifacts into the reviewer's context;
- the full text of `invariants.md` and `review-rubric.md`;
- the instruction: work in the runnable repo, follow the refutation protocol, **run** the relevant
  `verify` commands / drive the CLI in a throwaway index (`.agents/factory/bin/temp_index.sh sh -c "…"`,
  never the developer's real one), and **do NOT read `PLAN.md`/`TECH.md`/`research/`
  or `META.md`** (`META.md` is the harness self-improvement log — it leaks author intent, same reason
  as PLAN/TECH).
- the conduct rule: **no edits to tracked files** (revert any instrumentation before returning;
  `git status --porcelain` must be clean on hand-back) **and, if you mutated build state to build a
  negative control, confirmation that it was restored and the restore verified — `git status` cannot
  see `target/`**; and the rubric's "Verdict & loop" section is
  the orchestrator's job — the reviewer must not write `REVIEW.md`, call `ReportFindings`, or run
  `set_phase.py`;
- required return: a structured findings list (severity, CONFIRMED/PLAUSIBLE, file:line, failure
  scenario, the executed evidence) + a requirement→evidence matrix (every R-ID: implemented? verified
  how?) + any unmapped (scope-creep) changes.

**Artifact-deliverable R-IDs are graded by you, not by the reviewer.** Some requirements are satisfied
by a committed document (a research audit, a protocol doc, an assessment) that lives under `spec/` —
which the reviewer is blinded to, and which the `':(exclude)spec/'` pathspec strips from its diff. The
reviewer therefore *structurally cannot* verify them, and left unsaid it will either report them as
unverifiable or guess. So: identify them from **`GOAL.md`'s Clarifications and non-goals first** — a
requirement whose acceptance is phrased as "recorded", "documented", "assessed", or "evidenced in the
spec" is one — with `TECH.md`'s `satisfies` notes as a secondary cross-check (`satisfies` maps R-IDs to
phases, not to deliverable kind, so it cannot carry this signal alone). Apply the one-line test:
*would the reviewer, seeing only the spec-excluded diff, have anything to run?* If no, the
orchestrator owns it. **Name them explicitly in the delegation prompt as out of scope** ("R1 and R8 are satisfied by committed
artifacts under `spec/`; do not attempt them — the orchestrator grades those"), verify them yourself by
reading the artifact, and have `REVIEW.md`'s requirement→evidence matrix **record who verified each**.
This does not weaken blindness: the reviewer still never reads `spec/`. It stops the evidence spine
from silently splitting across two contexts with neither owning the gap.

`debate`: launch **two** independent reviewers (one instructed to argue "ship", one "block") and
reconcile their findings.

### Step 3 — Collect, sanity-check, and report
Read the reviewer's returned findings. Confirm the reviewer left the tree clean
(`git status --porcelain` empty; if not, inspect and revert its leftovers before anything else) **and
that any build-state mutation it reports was restored and the restore verified — `git status` does not
cover `target/`**.
Do a light second-pass sanity check (drop anything not backed by cited evidence).

**A gate's applicability is not settled by a path-scoped diff.** If you did not observe a gate's
current state, record it as "**not observed**" — never "not triggered" or "satisfied". A gate that is
red on `base` is a fact about the branch's merge-readiness even when the diff does not touch its
inputs, and reasoning about CI from the diff is how a red man-page gate got reported as owed-nothing.
(`xdu-publish` Step 1 reads the actual rollup; this session has no `gh` and is not expected to.)

Then:
1. **Cycle 1:** write `spec/{slug}/REVIEW.md` from the template (verification run,
   requirement→evidence matrix, findings most-severe-first, human-gate triggers). **Cycle 2+
   (`review.cycle` ≥ 1): never overwrite** — append a dated `## Review cycle {n} — {verdict}
   ({YYYY-MM-DD})` section; the file is the cumulative review record. A later cycle defaults to a
   fresh blind pass over the full (spec-excluded) diff; the human may instead scope it to verifying
   the remediation of the named findings — record in the section which mode was used.
2. Call `ReportFindings` with the verified findings (most-severe first; empty array if clean),
   `verdict` = CONFIRMED/PLAUSIBLE per finding.

### Step 4 — Set verdict + route
- **Clean (no CONFIRMED):**
  `set_phase.py spec/{slug}/TECH.md --verdict approved --reviewed-commit {sha} --touch` → recommend
  `/xdu-publish`.
- **CONFIRMED findings:**
  `set_phase.py spec/{slug}/TECH.md --top-status blocked --verdict changes-requested
  --reviewed-commit {sha} --blocked-reason "<short>" --touch` → recommend `/xdu-build` to fix the
  named R-IDs/invariants. If any CONFIRMED finding hit the coupled core / a destructive-rm /
  schema-stability / atomic-write / SQL-injection invariant, **STOP and require explicit human
  sign-off** before any further step.
- **PLAUSIBLE only:** surface to the human for triage; do not auto-block.

Every `--verdict` call auto-increments the durable `review.cycle` counter in `TECH.md` — do not
manage it by hand; it is the source of truth for the ≤3-cycle bound and REVIEW.md's "Cycle {n}".

**Meta-note (orchestrator only · silence by default).** Reflect on the **review skillset itself** — not
the diff, not the code. *You (the orchestrator)* may record a finding; the blind reviewer never does,
and content/correctness issues belong in `REVIEW.md`, not here. You may also add a one-line
**What worked well** note when a part of the review skillset materially helped. The bar for a *finding*
is the one test: *was this the skill's fault — not mine, not the task's?* (an ambiguous rubric step, a curated-input/allowed-tools
mismatch, guidance that made the delegation misfire). If met, record it in `spec/{slug}/META.md` (create
from [`templates/META.md`](../../factory/templates/META.md) if absent, else append) — ≤3 terse findings,
next unused `F#`, always `status=open`, "· seen again" instead of duplicating; a fix that would weaken a
non-negotiable gate (blind-review integrity, executed-evidence spine, the human gate, an `invariants.md`
item) is `severity=high` and must say so. **Records only** — `/xdu-harness` applies fixes later:
```markdown
## F<n> — <one-line title>
`origin=xdu-review:<step> severity=<high|medium|low> category=<instruction|steering|tooling|template|missing-guidance> status=open target=<best-guess file>`
- **What happened:** <what the skill made you do, or fail to do>.
- **Skill cause:** <why it's the instructions' fault — not yours, not the task's>.
- **Recommended fix:** <the change to the skill/template/script>.
- **Confidence:** <high|med|low> · **Effort:** <small|medium|large>
```

Then **commit the review artifacts** so the tree stays clean for the loop:
```
git add spec/{slug}/REVIEW.md spec/{slug}/TECH.md   # + spec/{slug}/META.md if you recorded a meta-note
git commit -m "[{category}] Review {slug}: cycle {n} — {verdict}"
```
**No `Co-Authored-By` trailer** (attribution lives in the PR body, not the commit). Do not push.

### Step 5 — Optional completeness sub-pass (`completeness`)
Launch a **separate** fresh subagent that *may* read `TECH.md` and ask: was every planned phase
actually shipped? did scope balloon beyond the appetite? Keep it isolated from the correctness pass so
the plan never contaminates the correctness verdict. Append its notes to `REVIEW.md`.

### Final report
Verdict, CONFIRMED/PLAUSIBLE counts, human-gate status, R-ID coverage, and the recommended next step
(`/xdu-build` to remediate, or `/xdu-publish` when approved). Note the review cycle count; if it's the
2nd–3rd cycle without convergence, escalate to the human.

## Examples

- `/xdu-review` — blind correctness pass; write `REVIEW.md`; set verdict; route.
- `/xdu-review debate` — two independent reviewers for a coupled-core diff.
- `/xdu-review completeness` — correctness pass **plus** the separate did-we-ship-everything sub-pass.
- `/xdu-review status` — show the current verdict and existing findings; no work.

## Notes

- The blind reviewer sees `GOAL.md`, not `PLAN.md`/`TECH.md`: `GOAL.md` is *what/why* (legitimate
  ground truth); the plan is the author's *how* (grading it invites plan-sycophancy).
- Single-model review in a fresh context removes anchoring bias but not family-level self-preference —
  hence the executed-evidence spine and the human gate. This is risk-reduction, not proof.
