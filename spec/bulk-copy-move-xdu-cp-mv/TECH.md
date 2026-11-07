---
slug: bulk-copy-move-xdu-cp-mv
title: 'Bulk copy and move: xdu-cp and xdu-mv over a shared select-act engine'
kind: feature
appetite: big
status: in_review
branch: feature/bulk-copy-move-xdu-cp-mv
base: main
current_phase: done
last_updated: '2026-11-07'
phases:
- id: P1
  name: Selection engine and xdu-cp's atomic, no-replace copy
  status: done
  satisfies:
  - R1
  - R3
  - R4
  - R6
  - R8
  - R9
  depends_on: []
  parallel: false
  hammerable: false
  hill: downhill
  attempts: 2
  verify: 'set -eu; mkdir -p target; cargo test --locked --lib >target/p1-lib.log
    2>&1; for t in select::tests::the_selection_sql_binds_the_glob_and_the_pattern
    select::tests::the_selection_always_orders_by_path_once select::tests::the_selection_reuses_query_filters_predicates
    select::tests::an_index_path_with_glob_metacharacters_is_refused select::tests::a_partition_name_that_could_leave_the_index_is_refused
    atomic_fs::tests::publish_never_replaces_an_existing_name atomic_fs::tests::the_link_fallback_never_replaces_an_existing_name
    atomic_fs::tests::the_link_fallback_rechecks_the_source_before_unlinking atomic_fs::tests::a_failed_copy_leaves_nothing_at_the_final_name
    atomic_fs::tests::copy_atomic_abandoned_at_every_step_never_leaves_a_partial_final_name
    atomic_fs::tests::a_non_regular_source_is_never_opened atomic_fs::tests::a_symlink_in_the_destination_tree_is_not_followed
    atomic_fs::tests::a_symlinked_source_parent_is_not_followed relocate::tests::a_row_outside_root_or_with_a_dot_dot_component_is_refused;
    do grep -qF "test $t ... ok" target/p1-lib.log || { echo "P1: $t did not pass"
    >&2; exit 1; }; done; cargo test --locked --test cp_tests >target/p1-cp.log 2>&1;
    for t in cp_copies_exactly_the_selected_set_under_root cp_dry_run_writes_nothing
    cp_dry_run_and_the_real_run_select_identical_rows_under_limit cp_refuses_an_s3_index_before_anything
    cp_refuses_a_missing_or_non_directory_into cp_refuses_a_path_outside_root_naming_the_first
    cp_refuses_a_destination_equal_to_its_source cp_selects_the_right_rows_with_a_quote_in_the_index_and_partition
    cp_refuses_a_glob_metacharacter_in_the_index_path cp_refuses_a_partition_that_does_not_exist
    cp_counts_an_existing_destination_and_exits_non_zero cp_declined_prompt_writes_nothing
    cp_accepted_prompt_copies cp_empty_match_exits_zero cp_skips_a_symlink_at_the_source_path
    cp_skips_a_fifo_and_a_directory_at_the_source_path cp_accepts_every_xdu_find_filter
    cp_reports_failures_on_stderr_and_exits_non_zero cp_selects_the_true_rows_under_a_hive_style_partition
    cp_selects_the_true_rows_from_an_index_under_a_hive_style_directory; do grep -qF
    "test $t ... ok" target/p1-cp.log || { echo "P1: $t did not pass" >&2; exit 1;
    }; done; if grep -nE "fs::rename|fs::copy|libc::rename\(|libc::renameat\(|File::create|O_TRUNC|truncate\(true\)|fs::write"
    src/select.rs src/atomic_fs.rs src/relocate.rs src/bin/xdu-cp.rs | grep -vE "^[^:]+:[0-9]+:[[:space:]]*//";
    then echo "P1: a replacing rename, copy or truncating open is in the new code"
    >&2; exit 1; fi; if grep -nE "read_parquet\([^?]|rayon::|crate::(atomic_fs|relocate)"
    src/select.rs | grep -vE "^[0-9]+:[[:space:]]*//"; then echo "P1: select.rs interpolates
    the glob or reaches past the selection" >&2; exit 1; fi; test -f doc/xdu-cp.1.scd;
    if grep -nE "^[[:space:]]*[.'']" doc/xdu-cp.1.scd; then echo "P1: roff control
    line" >&2; exit 1; fi; .agents/factory/bin/temp_index.sh sh -c "set -eu; mkdir
    dest; r=\$(cd tree && pwd -P); xdu-cp --into dest --root \"\$r\" -u alice --force
    >/dev/null; test -f dest/alice/logs/app.log; test -f dest/alice/data.bin; test
    ! -e dest/bob; test -f tree/alice/data.bin; if xdu-cp --into dest --root \"\$r\"
    -u alice --force >/dev/null; then echo existing must exit non-zero >&2; exit 1;
    fi; test \"\$(xdu-cp --into dest --root \"\$r\" -n | grep -c -- \" -> \")\" -eq
    4"; cargo test --locked --test mv_tests >target/p1-mv.log 2>&1; grep -qF "test
    mv_moves_only_the_true_rows_under_a_hive_style_partition ... ok" target/p1-mv.log;
    if grep -n "read_parquet(" src/select.rs src/relocate.rs src/bin/xdu-cp.rs src/bin/xdu-mv.rs
    | grep -vE "^[^:]+:[0-9]+:[[:space:]]*//" | grep -vF "hive_partitioning = false";
    then echo "P1: a select-act read_parquet leaves Hive partitioning on" >&2; exit
    1; fi; if grep -nF "derives no partition column" AGENTS.md .agents/factory/invariants.md;
    then echo "P1: the false no-partition-column claim is live" >&2; exit 1; fi; grep
    -qF "hive_partitioning = false" AGENTS.md; test -f issues/hive-partition-columns-find-rm.md;
    grep -qF "Priority: high" issues/hive-partition-columns-find-rm.md; grep -qF "hive-partition-columns-find-rm"
    ROADMAP.md; cargo test --locked --test rm_tests >target/p1-rm.log 2>&1; git diff
    --quiet edcc163 -- src/bin/xdu-rm.rs tests/rm_tests.rs; cargo clippy --locked
    --all-targets --all-features -- -D warnings; cargo fmt --all -- --check; echo
    P1-GATE-GREEN'
