---
slug: duckdb-extension-distribution
title: Ship DuckDB extensions instead of downloading them
kind: feature
appetite: small
status: in_progress
branch: feature/duckdb-extension-distribution
base: main
current_phase: P3
last_updated: '2026-11-05'
phases:
- id: P1
  name: 'Engine module: connections refuse autoload; extensions load by path or fail
    naming it'
  status: done
  satisfies:
  - R3
  - R4
  - R5
  depends_on: []
  parallel: false
  hammerable: false
  hill: uphill
  verify: 'set -eu; cargo test --locked --lib engine:: 2>&1 | tee /dev/stderr | grep
    -qE ''test result: ok. [1-9][0-9]* passed''; cargo test --locked --test offline_tests
    --test find_tests --test rm_tests; grep -qF ''disallowed-methods'' clippy.toml;
    cargo clippy --locked --all-targets --all-features -- -D warnings; if grep -rnF
    ''open_in_memory()'' src/bin; then exit 1; fi; for f in src/bin/xdu-find.rs src/bin/xdu-rm.rs
    src/bin/xdu-view.rs; do grep -qF ''open_connection'' $f; done; echo P1-GATE-GREEN'
- id: P2
  name: Probe, fetch and check scripts, proven on the host's own release leg
  status: done
  satisfies:
  - R1
  - R3
  - R4
  - R6
  depends_on:
  - P1
  parallel: false
  hammerable: false
  hill: uphill
  verify: sh spec/duckdb-extension-distribution/verify/p2.sh
- id: P3
  name: Release legs fetch, check and ship the pinned set; CI off the retired macOS
    label
  status: in_progress
  satisfies:
  - R1
  - R6
  depends_on:
  - P2
  parallel: false
  hammerable: false
  hill: crest
  verify: sh spec/duckdb-extension-distribution/verify/p3.sh
- id: P4
  name: Install channels and the recorded layout contract; deferral ledger
  status: done
  satisfies:
  - R2
  depends_on:
  - P2
  parallel: false
  hammerable: false
  hill: downhill
  verify: sh spec/duckdb-extension-distribution/verify/p4.sh
review:
  last_reviewed_commit: 9cd2582
  verdict: changes-requested
  blocked_reason: 'review F1-F3: R6(b) xdu-view unrun, map drift, probe rel dir'
  cycle: 1
---
# TECH.md — Ship DuckDB extensions instead of downloading them

The **context engine and finite-state machine** for building this feature. The YAML
frontmatter above is the resume ground-truth (read it with
`uv run --with pyyaml python .agents/factory/bin/next_phase.py spec/duckdb-extension-distribution/TECH.md`);
the per-phase checklists below are the work. `xdu-build` executes the next actionable phase, runs its
`verify:` command, updates state via
`uv run --with pyyaml python .agents/factory/bin/set_phase.py …`, and makes one atomic code+state commit.

- **Vision / requirements (locked):** [`GOAL.md`](GOAL.md) — R-IDs are the contract.
- **Authoritative design:** [`PLAN.md`](PLAN.md).
- **Backing research:** [`research/00-digest.md`](research/00-digest.md) + briefs.
- **Gate scripts:** `verify/p2.sh`, `verify/p3.sh` (+ `p3.py`) and `verify/p4.sh` were written at plan time,
  before the code they drive. A phase may fix a bug in its gate script only with the reason in the
  commit body, and never by weakening a control that must go red.

## Frontmatter field reference

See the template ([`.agents/factory/templates/TECH.md`](../../.agents/factory/templates/TECH.md));
nothing here departs from it.

## Conventions (apply to every phase)

- Commit conventions, code style, and load-bearing invariants come from
  [`AGENTS.md`](../../AGENTS.md) — it is the constitution. Consult
  [`.agents/factory/invariants.md`](../../.agents/factory/invariants.md) (§4, §5, §10, §11, §13 here).
- One phase per `xdu-build` invocation; one atomic commit containing **both** the code and the
  `TECH.md` state change, subject `[feature] Build duckdb-extension-distribution P<n>: …`.
