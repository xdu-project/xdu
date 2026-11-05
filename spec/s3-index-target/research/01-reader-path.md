# Research 01 — Reader path for an `s3://bucket/prefix` index

Scope: `xdu-find`, `xdu-view`, the shared lib they call, `src/engine.rs`, and `xdu-rm`'s refusal
(R2, R4, R6, R7, R8). Anchors are against `e3593e9`. Design is fixed upstream: query in place
through `httpfs` ranged reads, no local copy, S3 locator and partition names bound, credentials
never in SQL.

## 0. Blocking finding: `Config` keys at open cannot coexist with autoload off

Spike claim 11 opened a connection with `s3_*` keys through `Config::with` and default flags,
after an earlier `INSTALL httpfs` had filled `$HOME/.duckdb`. That is not xdu's configuration.
The vendored 1.4.4 amalgamation (`libduckdb-sys-1.4.4/duckdb.tar.gz`) shows why it worked:

- `DBConfig::SetOptionByName` (`src/main/config.cpp:272-283`) parks an unknown key such as
  `s3_access_key_id` in `unrecognized_options`.
- `DatabaseInstance::LoadExtensionSettings` (`src/main/database.cpp:220-262`), which runs during
  open (`:319`), autoloads the owning extension only `if (autoload_known_extensions)`. Otherwise
  it throws `"The following options were not recognized: s3_access_key_id, …"`.
- Autoload resolves `<extension_directory>/<version>/<platform>/httpfs.duckdb_extension`
  (`extension_load.cpp:403-414`). That is not the flat `lib/xdu/extensions/httpfs.duckdb_extension`
  layout, and changing the layout is a non-goal.

So `open_connection` with autoload off plus `s3_*` keys fails at open. `engine.rs:39-43` cannot
just grow a `Config` argument. This is source reading, not a run, so the probe subagent should
confirm it in one line. Candidate routes, each needing a probe:

1. **LOAD, then set the keys without the secret in SQL text:** `SET VARIABLE xdu_k = ?` (bound;
   the `VARIABLE` scope binds through a select node, `bind_set.cpp:27-35`), then
   `SET GLOBAL s3_access_key_id = getvariable('xdu_k')`, then `RESET VARIABLE`. Plain `SET x = ?`
   is refused (`bind_set.cpp:23-24`, "SET statements cannot have parameters"). Whether
   `ConstantBinder` accepts `getvariable` is unverified.
2. **`CREATE SECRET (TYPE s3, KEY_ID ?, …)`:** secret options are `ParsedExpression`s
   (`create_secret_info.hpp:44`), so a bound parameter or `getvariable(...)` may bind.
3. **Autoload on, autoinstall off, `extension_directory` at a versioned tree:** this needs a
   layout change, and it reopens the autoload surface `offline_tests` guards. Reject.

Routes 1 and 2 both violate R5's literal "through DuckDB's `Config` API" (and R6's "only through
its Config API"), so either one needs a **GOAL amendment** at the gate: "never as SQL text; bound
parameters or Config". The non-negotiable part (no secret in any SQL string, log or output)
still holds.

## 1. Where the index location is touched