- id: P2
  name: xdu-cp carries metadata, re-checks with --safe and counts every outcome
  status: done
  satisfies:
  - R1
  - R2
  - R7
  - R9
  depends_on:
  - P1
  parallel: false
  hammerable: false
  hill: uphill
  attempts: 1
  verify: 'set -eu; mkdir -p target; cargo test --locked --lib >target/p2-lib.log
    2>&1; for t in select::tests::the_live_check_agrees_with_the_sql_on_every_boundary
    select::tests::size_uses_the_measure_that_matches_the_indexed_size select::tests::a_file_that_shrank_below_min_size_fails_the_live_check
    select::tests::a_non_regular_file_never_holds atomic_fs::tests::a_source_written_during_the_copy_is_skipped_and_nothing_published
    atomic_fs::tests::a_refused_chown_is_counted_and_drops_the_set_id_bits atomic_fs::tests::a_refused_owner_still_carries_the_group
    atomic_fs::tests::metadata_is_set_owner_then_mode_then_times select::tests::an_unstated_size_measure_also_requires_the_indexed_mtime
    atomic_fs::tests::a_walk_passes_a_search_only_ancestor atomic_fs::tests::a_directory_that_must_be_flushed_is_opened_for_reading_or_fails_closed
    atomic_fs::tests::a_symlinked_source_parent_is_not_followed; do grep -qF "test
    $t ... ok" target/p2-lib.log || { echo "P2: $t did not pass" >&2; exit 1; }; done;
    cargo test --locked --test cp_tests >target/p2-cp.log 2>&1; for t in cp_carries_bytes_mode_atime_and_mtime
    cp_carries_owner_and_group_when_permitted cp_safe_skips_a_file_that_no_longer_matches_each_filter
    cp_safe_keeps_a_file_that_still_matches cp_safe_skips_a_file_replaced_by_a_symlink
    cp_summary_counts_each_outcome_separately cp_never_modifies_a_source cp_keeps_diagnostics_off_stdout
    cp_verbose_lists_in_path_order cp_safe_skips_a_file_that_grew_past_the_bound_by_the_index_block_size
    cp_copies_through_search_only_ancestors cp_fails_closed_on_a_destination_directory_it_cannot_read;
    do grep -qF "test $t ... ok" target/p2-cp.log || { echo "P2: $t did not pass"
    >&2; exit 1; }; done; if grep -nE "fs::rename|fs::copy|libc::rename\(|libc::renameat\(|File::create|O_TRUNC|truncate\(true\)|fs::write"
    src/select.rs src/atomic_fs.rs src/relocate.rs src/bin/xdu-cp.rs | grep -vE "^[^:]+:[0-9]+:[[:space:]]*//";
    then echo "P2: a replacing rename, copy or truncating open is in the new code"
    >&2; exit 1; fi; cargo test --locked --test mv_tests >target/p2-mv.log 2>&1; for
    t in mv_safe_skips_a_file_that_grew_past_the_bound_by_the_index_block_size mv_moves_through_search_only_ancestors
    mv_fails_closed_on_a_source_directory_it_cannot_read; do grep -qF "test $t ...
    ok" target/p2-mv.log || { echo "P2: $t did not pass" >&2; exit 1; }; done; test
    "$(id -u)" -ne 0 || echo "P2: running as root, so the permission cases skipped;
    they run on any other user" >&2; if grep -nE "sync_durable\(" src/atomic_fs.rs
    src/relocate.rs | grep -vE "^[^:]+:[0-9]+:[[:space:]]*//" | grep -vE "fn sync_durable|sync_durable\((out|self\.fd)\.as_fd\(\)\)";
    then echo "P2: a directory is flushed without DirHandle::sync" >&2; exit 1; fi;
    for f in doc/xdu-cp.1.scd doc/xdu-mv.1.scd; do grep -qF "skipped as changed since
    the index" "$f"; grep -qF "both live measures" "$f"; grep -qF "# PERMISSIONS"
    "$f"; grep -qF "0711" "$f"; done; grep -qF -- "--safe" doc/xdu-cp.1.scd; grep
    -qiF "indexed size" doc/xdu-cp.1.scd; .agents/factory/bin/temp_index.sh sh -c
    "set -eu; mkdir dest; r=\$(cd tree && pwd -P); xdu-cp --into dest --root \"\$r\"
    --safe --force >out.txt; grep -qE \"^Copied: 4\$\" out.txt; cmp tree/alice/data.bin
    dest/alice/data.bin; mkdir dest2; chmod 111 tree/alice; rc=0; xdu-cp --into dest2
    --root \"\$r\" -u alice --force >out2.txt || rc=\$?; chmod 755 tree/alice; test
    \$rc -eq 0; grep -qE \"^Copied: 2\$\" out2.txt; cmp tree/alice/logs/app.log dest2/alice/logs/app.log";
    cargo test --locked --test rm_tests >target/p2-rm.log 2>&1; git diff --quiet edcc163
    -- src/bin/xdu-rm.rs tests/rm_tests.rs; cargo clippy --locked --all-targets --all-features
    -- -D warnings; cargo fmt --all -- --check; echo P2-GATE-GREEN'
- id: P3
  name: xdu-mv relocates on one filesystem with a no-replace rename
  status: done
  satisfies:
  - R3
  - R4
  - R5
  - R6
  - R7
  - R8
  - R9
  depends_on:
  - P2
  parallel: false
  hammerable: false
  hill: uphill
  verify: 'set -eu; mkdir -p target; cargo test --locked --lib >target/p3-lib.log
    2>&1; for t in relocate::tests::a_cross_device_move_under_the_refuse_policy_keeps_the_source
    atomic_fs::tests::a_same_filesystem_move_keeps_the_inode atomic_fs::tests::a_same_filesystem_move_never_replaces
    atomic_fs::tests::a_same_filesystem_move_by_the_link_route_never_replaces atomic_fs::tests::a_same_filesystem_move_skips_a_non_regular_object;
    do grep -qF "test $t ... ok" target/p3-lib.log || { echo "P3: $t did not pass"
    >&2; exit 1; }; done; cargo test --locked --test mv_tests >target/p3-mv.log 2>&1;
    for t in mv_renames_on_one_filesystem_keeping_the_inode mv_never_replaces_an_existing_destination
    mv_dry_run_writes_nothing mv_dry_run_and_the_real_run_select_identical_rows_under_limit
    mv_refuses_an_s3_index_before_anything mv_refuses_a_path_outside_root_before_moving_anything
    mv_refuses_a_missing_into mv_declined_prompt_moves_nothing mv_empty_match_exits_zero
    mv_safe_skips_a_changed_file_and_leaves_it mv_skips_a_symlink_at_the_source_path
    mv_skips_a_directory_at_the_source_path mv_skips_a_fifo_at_the_source_path mv_summary_counts_and_exit_status
    mv_accepts_every_xdu_find_filter; do grep -qF "test $t ... ok" target/p3-mv.log
    || { echo "P3: $t did not pass" >&2; exit 1; }; done; cargo test --locked --test
    cp_tests >target/p3-cp.log 2>&1; if grep -nE "fs::rename|fs::copy|libc::rename\(|libc::renameat\(|File::create|O_TRUNC|truncate\(true\)|fs::write"
    src/select.rs src/atomic_fs.rs src/relocate.rs src/bin/xdu-cp.rs src/bin/xdu-mv.rs
    | grep -vE "^[^:]+:[0-9]+:[[:space:]]*//"; then echo "P3: a replacing rename,
    copy or truncating open is in the new code" >&2; exit 1; fi; test -f doc/xdu-mv.1.scd;
    if grep -nE "^[[:space:]]*[.'']" doc/xdu-mv.1.scd; then echo "P3: roff control
    line" >&2; exit 1; fi; .agents/factory/bin/temp_index.sh sh -c "set -eu; mkdir
    dest; r=\$(cd tree && pwd -P); xdu-mv --into dest --root \"\$r\" -u bob -n >/dev/null;
    test -f tree/bob/notes.txt; test ! -e dest/bob; xdu-mv --into dest --root \"\$r\"
    -u bob --force >/dev/null; test -f dest/bob/notes.txt; test ! -e tree/bob/notes.txt";
    cargo test --locked --test rm_tests >target/p3-rm.log 2>&1; git diff --quiet edcc163
    -- src/bin/xdu-rm.rs tests/rm_tests.rs; cargo clippy --locked --all-targets --all-features
    -- -D warnings; cargo fmt --all -- --check; echo P3-GATE-GREEN'
- id: P4
  name: Man pages, README and packaging in lockstep
  status: done
  satisfies:
  - R10
  depends_on:
  - P3
  parallel: false
  hammerable: false
  hill: downhill
  verify: 'set -eu; mkdir -p target; for f in doc/xdu-cp.1.scd doc/xdu-mv.1.scd; do
    grep -qF "The index is not updated." "$f"; grep -qF "Reading a source may update
    its atime" "$f"; grep -qF "XDU_INDEX" "$f"; grep -qF "XDU_JOBS" "$f"; if grep
    -nE "^[[:space:]]*[.'']" "$f"; then echo "P4: roff control line in $f" >&2; exit
    1; fi; done; grep -qF "xdu-cp -i" README.md; grep -qF "xdu-mv -i" README.md; test
    "$(grep -c "xdu-rm xdu-cp xdu-mv; do" .github/workflows/release.yaml)" -eq 3;
    test "$(grep -c "xdu-rm xdu-cp xdu-mv; do" packaging/extensions/check.sh)" -eq
    3; grep -qF "BINARIES=\"xdu xdu-find xdu-view xdu-rm xdu-cp xdu-mv\"" install.sh;
    grep -qF -- "--bin xdu-cp --bin xdu-mv" Dockerfile; grep -qF "target/release/xdu-mv"
    Dockerfile; grep -qF "target/release/xdu-mv" packaging/rpm/xdu.spec; grep -qF
    "xdu*" hpccm/xdu.py; grep -qF "xdu*" hpccm/xdu.def; grep -qF "xdu*" hpccm/xdu.docker;
    grep -qF "check share/man/man1/xdu-cp.1" .github/workflows/test.yaml; grep -qF
    "check share/man/man1/xdu-mv.1" .github/workflows/test.yaml; grep -qF "share/bash-completion/completions/xdu-cp.bash"
    .github/workflows/test.yaml; grep -qF "share/zsh/site-functions/_xdu-mv" .github/workflows/test.yaml;
    grep -qF "\"xdu-cp\", \"xdu-mv\"" tests/cli_version_tests.rs; cargo test --locked
    --test cli_version_tests >target/p4-version.log 2>&1; cargo build --locked --release
    --bin gen-completions >target/p4-gen.log 2>&1; mkdir -p target/p4-comp/bash target/p4-comp/zsh;
    ./target/release/gen-completions target/p4-comp/bash target/p4-comp/zsh 2>>target/p4-gen.log;
    test -s target/p4-comp/bash/xdu-cp.bash; test -s target/p4-comp/bash/xdu-mv.bash;
    test -s target/p4-comp/zsh/_xdu-cp; test -s target/p4-comp/zsh/_xdu-mv; grep -qF
    "xdu-cp" AGENTS.md; sed -n ''/^## 4\./,/^## 5\./p'' .agents/factory/invariants.md
    >target/p4-s4.txt; for lit in "xdu-mv" "xdu-cp" "y/N" "--dry-run" "--force" "ORDER
    BY path" "select::holds_live" "deterministic_limit_clause" "relocate::confirm"
    "destructive at the source" "indexed one" "src/relocate.rs" "today it checks only
    atime"; do grep -qF -- "$lit" target/p4-s4.txt || { echo "P4: invariants §4 does
    not name $lit" >&2; exit 1; }; done; test "$(grep -c "^- " target/p4-s4.txt)"
    -ge 6; grep -qF "xdu-mv" .agents/factory/invariants.md; grep -qF "src/atomic_fs.rs"
    .agents/factory/invariants.md; grep -qF "xdu-cp" issues/manpage-gate-coverage-gaps.md;
    test -f issues/www-cp-mv-at-release.md; grep -qF "www-cp-mv-at-release" ROADMAP.md;
    grep -qF "xdu-rm.1" issues/xdu-rm-on-shared-engine.md; git diff --quiet edcc163
    -- doc/xdu-rm.1.scd www/; if grep -nE "fs::rename|fs::copy|libc::rename\(|libc::renameat\(|File::create|O_TRUNC|truncate\(true\)|fs::write"
    src/select.rs src/atomic_fs.rs src/relocate.rs src/bin/xdu-cp.rs src/bin/xdu-mv.rs
    | grep -vE "^[^:]+:[0-9]+:[[:space:]]*//"; then echo "P4: a replacing rename,
    copy or truncating open is in the new code" >&2; exit 1; fi; cargo test --locked
    >target/p4-all.log 2>&1; git diff --quiet edcc163 -- src/bin/xdu-rm.rs tests/rm_tests.rs;
    cargo clippy --locked --all-targets --all-features -- -D warnings; cargo fmt --all
    -- --check; echo P4-GATE-GREEN'
- id: P5
  name: 'xdu-mv crosses filesystems: durable copy, unchanged check, then unlink'
  status: done
  satisfies:
  - R5
  depends_on:
  - P4
  parallel: false
  hammerable: false
  hill: downhill
  verify: 'set -eu; mkdir -p target target/p5-tmp target/p5-samefs; cargo test --locked
    --lib >target/p5-lib.log 2>&1; for t in atomic_fs::tests::exdev_routes_to_the_copy_path
    atomic_fs::tests::move_across_fails_closed_at_every_step atomic_fs::tests::move_across_abandoned_at_every_step_leaves_the_file_whole_somewhere
    atomic_fs::tests::move_across_flushes_file_and_directory_before_unlinking atomic_fs::tests::move_across_keeps_a_source_that_grew_during_the_copy
    atomic_fs::tests::move_across_keeps_a_source_whose_mtime_was_restored atomic_fs::tests::move_across_keeps_a_source_swapped_before_the_unlink
    atomic_fs::tests::move_across_rolls_back_when_the_source_unlink_fails atomic_fs::tests::move_across_counts_an_existing_destination_without_copying
    atomic_fs::tests::move_across_carries_metadata relocate::tests::the_production_policy_routes_exdev_to_the_copy_path
    relocate::tests::a_cross_device_move_under_the_refuse_policy_keeps_the_source;
    do grep -qF "test $t ... ok" target/p5-lib.log || { echo "P5: $t did not pass"
    >&2; exit 1; }; done; test -f tests/xfs-wire.sh; test -f tests/mv_xfs_tests.rs;
    cargo test --locked --test mv_xfs_tests -- --list >target/p5-xfs-list.log 2>&1;
    test "$(grep -c ": test$" target/p5-xfs-list.log)" -ge 3; if env -u XDU_TEST_XFS_DIR
    sh tests/xfs-wire.sh "" 3 >target/p5-xfs-unset.log 2>&1; then echo "P5: xfs-wire.sh
    passed with no second filesystem" >&2; exit 1; fi; grep -qF "XDU_TEST_XFS_DIR
    is not set" target/p5-xfs-unset.log; if TMPDIR="$PWD/target/p5-tmp" XDU_TEST_XFS_DIR="$PWD/target/p5-samefs"
    sh tests/xfs-wire.sh "" 3 >target/p5-xfs-same.log 2>&1; then echo "P5: xfs-wire.sh
    passed on the same filesystem" >&2; exit 1; fi; grep -qF "XDU_TEST_XFS_DIR is
    not on another filesystem" target/p5-xfs-same.log; grep -qF "/dev/shm" .github/workflows/test.yaml;
    grep -qF "hdiutil create" .github/workflows/test.yaml; grep -qF "sh tests/xfs-wire.sh"
    .github/workflows/test.yaml; grep -qF "XDU_TEST_XFS_DIR" AGENTS.md; grep -qiF
    "another filesystem" doc/xdu-mv.1.scd; grep -qF ".xdu-" doc/xdu-mv.1.scd; grep
    -qF "The index is not updated." doc/xdu-mv.1.scd; if grep -nE "^[[:space:]]*[.'']"
    doc/xdu-mv.1.scd; then echo "P5: roff control line" >&2; exit 1; fi; test -f issues/xdu-mv-quarantine-unlink.md;
    grep -qF "xdu-mv-quarantine-unlink" ROADMAP.md; if grep -nE "fs::rename|fs::copy|libc::rename\(|libc::renameat\(|File::create|O_TRUNC|truncate\(true\)|fs::write"
    src/select.rs src/atomic_fs.rs src/relocate.rs src/bin/xdu-cp.rs src/bin/xdu-mv.rs
    | grep -vE "^[^:]+:[0-9]+:[[:space:]]*//"; then echo "P5: a replacing rename,
    copy or truncating open is in the new code" >&2; exit 1; fi; cargo test --locked
    >target/p5-all.log 2>&1; git diff --quiet edcc163 -- src/bin/xdu-rm.rs tests/rm_tests.rs
    doc/xdu-rm.1.scd; cargo clippy --locked --all-targets --all-features -- -D warnings;
    cargo fmt --all -- --check; echo P5-GATE-GREEN'
review:
  last_reviewed_commit: 5c7f2b5
  verdict: changes-requested
  blocked_reason: 'review cycle 1: 4 CONFIRMED (invariants §4 drift; --safe block-size;
    Hive partition; search-only ancestor); human gate'
  cycle: 1
