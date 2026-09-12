# PLAN — Portable Linux release baseline for RHEL8/9

> **Status:** Draft for review · **Last updated:** 2026-09-11
> **Authoritative technical design.** The *how*. Vision/contract is [`GOAL.md`](GOAL.md);
> the phased executable roadmap is [`TECH.md`](TECH.md). No `research/` — appetite is small
> and the unknowns were closed with three image probes plus one toolchain probe, recorded
> under rabbit holes below.

## 1. Summary

Move the two gnu legs of `.github/workflows/release.yaml` off `ubuntu-24.04` into
`manylinux_2_28` containers on native runners, keep the C++ runtime dynamically linked
(measured usage sits under RHEL8 stock ceilings, and a static-link attempt leaves the
dynamic needs in place), gate every release leg on symbol-version scans that fail closed,
and retarget the HPCCM recipe to a bookworm-class runtime once the floor is proven. No
`src/` change: this is release topology plus a recipe default.

## 2. Design

**Builder move (`release.yaml` build matrix).** Both gnu legs run as container jobs:
`quay.io/pypa/manylinux_2_28_x86_64:latest` on `ubuntu-24.04`,
`quay.io/pypa/manylinux_2_28_aarch64:latest` on `ubuntu-24.04-arm` (both images resolve;
docker.yaml already proves the arm runner). Compilation inside the container is native per
arch, so the `gcc-aarch64-linux-gnu` cross install and the `.cargo/config.toml` linker
stanza go away. Rust arrives via rustup (`curl` ships in the image), pinned to the channel
`rust-toolchain.toml` names — `dtolnay/rust-toolchain` cannot run inside a container job.
`cargo build --release --locked --target ${{ matrix.target }}` is unchanged otherwise, and
the assemble/publish jobs are byte-untouched (R4 rests on that diff plus a local layout
replication in P2). Release builds accept cold cargo registries: `Swatinem/rust-cache`
paths do not map across the container boundary, and a warm cache is not worth a poisoning
vector on the publish path. The 90-minute timeout already covers a cold bundled-DuckDB
compile.

**Dynamic C++ runtime (measured safe).** The original design statically linked
libstdc++/libgcc, but the P2 proof overturned it two ways: a static-link attempt leaves
`NEEDED libstdc++.so.6 libgcc_s.so.1` in place even through direct rustc invocation, while
measured usage needs at most `GLIBCXX_3.4.22` against RHEL8 stock 3.4.25 and `GCC_4.2.0`
against libgcc-8.5.0 — dynamic linkage with wide margin. So the legs carry no `RUSTFLAGS`,
and the R3 gate below asserts the version ceilings instead of a linkage mechanism.

**R3 symbol guard (release.yaml, post-build, pre-assemble).** One step per gnu leg, running
inside the same container where `objdump` is confirmed present. For each of the four
binaries it asserts version ceilings and fails closed on each: the maximum `GLIBC_X.Y`
node (via `objdump -T`, pattern `GLIBC_[0-9.]*`, `sort -Vu`) is at most `GLIBC_2.28`, with an
empty result treated as failure rather than a vacuous pass; `GLIBCXX_*` is at most the
RHEL8-stock `3.4.25` and `GCC_*` at most `8.0.0` (libstdc++-8.5.0 / libgcc-8.5.0, measured
on ubi8), with absent families passing (pure-Rust `xdu` needs no C++ symbols); and a
`NEEDED` allowlist (libc, libm, libdl, libpthread, libgcc_s, libstdc++) catches any
surprise new `.so`. `GLIBC_PRIVATE` nodes are excluded by the numeric pattern — they are
intra-libc implementation detail, not a floor requirement. The adversarial review of this
gate is recorded under rabbit holes; its verdict is that the gate fails closed in every
examined case and has no known fail-green input.

**HPCCM retarget (R5).** With the floor at 2.28, bookworm's 2.36 runs every binary, so the
recipe default `runtime_base` returns to `debian:bookworm-slim` with both specs regenerated
(`make -B`) in the same change. The per-binary glibc prose in `xdu.py`/`README.md` is
rewritten as the new floor statement, and the `--version` smoke runs stay — they remain the
guard against a future baseline regression. Proof without waiting for a release: regenerate
with `--userarg version=<latest published tag>` into a scratch directory, `docker build`
that Dockerfile (stage 0 downloads the real tarball and exercises the SHA256SUMS check),
and run all four `--version` entrypoints.

**R1 proof without a release.** The true R1 verdict arrives at the next release, but the
same probes run pre-merge: compile the tree locally inside the manylinux container with the
P1 flags, then execute each binary's `--version` under `ubi8/ubi-minimal` (glibc 2.28
exactly) and `debian:bookworm-slim` (2.36), plus the `GLIBC_*` strings scan. The exact probe
commands are fixed in §6 so the release run repeats them verbatim.

### Requirement → design map

