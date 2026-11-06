---
status: unshaped
kind: fix
appetite: small
---

# The man-page literal gate is correct but narrow: three remaining gaps in what it can see

> **Pre-shaped candidate, not a contract.** `/xdu-feature` promotes this into `spec/{slug}/GOAL.md`,
> where appetite, non-goals and R-IDs get negotiated. Do not copy it verbatim.

## Problem

The `Assert critical literals survive into the published man-page text` step in
`.github/workflows/test.yaml` is now robust *at what it checks* — its verdict depends on content, not
layout, across both `scdoc` versions, 161 render widths and both userlands. These are gaps in its
**coverage model**: things it is structurally unable to see. All were measured. A fourth item,
originally filed here as (c), turned out to be an unmet `R7` rather than a coverage gap and was fixed
in that pass — it is kept below, struck through, because the rationale recorded for deferring it was
disproven and a future shaping session must not inherit it.

**(a) No page identity.** No literal on the four original pages names the binary it belongs to, so
the gate cannot tell them apart. Copying `xdu-find.1` over `xdu-view.1` and running the gate is
**green** — both pages assert only `XDU_INDEX`. A build that shipped the same page four times would
pass. `xdu-cp.1` and `xdu-mv.1`, added later, each count a worked destination path the other page
does not carry (`/cold/alice/logs/app.log`, `/archive/carol/runs/r1.h5`), so those two cannot be
swapped silently; the original four still can.

**(b) The page list is hard-coded.** The gate names exactly six `check` targets, while the render
step loops over `doc/*.scd`. A seventh page is therefore *entirely* unasserted. Measured: a new
`doc/xdu-new.1.scd` containing the un-escaped `_OUTDIR_/*/*.parquet` renders at `scdoc` exit 0,
publishes the exact historical corruption `OUTDIR//.parquet`, and the gate is **green**. The
source-side roff-control tripwire does glob `doc/*.scd`, but it only catches lines beginning with `.`
or `'` — a different class. A new binary's man page joins the build with no literal coverage and
nothing says so. The bulk copy/move cycle exercised exactly this: `doc/xdu-cp.1.scd` and
`doc/xdu-mv.1.scd` were rendered by CI from the commits that added them, unasserted, until their
`check` lines and produced-file entries were added by hand in a later phase of the same branch. The
list went from four pages to six because someone remembered, which is the gap.

**(c) ~~Four of the ten assertions are inert.~~ RESOLVED — and the rationale below was wrong.**
The set is 6 literals on `xdu.1`, 1 on `xdu-find.1`, 1 on `xdu-view.1`, 2 on `xdu-rm.1`. The four
env-var names (`XDU_INDEX` ×3, `XDU_JOBS`) are written `*XDU_INDEX*` in the sources — bold with a
**mid-word** underscore, which `AGENTS.md` documents as safe by construction. This issue originally
claimed they therefore "have no silent-corruption mode: they can only fail if the page is missing or
empty".

**That is false.** Review cycle 2 of `manpage-literal-assertion-fails-on-ubuntu` measured each of them
appearing **twice** on its page — once as a cross-reference in the flag description, once as the
`ENVIRONMENT` entry — while all four were asserted by *presence*. Corrupting only the `ENVIRONMENT`
entry (`doc/xdu-find.1.scd:66` `*XDU_INDEX*` → `*XDU-INDEX*`) publishes a variable name that does not
exist, and the gate exited **0** with its `OK:` line. No missing or empty page involved. The same
reproduced on `doc/xdu-rm.1.scd:81`, the destructive binary's page, and on deleting the whole
`ENVIRONMENT` section.

That was an unmet `R7`, not a coverage gap, so it was **fixed in that pass**, not deferred: all four
are now `2x:` counted. Those counts are hand-maintained, though — the derivation that found them ran
in that pass's `verify/` harness and did not outlive it, which is gap (b)'s territory below. What
survives of (c) is the weaker,
genuinely-deferred point: an env-var name is still a *thin* literal, and one well-chosen path or glob
per page would catch more classes than four env-var names do. R3 below is that residue.