- **No `Co-Authored-By` trailer.**
- **No CLI change in this cycle**, so no `doc/*.scd` change and no edit to the CI literal list. If a
  phase finds it needs one, stop and escalate: that would be an operator-visible diagnostic, which
  is a non-goal.
- **Network.** P2 and P4 fetch from `extensions.duckdb.org`. P3's gate fetches `pyyaml` through
  `uv` (`pypi.org`, `files.pythonhosted.org`) into fresh cache dirs under `$TMPDIR`. Nothing else
  needs it.
- **No push, no `gh`.** No phase pushes, opens a PR, or calls `gh`. P3's external wait is handed
  back to the foreman (run A / run B below).
- **No `spec/` ids in source, comments, scripts or workflow files.**

## Review cycle 1 — remediation triage

[`REVIEW.md`](REVIEW.md) (cycle 1, changes-requested at 9cd2582) raised three findings. Triage:

- **F1 (MEDIUM), fix now → P2.** `check.sh` runs every shipped binary under the cold `HOME`,
  `xdu-view` included (`--version`, stdin `/dev/null`, stdout a pipe). P2's gate gains the
  review's control: a tarball whose `bin/xdu-view` is `#!/nonexistent/interpreter` plus the engine
  `source_id` must fail `check.sh`, on that binary. No existing control is weakened.
- **F2 (MEDIUM), fix now → P4.** `AGENTS.md`'s Repository map gains `src/engine.rs`,
  `examples/extension-probe.rs`, `clippy.toml` (its `disallowed-methods` rule and what it
  enforces) and `packaging/extensions/`, in the map's existing style. P4 owns the `AGENTS.md`
  contract text.