| Site | Today | For S3 |
|---|---|---|
| `src/cli.rs:79-80`, `:162-163`, `:211-212` | `index: PathBuf`, `env = "XDU_INDEX"` (find, view, rm) | Parse once into `IndexLocator` (see the recommendation) |
| `xdu-find.rs:16-19`, `xdu-view.rs:1884-1887`, `xdu-rm.rs:35-38` | `canonicalize()` → "Index directory not found" | Local only. An S3 locator skips it. Normalise the prefix instead (trailing `/` would make `prefix//*/*.parquet`, a different key) |
| `lib.rs:100-122` `read_completion_marker` | `fs::metadata` → `is_file` + `MARKER_READ_LIMIT` (`:62`, 64 KiB) → `fs::read_to_string` | S3 variant through the connection (§4) |
| `lib.rs:166-191` / `:199-221` warning and version gate | `&Path`, each re-reads the marker, messages use `index.display()` | Take a pre-read marker plus a display string. Read once (it is network round trips now) |
| `lib.rs:146-151` `index_glob` | `format!("{}/*/*.parquet", index.display())` | S3 sibling sharing the layout tail. §3 keeps it "the one place" |
| `xdu-find.rs:87-96` `--top`; `xdu-view.rs:685,703,1197,1215` | `regexp_extract(filename, '.*/([^/]+)/[^/]+\.parquet$', 1)` | Unchanged. Spike claim 8: works on `s3://` filenames |
| `xdu-view.rs:1469,1504,1562` preview | `File::open(<indexed path>)` | Not the index: it opens the *source* file. On a host without the source tree it degrades to "(unreadable)" (`:1478-1491`). Parity, no change |
| `xdu-view.rs:490-491`, `:550-559` | `App { index_path: PathBuf }` | Holds `IndexLocator` |

No reader does `read_dir` on the index. Partition discovery is entirely the `filename` regexp
inside SQL. Listing goes through DuckDB's glob, so it is LIST over S3 (flat 2 calls, spike).

## 2. Reader SQL today, and binding only for S3

Every site uses `format!` plus `conn.prepare(&sql)` plus `stmt.query([])`. No bound parameters
anywhere.

- **find:** 7 statements, one per mode: `--top` `:87-96`, `--count` `:108-111`, then path, size,
  atime, csv and json at `:123`, `:135`, `:148`, `:161`, `:201`.
- **view:** 8 statements in 6 functions: `query_root_files` `:599-608`, `load_partitions`
  `:681-716` (2 variants), `discover_partition_root` `:759-762`, `load_directory` `:808-847`,
  `make_partition_column` `:1193-1228` (2 variants), `make_directory_column_tree` `:1296-1331`.
  `load_partitions` ≡ `make_partition_column` and `load_directory` ≡ `make_directory_column_tree`
  in SQL, so there are 4 distinct queries.
- `QueryFilters::to_conditions` (`lib.rs:698-749`) inlines integers and single-quote-escapes the
  pattern. `to_order_by` and `to_partition_order_by` (`:348-372`) return `&'static str`. None of
  these carry the locator, so none need binding (§9 untouched).
- `--limit` is a bare `LIMIT n` (`find:73-77`). Partition scoping (`-u`) reaches SQL only inside
  the glob string. `ROOT_PARTITION` appears in `HAVING` as a constant. So **binding the one
  `read_parquet` argument covers R6 entirely.** Each statement has exactly one `?`. `?` inside
  string literals (a user regex) is not a placeholder.

Helper, one per lib:
`fn parquet_source(loc: &IndexLocator, partition: Option<&str>, filename: bool) -> (String, Vec<String>)`.

- Local returns `read_parquet('<index_glob>')` or `read_parquet('<glob>', filename=true)` with
  empty params. That is byte-identical SQL.
- S3 returns `read_parquet(?)` or `read_parquet(?, filename=true)` with `[glob]`.

Each site changes `read_parquet('{glob}'…)` to `{src}`, and `query([])` to
`query(duckdb::params_from_iter(&params))`. Empty params behave the same as `[]`. Invasiveness is
mechanical: 15 statement edits, no logic change. A lib unit test can pin the local strings.

Unprobed: `read_parquet(?, filename=true)` (the spike bound only `read_parquet(?)`), and the
bound form inside a CTE (`view:808`, `:1296`).

Latent: `path LIKE '{prefix}%'` (`view:802-804`, `:1290-1292`) is built from row data, not the
locator, so R6 does not require binding it. But a bucket's writers are a wider set than a local
dir's, so binding it for S3 is cheap, adds a second `?`, and is worth deciding explicitly.

## 3. xdu-view

