---
name: xdu-release
description: >-
  Human-gated cutter of xdu versions — the operational sibling of xdu-harness that fills the gap
  xdu-publish leaves ("version bumps / releases are a separate concern... out of scope here"). Two
  modes: `release` (final vX.Y.Z off main) and `pre-release` (SemVer prerelease vX.Y.Z-rc.N off main;
  ghcr latest/major/minor suppressed by metadata-action type=semver). Shared core: bump the single
  version source (Cargo.toml) + `cargo update -p xdu`, bump the hpccm recipe version + regenerate its
  specs, rebuild man/completions only if the CLI changed,
  run the CI-mirror gate (cargo fmt --check → clippy -D warnings → test --locked → build --release
  --locked), sign an annotated tag, then — only after an explicit human OK before the first irreversible
  step — push + `gh release create`, and verify the GitHub release / ghcr / Actions. Rehearses every op
  + the full gate in an isolated git-worktree dry-run first. Operational, NOT a lifecycle step: never
  writes META findings, never recurses, never weakens a gate.
disable-model-invocation: true
argument-hint: "<release|pre-release> <vX.Y.Z> [--skip-dry-run] | status"
allowed-tools: Read, Edit, Grep, Glob, AskUserQuestion, Bash(cargo build *), Bash(cargo test *), Bash(cargo clippy *), Bash(cargo fmt *), Bash(cargo update *), Bash(cargo run *), Bash(scdoc *), Bash(git status *), Bash(git branch *), Bash(git rev-parse *), Bash(git log *), Bash(git diff *), Bash(git fetch *), Bash(git switch *), Bash(git add *), Bash(git commit *), Bash(git push *), Bash(git tag *), Bash(git worktree *), Bash(gh release *), Bash(gh run *), Bash(gh repo *), Bash(curl *), Bash(mktemp *), Bash(head *), Bash(ls *)
---

# xdu-release — cut a version (release / pre-release), human-gated

## When to Use

Invoke `/xdu-release` to bump the version and cut a release — the concern `/xdu-publish` explicitly
leaves out ("Version bumps / releases are a separate concern (`/xdu-release`), out of scope here"). It
is an **operational sibling of `/xdu-harness`, NOT a lifecycle step**: it touches no `spec/`, no FSM,
no `GOAL/PLAN/TECH/REVIEW`; it moves the version and tag and publishes artifacts. This is where
**irreversible, permanent** outward publishes happen — a version tag and its release assets can
**never** be reused — so it always confirms before the first push and rehearses everything in an
isolated git worktree first.

Reference: [`factory/invariants.md`](../../factory/invariants.md) §13 (version single-sourced;
`share/` generated + CI-asserted; the CI-mirror gate), the "Packaging & release" section of
[`AGENTS.md`](../../../AGENTS.md), and this skill (it codifies the maintainer's release and
pre-release procedures — this file is now their ground truth).

**Harness portability.** Runs on any harness — see [`factory/portability.md`](../../factory/portability.md).
Fallbacks: run the *Current state* commands yourself if not auto-injected; ask in plain text and STOP
if `AskUserQuestion` is unavailable. `git` / `gh` / `cargo` are portable shell, and the worktree
dry-run is plain `git worktree` (available everywhere) — no Claude-specific affordance is load-bearing
here.

## User Instructions

Additional instructions provided with the invocation: $ARGUMENTS

## Current state (injected at load)

- Branch: !`git branch --show-current`
- Tree (must be clean): !`git status --porcelain | head -n 20`
- Version (Cargo.toml — the only source): !`head -n 5 Cargo.toml`
- Recent tags: !`git tag -l --sort=-v:refname | head -n 8`
- main tip: !`git log --oneline -1 main 2>/dev/null`
- Default remote branch: !`gh repo view --json defaultBranchRef -q .defaultBranchRef.name 2>/dev/null || echo "(gh unavailable)"`

(The GitHub release assets / ghcr tags are checked in the post-publish verify step, not at load — they
are network probes.)

## Argument Parsing

Parse `$ARGUMENTS` case-insensitively for the mode/flags (the version is case-sensitive). If
self-contradictory or ambiguous, STOP and ask.

- **Mode** (first positional, required): `release` | `pre-release`. Missing → STOP and offer the two
  via `AskUserQuestion`.