---
# TECH.md — Bulk copy and move: `xdu-cp` and `xdu-mv` over a shared select-act engine

The **context engine and finite-state machine** for building this feature. The YAML
frontmatter above is the resume ground-truth (read it with
`uv run --with pyyaml python .agents/factory/bin/next_phase.py spec/bulk-copy-move-xdu-cp-mv/TECH.md`);
the per-phase checklists below are the work. `xdu-build` executes the next actionable phase, runs
its `verify:` command, updates state via
`uv run --with pyyaml python .agents/factory/bin/set_phase.py …`, and makes one atomic code+state
commit.

- **Vision / requirements (locked):** [`GOAL.md`](GOAL.md) — R-IDs are the contract.
- **Authoritative design:** [`PLAN.md`](PLAN.md) (section numbers below are PLAN's).
- **Backing research:** [`research/00-digest.md`](research/00-digest.md) + briefs 01–04.

## Frontmatter field reference

- `status` (top): `planned | in_progress | blocked | in_review | done` (`done` is stamped by
  `xdu-publish` after confirmation, just before landing — the terminal state of the retained record)
- `appetite`: `small | big` — caps phase count and build-iteration budget (circuit breaker).
- phase `status`: `pending | in_progress | done | blocked`
- `satisfies`: GOAL R-IDs this phase delivers (traceability anchor for `xdu-review`).
- `depends_on`: phase ids that must be `done` first (a phase is actionable only when met).
- `parallel`: `true` only for genuinely independent, non-coupled work. Every phase here touches
  `src/cli.rs`, `src/lib.rs` or a file a later phase edits, so all are `false`.
- `hammerable`: `false` marks a correctness/security phase that scope-hammering must **never**
  cut. P5 is the GOAL's designated first cut, but that cut goes through a GOAL amendment (below),
  never through hammering.
- `hill`: `uphill` → `crest` → `downhill`.
- `attempts`: durable failed-verify counter, bumped by `set_phase.py --phase P<n> --record-attempt`.
- `verify`: the exact command that proves the phase. Every gate names its tests and greps
  `test NAME ... ok` from a log, so a renamed or vanished test turns it red.
- `review.cycle`: completed review passes, auto-incremented by every `set_phase.py --verdict …`.

## Conventions (apply to every phase)

- Commit conventions, code style, and load-bearing invariants come from
  [`AGENTS.md`](../../AGENTS.md). Consult
  [`.agents/factory/invariants.md`](../../.agents/factory/invariants.md) §2, §2b, §4, §5, §8, §10,
  §11 and §13 for this change.
- One phase per `xdu-build` invocation; one atomic commit containing **both** the code and the
  `TECH.md` state change. Subjects `[feature] Build bulk-copy-move-xdu-cp-mv P<n>: …`.
- **No `Co-Authored-By` trailer.**
- A CLI change updates its `doc/*.scd` page **in the same commit** (§10). `scdoc` is not
  installed on every agent host; when it is absent, say so in the commit body, and read the
  page through `scdoc < f.scd | mandoc -Tutf8 | col -b` wherever it is.
- **Untouchable:** `src/bin/xdu-rm.rs`, `tests/rm_tests.rs`, `doc/xdu-rm.1.scd`, `XduRmArgs`,
  `XduFindArgs`, `parquet_source`, `deterministic_limit_clause`, `QueryFilters` and its
  `to_conditions`. `src/lib.rs` gains three `pub mod` lines and nothing else.
  `tests/common/mod.rs` gains arms and functions; no existing one changes.
- **Never**, in non-comment lines of `src/select.rs`, `src/atomic_fs.rs`, `src/relocate.rs` or
  the two new bins: `std::fs::rename`, `std::fs::copy`, `libc::rename`, `libc::renameat`,
  `File::create`, `O_TRUNC`, `.truncate(true)`, `fs::write` (every code phase's gate greps).
- No `spec/` R-/P-ids in source (§13). Test names describe behaviour.
- Index rows hold the canonical crawl root: tests pass `--root` as `source.canonicalize()`
  (macOS `TempDir` lives under `/var` → `/private/var`). Index fixtures use `--apparent-size`
  where a size is asserted exactly. A test that asserts a carried gid first sets the source's
  gid to `getgid()` (a temp dir's group can be one the caller is not in).
- **Calendar checkpoint (GOAL Notes, the recorded cut).** If, by end of **2026-11-10**, P5's
  local gate and CI's `XFS tests` step on both OSes are not green, stop and raise the cut to the
  foreman. It is applied only through a GOAL amendment; then P5 is replaced by **P5-cut** (below).

---

## Phase P1 — Selection engine and xdu-cp's atomic, no-replace copy
**Satisfies:** R1, R3, R4, R6, R8, R9 · **Depends on:** — · **Size:** L, ~1.5 days
**Goal:** `xdu-cp` exists end to end. It selects through a bound, path-ordered statement,
refuses everything R3 names before any write, previews with a zero-write dry-run, confirms,
and copies each regular file atomically into the `--root` layout without ever replacing a name.
Metadata carriage beyond bytes, the S1 torn-copy check and `--safe` arrive in P2.

- [x] `src/cli.rs`: `SelectArgs` (`#[derive(Args)]`, PLAN §2.7, every flag of R8 in `xdu-find`'s
  order and wording, then `xdu-rm`'s `-l -n --safe -f -v -j`); `XduCpArgs` with
  `#[command(flatten)] select`, `--into DIR` (required), `--root DIR` (default `/`), `name`,
  `version`, `about`, `after_help` examples. `--safe` is accepted here and wired in P2: until P2,
  passing it refuses with a clear error rather than being silently ignored.
- [x] `src/select.rs` (PLAN §2.2): `Row` (with the indexed `size`), `Selection`, `refuse_s3`,
  `build_filters`, `checked_glob` (metacharacter, `/`, `.`, `..`, empty and missing-partition
  refusals), `selection_sql` (pure; `read_parquet(?)`, `regexp_matches(path, ?)`, the other
  predicates from `to_conditions` on a clone with the pattern cleared, the order tail written
  once), `select()`.
- [x] `src/atomic_fs.rs` (PLAN §2.5): `DirHandle`; destination walk-and-create and source walk,
  both `O_NOFOLLOW` per component with a per-worker parent cache; `open_source` (`fstatat`
  NOFOLLOW first, open only `S_ISREG`, then `O_NOFOLLOW|O_NONBLOCK` open and a (dev, ino)
  match); `rename_noreplace` with the full fail-closed chain (raw `SYS_renameat2` on Linux,
  `renameatx_np` on macOS, the `linkat` route with its source re-check and NFS-retransmit inode
  check); `sync_durable`; `copy_atomic` steps 1–3, 5, 7, 8 (existing pre-check, `.xdu-*.part`
  temp, bytes, flush, publish, directory flush). The crate-private `Faults`/`StepHook` seam.
- [x] `src/relocate.rs` (PLAN §2.3, §2.4, §2.6): `RootMap`, `check_whole_set`, `confirm` (prompt
  on stderr), the chunked ordered executor on a local pool, `Outcome`, `Summary`, `run_cp`.
- [x] `src/bin/xdu-cp.rs`: thin; S3 refusal first; exit status from the summary rule.
- [x] `Cargo.toml` `[[bin]] xdu-cp`; `src/bin/gen-completions.rs` entry; `tests/common/mod.rs`:
  `binary_path` arm, `run_cp`, `run_with_stdin`.
- [x] `doc/xdu-cp.1.scd`: every flag, the mapping (its own worked example), the R3 refusals, the
  glob-metacharacter refusal, exit status, `.xdu-*.part` leftovers. AGENTS.md's scdoc escaping
  rules apply; no line starts with `.` or `'`.
- [x] Unit tests named in the `verify:` (the `linkat` route forced through `Faults`; abandon at
  every `copy_atomic` step); `tests/cp_tests.rs` with the named tests (fixtures as `rm_tests.rs`).
- **Verify:** see frontmatter (named lib and `cp_tests` tests; grep bans; a `temp_index` drive:
  copy one partition, a re-run exits non-zero on existing files, dry-run lists all four;
  `rm_tests` and the untouched-file diff; clippy; fmt).
- **Build notes (2026-11-06).** Gate green locally on macOS (APFS) on its second run; the first
  went red on a clippy `wrong_self_convention` in a `cp_tests` helper (recorded as attempt 1).
  Deviations from PLAN, none touching a GOAL R-ID: `selection_sql` takes the glob as its first
  argument, so the pure-function test pins the bound glob too; the `linkat` route withdraws its
  new link only while that inode keeps another name (never the last name of a file), and also
  withdraws it when the source `unlinkat` fails, so a failed publish leaves the prior state;
  a failed directory flush after publish withdraws the final name, so a failed copy leaves
  nothing at it; a symlinked source *parent* counts as skipped (not a regular file at the
  path), not missing; `relocate` imports `copy_atomic` unqualified because the gate's
  `fs::copy` ban also matches `atomic_fs::copy_atomic`. `--safe` refuses until P2, as planned.
  `scdoc` is not installed on the build host, so `doc/xdu-cp.1.scd` was not rendered locally.
- **CI must show (not provable on this host):** the Linux leg compiles and passes the
  `SYS_renameat2` arm of `atomic_fs::native_rename_noreplace` and every named `atomic_fs` and
  `cp_tests` test (only `aarch64-apple-darwin` is installed here, so the Linux `cfg` arms were
  never compiled locally), with Linux clippy and `RUSTFLAGS=-D warnings` clean; and the man-page
  render step renders `doc/xdu-cp.1.scd` (it globs `doc/*.scd`; literal counts arrive in P4).
- **Touches:** `src/cli.rs` (high-blast, additive), `src/lib.rs` (high-blast, three lines), new
  `src/select.rs`, `src/atomic_fs.rs`, `src/relocate.rs`, `src/bin/xdu-cp.rs`,
  `src/bin/gen-completions.rs`, `Cargo.toml`, `tests/common/mod.rs`, `tests/cp_tests.rs`,
  `doc/xdu-cp.1.scd`.

## Phase P2 — xdu-cp carries metadata, re-checks with --safe and counts every outcome
**Satisfies:** R1, R2, R7, R9 · **Depends on:** P1 · **Size:** M, ~0.5 day
**Goal:** a copy carries the source's bytes (never torn), permission bits, atime and mtime, and
owner and group where permitted (a refusal counted, never silent); `--safe` re-checks every given
filter against the live object; the summary counts every outcome separately.

- [x] `copy_atomic` step 4 (PLAN §2.5): `fchown` from S0 (any error a refusal; group-only
  retry; set-id bits dropped when an id was not carried), `fchmod(mode & 0o7777)`, `futimens`
  with nanoseconds, in that order. Step 6: the S1 check for both tools.
- [x] `select.rs`: `Live`, `Verdict`, `holds_live` (PLAN §2.2) against the stored thresholds;
  size by the measure that matches the row's indexed size, both when neither does. A DuckDB
  boundary-grid test evaluates `to_where_clause()` over one-row `VALUES` and asserts the same
  boolean as `holds_live` at `t-1`, `t`, `t+1` for every clock and every mode operator.
- [x] Wire `--safe` in `relocate` on the `fstat` of the opened source; remove P1's refusal.
- [x] `Summary` and `-v` listing complete (R9 taxonomy); failures always on stderr.
- [x] `doc/xdu-cp.1.scd`: `--safe` semantics (every filter; the size rule, naming the "indexed
  size"; the regular-file rule), ownership best-effort, the R10 sentences ("The index is not
  updated." and "Reading a source may update its atime …", each kept on one source line).
- [x] Tests named in the `verify:`. `cp_carries_owner_and_group_when_permitted` sets the source's
  gid to `getgid()` and chowns to the caller's own ids (the only success path CI can take
  without root); the refusal paths go through the `Faults` seam.
- **Verify:** see frontmatter.
- **Build notes (2026-11-06).** Deviations from PLAN, none touching a GOAL R-ID:
  the `--safe` size rule follows the foreman's plan-gate ruling (GOAL Clarifications), not
  PLAN §2.2's per-row match: `holds_live` takes the size measure the index records
  (`Selection.size_mode`) and tests that live measure, and when the index records none, which
  is every index today (the marker has no size key), a size filter must hold for both the
  length and `blocks*512`. `Live` therefore does not carry the row's indexed size, and an
  unchanged small file can fail a tight `--max-size` under `--safe`; `xdu-cp(1)` says so.
  `Step` gains `Owned`, `Moded` and `Timed` (after each metadata call), so the abandon and
  fail sweeps cover them and the order test observes the temp between calls. `Placed` gains
  `Done { owner_kept }` and `SourceChanged`, counted as skipped ("changed during the copy").
  The summary keeps one `Skipped` count, as R9 lists it; `-v` names each reason, and a copy
  whose ownership was refused lists as `copied (ownership not preserved): …`. `Faults` gains
  `refuse_owner` and `refuse_group`. `--owner`'s live check is proven only by the unit
  boundary grid: changing a file's owner needs root, so `cp_safe_skips_a_file_that_no_longer_matches_each_filter`
  covers every other filter, and its `--group` case runs only where the caller has two groups.
  This host's sandbox strips set-id bits on `chmod`, so the set-id assertions in
  `a_refused_chown_is_counted_and_drops_the_set_id_bits`, `metadata_is_set_owner_then_mode_then_times`
  and `cp_carries_owner_and_group_when_permitted` hold trivially here (each prints a note) and
  bite on the CI legs. `scdoc` is not installed on the build host, so `doc/xdu-cp.1.scd` was
  not rendered locally.
- **Touches:** `src/select.rs`, `src/atomic_fs.rs`, `src/relocate.rs`, `tests/cp_tests.rs`,
  `doc/xdu-cp.1.scd`.

## Phase P3 — xdu-mv relocates on one filesystem with a no-replace rename
**Satisfies:** R3, R4, R5 (same filesystem), R6, R7, R8, R9 · **Depends on:** P2 · **Size:** M, ~0.5 day
**Goal:** `xdu-mv` exists end to end on one filesystem, sharing every gate with `xdu-cp`. A
cross-device rename is reported per file and the source kept; that is the shipped behaviour if
the cut applies.

- [x] `src/cli.rs`: `XduMvArgs` (flatten `SelectArgs`, `--into`, `--root`, `version`).
- [x] `atomic_fs`: `move_same_fs` (PLAN §2.5): **always** `fstatat(src_parent, name, NOFOLLOW)`
  and skip anything but `S_ISREG`; `holds_live` on it under `--safe`; the existing-destination
  pre-check; `rename_noreplace`.
- [x] `relocate`: `run_mv` with a `CrossFs` policy enum, `Refuse | Copy`. This phase ships
  `Refuse`: `CrossDevice` fails the file with "SRC: crossing filesystems is not supported by this
  build" on stderr, the source kept. `Copy` arrives in P5; `Refuse` stays as the cut's behaviour
  and is unit-tested either way.
- [x] `src/bin/xdu-mv.rs`, `Cargo.toml` `[[bin]]`, `gen-completions`, `tests/common` arm and
  `run_mv`.
- [x] `doc/xdu-mv.1.scd` (same coverage as `xdu-cp.1`, the R10 sentences, its own worked mapping
  example with paths distinct from `xdu-cp.1`'s), including the swap note for the
  same-filesystem rename.
- [x] Tests named in the `verify:` (directory and FIFO cases run **without** `--safe`).
- **Verify:** see frontmatter (named lib and `mv_tests` tests; `cp_tests` still green; grep bans;
  a `temp_index` drive: dry-run moves nothing, the real run moves `bob`).
- **Build notes (2026-11-06).** Red first: `move_same_fs` was written without its `S_ISREG`
  guard and the tests run against it: `a_same_filesystem_move_skips_a_non_regular_object`,
  `mv_skips_a_directory_at_the_source_path`, `mv_skips_a_fifo_at_the_source_path`,
  `mv_skips_a_symlink_at_the_source_path` and `mv_summary_counts_and_exit_status` failed (the
  directory and the FIFO were renamed into `--into`), then the guard went in. Deviations from
  PLAN, none touching a GOAL R-ID: `CrossFs` has the one variant `Refuse` until P5 adds `Copy`,
  with `CROSS_FS` naming the shipped policy and `CROSSING_REFUSED` the message;
  `move_same_fs` takes the destination directory as a closure and the `--safe` check as an
  `admit` closure, so `atomic_fs` stays free of `select` and no destination directory is
  created for a file that is skipped (one refused as crossing filesystems does leave its
  directories, which `xdu-mv(1)` says); `Step` gains `Cleared` (after the existing-destination
  pre-check, before the rename) so a test plants a name in that window and proves the rename
  itself refuses on both routes; `Faults` gains `force_cross_device`; a rename's `ENOENT` counts
  as missing only when the source name is gone. `run_cp` and `run_mv` share one private `run`
  (`Action`, `Verbs`); `xdu-cp`'s output is unchanged and `cp_tests` passes unedited. The
  inode carries owner, group, mode and times, so a move never counts ownership not preserved.
  `mv_safe_skips_a_changed_file_and_leaves_it` covers `--min-size`, `--older-than`,
  `--mtime-older-than` and `--mode`; the other filters share `holds_live` with `xdu-cp`, whose
  P2 tests and boundary grid cover them. `scdoc` is not installed on the build host, so
  `doc/xdu-mv.1.scd` was not rendered locally.
- **CI must show (not provable on this host):** the Linux leg compiles and passes
  `move_same_fs` through the `SYS_renameat2` arm and every named `atomic_fs` and `mv_tests`
  test, with Linux clippy clean; the man-page render step renders `doc/xdu-mv.1.scd` (it globs
  `doc/*.scd`; literal counts arrive in P4).
- **Touches:** `src/cli.rs` (high-blast, additive), `src/atomic_fs.rs`, `src/relocate.rs`,
  `src/bin/xdu-mv.rs`, `src/bin/gen-completions.rs`, `Cargo.toml`, `tests/common/mod.rs`,
  `tests/mv_tests.rs`, `doc/xdu-mv.1.scd`.

## Phase P4 — Man pages, README and packaging in lockstep
**Satisfies:** R10 · **Depends on:** P3 · **Size:** M, ~0.5 day
**Goal:** both tools ship everywhere the four existing binaries ship, documented, with their man
pages under the counted literal gate. With P1–P4 done the cycle is publishable even if P5 is cut.

- [x] `doc/xdu-cp.1.scd`, `doc/xdu-mv.1.scd`: final R10 pass; SEE ALSO; `XDU_INDEX` and
  `XDU_JOBS` each named twice (flag and ENVIRONMENT), as `xdu-rm.1` does. Cross-references in
  `doc/xdu.1.scd` (SEE ALSO, the marker-warning list, the lossy-path note), `doc/xdu-find.1.scd`
  and `doc/xdu-view.1.scd` SEE ALSO. **Not** `doc/xdu-rm.1.scd` (GOAL non-goal): add a line to
  `issues/xdu-rm-on-shared-engine.md` that its migration also adds `xdu-cp(1)`/`xdu-mv(1)` to
  `xdu-rm.1`'s SEE ALSO.
- [x] `.github/workflows/test.yaml`: `check share/man/man1/xdu-cp.1 '2x:XDU_INDEX' '2x:XDU_JOBS'
  '1x:s3://BUCKET/PREFIX' '<N>x:<its worked destination path>'` and the same for `xdu-mv.1`
  with its own path (free of `_` and `*`; counts derived from an ubuntu-24.04 render and
  re-counted by hand); +2 man, +2 bash, +2 zsh entries in the produced list; "all six" in its OK
  line. The same-commit three-place obligation (§13): `AGENTS.md` Commands and `invariants.md`
  §13 name the new literal class.
- [x] `release.yaml` lines 168, 208, 289 and `packaging/extensions/check.sh` lines 49, 100, 108:
  `… xdu-rm xdu-cp xdu-mv; do`; optional `xdu-cp --dry-run` cold smoke beside `check.sh:122`.
  `Dockerfile` `--bin` and `cp` lists and its "four" comment **in the same commit as `check.sh`**
  (both run it).
- [x] `install.sh` `BINARIES`; `packaging/rpm/xdu.spec` (`--bin`, `install`, `%files`).
- [x] `hpccm/xdu.py`: replace the named `--version` smoke with a loop over `{bindir}/xdu*`;
  regenerate `xdu.def` and `xdu.docker` with `make -B` (hpccm is not on the planning host; if it
  is absent on the build host, say so, hand-edit the specs to the recipe's exact output, and
  rely on CI's `make check`); `hpccm/README.md` binary list.
- [x] `tests/cli_version_tests.rs` array.
- [x] `README.md`: one `xdu-cp -i …` and one `xdu-mv -i …` example after "Bulk Deletion".
- [x] `AGENTS.md` (binary counts, `#[command]` count, pages count, repo map, data flow, CLI
  surface, tarball layout, §5's engine statement) and `invariants.md` (§5 engine binding, §13
  counts and tarball, the high-blast list gains `src/select.rs`, `src/atomic_fs.rs`,
  `src/relocate.rs`, `src/bin/xdu-mv.rs`). `issues/manpage-gate-coverage-gaps.md` and its
  ROADMAP paragraph: six pages, two hand-added this cycle.
- [x] File `issues/www-cp-mv-at-release.md` (from `templates/ISSUE.md`) with a `ROADMAP.md` entry:
  at the release that ships the tools, move them from the site's roadmap (`www/index.html`,
  † v0.8) to its tools table, and correct "one shared engine behind `rm`, `cp` and `mv`" (rm is
  not migrated). `www/` deploys on push to `main`, so it is not edited in this cycle.
- **Verify:** see frontmatter (the R10 sentences and roff check on both pages; README; every
  packaging list by exact count; generated completions; `cli_version_tests`; issue files;
  `doc/xdu-rm.1.scd` and `www/` untouched; the whole suite; clippy; fmt). CI's packaging job is
  the render, literal-count and `make check` proof.
- **Build notes (2026-11-06).** Red first: the stored gate, run before any edit, stopped at
  `grep -qF "xdu-cp -i" README.md`. After the work it ran green as stored; mutating the
  `check.sh` engine loop (dropping `xdu-mv`) and the `cli_version_tests` array each turned it red,
  and both files were restored by copy and `cmp`. `scdoc` is not installed on the build host, so
  the counts in the new `check` lines were derived by an emulation of the CI step (scdoc's `*`,
  boundary-`_` and backslash rules, then the whitespace strip) that reproduces all 54 existing
  `check` specs on the edited pages with no mismatch; it gives `XDU_INDEX` 2, `XDU_JOBS` 2,
  `s3://BUCKET/PREFIX` 1 and the worked path 1 on each new page. hpccm 26.5.0 regenerated
  `xdu.def`/`xdu.docker`, and `make check` is green with the specs staged. The `check.sh` tool
  smoke, with the new `xdu-cp --dry-run` lines, was run against `target/release` under a cold
  HOME and matched the two `.log` files with nothing written under `--into`; the glob smoke loop
  was checked to fail on a failing binary and on an empty `bin/`. Deviations, none touching a GOAL
  R-ID: the worked-path literal is counted once per page (`/cold/alice/logs/app.log`,
  `/archive/carol/runs/r1.h5`), and the pages' mirrored paths are left unasserted; `xdu-find.1`'s
  "refuses S3 indices" sentence now names `xdu-cp`(1) and `xdu-mv`(1) too; `xdu-cp.1` gains
  `xdu-mv`(1) in SEE ALSO and one over-long source line is rewrapped; the README's move example
  stays on one filesystem (an attic under `/home`), so it holds whether or not P5 lands, and points
  at `xdu-mv`(1) for other filesystems; `AGENTS.md`'s high-risk quick reference gains the engine
  files beside `invariants.md`'s high-blast list; the `release.yaml` floor comment, `docker.yaml`
  and the rpm spec's comment count six binaries. Not touched: `build.sh` (its output list already
  omits `xdu-rm`, pre-existing) and `.agents/skills/xdu-release/SKILL.md:96`, whose "all four
  `#[command(...)]` blocks" is now stale but is a skill's text (META F3, for `/xdu-harness`).
- **CI must show (not provable on this host):** the man-page render and the new `check` lines for
  `xdu-cp.1` and `xdu-mv.1` on ubuntu-24.04's scdoc, with the "all six" produced-files step; the
  Linux `cli_version_tests`; `make check` in the packaging job; the Docker build (`docker.yaml`'s
  path filter takes `Dockerfile` and `packaging/extensions/**`), which runs the edited `check.sh`
  on the six-binary tree. The release legs (`release.yaml`'s floor loop and both assembly loops)
  run only at a tag; `check.sh`'s presence loop is their backstop there.
- **Touches:** `doc/*.scd` (not `xdu-rm.1.scd`), `README.md`, `.github/workflows/test.yaml`,
  `.github/workflows/release.yaml`, `packaging/extensions/check.sh`, `Dockerfile`, `install.sh`,
  `packaging/rpm/xdu.spec`, `hpccm/` (recipe, specs, README), `tests/cli_version_tests.rs`,
  `AGENTS.md`, `.agents/factory/invariants.md`, `issues/`, `ROADMAP.md`.

## Phase P5 — xdu-mv crosses filesystems: durable copy, unchanged check, then unlink
**Satisfies:** R5 (cross filesystem) · **Depends on:** P4 · **Size:** L, ~1.0 day
**Goal:** a cross-device move copies as `xdu-cp` does, unlinks the source only after the copy
and its directory are durable and the source is unchanged, and leaves every file whole
somewhere at every step. Proven by fault-seam unit tests on every OS and by a real second
filesystem in CI. This is the last phase and the GOAL's first cut.

- [x] `atomic_fs::move_across` (PLAN §2.5): S0 at open; the source-parent `faccessat` pre-check;
  `copy_atomic` steps 1–6 (S1 included); publish; directory flush; S2 check (dev, ino, type,
  size, mtime, ctime) immediately before `unlinkat(src_parent, name)`; inode-checked rollback of
  our destination on a failed flush, an S2 mismatch or a failed unlink (counted failed or
  skipped as PLAN says); best-effort source-directory flush. Never writes the source's times.
- [x] `relocate`: the binary's policy becomes `CrossFs::Copy`; `CrossDevice` routes to
  `move_across`. `Refuse` stays and keeps its unit test.
- [x] Unit tests named in the `verify:`: a `StepHook` that fails at each `Step`, one that
  abandons at each `Step` without cleanup (simulated crash; then assert source whole or final
  name whole, never a partial final name), a recording hook asserting no unlink precedes both
  flushes, mutation cases (append; same-size rewrite with the mtime restored, which only ctime
  catches; a swap of the source name before the unlink), a forced unlink failure, and a
  `relocate` test that drives the **production** policy through a forced `EXDEV`.
- [x] `tests/mv_xfs_tests.rs`: at least three `#[ignore = "needs XDU_TEST_XFS_DIR on a second
  filesystem"]` tests (a move across carrying bytes, mode and times; an existing destination
  skipped and counted; the source gone and the destination's `st_dev` different). The shared
  fixture **panics** with "XDU_TEST_XFS_DIR is not set" when the variable is unset, and with
  "XDU_TEST_XFS_DIR is not on another filesystem" when a probe rename from a `TempDir` into it
  does not return `EXDEV`, so the tier can never pass vacuously.
- [x] `tests/xfs-wire.sh PREFIX MIN`, modelled on `tests/s3-wire.sh`: runs `mv_xfs_tests` with
  `--include-ignored` and fails unless the passed count equals the listed count and is at least
  MIN.
- [x] `.github/workflows/test.yaml`, test matrix, before `Test`: the Linux step mkdirs
  `/dev/shm/xdu-xfs`, asserts its `stat -c %d` differs from `$GITHUB_WORKSPACE`'s and exports
  `XDU_TEST_XFS_DIR`; the macOS step creates and attaches an APFS image with `hdiutil create -ov
  -size 256m -fs APFS` in a three-try retry loop, asserts the device differs and exports the
  mount. Then an `XFS tests` step: `sh tests/xfs-wire.sh "" N`, N the file's listed count. No
  step runs anything under `spec/`.
- [x] `AGENTS.md` Testing: the `XDU_TEST_XFS_DIR` tier and how to run it on a host with a second
  filesystem.
- [x] `doc/xdu-mv.1.scd`: the cross-filesystem sequence ("another filesystem"), the flush cost,
  the remaining races (PLAN §2.5), destinations without a no-replace rename or hard links. Do not
  repeat the page's worked destination path or add an `XDU_*` mention: P4's counted literals
  must stay true; if they change, re-count and update `test.yaml` in the same commit.
- [x] File `issues/xdu-mv-quarantine-unlink.md` (from `templates/ISSUE.md`) with a `ROADMAP.md`
  entry: the quarantine rename that would close the final-name swap race (PLAN §5).
- [x] **Deferral ledger (this is the last phase):** walk P1–P5's bodies and commit messages for
  "do not fix here", "known limitation", "follow-up", "deferred", "later"; each must have an
  `issues/` file and a `ROADMAP.md` entry. Expected set: `xdu-mv-quarantine-unlink` (P5),
  `www-cp-mv-at-release` (P4), the `xdu-rm.1` SEE ALSO line in
  `issues/xdu-rm-on-shared-engine.md` (P4). `issues/s3-index-rm.md` and
  `issues/local-reader-bound-parameters.md` are widened and narrowed at publish (GOAL), not here.
- **Verify:** see frontmatter. The local gate proves the seam layer and the production routing,
  that the tier exists with at least three tests, that `xfs-wire.sh` fails for the right reason
  with no second filesystem and on the same filesystem, and that CI wires both OS steps. **The
  real-`EXDEV` run is proven only by CI's `XFS tests` step**; the sandbox on the planning host
  refused every disk-image route (research 02). That CI result is part of the checkpoint.
- **Build notes (2026-11-06).** Red first: the stored gate, run before any edit, stopped at
  `atomic_fs::tests::exdev_routes_to_the_copy_path did not pass`. After the work it ran green as
  stored. Three mutants, each built from a fresh-mtime copy of the tree in its own
  `CARGO_TARGET_DIR` under `target/`, each turned it red: dropping the S2 re-check failed the
  grew, mtime-restored and swapped tests; shipping `CrossFs::Refuse` failed
  `the_production_policy_routes_exdev_to_the_copy_path`; a no-op withdrawal failed those three
  plus `move_across_rolls_back_when_the_source_unlink_fails` and
  `move_across_fails_closed_at_every_step`. The real tree was checked unmutated afterwards. The
  tier's bodies were also run once locally against a same-filesystem directory with the probe
  disabled (a throwaway copy, moved aside): the two content tests passed and the device test
  failed, as it must. Deviations from PLAN, none touching a GOAL R-ID: a source that vanished
  after its copy was published (at S2 or at the `unlinkat`) keeps its copy, the only whole
  instance left, and counts as failed, since withdrawing it would leave the file at neither name;
  a failure after the unlink (the best-effort source-directory flush, or a hook failing there)
  cannot undo the move, which counts as moved; `move_across` re-runs `--safe` on the `fstat` of
  the opened source; a source replaced between its type check and its open counts as skipped
  ("changed while being opened"); `Faults` gains `refuse_source_unlink`, and `move_across`
  clears `force_cross_device` for its own publish, so a forced `EXDEV` stands for the rename
  that sent the file there; `Step` gains `SourceRechecked` and `SourceUnlinked`; the mutation
  tests mutate at `DirSynced` (after the publish, before S2), the window S2 closes, while the
  swap between S2 and the unlink is the race the man page lists and the new issue covers; the
  macOS step retries `hdiutil attach` as well as `create`; the `XFS tests` step sits right after
  `Test`; `xfs-wire.sh` does not check the variable itself, so a failure is the fixture's own;
  the tier asserts no owner or group (user-attached macOS images ignore ownership), which
  `move_across_carries_metadata` proves on every host; a move to another filesystem can count
  "Ownership not preserved", and `xdu-mv(1)` now says so. `AGENTS.md`'s tool table, repository
  map and high-risk paragraph describe the cross-filesystem path. Deferral ledger: no new hit in
  P1–P5's bodies or commit messages; the expected set is filed (`xdu-mv-quarantine-unlink`,
  `www-cp-mv-at-release`, the `xdu-rm.1` SEE ALSO line). The P4 literal counts were re-run by
  the P4 emulation (62 specs, 0 mismatches). `scdoc` is not installed on the build host, so
  `doc/xdu-mv.1.scd` was not rendered locally. Nothing ran on a real second filesystem here: the
  sandbox refuses `hdiutil`.
- **CI must show (not provable on this host):** on both `Test` legs, the step "Second filesystem
  for the cross-filesystem move tests (Linux)" or "(macOS)" green, then the `XFS tests` step
  ending in `xfs-wire: '' 3 of 3 passed with XDU_TEST_XFS_DIR=/dev/shm/xdu-xfs` (ubuntu-24.04)
  and `xfs-wire: '' 3 of 3 passed with XDU_TEST_XFS_DIR=…/xdu-xfs` under `$RUNNER_TEMP`
  (macos-15), each preceded by `test result: ok. 3 passed; 0 failed`; the Linux leg compiling
  `move_across` (`AT_EACCESS`, `faccessat`) and passing every named `atomic_fs` and `relocate`
  test with clippy clean; and the man-page step's `xdu-mv.1` literal counts unchanged.
- **Touches:** `src/atomic_fs.rs`, `src/relocate.rs`, `tests/mv_xfs_tests.rs`,
  `tests/xfs-wire.sh`, `.github/workflows/test.yaml`, `AGENTS.md`, `doc/xdu-mv.1.scd`,
  `issues/xdu-mv-quarantine-unlink.md`, `ROADMAP.md`.

### P5-cut — applies only after a GOAL amendment (the recorded first cut)
**Satisfies:** R5 as amended · **Size:** S, ~0.25 day. The foreman amends `GOAL.md` (R5's
cross-filesystem path becomes a refusal with a clear error, and the path is seeded), then
rewrites P5's frontmatter to this phase with `set_phase.py`. Work: keep P3's per-file
`CrossFs::Refuse` (no `st_dev` pre-flight: it misfires on overlayfs and misses bind mounts,
research 02); state the refusal in `xdu-mv(1)`; file `issues/xdu-mv-cross-filesystem.md` (from
`templates/ISSUE.md`, carrying PLAN §2.5's `move_across` design, research 02's CI plan and the
quarantine hardening) with a `ROADMAP.md` entry; run the deferral ledger above with
`xdu-mv-cross-filesystem` in place of `xdu-mv-quarantine-unlink`. Verify: the named
`relocate::tests::a_cross_device_move_under_the_refuse_policy_keeps_the_source`, `grep -qiF
"crossing filesystems" doc/xdu-mv.1.scd`, `test -f issues/xdu-mv-cross-filesystem.md`, its
ROADMAP grep, and the `rm_tests`/diff/clippy/fmt tail.

---

## Remediation — review cycle 1 (foreman rulings, 2026-11-07)

[`REVIEW.md`](REVIEW.md) cycle 1 returned changes-requested with four CONFIRMED findings and
triggered the human gate (findings in `src/select.rs` and `src/atomic_fs.rs`, and invariant §4).
The foreman signed the gate off and ruled on each fix's shape. This is the remediation plan; each
finding reopens the phase whose `satisfies` covers it, and each fix carries a test that was red
before it and green after. No `GOAL.md` R-ID changes. META F5 (three stale copies of the
high-blast list) stays open for `/xdu-harness` and is not touched here.

| Finding | Ruling | Phase |
|---------|--------|-------|
| 1 (HIGH) §4 covers only `xdu-rm` | Widen `invariants.md` §4 to the gates `AGENTS.md` invariant 4 now names for `xdu-cp` and `xdu-mv`, worded parallel to the `xdu-rm` items | P4 |
| 2 (MEDIUM) `--safe` size on an index of unknown size mode | No index-format change (§1 untouched). When the index does not record its size mode and a size filter is given, `--safe` also requires the live mtime to equal the indexed mtime; a file changed since the crawl fails closed as changed. The both-measures check stays. Both man pages state exactly this | P2 |
| 3 (MEDIUM) Hive `key=value` names rewrite columns | Turn DuckDB's Hive detection off in the select-act reader only; correct `AGENTS.md`'s false sentence; seed `xdu-find`/`xdu-rm`'s share (high priority), touching neither them nor `view_query.rs` | P1 |
| 4 (MEDIUM) search-only ancestor fails every file | Walk intermediate source and destination components without read permission (`O_SEARCH` on macOS, `O_PATH` on Linux). §2 does not weaken: a directory that is flushed is still opened so that the flush works, and where that open is refused the file fails closed with a clear message, stated in the man pages | P2 |

P2 takes two findings in two commits; it is `done` only after the second.

### P1 — finding 3: Hive partitioning off in the selection reader
- [x] `src/select.rs`: the selection reads `read_parquet(?, hive_partitioning = false)`
  (`SELECTION_SOURCE`); the SQL-text tests pin it.
- [x] `cp_selects_the_true_rows_under_a_hive_style_partition` (`-u size=0` with `--max-size 1K`
  selects nothing, `--min-size 1M` the 5 MB file),
  `cp_selects_the_true_rows_from_an_index_under_a_hive_style_directory` (an index under
  `mtime=0/` selects nothing by `--mtime-older-than 3650`), and
  `mv_moves_only_the_true_rows_under_a_hive_style_partition`. All three were red with the
  option removed and green with it.
- [x] Sweep: `AGENTS.md`'s project paragraph claimed DuckDB "derives no partition column" from
  the layout; it now says bare names are a convention, names the engine as covered and the other
  readers as not. No other live site made the claim; `www/` calls the layout "Hive-partitioned",
  a description rather than a reader claim, and is frozen on this branch (P4's diff gate).
- [x] Seed `issues/hive-partition-columns-find-rm.md` (high priority: destructive in `xdu-rm`;
  names `view_query.rs` for the same pass) with its `ROADMAP.md` entry.
- **Verify:** the P1 gate plus the three tests, the predicate that every non-comment
  `read_parquet(` in the engine's files carries `hive_partitioning = false`, the false phrase
  absent from `AGENTS.md` and `invariants.md`, and the seed and ROADMAP pair.
- **Build notes (2026-11-07).** The retuned gate went red once on a clippy
  `cloned_ref_to_slice_refs` in the new `cp_tests` case (attempt 2), then green. The predicate
  was seen red with the option removed. The same sweep updated `AGENTS.md`'s module map and
  invariant 5 text and `invariants.md` §5, which quoted the statement as `read_parquet(?)`.

### P2 — finding 2: `--safe` size with an unrecorded size mode
- [x] `select::holds_live` takes the row's indexed mtime. With no recorded size mode and a
  `--min-size` or `--max-size` filter, a live mtime other than the indexed one fails as
  `changed since the index was built` (`CHANGED_SINCE_INDEXED`), after the both-measures size
  checks, so a file that already fails a size filter still reports that filter.
- [x] `relocate::Planned` carries the row's indexed mtime to both workers.
- [x] `doc/xdu-cp.1.scd` and `doc/xdu-mv.1.scd` SAFE MODE state the two rules, the probe's case,
  what each skips that is unchanged (a small file in a large block; a touched file), and that a
  change inside the recorded second leaves only the first rule.
- [x] Tests: `select::tests::an_unstated_size_measure_also_requires_the_indexed_mtime`, and the
  review's probe as `cp_safe_skips_a_file_that_grew_past_the_bound_by_the_index_block_size` and
  `mv_safe_skips_a_file_that_grew_past_the_bound_by_the_index_block_size` (index with
  `--block-size 1M`; a fresh index confirms the file no longer matches by that measure). All
  three were red with the rule disabled and green with it. `tests/common` gains
  `build_index_block_size` and `append_bytes`.

### P2 — finding 4: search-only ancestors
- [x] `atomic_fs`: `DirHandle` records whether it was opened for reading (`Access::Search` or
  `Access::Read`). Walks open every intermediate component search-only (`O_SEARCH` on macOS,
  `O_PATH` on Linux, read elsewhere) and the last one with the caller's `Access`; a walk with no
  component reopens its base as `.`. `DirHandle::sync` and `require_flushable` refuse a
  search-only handle on every platform, so macOS tests fail as Linux's `fsync` on an `O_PATH`
  descriptor would; `copy_placed` checks before its temp file and `move_across` before copying.
- [x] `xdu-cp` walks its source search-only to the end (it never flushes a source directory);
  `xdu-mv` opens the source directory for reading (it flushes it after a move across
  filesystems); both open each destination directory for reading (flushed after the publish).
  The `/` and `--into` anchors are opened search-only. A refused read open fails that file with
  `cannot open directory NAME for reading (…), which flushing it to stable storage needs`.
- [x] Man pages: a PERMISSIONS section in each (search on every directory passed; read on the
  directories flushed; the failure). `AGENTS.md` records the rule and the test that holds it.
- [x] Tests: `atomic_fs::tests::a_walk_passes_a_search_only_ancestor`,
  `atomic_fs::tests::a_directory_that_must_be_flushed_is_opened_for_reading_or_fails_closed`,
  `cp_copies_through_search_only_ancestors` (0111 source and destination ancestors, a 0311
  source directory), `cp_fails_closed_on_a_destination_directory_it_cannot_read`,
  `mv_moves_through_search_only_ancestors`,
  `mv_fails_closed_on_a_source_directory_it_cannot_read`. The four integration tests were red
  with `atomic_fs.rs` and `relocate.rs` at the F2 commit and green after; the unit tests need
  the new API. Each skips cleanly as root. `a_symlinked_source_parent_is_not_followed` now
  walks both ways.
- **Verify:** P2's gate retuned to every test above, the predicate that no directory is flushed
  except through `DirHandle::sync` (every other `sync_durable` call flushes a file), the man-page
  literals of both findings, and a `temp_index` drive copying `-u alice` through a 0111 `alice`.
- **Build notes (2026-11-07).** The retuned gate went red once (attempt 1): its man-page literal
  `changed since the index was built` straddles a line break in the source, so the gate now
  greps `skipped as changed since the index`. Then green. The flush predicate was seen red with
  one `dest.sync()` put back as `sync_durable(dest.as_fd())`. Not root here, so no permission
  case skipped. `scdoc` is not installed, so neither page was rendered locally; CI renders.
- **CI must show (not provable on this host):** the Linux leg compiling the `O_PATH` arm and
  passing every test above, in particular `a_symlinked_source_parent_is_not_followed` (an
  `O_PATH|O_NOFOLLOW|O_DIRECTORY` open of a symlink must fail `ENOTDIR`) and the flush of every
  destination directory (a search-only handle there would fail with `EBADF` only on Linux).

### P4 — finding 1: invariant §4 covers the new destructive gates
- [x] `.agents/factory/invariants.md` §4 is headed for all three tools and gains three items
  parallel to the `xdu-rm` ones: the prompt (`relocate::confirm`), the zero-write and
  zero-removal dry run, `--force`, the empty set and the whole-set refusals before any write;
  `--safe` over every filter (`select::holds_live`), the regular-file rule, and the size rule of
  finding 2; and `ORDER BY path` always, `--limit` through `deterministic_limit_clause`. It names
  `xdu-mv` destructive at the source and the files that hold the gates. The `xdu-rm` items are
  unchanged. `AGENTS.md` invariant 4 already named these gates and is unchanged; the three stale
  high-blast copies stay with META F5.
- **Verify:** P4's gate plus a predicate over the extracted §4 text: every gate and file named
  above, the unchanged `xdu-rm` gap sentence, and at least six items. It is red on the cycle-1
  §4 (no `xdu-mv`, no `relocate::confirm`) and green now.

### Remediation gates and notes for the foreman (2026-11-07)

- **Gates, local (macOS, APFS, not root):** P1, P2 and P4 gates green. The P4 gate's full
  `cargo test --locked`: lib 213, `cp_tests` 32, `mv_tests` 19, `rm_tests` 19, crawl 25,
  find 9, s3 non-wire 4, every other suite green; ignored 53 (50 S3 wire, 3 `mv_xfs_tests`),
  as designed. `cargo clippy --all-targets --all-features -D warnings` and `cargo fmt --check`
  clean.
- **Not run locally:** the S3 wire tier (no pinned `httpfs` here) and the cross-filesystem tier
  (the sandbox refuses `hdiutil`); CI covers both. `scdoc` is not installed, so neither man page
  was rendered or its literals counted here; the CI packaging step renders both and asserts the
  literals, which this remediation did not change. The Linux `O_PATH` arm was not compiled here.
- **Found, not fixed (outside the four rulings):**
  - `walk_create_nofollow` never flushes the parent of a directory it creates. A copy published
    into a new destination directory is flushed with that directory, but the new directory's own
    entry in its parent is not, so after a power loss the copy can be unreachable. For a move
    across filesystems the source is already removed by then, which strains "never at neither
    name". Pre-existing in cycle 1, not introduced here; a fix flushes each parent after
    `mkdirat` (needing it opened for reading too).
  - With every anchor now search-only, an `--into` its user cannot read no longer refuses the
    run up front; each file placed directly in it fails with the PERMISSIONS message instead,
    while files placed in its subdirectories succeed.
  - `www/index.html` calls the layout "Hive-partitioned" (a description; `www/` is frozen here),
    and `view_query.rs`'s `partitions_sql` comment says Hive detection yields no column; both
    are named in the new seed.
  - `AGENTS.md` invariant 4 is still titled "`xdu-rm` destructive safety", though its body
    covers `xdu-cp` and `xdu-mv`; `invariants.md` §4's title now names all three.

---

## Calendar and fit

| Day | Work |
|-----|------|
| 2026-11-06 | Plan (this) |
| 11-07 – 11-08 | P1 (1.5 d), P2 starts |
| 11-09 | P2, P3 |
| 11-10 | P4, P5 (**checkpoint end of day: P5's gate and CI's `XFS tests` green, else the cut**) |
| 11-11 | CI, blind review |
| 11-12 | One remediation |
| 11-13 | Publish |

Estimate 4.0 build-days (P1 1.5, P2 0.5, P3 0.5, P4 0.5, P5 1.0) against about 3. P1–P4 (3.0 d)
fit; P5 does not, with margin. **The cut is the expected path**: plan on P5-cut unless P1–P4
land early. The checkpoint keys on CI, so the foreman must start a CI run on P5's branch head on
11-10.

## How `xdu-build` drives this

1. `next_phase.py` prints the next actionable phase (statuses are authoritative; the
   `current_phase` pointer is reconciled against them).
2. Pre-flight: clean tree, on `branch`, `base` reachable.
3. Execute every `[ ]` in the phase (consult `PLAN.md` / `research/` for detail).
4. Run the phase's `verify:` command — never advance on a checkbox alone.
5. Amend this file freely if reality diverges (regenerate frontmatter with `set_phase.py`; note the
   amendment in the commit body). STOP and escalate only on a **`GOAL.md` contradiction**.
6. Mark the phase `done`, advance `current_phase`, `--touch`; one `[feature]` commit; stop and
   report.
