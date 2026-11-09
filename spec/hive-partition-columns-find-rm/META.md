# META — A `key=value` directory name reads like any other in `xdu-find`, `xdu-rm`, `xdu-view`

> **Harness feedback log** for this feature — the producer artifact of the factory's self-improvement
> loop. Written by the lifecycle skills (`xdu-feature` / `xdu-plan` / `xdu-build` / `xdu-review`) when
> the **skillset itself** costs something; read by `xdu-publish` (surfaced in the PR) and applied by
> `/xdu-harness`. The blind `xdu-review` correctness reviewer never reads this file.

- **slug:** hive-partition-columns-find-rm

## What worked well

- `xdu-review` debate mode: because both blind reviewers saved their own test logs, the
  orchestrator could check one against the other. That caught "ship" summing the R6 backport run
  to 247. Its log gives the same count as "block" for every test binary, 251 in all.

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

## F4 — xdu-build has no run B, though P2 closes on one · seen again
`origin=xdu-build:P2 severity=medium category=missing-guidance status=open target=.agents/skills/xdu-build/SKILL.md`
- **What happened:** P2 was split into run A (local) and run B (CI logs) by its TECH body. Run B's
  evidence path (`evidence/p2-ci.md`), its shape, the merge-ref reasoning, the Release-log lines to
  quote and the choice to re-run the stored `verify:` were all taken from
  `spec/s3-index-target/evidence/` and the dispatcher's brief, not from the skill.
- **Skill cause:** Same as `spec/s3-index-target/META.md` F12, still open: Steps 4–7 assume a
  phase's evidence is local, and the run A / run B split lives only in earlier cycles' files.
- **Recommended fix:** As F12 there: a "Run B (close on CI evidence)" step naming
  `spec/{slug}/evidence/p<n>-ci.md`, verbatim lines with run id, job, step and timestamp, the
  merge ref's tree, a re-run of the stored `verify:`, and the close commit's subject.
- **Confidence:** high · **Effort:** small

## F5 — The blind reviewer's runnable repo includes the build's leftover scratch in `target/`
`origin=xdu-review:step-2 severity=medium category=missing-guidance status=open target=.agents/skills/xdu-review/SKILL.md`
- **What happened:** at review time `target/` held the build phases' scratch: the gate logs
  (`p1-lib.log`, `p1-it.log`, `p1-gate.out`), the gate script `p1-verify.sh`, a draft commit
  message, and the `plan`, `scope`, `backport`, `red-*` and `ci` directories. The reviewer is given
  "the full runnable repo" and is sent to `target/release` to run binaries, so it could read all of
  this. The orchestrator fenced it off by adding a rule to each brief.
- **Skill cause:** Step 2's blindness list and the rubric's "What the reviewer sees" name only
  `PLAN.md`, `TECH.md`, `research/` and `META.md`. Yet the build gates in `TECH.md` write their
  logs and scripts into `target/`, so this leak is foreseeable on every cycle, not a one-off.
- **Recommended fix:** add "any file under `target/` that the reviewer did not create; run
  `target/release` binaries only through `temp_index.sh`" to the do-not-read list in Step 2 and in
  the rubric. Better still, have `xdu-build` keep its scratch under a single
  `target/xdu-build/{slug}/`, so the exclusion is one path. This strengthens blindness and weakens
  no gate.
- **Confidence:** high · **Effort:** small

## F6 — The curated inputs pasted "inline" had to be retyped, twice, for debate mode
`origin=xdu-review:step-2 severity=low category=instruction status=open target=.agents/skills/xdu-review/SKILL.md`
- **What happened:** Step 2 says "inline". This cycle took that literally: GOAL.md,
  `invariants.md` and `review-rubric.md`, about 650 lines, were regenerated by hand into each of
  the two debate briefs. No check confirms that either copy matches the committed text.
- **Skill cause:** Same as `spec/s3-index-target/META.md` F9, still open. "Inline" names a
  transport, while blindness needs an exact, curated set of inputs.
- **Recommended fix:** As F9 there: one brief file assembled verbatim by `cat` from the committed
  files at the reviewed head, kept outside `spec/` and recorded in `REVIEW.md`.
- **Confidence:** med · **Effort:** small
