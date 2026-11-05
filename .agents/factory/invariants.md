# Invariant gate & footgun checklist

A curated, explicitly-enumerated subset of the load-bearing invariants in
[`AGENTS.md`](../../AGENTS.md), maintained **in lockstep with it** (`AGENTS.md` is ground truth — if
this drifts, fix it). Two consumers:

- **`xdu-plan` (gate):** before research *and* after PLAN/TECH is drafted, walk the sections a change
  touches and confirm the design honors each. Record any bend in PLAN's deviation-justification table.
- **`xdu-review` (footgun list):** a violation of any §1–§12 invariant here — **lettered subsections
  such as §2b/§2c included** — is **auto-CRITICAL** and,
  when it touches a high-blast-radius file, forces a human sign-off gate. A §13 project-conventions
  violation is **HIGH**, not auto-CRITICAL.

Only invoke the sections relevant to the change — do not manufacture findings against untouched
subsystems.

## High-blast-radius files (any CONFIRMED finding here → mandatory human gate)

`src/bin/xdu-rm.rs` (destructive) · `src/bin/xdu.rs` (crawl concurrency scaffold + marker sequencing) ·
`src/crawl.rs` (atomic finalize + stale-chunk prune + reserved-index-name collision rejection) ·
`src/lib.rs` (schema + `QueryFilters`/SQL + `index_glob` + layout constants + `RESERVED_INDEX_NAMES`) ·
`src/cli.rs` (the one CLI definition) ·
`src/s3.rs` (`S3Sink`'s chunk PUTs, prune and marker sequencing + the S3 partition-name refusal +
the one credential resolver and the reader's bound `CREATE SECRET`) ·
`src/view_query.rs` (every `xdu-view` statement) ·
`src/engine.rs` (the one connection open, the extension load and `open_s3_connection`)

---

## 1. Parquet schema stability (`src/lib.rs::get_schema`) — highest blast radius

- `get_schema()` is the sole writer↔reader contract: exactly **eight non-null** fields, fixed order —
  `path: Utf8`, `size: Int64`, `uid: Int64`, `gid: Int64`, `mode: Int64`, `atime: Int64`,
  `mtime: Int64`, `ctime: Int64`. `size` is bytes as `i64` (meaning depends on the
  `SizeMode` chosen at index time — disk-usage vs apparent vs block-rounded — and is **not** recorded
  in the index); `atime`/`mtime`/`ctime` are Unix epoch seconds as `i64`; `mode` is permission
  bits (`st_mode & 0o7777`).
- The on-disk version is the `format` key in the `.xdu-complete` marker (`INDEX_FORMAT_VERSION`,
  `lib.rs`): version 2 names this eight-column layout, and every reader refuses a marker whose
  version it does not understand instead of misreading the rows. Any change to `get_schema()` or a
  reader's `read_parquet` column list is therefore a **breaking, cross-cutting** index-format change
  touching `lib.rs` + the crawler + all three readers + every README `read_parquet` example, and it
  must bump the version in the same commit. `get_schema()` is the *only* in-code statement of the
  row shape — there is deliberately no mirror struct to drift out of step with it.
- Issues #2 / #3 (add `owner`/`group`/`permissions`) were exactly this class — the richer-schema
  cycle added `uid`/`gid`/`mode` (plus `mtime`/`ctime`) and bumped the version alongside,
  as required.

## 2. Atomic finalization (`src/crawl.rs::PartitionBuffer::finalize`, through `IndexSink`)

- Locally (`crawl::LocalSink`): write each chunk to `NNNNNN.parquet.partial`, then `fs::rename` to
  `.parquet` **within the same directory** (POSIX-atomic); prune stale higher-numbered `.parquet`
  chunks left by a prior larger run, stopping at the first gap.
- Never `File::create` a final `.parquet`, never cross-directory rename (breaks atomicity), never let
  a reader glob `.partial`.
- Into S3 (`s3::S3Sink`): an object store has no rename, so each chunk is **one PUT at its final
  key** `<prefix>/<partition>/NNNNNN.parquet`, sent as it is encoded. A single PUT is
  all-or-nothing, so no reader sees part of a chunk. Finalize is a LIST of the partition and a
  DELETE of **every** higher-numbered chunk, past gaps (only a foreign edit makes a gap, and a
  re-run must leave no stale chunk). Nothing of the index is written to local disk on this path:
  no `.partial`, no staging, no cache.
- **Known limitation:** finalize is per-file, **not** per-partition atomic — a reader during finalize
  can see a mix of new and old chunks, and a crash mid-finalize leaves the partition inconsistent. Do
  **not** index a partition concurrently with purging it; document this rather than pretending
  otherwise.

## 2b. Run-level completion marker (`<index>/.xdu-complete`)

Per-chunk atomicity (§2) covers one *file*; it cannot express whether the **run** finished. When one
driver fails, the partitions that already succeeded remain on disk as real `.parquet` chunks,
indistinguishable from a complete index. The marker is the only thing that separates them, so its
ordering is load-bearing:

- `COMPLETION_MARKER` = `.xdu-complete` (`lib.rs`), at the **index root**. A dotfile, so the readers'
  `*/*.parquet` glob never mistakes it for a partition — and a **reserved name** (§3), so a partition is
  never written over the marker either. Both directions, or the outdir can be bricked.
- **Cleared after pre-flight passes**, before any driver writes — so a run rejected without touching the
  index leaves an existing marker intact, while a crash mid-crawl leaves the index visibly unattested.
  For an `s3://` output the pre-flight also holds one LIST of the prefix (reachable endpoint,
  accepted credentials, existing bucket; an absent or empty prefix is a new index) and the refusal
  of every top-level name that would not survive as a key and a glob
  (`IndexSink::check_top_level`).
  The marker DELETE is then the first write, so read-only credentials fail with the marker intact.
  **Written only on the success path**, after every partition finalized. Never write it on a failure
  path, and never move the clear earlier than the last check that can still reject the run.
- Body is `key=value` lines (`xdu`, `completed_at`, `files`, `bytes`, `vanished`, `errors`,
  `lossy_paths`, `format`), so an `--allow-errors` run still records how much it skipped. `format`
  carries the run-level index format version (`INDEX_FORMAT_VERSION`); it is what the readers'
  version gate checks.
- All three readers call `lib::index_version_error` before `lib::index_completion_warning` on a local
  index; on an `s3://` index `xdu-find` and `xdu-view` read the marker once with
  `lib::read_completion_marker_s3`, then call `lib::marker_version_error` before
  `lib::marker_completion_warning` on that read (the local pair are these two over the local read).
  A marker stating no understood version — absent marker included — is a **refusal** (no rows, no
  deletions), never a warning. The warning stays soft and covers only a versioned marker recording
  tolerated errors, which remains queryable. The read is size-capped (`MARKER_READ_LIMIT`) and must
  not block, so a FIFO or an enormous file left at that path cannot hang or exhaust a reader. Locally
  that is one `stat` before any open; over S3 it is a LIST proving the prefix holds objects, then a
  HEAD for the marker's size (a missing key is an absent marker), and a marker over the cap is refused
  as `MarkerRead::Oversized` without its GET.
- **Known limitation (recorded, not fixed):** the marker describes the whole index but its counts come
  from one run, so a `--partition`-scoped run clears and rewrites it from its own stats and can retire a
  still-correct warning — `issues/marker-scoped-run-attestation.md`. Do not fix this incidentally; it
  needs its own pass.

## 2c. Fail-loud crawl errors; `--allow-errors` is opt-in

- **Default:** a walk or stat error fails the run **non-zero** with a diagnostic naming the path and
  errno, and **no marker is written**. This is the invariant that prevents a silently-incomplete index:
  jwalk emits one `Err` in place of an unreadable subtree's entire contents, so swallowing it loses
  arbitrarily many files while still exiting 0.
- **`--allow-errors` must stay opt-in.** It counts and reports skipped entries, exits 0, and records
  `errors=N` in the marker. Never make tolerance the default; never let it suppress the per-entry stderr
  report.

## 3. Partition scheme

- Layout is `<index>/<partition>/NNNNNN.parquet`, `partition` = a top-level subdirectory name.
- Loose files directly under the indexed root go to the reserved partition **`__root__`**
  (`ROOT_PARTITION`, defined in `lib.rs` as an index-layout constant and re-exported by `crawl`),
  crawled at `max_depth(1)`; the two never overlap (no file double-counted).
- **Reserved names are a class, not a list of facts.** `<index>/` holds exactly two kinds of entry:
  partition directories, and the reserved `COMPLETION_MARKER` dotfile. `lib::RESERVED_INDEX_NAMES`
  pairs every claimed name with what claims it, and `crawl::build_work_queue` **iterates that list** to
  **reject** a top-level source directory bearing one, with a clear error, **unconditionally** — even
  when a `--partition` filter would have excluded it, because the collision is with the on-disk layout,
  not with the selection. A change that reserves a **new** name at the index root must add it to that
  list in the same commit; the guard then covers it by construction rather than by being remembered.
  - `__root__` would collide with the synthetic partition — both write `000000.parquet` into the same
    directory and each `finalize` prunes the other's chunks.
  - `.xdu-complete` would be created as a *directory* at the marker path, so the run cannot write its
    attestation (`EISDIR`) and **no later run, from any source tree, can clear it** (`EPERM`) — a
    correct index reported as a failed run, and a bricked outdir until someone `rmdir`s it by hand.
  - Both directions matter: readers' `*/*.parquet` glob never mistakes the marker *for* a partition
    (it is a dotfile), and this guard is what stops a partition being written *over* the marker. When
    reviewing a diff that adds a reserved on-disk name, ask both questions.
- Chunk ids are zero-padded sequential; readers glob `*/*.parquet`.
- `lib::index_glob(index, partition)`, and `lib::index_glob_s3` for an `s3://` index, are the
  **only** places this layout becomes a `read_parquet` glob, and both write it through one shared
  tail. Every reader goes through them; `xdu-find` and `xdu-view` by way of `lib::parquet_source`,
  which binds the S3 glob. Do not hand-build the glob in a reader.

## 4. `xdu-rm` destructive safety — read before touching `src/bin/xdu-rm.rs`

- Default requires an interactive `y/N` confirmation (anything but `y`/`yes` aborts, deleting nothing);
  `--dry-run` performs **zero** deletions; `--force` skips the prompt; empty match set prints a
  message and exits 0.
- `--safe` re-`stat`s each file immediately before unlink and skips it if the current metadata no
  longer matches the query (the stale-index guard). It must re-verify **every** filter it is combined
  with — today it checks only atime (`--older-than`) and size (`--max-size`); `--min-size`,
  `--newer-than`, and `--pattern` are **gaps** and the man page overclaims. Closing that gap must not
  weaken any existing check.
- **Any deletion (or previewable action) combined with `--limit` MUST carry a deterministic
  `ORDER BY`** so `--dry-run` and the real run select **identical** rows. A `LIMIT` without `ORDER BY`
  returns an arbitrary/unstable subset.

## 5. DuckDB injection surface (all readers and the extension loader)

- Every user-supplied value that reaches `read_parquet(...)` or a `WHERE`/`LIKE` clause is an
  injection surface: `--pattern`, `--partition` (`-u`), and the index path. Today only `--pattern` is
  escaped (single-quote doubling); for a **local** index, partition names and index paths are
  interpolated **raw**, a known exception (binding them is a follow-up seed).
- For an **`s3://` index** nothing the locator or `-u` carries is interpolated. `lib::parquet_source`
  returns `read_parquet(?)` (or `read_parquet(?, filename=true)`) with the whole glob as the one bound
  value, built by `lib::index_glob_s3`, and `lib::read_completion_marker_s3` binds the URLs of its
  `glob(?)` and `read_blob(?)` statements. The unit test
  `tests::test_parquet_source_keeps_local_sql_byte_for_byte` pins the local text, and the wire test
  `find_prefix_with_a_quote_matches_local` holds the S3 rule with a `'` in the prefix. Any new
  reader statement over an S3 index takes its source from `parquet_source` on the same terms.
- `xdu-view`'s statements live in `lib::view_query`, each built by a pure `*_sql` function. They
  take their source from `parquet_source` too, and the S3 arm also binds the directory prefix a
  listing matches with `path LIKE ?`; the local arm keeps the prefix in a `LIKE` literal, as
  before.
  The unit test `view_query::tests::local_view_sql_is_byte_for_byte_what_xdu_view_sent` pins every
  local statement against a frozen copy of the binary's old builders, and the wire test
  `view_quotes_in_the_prefix_and_a_directory_match_local` holds the S3 rule with a `'` in both the
  prefix and a listed directory.
- Route all such values through **one** validated escaping/quoting helper (or bound parameters). Numeric
  filters (`i64`) are injection-safe by type. Forbid raw `format!` of a partition name / index path
  into SQL — in the destructive `xdu-rm` a quote or `../`/glob-meta could escape the intended partition.
- The extension path is an injection surface too. `engine::load_extension_from` (`src/engine.rs`)
  puts the absolute path of a pinned extension file into `LOAD '<path>'`. The directory is
  operator-controlled: `XDU_EXTENSION_DIR` when set and non-empty, taken as given, otherwise
  `<canonical exe>/../../lib/xdu/extensions` from the canonicalized `current_exe()`. The name is
  restricted to `PINNED_EXTENSIONS`. Before the `LOAD`, the only SQL it runs, `load_extension_from`
  refuses a path that is not valid UTF-8 or that contains `'`, rather than escaping it. The unit
  tests `engine::tests::quoted_directory_is_refused_before_any_sql` and
  `engine::tests::non_utf8_directory_is_refused_before_any_sql` hold the rule; the second skips on a
  filesystem that rejects non-UTF-8 names (APFS) and runs on the Linux CI leg. Any new statement in
  `src/engine.rs` built from a path or an environment value joins this surface on the same terms.
- `engine::open_s3_connection` is on the engine surface too. After the `LOAD` it runs one prepared
  `CREATE TEMPORARY SECRET` (text from `S3Settings::duckdb_secret_statement`) whose every value,
  the key pair, session token, region, endpoint, URL style, SSL flag, requester-pays flag and KMS
  key, is a bound parameter; the text is fixed. It binds every key `httpfs` would otherwise take
  from the `s3_*` setting it fills from the environment at load (`AWS_*`, `DUCKDB_S3_*`), the
  endpoint included when it is AWS's own, so no stray variable can redirect or re-sign a reader:
  `s3::tests::the_secret_statement_text_is_fixed_and_names_every_key_httpfs_would_fall_back_on` and
  the `harness_stray_*` wire tests hold that. No credential is ever SQL text, and
  `duckdb_secrets()` lists the secret redacted.

## 6. Unix-only

- `std::os::unix::fs::MetadataExt` is load-bearing: `atime()` for access time and `blocks()`×512 for
  `SizeMode::DiskUsage`. Do not introduce code paths that assume portability.
- A future S3 / cross-platform source (roadmap Phase 4) must special-case — S3 objects have no atime.

## 7. Shared rayon-pool concurrency (`src/bin/xdu.rs`)

- A **single** `Parallelism::RayonExistingPool` backs **all** jwalk walkers; up to `--jobs` driver
  `std::thread`s pull partitions from a `Mutex<VecDeque>` work queue; rayon work-stealing balances
  directory reads across active walkers so one huge partition can't starve the rest.
- `std::thread::scope` joins the drivers; the first `Err`/panic surfaces as the process result
  (non-zero exit). Thread budget: N pool + C drivers + 1 main, plus 2 tokio workers when the output
  is `s3://`: `S3Sink` owns that runtime, driver threads `block_on` it for each request, and rayon
  workers never touch it. Preserve this shape.

## 8. Symlinks excluded

- `follow_links(false)` + `entry.file_type().is_file()` before recording — the index holds only
  regular files. Don't start following symlinks (double-counting / traversal risk).

## 9. `SortMode` age inversion (must stay consistent SQL ↔ in-memory)

- `age-desc` = oldest first = `atime ASC`; `age-asc` = newest first = `atime DESC` (inverted by
  design). The SQL builders (`to_order_by` / `to_partition_order_by`) and `xdu-view`'s in-memory
  `sort_entries` must agree; directories-first applies only to `Name` sort.

## 10. CLI single source of truth (`src/cli.rs`)

- `src/cli.rs` is the one definition of every flag; `gen-completions` emits completions from it and
  `doc/*.scd` man pages describe it — completions/man cannot advertise a flag the binaries lack.
- **A CLI change updates the affected `doc/*.scd` in the same commit.** Prefer clap `ValueEnum` /
  `value_parser` over free-form `String` validated late (so completion/help offer valid values).
- The `-p` overload is real (partition in `xdu`, pattern in the query tools; partition is `-u`
  there) — change both sides coherently, never one in isolation.

## 11. Altitude / testability

- Binary targets stay thin: arg parse, terminal setup/teardown, the event loop. All parsing,
  formatting, file-type sniffing, SQL-building, and state-transition logic belongs in the `xdu`
  library so `tests/` and unit tests can reach it. (This is the root cause of the untested 2487-line
  `xdu-view.rs`.) Model the TUI as a pure state machine where feasible (`update(&mut App, Action)`
  with no terminal I/O).

## 12. TUI terminal safety (`src/bin/xdu-view.rs`)

- Raw mode + alternate screen must be restored on **every** exit path **including panic** — via a
  `Drop` guard or panic hook, **not** plain sequential code after the event loop (`panic = "abort"`
  is set in `Cargo.toml`, so an unwind won't run trailing restore code).
- Sanitize previewed file bytes (`strip_ansi`) before any line reaches ratatui (terminal-escape
  injection from attacker-controlled file contents). Slice on char boundaries, never raw byte indices
  computed from a display width.

## 13. Project conventions (same-commit / packaging) — violations are HIGH, not CRITICAL

- **Version is single-sourced from `Cargo.toml`** — never hardcode a version in `src/`; read it from
  `CARGO_PKG_VERSION`. All four `#[command(...)]` blocks in `src/cli.rs` set `version`, so clap
  derives `-V`/`--version` from `Cargo.toml` for every user-facing binary.
- `share/` is a **generated** artifact (man via `scdoc` from `doc/*.scd`; completions via
  `gen-completions` from `src/cli.rs`), git-ignored, rebuilt in CI and by `/xdu-release`; CI asserts
  it generates. `.scd` sources carry no version string (a pure version bump doesn't touch them).
- `hpccm/` specs are **generated-committed, the reverse of `share/`**: `xdu.py` is the source,
  `xdu.def` + `xdu.docker` are committed artifacts regenerated with `make -B`. A recipe edit
  without its specs is a drifted tree — CI's `make check` step in the packaging job fails on it,
  and `make check` is the local form. The recipe `VERSION` pins the installed release, so unlike
  the `.scd` sources it moves on every version bump: `/xdu-release` bumps it in lockstep with
  `Cargo.toml` and carries the regenerated specs in the bump commit.
- **`doc/*.scd` authoring is its own footgun** — `scdoc` publishes a **wrong page at exit 0** (a
  mis-escaped `*` turned `_OUTDIR_/*/*.parquet` into `OUTDIR//.parquet`; a line beginning with `.`
  loses that period), so a green render gate proves nothing about the text. The escaping and
  line-start rules are single-sourced in `AGENTS.md`'s **Commands** section — read them before
  touching a `.scd`, and do not restate them here. Reviewing a `.scd` change means reading the
  published text (`scdoc < f.scd | mandoc -Tutf8 | col -b`), never exit 0 alone.
- **Asserting a literal survived is a separate step, and it must be layout-insensitive.** `mandoc`
  breaks lines wherever the fill lands, including *inside* a token, so an un-normalized `grep` calls
  an intact literal missing; `col -b` also indents with TABs. Match the way CI does — pipe the page
  through `| tr -d '[:space:]'` and strip the literal the same way. A local check that skips this can
  be green on homebrew `scdoc` (which escapes hyphen-minus) and red on the distro package (which does
  not). This normalization is **one rule in three places** — `AGENTS.md`'s Commands section, the
  `Assert critical literals` step in `.github/workflows/test.yaml`, and this bullet — and changing it
  is a same-commit obligation across all three, like the CLI↔man-page rule.
- **A literal published more than once must be asserted by COUNT, not presence.** `grep -q` is
  satisfied by whichever copy survived, so corrupting one of two occurrences passes a presence check
  while the page ships wrong. The gate's `Nx:LITERAL` spec asserts exactly N. **"More than once" is a
  property of the page, not of the literal** — an env-var name appears twice because the flag
  description cross-references the `ENVIRONMENT` entry, so an ordinary `.scd` edit can duplicate a
  literal that used to be unique and silently re-open the hole. **That judgement is maintained by
  hand** in the workflow's literal list — nothing standing re-derives it, so re-count when you touch a
  `.scd` carrying an asserted literal. (A feature's `spec/{slug}/verify/` harness can derive the counts
  during a build pass, but it is a record once the spec merges, not a gate.) Coverage of a *new* page
  is likewise not automatic — the `check` list names four pages while the render step globs
  `doc/*.scd`. Both gaps are the standing record in
  [`issues/manpage-gate-coverage-gaps.md`](../../issues/manpage-gate-coverage-gaps.md).
  `AGENTS.md`'s Commands section
  documents the matching local form — a presence-only local check on a duplicated literal cannot
  predict CI however well it normalizes. Counting is also what makes the strip dangerous in the other
  direction: fusing two adjacent tokens is at worst a false *green* for presence but a false *red* for
  a count, so a counted needle must be specific enough that fusion cannot synthesize it
  (`.partial suffix`, never bare `.partial`). Adding, removing or re-counting a literal carries the
  same three-place same-commit obligation as the normalization above.
- Release tarball layout — `bin/{xdu,xdu-find,xdu-view,xdu-rm}` +
  `share/{man/man1/*.1, bash-completion/completions/*, zsh/site-functions/*}` +
  `lib/xdu/extensions/{name}.duckdb_extension` — matches `install.sh` extraction exactly; keep them
  in lockstep. The extensions land beside the *physical* install directory of the binaries
  (`<canonical exe>/../../lib/xdu/extensions`, overridable with `XDU_EXTENSION_DIR`), and the
  installer replaces each by copy-then-rename, never an in-place copy. Connections refuse
  autoload/autoinstall, so a missing file fails naming its path; nothing downloads at runtime.
- Non-TTY runs keep **stdout** clean/pipeable (progress and status go to **stderr**).
- Reuse shared helpers; do not duplicate logic across bins or reimplement production logic in tests.
- **Never embed `spec/` `R#`/`P#` ids in source** (they restart per feature and collide across
  branches). Greppable, and reviewers cite the result — keep grading this one.
- **Prose, comment and commit-message voice** per `AGENTS.md` § *Prose and comments*, which also
  carries the commit subject cap and the hand-wrap widths. Single-sourced there — do not restate the
  numbers here. This is **hygiene, not a review finding**: a 105-column line is not a §13 violation.
- Pre-release gate (mirrored by CI and `/xdu-release`): `cargo fmt --all -- --check`,
  `cargo clippy --all-targets --all-features -- -D warnings`, and `cargo test` all pass clean.
