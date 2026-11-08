# 03 — Shared select-then-act engine: selection, `--safe`, executor, tests, CLI

Research brief for `/xdu-plan`. Covers R1, R3 (refusal order), R6–R9; invariants §2b, §4, §5, §10, §11.
Probes were built under `target/tmp/` against the lock's `duckdb 1.4.4` (`bundled`, `parquet`) and
`clap 4.6.7`; no tracked file changed.

## 1. Bound selection SQL (§5, R1)

**Today.** `lib::parquet_source` (`src/lib.rs:180`) returns `(from_text, params)`: local →
`read_parquet('<glob>')` with nothing bound (pinned byte-for-byte by
`test_parquet_source_keeps_local_sql_byte_for_byte`), S3 → `read_parquet(?)` + `[glob]`. `xdu-find`
and `view_query` pass `params_from_iter(&params)` to `stmt.query`. `xdu-rm` bypasses
`parquet_source` and `format!`s `index_glob` into a literal (`src/bin/xdu-rm.rs:86`). Every other
value is text: the regex is quote-doubled (`QueryFilters::to_conditions`, `src/lib.rs:887`), numbers
are `i64`/`u32` by type. `index_glob` uses `Path::display()`, which is lossy for non-UTF-8.

**Probe** (`target/tmp/bindprobe`, release build, `SELECT version()` = `v1.4.4`;
`SELECT path FROM read_parquet(?) ORDER BY path`, glob bound with `params_from_iter`):

| Bound glob | Result |
|---|---|
| index `idx'q`, `<idx>/*/*.parquet` | OK, both partitions |
| index `idx'q`, partition `al'ice` | OK, that partition only |
| same index, today's `read_parquet('<glob>')` text | `Parser Error: syntax error at or near "q"` |
| partition `x') UNION SELECT 1 --` | inert: `IO Error: No files found that match the pattern …` |
| index `idx[ab]` (sibling `idxa` exists) | **reads `idxa` only**: wrong index, no error |
| index `st*r` (sibling `stXr`) / `q?m` (sibling `qZm`) | **reads both** |
| partition `b?b` | matches `bob` |
| partition `p/../../idx'q` | **reads another index** |
| index `b{x,y}`, `a]b` | literal (braces and a lone `]` are not glob syntax) |
| index `x[y` (unmatched `[`), `b\s` | no files found (both are glob syntax) |
| escapes `idx[[]ab]`, `st[*]r`, `q[?]m`, `x[[]y` | literal match, correct rows |
| escapes `idx\[ab\]`, `st\*r` | no files found (backslash is not an escape) |
| exact file path with no wildcard, or `read_parquet([?, ?])` list | **still globbed** (`idx[ab]/p/chunk-0.parquet` reads `idxa`) |
| partition that does not exist | `IO Error: No files found …` (an error, not an empty set) |

So binding closes the `'` hole completely, but the bound string is still a glob. Enumerating chunk
files in Rust and binding the list does not avoid that: each list element is globbed too.

**Recommendation.**
- New engine-only builder; do **not** change `parquet_source`'s local arm (its byte-for-byte pin and
  the readers stay with `issues/local-reader-bound-parameters.md`). Text is fixed:
  `SELECT path, size, uid, gid, mode, atime, mtime FROM read_parquet(?) {WHERE …} ORDER BY path {LIMIT n}`,
  the glob the first bound value, built through `index_glob` (§3: the one place the layout becomes a glob).