- **`status`** as the sole token → Step 0 fast-path (no work).
- **Version** (second positional, required — **never inferred/auto-bumped**; a permanent tag is too
  dangerous to guess): carries the **`v` prefix** (`v0.4.2`, `v0.5.0-rc.1`). Validate SemVer, strictly
  greater than the latest tag (version-precedence ordering, e.g. via
  `git tag -l --sort=-v:refname | head -n 1`), and not already a tag
  (`git rev-parse -q --verify refs/tags/vX.Y.Z`). **Mode/suffix consistency:** `pre-release` REQUIRES a
  SemVer prerelease suffix (`-rc.N` / `-alpha.N` / `-beta.N`); `release` REQUIRES a final version (no
  suffix). Any mismatch → STOP.
- **Flags:** `--skip-dry-run` opts out of Step 2 (must be explicit; discouraged, and **forbidden for
  `release`** — the final cut is highest-stakes and never skips the rehearsal). Any unrecognized token →
  STOP and ask.
- **Infer from the mode (do not ask):** `--prerelease` (pre-release only), which docker tags move.
  Base branch is always `main`, worked **in place** (no release branch). **Require explicitly:** mode,
  version.

## Safety Principles

- **Confirm before every irreversible/outward step.** Push and the GitHub/ghcr publish are permanent —
  a version tag can NEVER be reused. Steps 1–6 are all reversible in-tree; the single Step 7
  `AskUserQuestion` gate precedes the first push. **No push without an explicit human OK.**
- **Dry-run first (default on).** Every op + the full gate is rehearsed in an isolated `git worktree`
  before a single real ref moves. `--skip-dry-run` requires an explicit flag and is forbidden for
  `release`.
- **GitHub Flow on `main`.** There is no `develop` or `master` and no back-merge — the bump commit and
  the tag both land on `main`, a single line of history. Never force-push `main`.
- **The gate is non-negotiable.** `cargo fmt --all -- --check`, `cargo clippy --all-targets
  --all-features -- -D warnings`, `cargo test --locked`, and `cargo build --release --locked` must ALL
  pass. A red gate is a STOP, never an override-to-ship.
- **Version is single-sourced, with one tracked exception.** Bump `Cargo.toml`;
  `cargo update -p xdu` refreshes the `xdu` entry in `Cargo.lock`. Confirm the bump with
  `xdu --version` — all four `#[command(...)]` blocks in `src/cli.rs` set `version`, so clap
  derives `-V`/`--version` from `Cargo.toml` — or read `Cargo.toml`, or the `xdu=` line of a
  completion marker, which is built from `CARGO_PKG_VERSION`. The `doc/*.scd` man sources carry
  **no** version string and completions derive from `src/cli.rs`, so a pure version bump does
  **not** rebuild them; `share/` is a generated artifact built in CI regardless
  ([`invariants.md`](../../factory/invariants.md) §13). The `hpccm/xdu.py` recipe `VERSION` is
  the single exception: it pins the release the containers install, so the bump moves it in
  lockstep with regenerated specs (Step 4). Never hardcode a version anywhere else.
- **Signed tags only.** A signed annotated tag (`git tag -s`, letting git pick the configured signing
  key — **do not hardcode a key id**), verified with `git tag -v` **before** any push.
- **Release notes are drafted, then confirmed.** Auto-draft from `git log <lasttag>..HEAD` grouped by
  `[category]`; present for human edit at Step 7; never publish unreviewed notes.
- **Never `rm`.** `target/` is gitignored (leave it); `git worktree remove` cleans the rehearsal. Do
  not delete files.
- **Operational, not meta.** `xdu-release` never writes `META.md` findings and never recurses; harness
  friction here goes to `/xdu-harness`.
- **Branch discipline + keep the trailer.** Work in place on `main`; use `[release]` subjects on the
  bump commit; never force-push. **No `Co-Authored-By` trailer** (attribution lives in the PR body, not the commit).

## Procedure

The **shared core** is Steps 4–5 (bump + version lock, then the gate); the two modes differ only at
Step 7 (`--prerelease`) and Steps 8–9 (which docker tags move):

| mode | base | working branch | tag lands on | `gh release` | ghcr `:latest` / `{{major}}` / `{{major}}.{{minor}}` |
|---|---|---|---|---|---|
| **release** | `main` | `main` (in place) | the `main` bump commit | *(stable)* | move to this version |
| **pre-release** | `main` | `main` (in place) | the `main` bump commit | `--prerelease` | **suppressed** (metadata-action `type=semver`) → only `:X.Y.Z-rc.N` + `:sha-<short>` |

### Step 0 — status fast-path (when requested)
`status` (or no meaningful args): report the current version, the latest tags, the `main` tip, whether
the intended target is already a tag, and whether a release looks in-flight (an unpushed local tag).
Stop.

