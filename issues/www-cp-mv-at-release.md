---
status: unshaped
kind: fix
appetite: small
---

# The site advertises `xdu-cp` and `xdu-mv` as roadmap items until the release that ships them

> **Pre-shaped candidate, not a contract.** This file records deferred work in enough detail that a
> future session does not have to re-derive it. It is **not** graded by `xdu-review` and must never be
> copied into `spec/{slug}/GOAL.md` verbatim — `/xdu-feature` promotes it, and that is where appetite,
> non-goals and the R-IDs get negotiated with a human. The `status:` field above is the guard, with
> three states: `unshaped` means proposal; `shaped on <branch> (<date>) — see spec/<slug>/GOAL.md`
> means a cycle adopted it and is still in flight; `resolved on <branch> (<date>) — see
> spec/<slug>/` means the cycle landed on `main`. Only `/xdu-feature` writes `shaped`, only the
> landing cycle writes `resolved`, and `/xdu-roadmap` deletes the file once the landing is confirmed.
>
> Deliberately **not** named `GOAL-{slug}.md`: every other GOAL in the factory is a locked contract, so
> a file carrying that name eventually gets treated as one.

## Problem

`xdu-cp` and `xdu-mv` exist on `main` since `bulk-copy-move-xdu-cp-mv` landed (#37), with man pages,
completions and a place in the release tarball, `install.sh` and the container recipes. The site
still describes them as future work, and part of that description is wrong:

- `www/index.html`, the tools table (`:431-449`), lists `xdu`, `xdu-find`, `xdu-view` and `xdu-rm`
  only, and the note under it (`:461`) says man pages and completions "for all four" ship in the
  release tarball. From the release that ships the new tools there are six.
- `www/index.html:893-900`, under "Bulk operations", lists `xdu-cp` and `xdu-mv` with the
  roadmap marker `† v0.8` and says the pattern "becomes one shared select-and-act engine behind
  `rm`, `cp` and `mv`". The engine serves `cp` and `mv` only. `xdu-rm` keeps its own selection and
  `--safe` until `issues/xdu-rm-on-shared-engine.md` lands, so the sentence overstates what ships.
- `www/index.html:1073` ("Before that: … all four binaries") is release history, true of the
  releases it describes, and stays as it is.

## Why it was deferred

`www/` deploys to the public site on every push to `main` (`.github/workflows/deploy-pages.yaml`),
not at a release. Editing it in the cycle that adds the tools would advertise binaries that no
published tarball, `install.sh` run or container image yet contains, for however long the next
release takes. The cycle therefore left `www/` untouched (`spec/bulk-copy-move-xdu-cp-mv/`, P4),
and its gate checks that. Pre-existing in the sense that the roadmap text predates the tools; the
overstatement about `rm` was introduced by that cycle's GOAL ruling, which kept `xdu-rm` off the
engine.

## Outcome / vision

On the day the release carrying `xdu-cp` and `xdu-mv` is published, the site lists them as tools
with their one-line roles, counts six binaries wherever it counts them, and describes the engine as
what it is: behind `cp` and `mv`, with `rm` to follow.

## Sketch of the acceptance criteria

- **R1** — WHEN the first release whose tarball holds `bin/xdu-cp` and `bin/xdu-mv` is published,
  the site's tools table SHALL list both, and every count of the binaries on the page SHALL match
  the tarball.
- **R2** — The "Bulk operations" roadmap SHALL no longer list `xdu-cp` and `xdu-mv` as future work,
  and no sentence on the site SHALL say that `xdu-rm` runs on the shared engine before it does.
- **R3** — The change SHALL reach `main` no earlier than the release it describes (in the release
  commit or right after the tag), so the site never advertises an unreleased binary.

## Notes

- A natural home is `/xdu-release` for the release that ships the tools: it already edits the
  version-bearing files in lockstep.
- Related: `issues/xdu-rm-on-shared-engine.md` (when it lands, the engine sentence can say `rm`
  too); `spec/bulk-copy-move-xdu-cp-mv/PLAN.md` §2.8.
- Found by: `bulk-copy-move-xdu-cp-mv` plan (research 04), filed at P4.