- Fail closed before any SQL with one helper, e.g. `select::checked_glob(index, partition) -> Result<String>`:
  refuse an index path (after `canonicalize`) that is not UTF-8 (`to_str()`), or that contains any
  of `*` `?` `[` `\`; refuse a partition name that is empty, `.` or `..`, contains `/`, or contains
  any of `*` `?` `[` `\`. A `'` is just a byte. Rationale: the bound string is still a glob, so a
  metacharacter silently selects a different index or partition with no error (probe rows above),
  the wrong-set failure a copy/move tool must not have. Refusal costs little: without `-u`, the `*`
  partition glob still covers a partition with metacharacters in its name. The error names the value
  and says why. A nonexistent `-u` partition is a DuckDB "No files found" error today in every tool;
  the plan should decide whether the engine maps it to a clear "no such partition" refusal (it can
  stat `<index>/<partition>` first) or keeps the raw error. Recommended: the clear refusal.
- *Alternative:* escaping each metacharacter as a one-character class (`[*]`, `[?]`, `[[]`) is
  proven correct in 1.4.4 (probe) and is the right long-term answer for
  `issues/local-reader-bound-parameters.md` R2. It relies on undocumented glob behaviour, so if the
  plan picks it, pin it with an integration test: an index dir `idx[ab]` beside a sibling `idxa`,
  asserting only the literal index's rows are selected, so a DuckDB bump that changes the glob
  dialect fails CI instead of quietly moving the wrong files. `\` has no escape (refuse it either way).
- **Bind the regex too**: `regexp_matches(path, ?)`. In the probe it works with a `'` in the
  pattern, an invalid regex fails at execution (`Invalid Input Error: missing ): (`, before any row,
  so it is still caught before the executor starts), and over 5 M rows a constant and a bound
  pattern cost the same (≈ 250 ms each, two runs). (`with_path_pattern` already rejects a bad glob
  before SQL.) It removes the last string
  literal from the engine's SQL, so the whole statement text is a pure function of which filters are
  set — easy to pin in a unit test. Build it as an engine function
  `selection_sql(&QueryFilters, limit) -> (String, Vec<String>)` (pattern as a param; numerics stay
  literal, they are injection-safe by type). `QueryFilters::to_conditions` stays as is for the readers
  and `xdu-rm`; the engine needs only a sibling that emits `regexp_matches(path, ?)` for the pattern
  and otherwise the **same** condition strings — factor the non-pattern conditions into one private
  helper both call, so the SQL predicates exist once.

## 2. Selection API, separable from the executor (R1)

```rust
// src/select.rs  (library; binaries stay thin, §11)
pub struct SelectArgs { /* clap Args, flattened; see §5 */ }
pub struct Row { pub path: String, pub size: i64, pub uid: i64, pub gid: i64,
                 pub mode: i64, pub atime: i64, pub mtime: i64 }   // all Int64 in get_schema
pub struct Selection { pub index: PathBuf /*canonical*/, pub filters: QueryFilters,
                       pub warning: Option<String>, pub rows: Vec<Row> /*ORDER BY path*/ }
pub fn refuse_s3(raw: &OsStr, tool: &str) -> Result<()>;      // first statement of main
pub fn build_filters(a: &SelectArgs) -> Result<QueryFilters>;  // resolve_user/group here, main thread
pub fn selection_sql(f: &QueryFilters, limit: Option<usize>) -> (String, Vec<String>); // pure
pub fn select(a: &SelectArgs) -> Result<Selection>;
//   canonicalize → index_version_error (Err) → index_completion_warning (returned; caller eprintln!s)
//   → build_filters → checked_glob → engine::open_connection → query(params_from_iter)
```

`xdu-tar` and a later `xdu-rm` call `select()` and consume `rows` in order; nothing in it touches
rayon, stdin or the destination. Dry-run, the prompt and `--force` are a second small layer
(`gate::confirm(count, into, force, dry_run) -> Proceed|Abort`) so `xdu-tar` can reuse or skip it.

**Always `ORDER BY path`**, limit or not. Reasons: R3 must "name the first offending path", which is
only well-defined over an ordered set; `--dry-run` and `-v` output become reproducible; `xdu-tar`
wants one ordered stream. Cost (probe, 5 M rows of ~40-byte paths, one chunk, release): ≈ 0.39 s
unordered vs ≈ 0.60 s with `ORDER BY path`, a 0.2 s difference that is negligible next to copying
5 M files. Use
`deterministic_limit_clause(limit)` when a limit is set and the literal `ORDER BY path` otherwise;
leave `deterministic_limit_clause` unchanged (`xdu-rm` and its doc depend on the empty tail).

**`--safe` predicate.** Pure, filesystem-free, beside `QueryFilters`:

```rust
pub struct Live { pub is_file: bool, pub len: i64, pub disk: i64 /*blocks*512*/,
                  pub uid: i64, pub gid: i64, pub mode: i64 /*&0o7777*/, pub atime: i64, pub mtime: i64 }
impl From<&std::fs::Metadata> for Live { … }        // from fs::symlink_metadata (lstat), never metadata
pub enum Verdict { Holds, NotRegular, Changed(&'static str /*flag name*/) }
pub fn holds_live(f: &QueryFilters, m: &Live) -> Verdict;
```

Mirror `to_conditions` exactly, against the epochs `QueryFilters` already stores (computed once at
filter build, `days_ago_epoch`), never a fresh `now` — `xdu-rm` recomputes `now` after the prompt
(`:133`), so its live and SQL thresholds differ by the prompt's wait:
`size >= min_size`, `size <= max_size`, `atime < older_than`, `atime >= newer_than`, `uid = owner_uid`,
`gid = group_gid`, mode `Exact: mode == b` / `Any: mode & b != 0` / `All: mode & b == b`,
`mtime < mtime_older_than`, `mtime >= mtime_newer_than`; pattern holds by construction.
Pin agreement with a table-driven unit test that evaluates `to_where_clause()` over a one-row
`VALUES` in an in-memory DuckDB and asserts the same boolean as `holds_live` on each grid point
(boundaries `t-1, t, t+1` for every clock, every mode operator).

**Size is the one column the live check cannot reproduce exactly.** The index's `size` is
`SizeMode`-dependent (`crawl.rs:255`): disk usage (`blocks*512`, the default), apparent length, or
length rounded to a block size — and the marker does not record which. `xdu-rm` compares `len()`
regardless. Default-mode indexes of small or compressed/sparse files will disagree with `len()`
near a threshold (a 100-byte file stored as 4096 selected by `--min-size 1K` would be skipped).
Options: (a) hold if the predicate holds for **either** `len` or `disk` — a growth past `--max-size`
still trips both, compression/rounding no longer false-skips; (b) both — fail-closed, surprising
skips; (c) record the size mode in the marker — index-format change, out of scope. Recommend (a),
documented in both man pages and the GOAL's R7 reading recorded in PLAN; the plan should say so
explicitly since R7 says "size against `--min-size`/`--max-size`" without naming the measurement.

## 3. Parallel executor

- **Pool:** a local `rayon::ThreadPoolBuilder::new().num_threads(jobs).build()?` and `pool.install`,
  not `build_global` — a library global pool fights parallel unit tests and `xdu-tar`. `-j 0` keeps
  `xdu-rm`'s meaning (rayon's default width).
