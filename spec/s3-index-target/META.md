# META — Build an index into S3 and query it in place

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

- **slug:** s3-index-target

## What worked well

- `xdu-review` Step 2, the `debate` variant: in review cycle 4 the "block" reviewer found a
  residual in the class the previous cycle fixed (the readers' CN fallback), which the "ship"
  reviewer's own TLS drives did not reach; negative controls built from a `git archive` export
  let the orchestrator confirm both findings through the project's own wire harness.

## Friction findings

Zero or more findings, appended below — each a markdown **section** so appending is a low-corruption
operation and a stdlib parser reads them (`uv run --with pyyaml python .agents/factory/bin/meta_status.py
spec/{slug}/META.md`). Skills always write `status=open`; only `/xdu-harness` flips it. `target` is a
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

## F1 — A pilot split's deferred scope has no named destination or owner
`origin=xdu-feature:step4 severity=medium category=missing-guidance status=open target=.agents/skills/xdu-feature/SKILL.md`
- **What happened:** The size circuit-breaker split this seed into a pilot and follow-ups. The skill
  says to "record the deferred scope in Non-goals", while `methodology.md` says a deferral becomes an
  `issues/{slug}.md` plus a `ROADMAP.md` entry. Nothing says which step files those seeds, or when,
  so the GOAL had to guess that they are filed when the cycle publishes.
- **Skill cause:** The circuit-breaker and the deferral convention each say where scope goes, and
  neither names the step that turns a pilot's Non-goals into seeds.
- **Recommended fix:** In the circuit-breaker bullet, state that each deferred item in Non-goals
  names its future seed path, and that `xdu-publish` files the seeds and `ROADMAP.md` entries (or
  that `xdu-feature` files them itself, if the shaping step should own it).
- **Confidence:** med · **Effort:** small

## F2 — The "on `main`" pre-flight does not admit a cycle worktree detached at `origin/main`
`origin=xdu-feature:step1 severity=low category=instruction status=open target=.agents/skills/xdu-feature/SKILL.md`
- **What happened:** The cycle ran in a worktree detached at `origin/main`. Step 1 says to stop
  unless the branch is `main`, and `git switch -c {branch} main` names the local `main`, which can be
  behind `origin/main`. The dispatcher had to say "treat it as on main" for the step to proceed.
- **Skill cause:** The safety check is written for the single-checkout flow only, though the factory
  now runs cycles in worktrees.
- **Recommended fix:** Accept a detached `HEAD` equal to `origin/main` after the fetch as "on main",
  and branch from `HEAD` in that case rather than from the local `main`.
- **Confidence:** med · **Effort:** small

## F3 — The adversarial-review rule for gate-shaped work reads as lean-path only
`origin=xdu-plan:step3 severity=medium category=instruction status=open target=.agents/skills/xdu-plan/SKILL.md`
- **What happened:** This big-appetite plan makes test infrastructure carry an R-ID (a byte meter
  for a cost bound, an opt-in test tier with a "nothing ignored" CI guard). Step 3's "gate-shaped
  deliverables" exception, which requires an adversarial reviewer, sits under the lean-path bullet
  and says the fan-out "is not skippable on the lean path", so it was unclear whether the big path
  owes the same review. The planner ran one anyway.
- **Skill cause:** The rule's placement and wording scope it to the path where research is skipped,
  while the failure it guards (a gate that fails green) does not depend on appetite.
- **Recommended fix:** Lift the gate-shaped rule out of the lean-path bullets into its own
  appetite-independent paragraph: any phase whose deliverable is a test, meter or CI guard that an
  R-ID rests on gets an adversarial reviewer before `PLAN.md` is committed.
- **Confidence:** med · **Effort:** small

## F4 — set_phase.py re-serializes the whole frontmatter on its first use
`origin=xdu-build:P1 severity=low category=tooling status=open target=.agents/factory/bin/set_phase.py`
- **What happened:** The first `set_phase.py` call of the build (P1 to `in_progress`, hill to
  `crest`) rewrote the plan's hand-written frontmatter from flow style into block style, re-quoted
  scalars and folded every `verify:` string across lines. A two-field state change became a
  100-line diff, and the only way to see that no gate changed was to load both versions with PyYAML
  and compare the `verify:` values (they were equal).
- **Skill cause:** The plan step writes the frontmatter by hand in one style and the build step's
  mandated script emits another, so the first build commit always carries a cosmetic rewrite of
  every phase's gate, which is exactly the text a reviewer must not have to re-read.
- **Recommended fix:** Have `set_phase.py` emit the same style the TECH template uses (flow lists,
  one-line single-quoted `verify:`), or have `xdu-plan` generate the frontmatter through the script
  so both steps share one serializer.
- **Confidence:** high · **Effort:** small

## F5 — An "is it absent" gate reads cargo's exit status, so any cargo failure passes it
`origin=xdu-build:P3 severity=high category=template status=open target=.agents/skills/xdu-plan/SKILL.md`
- **What happened:** P3's dependency-floor gate is `if cargo tree --locked -e normal -i $c
  >/dev/null 2>&1; then …exit 1; fi`, so any non-zero exit counts as "absent". Run with the
  registry unreachable (a sandbox denial of `static.crates.io` here), the same probe exited
  non-zero for a reason other than absence and printed "absent" for all three crates. It also
  checks the host target only: on macOS a dependency gated to Linux (native-tls pulls
  `openssl-sys` only off Apple targets) is invisible to it.
- **Skill cause:** The planning and build guidance says "see the gate fail once" but gives no rule
  for absence probes, where the failing direction is the green one; the plan wrote the probe in the
  shape that cannot fail closed. Weakness in a gate that guards a binding floor is high severity.
- **Recommended fix:** Absence probes must match the tool's own "not found" text (`did not match
  any packages`) and fail on any other non-zero exit, and a dependency probe names `--target all`
  (or each release target). Add that rule to the gate-writing guidance and retune P3's `verify:`
  through `set_phase.py --verify-file` at the foreman's word.
- **Confidence:** high · **Effort:** small
- **P3 run B:** on the foreman's ruling, P3's `verify:` was retuned this way (TECH, P3 gate
  amendment). The finding stays open for the gate-writing guidance itself.

## F6 — The blind diff's `spec/` exclusion hides a CI gate kept under `spec/`
`origin=xdu-review:step-2 severity=medium category=missing-guidance status=open target=.agents/skills/xdu-review/SKILL.md`
- **What happened:** `.github/workflows/test.yaml` runs `spec/s3-index-target/verify/wire.sh` as
  the CI S3 gate. The blind diff's `':(exclude)spec/'` pathspec drops that script, and the skill
  does not say whether a reviewer may read a `spec/` file that the non-spec diff runs. The
  orchestrator granted a one-file exception on its own judgement.
- **Skill cause:** the pathspec assumes `spec/` holds design records only. Once a branch puts a
  standing gate there, a later change to the gate is invisible to every blind review, and the
  skill gives no rule for that case.
- **Recommended fix:** in Step 2, tell the orchestrator to grep the spec-excluded diff for paths
  under `spec/`. Each file named there is executable gate code: hand it to the reviewer by name
  and list it in `REVIEW.md`. Pair this with the rule `REVIEW.md` finding 5 asks for, that a
  standing gate does not live under `spec/`. Blindness is unchanged, since PLAN, TECH,
  `research/` and META stay barred.
- **Confidence:** high · **Effort:** small

## F7 — The human gate keys on paths, and its widening rule covers moved code but not new homes
`origin=xdu-review:step-4 severity=high category=instruction status=open target=.agents/factory/review-rubric.md`
- **What happened:** `src/s3.rs` is new, and it carries §2's finalize and prune for S3, §2b's
  marker sequencing, and the credential resolver. Three CONFIRMED findings land in it. The gate
  lists name `src/crawl.rs` and `src/bin/xdu.rs` for the same duties, but not `src/s3.rs`. The
  rubric's "may only ever widen" rule is written for a diff that *relocates* code. So the skill
  left open whether the gate applied, and the orchestrator recorded "not triggered by the letter,
  triggered by the widening".
- **Skill cause:** the gate is decided by path, and the obligation to widen it is tied to
  relocation. A new file that takes on a listed duty, such as a second sink or a second reader
  surface, escapes both. This is a gap in a non-negotiable gate.
- **Recommended fix:** in the rubric and the skill's Step 4, key the gate to *duties*. A CONFIRMED
  finding in any file implementing a duty a listed path is there for (atomic finalize and prune,
  marker sequencing, reserved-name or partition-name refusal, reader SQL, the credential path)
  triggers it. Add the rule "a diff that gives a listed duty a new home widens every list in the
  same diff", worded for new code as well as moved code.
- **Confidence:** high · **Effort:** small

## F8 — Remediation's sweep rule bars the harness edits the rubric's widening rule requires
`origin=xdu-build:remediation-cycle-1 severity=medium category=instruction status=open target=.agents/skills/xdu-build/SKILL.md`
- **What happened:** review finding 2 asked that the high-blast path lists be widened to the
  files that now hold the listed duties. Four of the five lists are harness files: `invariants.md`,
  `review-rubric.md`, `xdu-review`'s SKILL.md and `templates/REVIEW.md`. Step 2.3 says a harness
  change "must not ride in on a product branch". The rubric says a diff that moves a duty
  "re-derives all of them in the same diff". The two rules point opposite ways. This run went
  ahead only because the gate judge ruled it.
- **Skill cause:** Step 2.3's four-homes test treats every edit under `.agents/` as a change to
  what an agent is told. A path-list widening is a different kind of edit: the rubric itself
  requires it to follow product code. The skill does not carve it out.
- **Recommended fix:** in Step 2.3, exempt widenings the rubric requires. A diff that gives a
  listed duty a new home widens every copy of the list in the same diff, and widening is the only
  harness edit allowed. Anything else under `.agents/` still goes through META and
  `/xdu-harness`.
- **Confidence:** high · **Effort:** small

## F9 — The reviewer's curated inputs are to be pasted "inline", which means retyping ~700 lines
`origin=xdu-review:step-2 severity=low category=instruction status=open target=.agents/skills/xdu-review/SKILL.md`
- **What happened:** Step 2 says to give the blind reviewer the full text of `GOAL.md`,
  `invariants.md` and `review-rubric.md` "inline". At cycle 2's head those total about 690 lines,
  and the `debate` variant needs two copies. A model orchestrator can put text inline only by
  regenerating it, and a slip there would silently hand the reviewer a different contract. This
  cycle instead built one brief file with `cat` from the three committed files, under the
  review's scratch directory in `target/`, and pointed both reviewers at it. That file was their
  only route to `spec/` content.
- **Skill cause:** "inline" names a transport, but what blindness needs is an exact, curated
  input set. The skill does not sanction the exact transport, so using it is a deviation that
  each orchestrator has to justify again.
- **Recommended fix:** in Step 2, prefer a brief file assembled verbatim by `cat` from the
  committed files at the reviewed head, outside `spec/`, and have `REVIEW.md` record that it was
  used. Blindness is unchanged: the file holds only the three allowed texts.
- **Confidence:** med · **Effort:** small · seen again (review cycles 3 and 4)

## F10 — A command a remediation writes into the manual is never run in the form it was written
`origin=xdu-build:remediation-cycle-2 severity=medium category=missing-guidance status=open target=.agents/skills/xdu-build/SKILL.md`
- **What happened:** cycle 1's remediation documented the wire gate in `AGENTS.md` as
  `XDU_TEST_EXTENSION_DIR=target/test-ext sh tests/s3-wire.sh "" 43`. Its own gate ran the script
  with the variable unset, so the script's absolute default applied, and the documented relative
  form failed 21 of 43 tests until review cycle 2 ran it.
- **Skill cause:** Step 2.3 says to gate on the defect's predicate and to cite only enforcement
  that survives the merge. It says nothing about a command the fix writes into the manual, whose
  defining property is "runs as written". So the gate checked that the text was present, not
  that it worked.
- **Recommended fix:** in Step 2.3, add: a remediation that writes or changes a runnable command
  in `AGENTS.md`, a man page or the README runs that command verbatim in its gate (or, when the
  command needs a network or a destructive target, runs it with only that argument redirected,
  and says so).
- **Confidence:** high · **Effort:** small

## F11 — The rubric defines security severity only for the SQL-injection surface
`origin=xdu-review:step-3 severity=medium category=instruction status=open target=.agents/factory/review-rubric.md`
- **What happened:** cycle 3 confirmed that the S3 readers accept any TLS certificate while the
  writer verifies. The defect had been present since cycle 1 and survived two `debate` rounds. To
  grade it, the orchestrator had to decide whether "security weakening (the DuckDB SQL-injection
  surface)" in the CRITICAL row is an example or the whole class. It chose HIGH, as a real bug
  on a common path.
- **Skill cause:** the severity table names one security surface by its parenthetical. Nothing
  in the rubric or in `invariants.md` names transport trust, though a diff that adds a network
  path brings that surface with it. Under "no speculative hardening", nothing pointed a reviewer
  there, so finding it depended on one reviewer's initiative.
- **Recommended fix:**
  - In `review-rubric.md`'s severity table, say whether security weakening outside the
    SQL-injection surface is CRITICAL or HIGH.
  - In Scope, add that a diff that opens a network path is checked for server authentication on
    every client it adds, and for parity between them. A reproduced gap there is a correctness
    finding, not hardening.
  - The product rule itself belongs in `invariants.md` via the build that fixes the finding, not
    here.
- **Confidence:** med · **Effort:** small

## F12 — xdu-build has no run B, though every phase here closes on one
`origin=xdu-build:P6 severity=medium category=missing-guidance status=open target=.agents/skills/xdu-build/SKILL.md`
- **What happened:** P6's run B was briefed as "follow the skill's run B / CI-evidence
  procedure". The skill has none. The evidence file's shape, the commit subject (`Close {slug}
  {id} on CI evidence`), which local gates to re-run at close, and the merge-ref reasoning were
  all taken from earlier close commits and `evidence/p1-ci.md`…`p5-ci.md`. TECH's run B item
  also named a red-first half (test 4(a), Linux writer, on the pre-phase tree) that no CI log
  of a post-phase head can show, and the skill does not say whether such an item blocks `done`.
- **Skill cause:** Steps 4–7 assume the phase's evidence is local. A phase split into run A
  (local) and run B (CI logs) is the norm on this branch, but the split, its artifact and its
  close rules live only in commit history.
- **Recommended fix:** add a "Run B (close on CI evidence)" step: quote lines verbatim from
  saved logs with run id, job and step, and check each with `grep -F`; establish the merge
  ref's tree; re-run the stored `verify:`; commit `[{kind}] Close {slug} {id} on CI evidence`.
  Say that a red-first which needs the pre-phase tree on another platform must run in run A
  (or a throwaway worktree), since run B sees only the fixed tree.
- **Confidence:** high · **Effort:** small

## F13 — temp_index.sh drives `target/release` whatever `CARGO_TARGET_DIR` says
`origin=xdu-review:step-2 severity=medium category=tooling status=open target=.agents/factory/bin/temp_index.sh`
- **What happened:** the `debate` variant runs two reviewers and the orchestrator at once, so in
  review cycle 4 each built into its own `CARGO_TARGET_DIR`. `temp_index.sh` builds with
  `cargo build --release --bins`, which follows that variable, but then runs
  `$root/target/release/xdu` and puts `$root/target/release` on `PATH`
  (`bindir="$root/target/release"`). That build dated from 11:57, before `fdc3270` (14:27) and
  the code head `5711e9d` (20:39), so every drive through the script would have tested a pre-TLS
  binary while looking current. The orchestrator told the reviewers to drive their own binaries
  directly instead.
- **Skill cause:** the skill and the rubric name `temp_index.sh` as the way to drive the CLI, and
  the script's own comment says it exists so that a drive never measures a stale build. It
  defeats that whenever the target directory moves, which concurrent reviewers need.
- **Recommended fix:** set `bindir="${CARGO_TARGET_DIR:-$root/target}/release"`, or take the
  executables' paths from `cargo build --message-format=json`. In Step 2, say that concurrent
  reviewers each take their own `CARGO_TARGET_DIR`.
- **Confidence:** high · **Effort:** small