### Step 1 — Parse + pre-flight
Parse `$ARGUMENTS` (see Argument Parsing) → resolve mode and version. Require a **clean tree**
(`git status --porcelain` empty → else STOP: commit/stash first). `git fetch origin`; confirm the
`main` tip matches `origin/main` (diverged → STOP). STOP if the target version is already a tag or is
not strictly greater than the latest tag, or if the mode/suffix rule is violated.

### Step 2 — Worktree dry-run (default ON; `--skip-dry-run` opt-out, forbidden for release)
Rehearse the ENTIRE release in isolation before any real ref moves:
1. `dir=$(mktemp -d)` (outside the repo, so it never dirties the working tree — mirrors
   `factory/bin/temp_index.sh`'s throwaway discipline); `git worktree add --detach "$dir/rel" main`
   (**`--detach`** — `main` is already checked out in the main tree and git refuses to check out the
   same branch twice; a detached worktree at the `main` commit sidesteps that and needs no branch of
   its own).
2. In that worktree, replay the shared core (Step 4: bump + `cargo update -p xdu`, the hpccm
   `VERSION` bump + `make -B` regen, and any man/completion rebuild only if the CLI changed) and
   the **full gate** (Step 5: `cargo fmt --check` + `cargo clippy … -D warnings` +
   `cargo test --locked` + `cargo build --release --locked`).
3. `git worktree remove "$dir/rel"`. Any red → STOP and report; **nothing in the real tree moved.**

Portability: this is plain `git worktree` + the same gate commands — no Claude-specific affordance. If
the *Current state* wasn't auto-injected, run the probes yourself first. (A few permission prompts may
appear for commands run against the `/tmp` worktree path; that is expected and harmless.)

### Step 3 — Mode setup (real tree)
Both modes work **in place on `main`** — no release branch, no cherry-pick. Confirm you are on `main`
at the intended commit; the Step 4 bump commit and the Step 6 tag land here. (There is no
`develop`/`master` and no back-merge — GitHub Flow keeps a single line of history.)

### Step 4 — Shared core: bump + version lock + commit
1. Edit the `version = "…"` line in `Cargo.toml` → `X.Y.Z` (strip the leading `v`; this is the ONLY
   source).
2. `cargo update -p xdu` (refresh the `xdu` entry in `Cargo.lock`).
3. **HPCCM recipe tracks the release.** Bump `VERSION` in `hpccm/xdu.py` to `X.Y.Z` (strip
   the leading `v`; the recipe re-adds it), regenerate from `hpccm/` with `make -B`
   (requires `hpccm` on `PATH` — `pipx install 'hpccm==26.5.0'`), and confirm the spec diff
   shows only version lines. **Extension guard:** from 0.6.0 (`FIRST_WITH_LIB` in `xdu.py`) the
   recipe's smoke step also runs `test -f {prefix}/lib/xdu/extensions/httpfs.duckdb_extension`;
   confirm both regenerated specs still carry that line. A release that drops it ships an image
   that builds green and fails at its first S3 query. The guard checks `httpfs` alone; if
   `xdu::engine::PINNED_EXTENSIONS` grows, extend it to every pinned name. Unlike `share/`, the
   specs are committed artifacts, so they ride the bump commit; CI (`make check` in the packaging
   job) asserts freshness.
4. **Man pages + completions rebuild only if the CLI changed.** The `doc/*.scd` sources carry no
   version string and completions derive from `src/cli.rs`, so a pure version bump touches neither —
   and `share/` is generated in CI (`release.yaml`) regardless and is git-ignored (do not commit it).
   A CLI change is the *feature's* same-commit responsibility and is already on `main`; a version cut
   normally rebuilds nothing here. Only if you must (an out-of-band CLI fix rode in): `scdoc <
   doc/NAME.1.scd > share/man/man1/NAME.1` and `cargo run --release --bin gen-completions …`.
5. **Verify** `git diff` shows ONLY the version bump: `Cargo.toml` (the version line),
   `Cargo.lock` (the `xdu` version), `hpccm/xdu.py` (the `VERSION` line), and the regenerated
   `hpccm/xdu.def` + `hpccm/xdu.docker` (version lines only; the extension guard's `test -f`
   stays in both).
6. Commit `[release] Bump version to vX.Y.Z`, staging **exactly** these files: `Cargo.toml`,
   `Cargo.lock`, `hpccm/xdu.py`, `hpccm/xdu.def`, `hpccm/xdu.docker`.
   **No `Co-Authored-By` trailer.**