- **Deterministic order:** process `rows` in chunks (e.g. 1024): `chunk.par_iter().map(act).collect::<Vec<Outcome>>()`
  preserves index order; the main thread then prints and tallies each chunk in order. Output is
  deterministic and still streams; tallying on the main thread makes `AtomicU64` unnecessary (keep a
  plain `Summary` struct; atomics only if the plan wants per-file progress inside a chunk).
- `Outcome { Done { owner_kept: bool }, Missing, Existing, Skipped(Verdict), Failed(String) }` —
  ownership-not-preserved is a flag on `Done`, counted separately (R9).
- **Streams (R9):** stdout carries only the dry-run listing (`SRC -> DEST`), the `-v` per-file lines,
  the empty-match message and the final summary. Every failure line goes to stderr **always** (not only
  under `-v` as in `xdu-rm`): a failed file makes the exit non-zero and the operator must be able to
  see which. Skips/missing/existing per-file lines: `-v` only, stdout, as part of the listing.
- **Summary on stdout,** like `xdu-rm`: `rm_tests.rs` asserts `Deleted: N` on stdout and the README
  documents `--dry-run | tail -1` and a cron `>> log 2>&1`. One line per non-zero count, the action
  count always (`Copied: N` / `Moved: N`).
- **Exit:** `1` when `failed > 0 || existing > 0` (R9), after printing the summary; `0` otherwise,
  including all-missing/skipped. Refusals (R3, gates, bad args) exit non-zero before any write.
- **Prompt on stderr,** not stdout: R9 keeps stdout to listing/summary, a `> log` redirect would
  otherwise swallow the question, and `cp -i`/`rm -i` prompt on stderr. EOF or anything but `y`/`yes`
  → `Aborted.` on stderr, nothing written, exit 0 (`xdu-rm` parity; flag it in PLAN if the plan prefers 1).