| R-ID | Design element(s) that satisfy it |
|------|-----------------------------------|
| R1 | Local manylinux compile + `--version` under ubi8-minimal and bookworm (P2); release run repeats the §6 probes |
| R2 | Container legs on `manylinux_2_28_{x86_64,aarch64}` images in `release.yaml` (P1) |
| R3 | Post-build symbol/NEEDED scan step in each gnu leg, fail-closed (P1); adversarial review below |
| R4 | Assemble/publish jobs untouched (diff evidence) + local `tar tzf` layout replication (P2) |
| R5 | `runtime_base` back to bookworm-slim + `make -B` regen + stand-in image proof (P3) |

## 3. Invariant gate (AGENTS.md constitution check)

Checked against [`.agents/factory/invariants.md`](../../.agents/factory/invariants.md) (§1–§13)
**before** research and **again** after this design was drafted.

- §13 (project conventions / packaging) — the only section touched. The hpccm `VERSION`
  lockstep with `Cargo.toml` plus regenerated specs is already the recorded rule there, and
  this design implements exactly that rule. The new release.yaml step extends the
  generated-artifact posture (assert, don't assume) without changing the required-checks set.
- §1–§12 — untouched. No schema, atomicity, partition, deletion-safety, SQL, Unix-only,
  concurrency, symlink, sort, CLI-truth, altitude, or TUI surface is involved; the change is
  confined to CI topology and committed container specs.

### Deviation justifications

| Deviation | Why needed | Simpler alternative rejected because |
|-----------|-----------|--------------------------------------|
| —         | —         | — |

## 4. Rabbit holes (resolved)

No `research/` fan-out (lean path); each unknown below was closed with a direct probe.

- *Do the pinned builder images exist for both arches, and is there a faithful RHEL8 probe
  image?* → `quay.io/pypa/manylinux_2_28_x86_64:latest`,
  `quay.io/pypa/manylinux_2_28_aarch64:latest`, and
  `registry.access.redhat.com/ubi8/ubi-minimal:latest` all resolve via manifest inspect.
- *Does manylinux_2_28's toolchain actually compile this tree, and what does it imply for
  libstdc++?* → glibc 2.28, GCC 14.2.1, `objdump`/`readelf`/`curl`/`git`/`python3` present.
  The P2 proof then overturned the static-link conclusion drawn here: measured linkage needs
  at most `GLIBCXX_3.4.22` / `GCC_4.2.0` (RHEL8 stock carries 3.4.25 / 8.0.0), and a
  static-link attempt leaves the dynamic needs in place — so the design keeps dynamic
  linkage and guards the ceilings instead.
- *Can the R3 gate pass while broken (fail-green)?* Adversarial pass over the drafted check:
  missing `objdump` errors under `set -eu` (fail-closed); fully static input yields no
  version nodes and the non-empty guard fails it (fail-closed); `GLIBC_PRIVATE` is correctly
  excluded (not a floor); the `NEEDED` allowlist is now inclusive (libc through libstdc++)
  with the version ceilings doing the real work — a future DuckDB bump pulling
  `GLIBCXX_3.4.30` trips the ceiling even though the `.so` name is allowed; running the step
  before assemble (ordering constraint recorded for P1) keeps a breach from reaching the
  tarball. Exercised for real in P2: the v0.5.1 floater trips both breach classes at once
  (`GLIBC_2.38`/`2.39` and `GLIBCXX_3.4.29`). No fail-green input found.

## 5. Risks & open questions

- **Cold release builds.** Dropping the cargo cache lengthens release legs (cold DuckDB
  compile, historically ~15–30 min) inside the 90-minute timeout. Accepted: releases are
  infrequent and the cache path across the container boundary is a poisoning vector. If a
  leg ever nears the timeout, the fix is a longer timeout or a `CARGO_HOME`-on-workspace
  cache — not this cycle.
- **rustup inside the container.** The pinned channel installs from `rust-toolchain.toml` at
  leg time; a yanked or unreachable toolchain would fail the leg loudly, never silently.
  No action beyond reading the channel dynamically (same `sed` the current legs use).
- **R1's final verdict lands at the next release.** P2 banks the full local pre-proof; §6
  fixes the probe commands so the release run repeats them instead of reinventing them.
- **`hammerable` is uniformly false.** Every phase guards either the publish path or its
  evidence; cutting any of them ships unverified release topology. That is deliberate, not
  an omission.

## 6. Verification strategy

Each phase's `verify:` is a runnable form of one of these probes (exact commands fixed here
so P2, P3, and the future release run stay identical):

- **Floor probe (R1):** build in the manylinux container with P1 flags; `strings` scan shows
  no `GLIBC_2.2[9-9]`+ requirement beyond 2.28; each binary prints `--version` and exits 0
  under ubi8-minimal and under bookworm-slim (bind-mounted read-only).
- **Gate probe (R3):** the release.yaml step itself, exercised locally against the P2
  binaries — intact tree passes; a deliberately floated binary (e.g. one built on 24.04)
  fails. The negative case is what makes the gate more than decoration.
- **Layout probe (R4):** replicate the assemble stanza locally and `tar tzf` the result:
  `bin/` holds exactly the four binaries, `share/` the man/completion trees.
- **Recipe probe (R5):** `make check` green on the committed tree; stand-in Dockerfile
  (prior release tag) builds and all four entrypoints answer under the bookworm image.

---

*Backing research: none on the lean path — probes above stand in for briefs.*