- All queries are inline in the binary (§11 debt). Nothing lists the filesystem.
- `App::new` (`:550-590`) runs `load_partitions` or `load_directory` before any terminal call.
  `main` opens the terminal only after `App::new` (`:1921-1925`).
- Refusals are testable today through `run_view` (`tests/common/mod.rs:178`), which is how
  `version_tests.rs:102-108` checks the viewer.
- Swallowed errors: `query_root_files` maps prepare or query errors to "no `__root__`"
  (`:611-618`). With a bound glob, the no-match error moves from prepare to query, which is still
  swallowed. A transport error there would also be swallowed silently. That is acceptable after
  the marker read proves reachability, but note it.

**Smallest seam:** move the 4 distinct queries into a lib module (`src/view_query.rs`). Each takes
`(&Connection, &IndexLocator, …, &QueryFilters, SortMode) -> Result<Vec<DirEntry>>`, and
`DirEntry` moves with them. The 6 `App` methods become one-line callers. A
`tests/s3_view_tests.rs` then asserts identical `Vec<DirEntry>` from local and S3 across filters
and all 7 `SortMode`s, with no TUI. This also pays down §11 and halves the binding sites.

Rejected: bin-internal `#[cfg(test)]` (it cannot reach `CARGO_BIN_EXE_xdu` or the tests/ S3
harness), and a hidden dump flag (new CLI surface in high-blast `cli.rs`, plus man-page
obligations).

## 4. The marker over S3 (R2 cap, R4 causes)

The parsers `completion_marker_format` and `completion_marker_errors` (`lib.rs:70-79`, `:130-139`)
take `&str` and ride along unchanged. Keep `MarkerRead { Absent, Unreadable, Body }`, but the S3
fetch must return `Result<MarkerRead>`. Today any `metadata` error means `Absent` (`:105-108`).
Over S3, a refused credential or an unreachable endpoint must surface as the cause (R4), not as
"carries no index format version".

Visible in the source (`direct_file_reader.cpp:56-75`, `:88-140`): `read_blob` opens the file
only for non-`filename` columns, and reads content only when `content` is projected. So
`SELECT size FROM read_blob(?)` is plausibly HEAD-only. Proposed sequence on the S3 connection:

1. `SELECT size FROM read_blob(?)` → over `MARKER_READ_LIMIT` → `Unreadable`, never fetched.
   A 404 → `Absent`. Distinguish 404 from 403 or connection errors either by DuckDB error text
   (brittle) or by a preceding `glob(?)`: LIST error = cause, empty = absent. Whether `glob()`
   lists a literal dotfile key is unverified.
2. `SELECT content FROM read_blob(?)` → `String::from_utf8`, with failure as an empty body. That
   mirrors the local `unwrap_or_default`. Use `read_text` and invalid UTF-8 becomes an error.

The HEAD-then-GET window equals the local stat-then-read window (`:105` → `:121`), so this is
parity, not a new hole. The alternative is one `Range: bytes=0-65535` GET through the writer's
Rust S3 client, which is atomic and gives typed 404/403. That only works if the client is
reachable from the readers. Plug-in point: a `read_completion_marker_s3(conn, loc)` beside
`:100`, with both gates consuming the result. The S3 order in each binary becomes: parse, open
S3 conn plus load httpfs (the R7 error comes first), marker, gate, warning, queries. Still no
rows before the gate.

## 5. `src/engine.rs`

- `open_connection()` (`:39-43`) stays byte-identical, and local paths keep calling only it.
  `load_extension` has no CLI caller today (grep). That structurally guarantees a local query
  loads nothing, and `offline_tests.rs:31-89` passes unchanged.
- Add `open_s3_connection(&S3Settings) -> Result<Connection>`: `open_connection()`, then
  `load_extension(&conn, "httpfs")`, then apply settings by the §0 route. Keys:
  `s3_access_key_id`, `s3_secret_access_key`, `s3_session_token`, `s3_region`, `s3_endpoint`,
  `s3_url_style`, `s3_use_ssl` (spike `capability.rs:69-75`).