- **F3 (LOW), folded into F1's phase → P2.** The probe compares the loaded path against the
  extension directory made absolute the same way the loader makes the file absolute, so a relative
  `XDU_EXTENSION_DIR` that loads correctly passes. The loader is unchanged: R3 takes the override
  as given. Unit tests in the probe hold a relative directory green and a genuinely outside path
  (and a sibling sharing the directory's name prefix) red; P2's gate drives the relative case.
- **Contract drift note.** R6 was amended at the PLAN gate (f3f09c2). The gate judge confirmed the
  amendment; it stands and is not reverted.
- **P3 reopened.** F1 and F3 change release-path inputs (`packaging/extensions/`, `examples/`,
  `Cargo.toml`), so the four-leg evidence at ab0f7d9 no longer describes the release path. P3
  runs again in two runs, as its body says: this remediation is run A; run B refreshes
  `evidence/p3-pr-run.md` from a CI run of the post-fix head.
- **One commit for P2, P3 run A and P4.** The reopened phases share a same-commit obligation:
  `AGENTS.md` (P4) now states what `check.sh` (P2) proves, and the tightened `verify/p3.sh` (P3)
  is what turns the stale evidence red once the release-path change lands. Split, one commit
  would carry a false manual or a green P3 gate over stale evidence.

---

## Phase P1 — Engine module
**Satisfies:** R3, R4, R5 · **Depends on:** —
**Goal:** every DuckDB connection in xdu opens with autoinstall and autoload off, and one library
function loads a pinned extension by absolute path or fails naming the path. Readers behave exactly
as before.

- [x] Add `src/engine.rs` and `pub mod engine;` in `src/lib.rs` (PLAN §2.1): `PINNED_EXTENSIONS`,
      `EXTENSION_DIR_ENV`, `EXTENSION_SUBDIR`, `open_connection`, `extension_dir_from` (pure),
      `extension_dir`, `load_extension_from`, `load_extension`. Canonicalize **only**
      `current_exe()`, take `.parent().parent()`, and never canonicalize the override. A missing
      override directory must still yield the R4 message with the path as given.
- [x] `clippy.toml` with `disallowed-methods` for `duckdb::Connection::{open, open_in_memory,
      open_with_flags, open_in_memory_with_flags}`; `#[allow(clippy::disallowed_methods)]` only
      inside `src/engine.rs` (including the red-first refusal test's plain connection).
- [x] Write the refusal test **first** against a plain `Connection::open_in_memory()`. It sets
      `extension_directory` to a `TempDir`, runs `SELECT '{}'::JSON`, and asserts the error carries
      DuckDB's autoload-disabled wording **and** the directory stays empty. Observe it red, then
      switch it to `open_connection` and observe green. Record both outcomes in the commit body: a
      green that was never red proves nothing here, and a network-less sandbox can turn the
      autoinstall attempt into an error that only the wording assertion tells apart.
- [x] Unit tests in `src/engine.rs`: both `current_setting`s are false; absent file → error contains
      the full `<dir>/httpfs.duckdb_extension` path and `XDU_EXTENSION_DIR`, directory untouched;
      the same holds for an override directory that does not exist; a name outside
      `PINNED_EXTENSIONS` is refused; a directory path containing `'` is refused before any SQL
      runs; `extension_dir_from` gives a non-empty override precedence, unchanged, ignores an
      empty one, and otherwise resolves to the exe's grandparent joined with `lib/xdu/extensions`.
- [x] `src/bin/xdu-find.rs`, `src/bin/xdu-rm.rs`, `src/bin/xdu-view.rs`: replace
      `Connection::open_in_memory()` with `xdu::engine::open_connection()`. Nothing else in these
      files changes. `xdu-rm.rs` is high-blast: the diff there is the one line plus an import.
- [x] `tests/offline_tests.rs` is **not** edited. Its module doc already describes the cold-`HOME`
      assertion; the "holds by refusal" story belongs in `AGENTS.md`, which P4 updates.
- **Verify:** `cargo test --lib engine::` reports at least one passing test, offline/find/rm suites
  green, `clippy.toml` carries the rule and clippy is clean, no `open_in_memory()` left in
  `src/bin`, each reader calls `open_connection`.
- **Touches:** `src/engine.rs` (new), `src/lib.rs` (one line), `clippy.toml` (new), `src/bin/xdu-find.rs`,
  `src/bin/xdu-rm.rs`, `src/bin/xdu-view.rs`.

## Phase P2 — Probe and scripts, on the host leg
**Satisfies:** R1, R3, R4, R6 (mechanism on the host leg; the four-leg run is P3 run B) · **Depends on:** P1
**Goal:** the host can do exactly what a release leg will do: build, fetch the pinned set for its
own engine and platform, assemble, and pass the cold-`HOME` load check. Every control that must go
red does.

- [x] `examples/extension-probe.rs` (PLAN §2.3). `manifest` validates its own field shapes and
      refuses an empty set. `load` uses **one** connection throughout: the settings read false, the
      `'{}'::JSON` refusal shows the autoload-off hint, each pinned extension is loaded and printed
      as `loaded=<name> <path>`, and a `read_parquet` against `http://127.0.0.1:9/x.parquet` fails
      with `httpfs`'s own connection error. Any `autoload` / `INSTALL` / `requires the extension`
      wording is a failure. Exit non-zero on any miss, with the reason on stderr.
- [x] `packaging/extensions/footer.sh`, `fetch.sh`, `check.sh` (PLAN §2.4). Write them in POSIX `sh`
      with `set -eu` and one assertion per line, because under `set -e` neither an `&&` list nor a
      `!` command stops the script. Capture the manifest by assignment, never `for x in $(…)`. Run
      every probe and reader under `env -i PATH="$PATH" HOME="$cold"`. The `lib/xdu/extensions`
      members must equal the manifest set. Add the three R4 negative controls (existing empty
      override, missing override, `lib/` removed), each matching the loader's wording and the
      exact path, and drive `xdu-rm --dry-run` beside `xdu-find --count`. Put scratch in a
      self-cleaning `mktemp -d` resolved with `pwd -P`. `check.sh` ends by printing
      `extension-check: ok platform=<platform> engine=<engine>` as its last line, and nothing
      after it. P3 run B reads that line from each CI log.
- [x] Confirm on the host's release build that the manifest's `source_id` occurs in `xdu-find`,
      `xdu-view` and `xdu-rm` after `strip` + LTO. **If it does not, stop and escalate.** Do not
      drop the check or swap in a weaker one. Name it what it is in comments: an engine-*version*
      tie (PLAN §5).
- [x] Confirm `load` leaves no file under the cold `HOME`, including any secrets directory `httpfs`
      might create.
- **Verify:** `sh spec/duckdb-extension-distribution/verify/p2.sh`. It covers the good path, the
  good path again under a hostile exported environment, and eleven controls that must fail:
  - `footer.sh` on the platform, version and magic fields tampered, on a short file, on a wrong
    engine, on a non-extension, and with empty arguments;
  - `check.sh` on a tarball without `lib/`, on one with an extra extension, and on one with a
    tampered extension.
- **Touches:** `examples/extension-probe.rs`, `packaging/extensions/{footer,fetch,check}.sh`.

> **Amendment (P2, review cycle 1 remediation):** F1 — `check.sh` tested `bin/xdu-view` only for
> its mode bits and the `source_id` bytes, so a tarball whose `xdu-view` could not execute passed.
> It now runs `--version` for all four binaries under the cold `HOME` and requires `<name>
> <version>` back. F3 — the probe's containment check compared the loader's absolute result
> against the override as given; it now makes the directory absolute first (`loaded_within`), with
> unit tests, and `Cargo.toml` sets `test = true` on the example so `cargo test` runs them in CI.
> **Gate retuned (controls added, none weakened):** `verify/p2.sh` adds a twelfth must-fail
> control (an unexecutable `xdu-view` carrying the `source_id`, which must fail `check.sh` with
> `bin/xdu-view --version did not run`), a relative-override drive that must load from
> `<cwd>/ext`, and the probe's unit tests with the outside-path case named.
>
> - [x] `check.sh` executes every shipped binary (F1).
> - [x] The probe resolves the extension directory as the loader does (F3), with unit tests.
> - [x] `verify/p2.sh` holds the F1 control and the F3 drive, each seen red first.

## Phase P3 — Release and CI workflows
**Satisfies:** R1, R6 · **Depends on:** P2
**Goal:** every release leg fetches, ships and checks the pinned set before upload; the workflows
run on runners that exist; a draft PR exercises all four legs without publishing, and their
results are recorded before review.

P3 waits on something outside the build, so it runs twice. **Neither run pushes, opens a PR, or
calls `gh`.**

- **Run A** does the checklist below, then runs `sh spec/duckdb-extension-distribution/verify/p3.sh
  structural` (must print `P3-RUN-A-GREEN`). It commits the code with P3 left `in_progress`
  (`set_phase.py --phase P3 --phase-status in_progress --hill crest`), then **hands back** to the foreman.
  It does not mark P3 done. The foreman pushes the branch and opens a draft PR, which triggers
  `release.yaml` on all four legs. P4 does not depend on P3, so the foreman may dispatch it while the
  legs run.
- **Run B** is resumed by the foreman with the run's URL and the four legs' logs (or the lines from
  them). It writes `spec/duckdb-extension-distribution/evidence/p3-pr-run.md`, which holds:
  - `run: <actions run URL>`;
  - `head: <sha the run built>`, which must be an ancestor of `HEAD`;
  - one line per leg, `<target>: <the leg's final check.sh line>`, copied verbatim.

  It then runs the full gate `sh spec/duckdb-extension-distribution/verify/p3.sh` and marks P3
  `done`. If any leg is red, run B does not mark P3 done. It records the failing leg's output in
  the evidence file, sets `--blocked-reason`, and hands back. A fix is a new run A.

- [x] `release.yaml` both build jobs (PLAN §2.5): one locked build of `--bins --example
      extension-probe`; a fetch step (Linux: after the glibc-floor step); assemble tars
      `bin share lib`; a check step on the exact tarball name being uploaded, before the upload.
- [x] `build-macos` matrix: `aarch64-apple-darwin` → `macos-15`, `x86_64-apple-darwin` →
      `macos-15-intel`. Keep the rust-cache key per target.
- [x] Add a `pull_request` trigger with `paths:` covering `.github/workflows/release.yaml`,
      `packaging/extensions/**`, `examples/**`, `src/engine.rs`, `.cargo/**`, `Cargo.toml`,
      `Cargo.lock` and `rust-toolchain.toml`. Give `TAG` a `pr<number>` fallback and `publish` an
      `if: github.event_name != 'pull_request'`. Make `cancel-in-progress` true for PR runs only,
      so release runs stay uncancellable. The check step passes the probe as
      `target/${{ matrix.target }}/release/examples/extension-probe`. Fetch and check carry no
      `if:` and no `continue-on-error`.
      *Amended in run A:* on a pull request, the `prepare`, `build` and `build-macos` checkouts
      take `github.event.pull_request.head.sha` instead of the merge ref, so the sha the run
      builds is a commit on this branch and run B's `head:` ancestor check can hold.
- [x] `test.yaml`: `macos-14` → `macos-15`. `docker.yaml`: add `examples/**` and
      `packaging/extensions/**` to the PR paths.
- **Verify:** `sh spec/duckdb-extension-distribution/verify/p3.sh`. In run A, only its
  `structural` mode is expected to pass. In run B, the full gate also requires the evidence file:
  one passing `check.sh` line per leg, with the platform matching its target, a single engine
  version across legs, and a `head` sha on this branch. `xdu-review` grades R6 from that file plus
  P2's host run.
- **Touches:** `.github/workflows/{release,test,docker}.yaml`; run B adds
  `spec/duckdb-extension-distribution/evidence/p3-pr-run.md`.

> **Amendment (P3, review cycle 1 remediation):** P3 is reopened because F1 and F3 changed
> release-path inputs after the evidence head (ab0f7d9). The remediation's code is this cycle's
> **run A**: no workflow changes, `p3.sh structural` green, P3 left `in_progress` at `crest`.
> **Run B** works exactly as above, against a CI run of the post-fix head.
> **Gate tightened:** the full `verify/p3.sh` checked only that the evidence head is an ancestor
> of `HEAD`, which stale evidence still satisfies. It now also requires that no release-path input
> (`release.yaml`'s `pull_request` paths) differs between the evidence head and `HEAD`. Until run B
> lands, the full gate is red on that check, as it should be.

## Phase P4 — Install channels, the contract, the deferral ledger
**Satisfies:** R2 · **Depends on:** P2
**Goal:** every channel R2 names puts `lib/xdu/extensions/` where the canonical executable looks,
and the new layout is the recorded contract.

- [x] `install.sh` (PLAN §2.6): install `lib/xdu/extensions/*` into
      `$(cd "$INSTALL_DIR" && pwd -P)/../lib/xdu/extensions` when present. Copy each file to a
      temporary name in that directory, then `mv` it over the old one, never `cp` in place.
      Tarballs without `lib/` install as before. Keep `rm` where AGENTS.md says the installer keeps
      it.
- [x] `Dockerfile`: builder gets `curl` and copies `examples/` + `packaging/extensions/`, builds the
      probe in the same `cargo build`, stages `/out/{bin,lib/xdu/extensions}`, runs `fetch.sh` then
      `check.sh` on a tarball of the staging. The runtime stage takes the checked tree whole
      (`COPY --from=builder /out/ /usr/local/`) and then runs
      `RUN test -f /usr/local/lib/xdu/extensions/httpfs.duckdb_extension`. Check that
      `.dockerignore` lets both new paths in. No container
      runtime is on the planning host, so `docker.yaml`'s PR validate job is this change's
      build evidence.
- [x] `hpccm/xdu.py`: docstring says the tarball's `lib/xdu/extensions/` lands in the prefix beside
      `bin/`. Run `make check` (regenerated specs must not drift).
- [x] `AGENTS.md`: the Packaging layout bullet, the §13 summary line, and the `offline_tests.rs`
      bullet (autoload is now refused at `open_connection`, and the cold-`HOME` test holds by
      refusal). Mirror the layout in `.agents/factory/invariants.md` §13 in the **same commit**.
- [x] `README.md` Installation: where extensions live, `XDU_EXTENSION_DIR`, and that
      `cargo install` builds carry none.
- [x] **Deferral ledger.** Walk P1–P4 for "defer", "follow-up", "known limitation", "not this
      cycle" and confirm each has an `issues/` home with a ROADMAP entry. The known ones:
      `issues/s3-index-target.md` gets the `XDU_EXTENSION_DIR` man-page entry, due when a CLI path
      first loads an extension, and the `cargo install` caveat.
      `issues/native-os-packages-deb-rpm.md` gets the requirement that `.rpm`/`.deb` ship
      `lib/xdu/extensions/` (the RPM spec builds from source and must fetch). A new
      `issues/hpccm-extension-guard.md` (from the ISSUE template) plus a ROADMAP entry records the
      HPCCM `test -f` guard, due in the release commit that first moves `VERSION` to a tarball
      carrying `lib/`. Adding the guard earlier would break the recipe on `main`. If the PR run shows
      `linux_amd64` mixed C++ runtimes need a `.cargo/config.toml` change, that goes to a new
      `issues/` file plus a ROADMAP entry, never to `META.md`.
      *Amended in the build:* the PR run showed no abort on `linux_amd64` (P3 evidence), so no
      `.cargo/config.toml` deferral was filed. A foreman ruling added
      `issues/macos-sdk27-bundled-duckdb-build.md` plus a ROADMAP entry: the macOS 27 SDK's libc++
      rejects the vendored DuckDB 1.4.4 build, so host builds need a 26.x `SDKROOT`, and CI
      images will meet it when they move to SDK 27.
- **Verify:** `sh spec/duckdb-extension-distribution/verify/p4.sh`. It runs `install.sh` through a
  `curl` shim with `XDU_INSTALL` outside `XDU_PREFIX` and again through a symlinked install
  directory, plus a second install over the first, then loads under a cold `HOME` from each, and
  greps the contract and deferral records.
  *Gate fix in the build:* the fixture staged an empty `share/man/man1/`, which no release tarball
  has, and `install.sh`'s existing `cp share/man/man1/*.1` failed on the unmatched glob. The
  fixture now stages one stub page; no control changed.

> **Amendment (P4, review cycle 1 remediation):** F2 — the Repository map now lists
> `src/engine.rs` (the one place a connection opens), `examples/extension-probe.rs`, `clippy.toml`
> and its `disallowed-methods` rule, and `packaging/extensions/`. The Packaging layout bullet says
> what `check.sh` proves, since F1 changed it. The enforcement it cites is CI's own: `cargo clippy
> --all-targets -D warnings` and the release legs' `check.sh` step. `invariants.md` §13 names no
> `check.sh` behaviour, so nothing there changes.
> **Class sweep, scope widened by one site:** the defect is "a path the map should name, it does
> not". `verify/p4.sh` now evaluates that predicate over the tree: every `src/*.rs`,
> `src/bin/*.rs`, `examples/*.rs` and `packaging/*/` path, and `clippy.toml`, must appear in the
> map block. Against the pre-fix `AGENTS.md` it is red on `src/engine.rs`; it then found
> `packaging/rpm/`, missing since before this cycle, so the map names it too. No existing check
> changed.
>
> - [x] Repository map entries for the four paths (F2), plus `packaging/rpm/` (sweep).
> - [x] `verify/p4.sh` asserts the map predicate, seen red first.
- **Touches:** `install.sh`, `Dockerfile`, `.dockerignore` (if needed), `hpccm/xdu.py`, `AGENTS.md`,
  `.agents/factory/invariants.md`, `README.md`, `ROADMAP.md`, `issues/s3-index-target.md`,
  `issues/native-os-packages-deb-rpm.md`, `issues/hpccm-extension-guard.md` (new).

---

## How `xdu-build` drives this

1. `next_phase.py` prints the next actionable phase (statuses are authoritative; the
   `current_phase` pointer is reconciled against them).
2. Pre-flight: clean tree, on `branch`, `base` reachable.
3. Execute every `[ ]` in the phase (consult `PLAN.md` / `research/` for detail).
4. Run the phase's `verify:` command — never advance on a checkbox alone.
5. Amend this file freely if reality diverges (regenerate frontmatter with `set_phase.py`; note the
   amendment in the commit body). STOP and escalate only on a **`GOAL.md` contradiction**.
6. Mark the phase `done`, advance `current_phase`, `--touch`; one `[feature]` commit; stop and report.
