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

## F2 — No guidance on who writes a phase's gate scripts, or where
`origin=xdu-plan:step6 severity=low category=missing-guidance status=open target=.agents/skills/xdu-plan/SKILL.md`
- **What happened:** Two gates here (red on `main`, and the backport onto `v0.6.0` + #36) need
  scratch trees and a dozen checks each, too much for one `verify:` line. The previous fix cycle
  kept such gates in `spec/{slug}/verify/*.sh`, but nothing says whether the plan or the build
  writes them. This plan wrote and exercised them, so the build is graded by gates written before
  its code.
- **Skill cause:** Step 6 and the TECH template cover only the inline `verify:` string. A script
  the build writes for its own gate can be shaped to its code, and the skill neither forbids nor
  recommends that.
- **Recommended fix:** In Step 6, name `spec/{slug}/verify/` as the home for multi-step gates. Say
  that the plan writes them, each with a header stating its contract, and exercises them against a
  probe where it can. Say that the build may amend one only in the phase commit, with the reason,
  and never to weaken its header.
- **Confidence:** med · **Effort:** small

## F3 — `temp_index.sh` drives `<cwd>/target/release` while its build honours `CARGO_TARGET_DIR` · seen again
`origin=xdu-build:P1 severity=medium category=tooling status=open target=.agents/factory/bin/temp_index.sh`
- **What happened:** P1's gate runs with `CARGO_TARGET_DIR` set to the cycle's warm target, so the
  drive's own `cargo build --release --bins` compiled into that directory (3m26s) and then ran
  `target/release`, which held whatever was there. The build pre-built `target/release` from the
  final tree (a second full release build) so the drive measured the current code.
- **Skill cause:** Same as `spec/s3-index-target/META.md` F13 and
  `spec/bulk-copy-move-xdu-cp-mv/META.md` F4, still open: the script hardcodes `bindir`.
- **Recommended fix:** As there: derive `bindir` from `${CARGO_TARGET_DIR:-$root/target}/release`.
- **Confidence:** high · **Effort:** small