- **Order of checks** in `main`: S3 refusal (raw `OsStr`, before pool/canonicalize, as `xdu-rm`
  `:30`) → `--into` exists and is a dir → `select()` gates → empty-set message, exit 0 → whole-set R3
  checks (outside `--root`, dest == source), naming the first path in `ORDER BY path` order — before
  `--dry-run` too, so the preview refuses what the run would → dry-run → prompt → executor.

## 4. Test infrastructure

- `tests/common/mod.rs::binary_path` is a closed `match` that panics on an unknown name, built on
  `env!("CARGO_BIN_EXE_<name>")` (compile-time). It needs two **new arms** (`xdu-cp`, `xdu-mv`) and
  new `run_cp`/`run_mv` helpers mirroring `run_rm`; `common::s3::Hermetic::command(name)` resolves
  through it too, so the S3-refusal tests can copy `test_s3_locator_is_refused_before_anything_runs`.
  `rm_tests.rs` compiles `common` into its own crate and uses only `build_index`,
  `create_test_file`, `run_rm`, `set_atime_days_ago`, `common::s3::Hermetic`: adding arms and
  functions is safe; changing any existing signature or behaviour is the risk. The env! arms require
  the `[[bin]]` entries in `Cargo.toml` (bins are listed explicitly) or every test crate fails to compile.
- Add one helper the suite lacks: run a binary with given **stdin** (no existing test drives `xdu-rm`'s
  prompt). Needed for R6 (`n`, empty, EOF → nothing written; `y` → acts).
- Fixtures as `rm_tests.rs`: `TempDir` → `source/<partition>/…` → `build_index` (`--apparent-size`,
  so `--min-size`/`--max-size` cases are exact) → `--into` a sibling dir. Index rows hold the
  **canonical** crawl root: on macOS `TempDir` is under `/var` → `/private/var`, so tests must pass
  `--root` as `source.canonicalize()`.
- `temp_index.sh`: builds release bins, a 4-file tree, exports `XDU_INDEX`/`XDU_JOBS`, puts bins on
  `PATH`, `cd`s into the site and runs `"$@"`. A `verify:` drive works unchanged:
  `.agents/factory/bin/temp_index.sh sh -c 'mkdir dest && xdu-cp --into dest --root "$(cd tree && pwd -P)" -u alice --force && find dest -type f'`.
  It uses disk-usage sizes; byte-exact size checks belong in `tests/`.

## 5. CLI (§10)

Probe (`target/tmp/clapprobe`): a `#[derive(Args)]` struct flattened with `#[command(flatten)]`
gives **byte-identical** `--help`, zsh and bash completions to the same fields written inline; help
order is declaration order at the flatten point; `env = "XDU_INDEX"` survives; the flattened struct's
doc comment does not leak into `about`. So: a new `SelectArgs` (`-i`, `-p`, `--regex`, `-u`, the ten
filters, `-l`, `-n`, `--safe`, `-f`, `-v`, `-j`) flattened into new `XduCpArgs`/`XduMvArgs` (which add
`--into`, `--root`), with `XduFindArgs`/`XduRmArgs` untouched — rm's struct must not change (R1).
Note `-f` is `--force` here (rm's meaning) but `--format` in `xdu-find` (R8 says "as `xdu-rm`
defines them"); man pages must say so. `gen-completions` needs the two new entries.

## Risks

1. Size-mode mismatch in `--safe` (above) — decide and document; the most likely user-visible surprise.
2. Glob refusal blocks an index path with `[`/`*`/`?` that `xdu-find` reads today; acceptable
   (fail-closed), stated in the error and man page; the readers' issue can lift it for everyone.
3. Memory: `Vec<Row>` of the whole set (≈ path + 48 B per row); 10 M rows ≈ 1–2 GB. `xdu-rm` holds
   the same order of magnitude; R3's whole-set pre-check needs the set anyway. Note, don't solve.
4. `binary_path` arms are compile-time `env!`: forgetting a `[[bin]]` breaks every test crate, rm's included.
5. Lossy (non-UTF-8) rows name no file: `lstat` → NotFound → `Missing`, as `xdu-rm` (GOAL non-goal).
