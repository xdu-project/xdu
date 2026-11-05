# 00 — Digest

`appetite: small`, so there was no fan-out. One targeted brief closed the GOAL's open leg (R6). R1
and R6 are gate-shaped deliverables, so one adversarial reviewer then attacked the drafted gate
design before `PLAN.md` was committed. The two briefs do not contradict each other. This file
records what each decided and where it landed.

## From [01 — R6 release load check](01-r6-release-load-check.md)

| Decision | Landed in |
|---|---|
| Fetch from `extensions.duckdb.org/<engine>/<platform>/`, with the engine and platform reported by the leg's own build, not a Rust-triple table | PLAN §2.3, §2.4 |
| The 512-byte footer is the static check (magic `4`, then platform and version at fixed offsets) | PLAN §2.4 `footer.sh` |
| No `-rdynamic` or export work: `httpfs` imports no `duckdb::` symbols | PLAN §4 |
| R6's loader is a same-build Cargo example, not a shipped binary. This touched R6's wording; GOAL R6 was amended to match (Clarification 4, 2026-11-05) | PLAN §2.3, §2.4; GOAL R6 |
| Exercise the engine/extension C++ boundary, not just `LOAD` (the `linux_amd64` static-`libstdc++` risk) | PLAN §2.3 step 4 |
| `macos-14` is retired: Apple legs move to `macos-15` / `macos-15-intel` (native, no Rosetta) | PLAN §2.5, deviation row 3 |
| No hash pin. Engine and platform are what the GOAL pins, and the signature is checked at `LOAD` | PLAN §5 |

## From [02 — adversarial review](02-adversarial-review.md)

| # | Finding (severity) | Landed in |
|---|---|---|
| 1 | Empty or broken manifest makes every gate vacuous (high) | §2.3 shape checks; §2.4 assignment capture, ≥1 entry, member set = manifest; P2 extra-extension control |
| 2 | Boundary query on a second connection never crosses the boundary (high) | §2.3 one connection; `httpfs`'s own error required; autoload wording rejected |
| 3 | Refusal never observed on the release artifact (medium) | §2.3 settings + `JSON` refusal in `load`; §2.1 `clippy.toml` `disallowed-methods`; `xdu-rm --dry-run` in step 5 |
| 4 | Environment leaks into `check.sh` (medium) | §2.4 `env -i`, `$cold` must exist and be empty; P2 hostile-env re-run |
| 5 | Canonicalizing the override breaks R4 and reddens macOS (medium) | §2.1 canonicalize only the exe, `.parent().parent()`; §2.4 three negative controls on the loader's wording |
| 6 | `install.sh` `cp` in place kills the next load (medium) | §2.6 temp name + `mv`; P4 second install |
| 7 | Docker checks the builder, not the image (medium) | §2.6 `COPY /out/ /usr/local/` + runtime `test -f` |
| 8 | PR path filter misses `.cargo/**` and friends; no PR cancellation (medium) | §2.5 paths; PR-only `cancel-in-progress` |
| 9 | `source_id` proves version, not build (low) | §2.4 step 4 renamed; build identity asserted structurally by P3 |
| 10 | P3 must cover both jobs, same tarball path, no conditional steps (low) | `verify/p3.py` |
| 11 | `footer.sh` empty args, other fields, short file (low) | §2.4; P2 controls |
| 12 | Non-release engine version shapes (low) | §2.3 `manifest` refuses them |
| 13 | HPCCM has no extension guard (low) | **Deferred**: adding it now breaks the recipe pinned to 0.5.2. New `issues/hpccm-extension-guard.md` + ROADMAP entry (P4 ledger) |
