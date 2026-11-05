---
status: unshaped
kind: feature
appetite: big
---

# Native OS packages (DEB / RPM), aimed at the official repos

> **Pre-shaped candidate, not a contract.** `/xdu-feature` promotes this into `spec/{slug}/GOAL.md`,
> where appetite, non-goals and the R-IDs get negotiated. Do not copy it verbatim.

## Problem

Installation is a release tarball plus `install.sh`. HPC and enterprise Linux users cannot install
or upgrade `xdu` through their system package manager — a small but real adoption friction. The
shipped `packaging/rpm/xdu.spec` (PR #12) covers ad-hoc and COPR-style EL builds only: it fetches
crates.io at build time, ships no man pages or completions, and hardcodes the version. None of
that survives review for the official repositories, where builders have no network and every
dependency must be accounted for.

## Why it was deferred

The original roadmap entry predates the `issues/` back-reference convention, and the RPM first
pass landed after it. This revision re-aims the seed at official-repo acceptance (Fedora/EPEL and
Debian), which is a materially bigger job than the COPR-style spec — hence the appetite change.
No new scoping was done beyond naming the gates; `/xdu-feature` negotiates the mechanism.

## Outcome / vision

`xdu` installable and upgradeable from the official Fedora/EPEL and Debian repositories, with the
in-repo `packaging/` tree as the maintained source of the distro tooling: `packaging/rpm/` for the
spec and its companions, and a tooling-mandated top-level `debian/` when the DEB side starts
(`deb/` is not a name Debian tooling accepts).

## Sketch of the acceptance criteria

- **R1** — The RPM build SHALL succeed with no network access (vendored crates or
  `rust2rpm`-generated spec against packaged crates), as koji/Mock builders require.
- **R2** — The RPM spec SHALL track the release version without a manual edit per release
  (wired into the release step alongside `Cargo.toml`).
- **R3** — The shipped packages SHALL include the man pages and shell completions, built from
  `doc/*.scd` and `src/cli.rs` — which likely means building distro packages from the release
  tarball rather than the tag archive.
- **R4** — The `debian/` tree SHALL build with `dpkg-buildpackage` offline (Debian Rust-team
  practice: `dh-cargo` with vendored sources), and pass `lintian` as the spec passes `rpmlint`.
- **R5** — The bundled-DuckDB C++ source SHALL be evaluated against each repo's bundling policy
  (system `libduckdb` where the distro carries one); a bundling exception or an unbundling
  feature is a shaping decision, not an afterthought.
- **R6** — CI SHALL validate both package builds (`mock`/`rpmbuild -ba` and the Debian build)
  so the tooling cannot rot between releases.
- **R7** — The `.rpm` and `.deb` packages SHALL ship the pinned DuckDB extensions at
  `<prefix>/lib/xdu/extensions/` beside `<prefix>/bin/` (for `/usr/bin`, that is
  `/usr/lib/xdu/extensions/`), built for the engine version and platform of the binaries they
  package, and SHALL prove they load under a cold `HOME` with `packaging/extensions/check.sh`
  semantics. The release tarball has carried that directory since the extension-distribution
  cycle, and the tools look nowhere else unless `XDU_EXTENSION_DIR` is set.

## Notes

- **The RPM spec builds from source and must fetch.** `packaging/rpm/xdu.spec` builds from the tag
  archive, so it ships no extensions today, and nothing in a source build produces them: they are
  upstream DuckDB binaries. The spec has to fetch them for the engine it just built (the
  `extension-probe` example and `packaging/extensions/fetch.sh` do exactly that), which collides
  with R1's no-network builders. Building distro packages from the release tarball (R3) would
  carry `lib/xdu/extensions/` for free; whichever route is chosen, R7 is the requirement.
- Related: GitHub issue #5; PR #12 (COPR-style first pass); `packaging/rpm/xdu.spec`;
  `spec/duckdb-extension-distribution/` (on `main` since #31), which set the layout.
- License posture helps: MIT with no bundled-exception baggage beyond DuckDB.
- Found by: original roadmap; re-aimed at official repos 2026-09-07.