**(d) `col -b` is locale-dependent, which silently bounds what can ever be asserted.** Under a
non-UTF-8 locale, `col -b` rewrites each multibyte character as the literal ASCII text `\xNN`:

```sh
$ printf 'a‐b\n' | LC_ALL=C     col -b | od -c | head -1
0000000   a   \   x   e   2   \   x   8   0   \   x   9   0   b  \n
$ printf 'a‐b\n' | LC_ALL=C.UTF-8 col -b | od -c | head -1
0000000   a 342 200 220   b  \n
```

Harmless **today** — every asserted literal is pure ASCII, so the gate's verdict is locale-invariant,
and P1's matrix confirms host and container agree. But the man pages do contain non-ASCII (em dashes
throughout `doc/*.scd`), so the moment anyone asserts a literal containing one, the gate's verdict
starts depending on the runner's locale — which is the same shape as the defect that produced this
whole pass. It also mis-measures adjacent work: it is why an initial count of U+2010 in the `groff`
render read zero while the page was demonstrably broken.

## Why it was deferred

The remaining three are **pre-existing** — (a) and (b) arrived with the gate in `9c579cf`; (d) is a
property of `col` that predates the repo. `spec/manpage-literal-assertion-fails-on-ubuntu/GOAL.md` scoped that
pass to making the existing assertion layout-insensitive (R1–R5, R7) and reconciling the documented
local check (R6). Widening *what* is asserted is a different question from making the existing
assertions correct, and folding it in would have meant negotiating new coverage under an appetite
sized for a two-line normalization fix.

## Outcome / vision

The gate's coverage grows with the project instead of silently lagging it: a new man page cannot join
the build unasserted, a page cannot be confused with another page, every binary the project ships has
at least one literal that would actually catch a mis-escape, and the verdict never depends on the
caller's locale.

## Sketch of the acceptance criteria

- **R1** — WHEN a `doc/*.scd` exists that the literal gate does not assert, the packaging job SHALL
  fail and name the unasserted page (closes (b) by construction rather than by remembering).
- **R2** — Each page SHALL assert at least one literal unique to that page, so swapping two rendered
  pages fails (closes (a); `xdu-cp.1` and `xdu-mv.1` already do).
- **R3** — `xdu-rm.1` SHALL assert at least one literal with a real silent-corruption mode — a path,
  glob or escaped-`*` construct — rather than only env-var names (the residue of (c); the
  duplicate-occurrence half was an unmet R7 and is already fixed).
- **R4** — The gate's verdict SHALL be identical under `LC_ALL=C` and a UTF-8 locale, or the job
  SHALL pin the locale it requires (closes (d)).

## Notes

- (b) is the one worth doing first and is nearly free: derive the page list from `doc/*.scd` and fail
  on any page with no `check` entry, rather than maintaining a parallel list. That converts a silent
  gap into a build error the next time a binary is added — and `ROADMAP.md` has the
  bulk-operations theme queued, so it will be exercised.
- (c)'s residue suggests the fix is choosing better literals, not more of them. One well-chosen path
  per page beats four env-var names — the counts now added stop single-occurrence corruption, but they
  do not make a thin literal thick.
- Reproductions for (a) and (b): render `doc/*.scd`, then either `cp share/man/man1/xdu-find.1
  share/man/man1/xdu-view.1`, or add a seventh `.scd` carrying `_OUTDIR_/*/*.parquet`; run the extracted
  gate body in either case and observe `OK:`.
- Related: [`manpage-groff-hyphenates-marker-path.md`](manpage-groff-hyphenates-marker-path.md) (the
  other deferral from the same pass — and the one (d) interferes with measuring);
  [`ci-gates-are-advisory.md`](ci-gates-are-advisory.md) (a gate that binds nothing is the wider
  version of this); `spec/manpage-literal-assertion-fails-on-ubuntu/EVIDENCE.md`.
- Found by: `manpage-literal-assertion-fails-on-ubuntu` P3; (c) reclassified and closed by its review cycle 2.
