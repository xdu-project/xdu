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

## What worked well

- `xdu-review` debate mode: the two blind reviewers' product findings did not overlap. "Block"
  found the Hive-partition and search-only-ancestor defects, and "ship" found the block-rounded
  `--safe` gap while proving every R-ID. One reviewer would likely have missed one side.

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

## F2 — The plan's rename ban `fs::copy` matches the plan's own `atomic_fs::copy_atomic`
`origin=xdu-build:P1 severity=low category=tooling status=open target=.agents/skills/xdu-plan/SKILL.md`
- **What happened:** Every code phase's gate greps the new files for `fs::copy` to ban
  `std::fs::copy`. PLAN names the primitive `copy_atomic` in the module `atomic_fs`, so the
  natural call `atomic_fs::copy_atomic(...)` in `relocate.rs` turned the ban red with no
  replacing copy anywhere. The build renamed the call site (an unqualified import) to pass.
- **Skill cause:** `xdu-plan` writes text bans without a word boundary and without checking them
  against the identifiers its own design introduces, so a gate can fail on a name it chose.
- **Recommended fix:** In `xdu-plan`, anchor ban patterns on a boundary (`(^|[^_[:alnum:]])fs::copy\(`)
  and dry-run each ban against the PLAN's named identifiers before storing the gate.
- **Confidence:** high · **Effort:** small

## F3 — `xdu-release` states the `#[command(...)]` count as a literal
`origin=xdu-build:P4 severity=low category=instruction status=open target=.agents/skills/xdu-release/SKILL.md`
- **What happened:** P4's sweep for the binary count found `.agents/skills/xdu-release/SKILL.md:96`
  saying "all four `#[command(...)]` blocks in `src/cli.rs` set `version`". There are six now. The
  sweep updated `AGENTS.md` and `invariants.md` §13, but a skill's own text is not a product sweep
  site, so the line was left stale.
- **Skill cause:** The skill restates a count that `AGENTS.md` and `invariants.md` §13 already own,
  so every new binary must also edit a harness file, which a product branch must not do.
- **Recommended fix:** Drop the number ("every `#[command(...)]` block in `src/cli.rs` sets
  `version`") or point at `AGENTS.md` instead of restating it.
- **Confidence:** high · **Effort:** small

## F4 — `temp_index.sh` drives `<cwd>/target/release` while its build honours `CARGO_TARGET_DIR` · seen again
`origin=xdu-review:step2 severity=medium category=tooling status=open target=.agents/factory/bin/temp_index.sh`
- **What happened:** the review gives each agent its own `CARGO_TARGET_DIR`. With that set, run
  from the worktree root, `temp_index.sh`'s `cargo build --release --bins` finished in 0.66 s
  into that dir. Then `command -v xdu-cp` inside the drive named
  `<worktree>/target/release/xdu-cp`, a build from Nov 6 17:57 that predates the merge of `main`.
  The reviewers were only safe because the brief made their target dir `<export>/target`.
- **Skill cause:** the skill makes `temp_index.sh` the one way to drive the CLI. The script's own
  comment says a stale binary is worse than no drive, yet it hard-codes
  `bindir="$root/target/release"` apart from the target dir cargo builds into.
- **Recommended fix:** take `bindir` from `${CARGO_TARGET_DIR:-$root/target}/release`, made
  absolute, or from `cargo metadata --no-deps --format-version 1`'s `target_directory`. Fail if
  `xdu` is missing there.
- **Confidence:** high · **Effort:** small

## F5 — Three restated copies of the high-blast path list lag `invariants.md` · seen again
`origin=xdu-review:step3 severity=low category=instruction status=open target=.agents/skills/xdu-review/SKILL.md`
- **What happened:** this branch widened `invariants.md`'s high-blast list with `src/select.rs`,
  `src/atomic_fs.rs`, `src/relocate.rs` and `src/bin/xdu-mv.rs`. The list restated in
  `review-rubric.md` (human sign-off gate), this skill's Safety Principles and
  `templates/REVIEW.md` still has eight paths. Both blind reviewers flagged it, and the
  orchestrator had to apply `invariants.md`'s list to see that findings in `select.rs` and
  `atomic_fs.rs` trigger the gate.
- **Skill cause:** the list is restated in three harness files. Only a code *move* obliges
  re-deriving the copies, and a product branch must not edit skill text (F3), so every new
  high-blast file leaves three stale copies. Same root as F3.
- **Recommended fix:** replace each copy with a pointer to `invariants.md`'s header, or inject
  that header into the skill at load (`!` command). This widens the effective gate and never
  weakens it.
- **Confidence:** high · **Effort:** small

## F6 — A later cycle's reviewers get the remediation commit subjects, which name the prior findings
`origin=xdu-review:step2 severity=medium category=instruction status=open target=.agents/skills/xdu-review/SKILL.md`
- **What happened:** cycle 2 ran the skill's default for a later cycle, a fresh blind pass. Step 2
  still gives each reviewer `git log --oneline main..HEAD`. On a remediated branch those subjects
  read "F3 — Hive partitioning off in the selection", "F4 — search-only ancestors", and so on,
  and include "Review …: cycle 1 — changes-requested". The REVIEW.md they were barred from is
  restated in the log they were handed.
- **Skill cause:** Step 2's curated inputs predate later cycles, and Step 3's "fresh blind pass"
  does not reconcile with them. `xdu-build` tags remediation commits with the finding ids, so
  every later cycle leaks this way.
- **Recommended fix:** from `review.cycle` ≥ 1, pass the log as hashes only
  (`git log --format=%h main..HEAD`), or strip remediation subjects, and say why in Step 2. This
  strengthens blindness and weakens no gate.
- **Confidence:** med · **Effort:** small

## F7 — No rule for the FSM after a human-approved fix past the review cap
`origin=xdu-build:P3 severity=low category=missing-guidance status=open target=.agents/skills/xdu-build/SKILL.md`
- **What happened:** after review cycle 3 (the cap) the human ruled a LOW fix should land without
  a fourth review. Remediation mode only knows "reopen, fix, `--top-status in_review`", which
  invites a review the loop no longer allows.
- **Skill cause:** neither `xdu-build` nor `xdu-review` says what state a post-cap, human-approved
  fix leaves, or how the approval is recorded for `/xdu-publish`.
- **Recommended fix:** add a post-cap branch to Step 1.3: record the human ruling in TECH.md and
  set a state (or `review.verdict`) that `/xdu-publish` accepts without another review.
- **Confidence:** med · **Effort:** small
