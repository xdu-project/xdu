# META — Ship DuckDB extensions instead of downloading them

> **Harness feedback log** for this feature — the producer artifact of the factory's self-improvement
> loop. Written by the lifecycle skills (`xdu-feature` / `xdu-plan` / `xdu-build` / `xdu-review`) when the
> **skillset itself** costs something; read by `xdu-publish` (surfaced in the PR) and applied by
> `/xdu-harness`. This file is **orthogonal** to the `GOAL → PLAN → TECH → REVIEW` spine — it is about
> the *toolchain*, not the feature — and is retained on merge like the rest of `spec/duckdb-extension-distribution/`.
>
> **Silence is the default.** The bar for a finding is one test: *was this the **skill's** fault — not
> mine, not the task's?* A merely-hard task, a self-inflicted error, or a one-off content/code issue
> (that belongs in `GOAL.md` / `REVIEW.md`) is **not** a finding. The blind `xdu-review` correctness
> reviewer never reads this file — it would leak author intent.

- **slug:** duckdb-extension-distribution

## What worked well

Brief, optional reinforcement: a part of a skill / the harness that materially helped, so `/xdu-harness`
knows what **not** to change. One line each, naming the skill/step. Skip the section entirely if
nothing stands out.

- `xdu-plan` Step 3's gate-shaped exception: on a `small` appetite, the one adversarial reviewer it
  forced found two high-severity false greens in the drafted R1/R6 gates (an empty manifest that
  makes every check vacuous, and a boundary query on a second connection that never crosses the
  boundary).
- `xdu-review` Step 2's artifact-deliverable split: naming the CI-outcome evidence as
  orchestrator-owned kept the blind reviewer on the mechanism, where a local negative control found
  that the release check passes a tarball whose `xdu-view` cannot run.

## Friction findings

Zero or more findings, appended below — each a markdown **section** so appending is a low-corruption
operation and a stdlib parser reads them (`uv run --with pyyaml python .agents/factory/bin/meta_status.py
spec/duckdb-extension-distribution/META.md`). Skills always write `status=open`; only `/xdu-harness` flips it. `target` is a
best-guess file with **no line number** (re-derive the exact edit at apply time to avoid staleness). If
an equivalent finding already exists, append "· seen again" to its title instead of duplicating —
recurrence is signal, not bloat.

Field enums — `severity`: `high` (a safety / gate / correctness gap) `| medium | low`; `category`:
`instruction | steering | tooling | template | missing-guidance`; `status`: `open` (written by skills)
`| applied | rejected | deferred` (written by `/xdu-harness`).

Schema (copy one block per finding, appending it **after** this fence — the fence is illustrative and
is skipped by the parser):

```markdown
## F1 — <one-line title of the skillset problem>
`origin=<skill>:<step> severity=<high|medium|low> category=<instruction|steering|tooling|template|missing-guidance> status=open target=<best-guess file>`
- **What happened:** <what the skill made you do, or fail to do>.
- **Skill cause:** <why this is the instructions' fault — not yours, not the task's>.
- **Recommended fix:** <the concrete change to the skill / template / script>.
- **Confidence:** <high|med|low> · **Effort:** <small|medium|large>
```

<!-- Real findings are appended below this line by the lifecycle skills. -->

## F1 — Verify authoring guidance covers quoting but not `set -e` semantics
`origin=xdu-plan:6 severity=medium category=missing-guidance status=open target=.agents/skills/xdu-plan/SKILL.md`
- **What happened:** drafting `verify:` commands and plan-time gate scripts, I wrote `! grep …`
  and `[ -n "$E" ] && [ -n "$PL" ]` as assertions under `set -eu`. Neither can fail the gate. POSIX
  `errexit` ignores a `!`-negated command and every command of an AND-OR list but the last, and
  `for x in $(cmd)` hides a failing `cmd`. All three were caught by re-reading, not by any rule.
- **Skill cause:** Step 6 tells the author to write `verify:` as plain shell and spells out the
  YAML quoting hazards, but says nothing about the shell semantics that make an assertion
  vacuous. That is the false-green class the gate-shaped exception exists to catch.
- **Recommended fix:** add to Step 6's verify-authoring paragraph: "one assertion per line; under
  `set -e` a `!` command or a non-final `&&` member never fails the gate. Write
  `if cmd; then exit 1; fi`, and capture command output by assignment, not inside `for … in $(…)`."
- **Confidence:** high · **Effort:** small

## F2 — Reviewer is told to run `verify` commands it is blinded to
`origin=xdu-review:2 severity=low category=instruction status=open target=.agents/skills/xdu-review/SKILL.md`
- **What happened:** Step 2 tells the delegated reviewer to "run the relevant `verify` commands".
  Every `verify:` lives in `TECH.md` or `spec/{slug}/verify/`, which the same step forbids the
  reviewer to read and the spec-excluded diff strips. I left the instruction out of the delegation,
  had the reviewer run the repo's own gates and its own drives, and ran `verify/p3.sh` myself.
- **Skill cause:** the instruction and the blindness rule contradict each other, and nothing says
  which wins or whether the orchestrator owns the planned gates.
- **Recommended fix:** in Step 2, replace "run the relevant `verify` commands" with "run the repo's
  gates (fmt, clippy, test) and drive the CLI". In Step 3, add that the orchestrator may re-run the
  `spec/{slug}/verify/` gates as its own evidence, recorded in REVIEW.md's verification run and kept
  out of the reviewer's prompt.
- **Confidence:** high · **Effort:** small

## F3 — Contract-drift check assumes a human is reachable before grading
`origin=xdu-review:1 severity=low category=missing-guidance status=open target=.agents/skills/xdu-review/SKILL.md`
- **What happened:** `GOAL.md` carries an R6 amendment made at the PLAN gate, before any build
  commit, and recorded as a dated Clarification by the gate judge. Step 1 says to confirm such
  commits with the human before grading. The review was dispatched by the factory foreman with no
  human channel, and the portability fallback ("ask in plain text and STOP") would halt the cycle on
  an amendment the dispatcher itself recorded. I graded the amended text and listed the commit in
  REVIEW.md.
- **Skill cause:** Step 1 treats a pre-build amendment recorded in Clarifications the same as a
  silent change during build, and says nothing about a run dispatched without a human in the loop.
- **Recommended fix:** in Step 1, an amendment committed before the first build commit and
  recorded as a dated Clarification is graded as written and listed in REVIEW.md. Only a `GOAL.md`
  change after the first build commit, or one with no Clarification entry, needs confirmation
  before grading.
- **Confidence:** med · **Effort:** small
