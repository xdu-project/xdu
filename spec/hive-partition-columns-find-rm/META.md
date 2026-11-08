# META — A `key=value` directory name reads like any other in `xdu-find`, `xdu-rm`, `xdu-view`

> **Harness feedback log** for this feature — the producer artifact of the factory's self-improvement
> loop. Written by the lifecycle skills (`xdu-feature` / `xdu-plan` / `xdu-build` / `xdu-review`) when
> the **skillset itself** costs something; read by `xdu-publish` (surfaced in the PR) and applied by
> `/xdu-harness`. The blind `xdu-review` correctness reviewer never reads this file.

- **slug:** hive-partition-columns-find-rm

## Friction findings

## F1 — The "on `main`" pre-flight does not admit a cycle worktree detached at `origin/main` · seen again
`origin=xdu-feature:step1 severity=low category=instruction status=open target=.agents/skills/xdu-feature/SKILL.md`
- **What happened:** The cycle ran in a worktree detached at `origin/main`. Step 1 stops unless the
  branch is `main`, and Step 3's `git switch -c {branch} main` names the local `main`. The dispatcher
  again had to say "treat the detached HEAD as on main and branch from HEAD".
- **Skill cause:** Same as `spec/s3-index-target/META.md` F2, still open: the check is written for
  the single-checkout flow, though the factory runs cycles in detached worktrees by design.
- **Recommended fix:** As F2 there: accept a detached `HEAD` equal to `origin/main` after the fetch
  as "on main", and branch from `HEAD` in that case.
- **Confidence:** high · **Effort:** small