### Step 5 — Gate (mirrors CI; non-negotiable)
Run all of: `cargo fmt --all -- --check`; `cargo clippy --all-targets --all-features -- -D warnings`;
`cargo test --locked`; `cargo build --release --locked`. Any failure → STOP (never override-to-ship).
`target/` is gitignored; leave it (do not `rm`).

### Step 6 — Land the release commit + signed tag
Both modes: no merge — the Step 4 bump commit is the current `main` HEAD, and the tag lands on it.
Then create a **signed annotated tag** `git tag -s vX.Y.Z -m "xdu vX.Y.Z"` (git picks the configured
signing key — no hardcoded key id) and `git tag -v vX.Y.Z` to verify the signature **before anything is
pushed**. Signing/verify failure → STOP.

### Step 7 — PAUSE: confirm before anything irreversible
Everything so far is reversible in-tree. Draft the release notes from `git log <lasttag>..HEAD` grouped
by `[category]` (link `#NN` issues if present). Then present via `AskUserQuestion`: the mode, version,
the tag layout, `--prerelease` yes/no, whether the ghcr `:latest`/`{{major}}`/`{{major}}.{{minor}}`
tags will move, the drafted notes (human-editable), and the exact push/publish commands. **Nothing is
pushed until an explicit OK.** `AskUserQuestion` unavailable → ask in plain text and STOP.

### Step 8 — Push + publish
Push the branch, then the tag (the tag must be on the remote before `--verify-tag`):
`git push origin main` then `git push origin vX.Y.Z`.

Then `gh release create vX.Y.Z --verify-tag [--prerelease] --title "xdu vX.Y.Z" --notes-file <file>`
(`--prerelease` for pre-release mode only). This fires `release.yaml` (→ per-target `.tar.gz` binaries
+ `SHA256SUMS`, attached as GitHub release assets) and `docker.yaml` (→ ghcr).

### Step 9 — Verify after publish
- **GitHub release:** `gh release view vX.Y.Z --json assets,isPrerelease` — the per-target
  `xdu-vX.Y.Z-<target>.tar.gz` assets and the `SHA256SUMS` file are present; `isPrerelease` is `true`
  and GitHub "Latest" stays on the prior stable for pre-release, `false`/Latest for release.
- **ghcr:** fetch an anonymous pull token
  (`curl -s "https://ghcr.io/token?scope=repository:xdu-project/xdu:pull"`), then
  `GET /v2/xdu-project/xdu/tags/list` — confirm `:latest`/`{{major}}`/`{{major}}.{{minor}}` moved
  (release) or are ABSENT/unmoved with only `:X.Y.Z-rc.N` + `:sha-<short>` published (pre-release;
  metadata-action `type=semver` suppresses them for a SemVer prerelease).
- **Actions:** `gh run list` / `gh run watch` for `release.yaml` + `docker.yaml` success.
- Confirm the `main` tip and the tag point where intended.

### Step 10 — Report
Mode, version, tag SHA + signature status, what published where (GitHub release URL, ghcr tags),
`:latest` / docker-tag state, CI run URLs, and any caveat.

## Examples

- `/xdu-release release v0.4.2` — final off `main`; ghcr `:latest`/`{{major}}`/`{{major}}.{{minor}}`
  move to this version.
- `/xdu-release pre-release v0.5.0-rc.1` — SemVer prerelease off `main`; `--prerelease`; ghcr
  `latest`/`{{major}}`/`{{major}}.{{minor}}` suppressed.
- `/xdu-release status` — current version, tags, `main` tip, in-flight check; no changes.
- `/xdu-release pre-release v0.5.0-rc.2 --skip-dry-run` — skip the worktree rehearsal (discouraged).

## Notes

- **Reference `invariants.md` §13, don't duplicate it** (single-source version from `Cargo.toml`;
  `share/` generated + CI-asserted; the `cargo fmt`/`clippy`/`test` gate). This skill introduces no new
  numbered invariant.
- **pre-release semantics recap:** GitHub "Latest" stays on the last stable release; ghcr suppresses
  `latest`/`{{major}}`/`{{major}}.{{minor}}` for a SemVer prerelease (docker/metadata-action
  `type=semver`), publishing only `:X.Y.Z-rc.N` + `:sha-<short>`. The `release` event runs `docker.yaml`
  from the **tagged commit's tree**, so `main`'s workflow is what runs.
- The maintainer's release + pre-release process is codified here; this SKILL.md is its ground truth.
- Never `rm`; `target/` is gitignored, and `git worktree remove` cleans the rehearsal.