- The R7 text lives at `:115-121` today: "DuckDB extension 'httpfs' not found at <path>; xdu does
  not download extensions. Install it there, or set XDU_EXTENSION_DIR to the directory that holds
  it". It lacks "copy `lib/xdu/extensions/` from a release tarball". Amend it, keeping the
  substrings `assert_names_absent_file` (`:207-227`) checks.
- CI gap: `test.yaml` never fetches httpfs (only `release.yaml:190`), so the S3 tests need
  `fetch.sh` plus `XDU_EXTENSION_DIR` in the test job.

## 6. xdu-rm (R8)

`args.index` is parsed at `xdu-rm.rs:26`. The earliest refusal point is `:27`, before the rayon
pool (`:29-32`), `canonicalize` (`:35`), the gates, `open_connection` (`:57`) and the query
(`:79-85`). Today an `s3://` argument already exits non-zero at `:38`, but with the wrong message.
Keep `index_glob(&Path, …)`'s signature so the rm diff is the refusal alone (high-blast file,
human gate).

## 7. Archived brief 02: what still holds

Still valid:
- The prologue shape.
- The 15 query sites.
- The data-derived `LIKE` prefix surface.
- Locator text echoes freely into diagnostics. Reject a `s3://user:secret@…` userinfo form at
  parse time.
- The marker parsers are transport-free.

Retired:
- The filename regexp risk (claim 8).
- The httpfs air-gap risk (#31 shipped the extension).
- The LIST consistency scare (S3 and MinIO are strongly consistent, and rm is refused anyway).

Staging-specific content: dropped.

## Change map

| File | Change | R |
|---|---|---|
| `src/lib.rs` (or a new `src/locator.rs`) | `IndexLocator`, prefix normalisation, refuse userinfo and glob metacharacters in bucket or prefix, `parquet_source`, S3 glob beside `index_glob`, gates over a pre-read marker, `read_completion_marker_s3` | R2, R4, R6 |
| `src/engine.rs` | `open_s3_connection`, credential application per §0, R7 message | R5–R7 |
| `src/cli.rs` | `index` → `IndexLocator` via `value_parser`, plus help text | R2, R8 |
| `src/bin/xdu-find.rs` | Branch the prologue, 7 sites through `parquet_source` | R2, R4, R6 |
| `src/view_query.rs` (new), `src/bin/xdu-view.rs` | Extract 4 queries, `App` holds the locator, prologue branch | R2, R6 |
| `src/bin/xdu-rm.rs` | Refuse at `:27` | R8 |
| `tests/` and `test.yaml` | S3 find and view parity, marker cap, R4 matrix, httpfs provisioning | R2, R4, R7, R10 |

## Top 3 scares

1. **§0.** The spec's credential mechanism does not work with autoload off. This needs a probe
   and a GOAL amendment before build. Otherwise R5/R6 are unmeetable as written.
2. **The marker 404 vs 403 vs unreachable distinction** through DuckDB errors. If only error
   text separates them, R4's "cause on stderr" rests on string matching across httpfs versions.
3. **The view extraction.** About 300 lines move out of a 2837-line untested binary, and it is
   phased last. If it slips, the cut is xdu-view's S3 support, through the GOAL gate.

## Recommendation: the locator type

Use `enum IndexLocator { Local(PathBuf), S3(S3Locator { bucket, prefix }) }`, parsed once in lib
and wired as clap's `value_parser` on all three readers' `index`. The rule: the argument bytes
start with `s3://`, else local unchanged, and non-UTF-8 is always local. Add a `Display` that
never carries credentials. `Local` canonicalizes in `parse` or `resolve()` to keep today's
error. Both `glob(partition)` and the marker read dispatch on the variant, so binaries never
build an S3 string themselves.
