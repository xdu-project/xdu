# META — Bulk copy and move: `xdu-cp` and `xdu-mv`

> **Harness feedback log** for this feature — the producer artifact of the factory's self-improvement
> loop. Written by the lifecycle skills (`xdu-feature` / `xdu-plan` / `xdu-build` / `xdu-review`) when the
> **skillset itself** costs something; read by `xdu-publish` (surfaced in the PR) and applied by
> `/xdu-harness`. This file is **orthogonal** to the `GOAL → PLAN → TECH → REVIEW` spine — it is about
> the *toolchain*, not the feature — and is retained on merge like the rest of `spec/{slug}/`.
>
> **Silence is the default.** The bar for a finding is one test: *was this the **skill's** fault — not
> mine, not the task's?* A merely-hard task, a self-inflicted error, or a one-off content/code issue
> (that belongs in `GOAL.md` / `REVIEW.md`) is **not** a finding. The blind `xdu-review` correctness
> reviewer never reads this file — it would leak author intent.

- **slug:** bulk-copy-move-xdu-cp-mv

## Friction findings

<!-- Real findings are appended below this line by the lifecycle skills. -->

## F1 — Step 3 branches by switching the main checkout; cycles run in worktrees · seen again
`origin=xdu-feature:step3 severity=low category=instruction status=open target=.agents/skills/xdu-feature/SKILL.md`
- **What happened:** Step 3 says `git switch -c {branch} main`, which moves the shared main
  checkout. The dispatcher overrode it with `git worktree add .worktrees/<slug> -b {branch} main`,
  as it has for earlier cycles. Same root as `spec/s3-index-target/META.md` F2 (the "on `main`"
  pre-flight written for one checkout).
- **Skill cause:** The procedure and its `allowed-tools` (no `git worktree`) assume a single
  checkout, though the factory now shapes every cycle in its own worktree.
- **Recommended fix:** Make the worktree form the default in Step 3 and add `Bash(git worktree *)`
  to `allowed-tools`; keep `git switch -c` as the single-checkout fallback, and fold in F2's
  pre-flight change.
- **Confidence:** high · **Effort:** small
