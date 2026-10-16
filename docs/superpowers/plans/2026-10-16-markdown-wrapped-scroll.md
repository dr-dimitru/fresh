# Markdown wrapped scrolling implementation plan

**Goal:** Integrate upstream master without losing the fork's scope, highlighting, and gutter fixes, then remove measured costs in small wrapped Markdown documents.

**Architecture:** Keep upstream's Rust rendering pipeline and TypeScript Markdown plugins. Preserve exact line labels and wrap geometry; do not use a zero large-file threshold as the fix.

**Spec:** User's four-step request in this session, with 100+ line files, wrapping, line numbers, and the threshold-zero comparison. Work on a branch in the existing checkout; no worktrees.

## Tasks

- [x] Review fork commits and fetch upstream (`b4fde85c6`).
- [x] Merge upstream on `perf/markdown-wrapped-scroll`. Preserve bounded large-file parsing alongside upstream's incremental ordinary-file parsing. Adapt Explorer scope to the upstream-owned UI window rather than restore removed viewport APIs. Regenerate plugin types.
- [x] Build and run merge regression coverage: highlighting, wrapping, Markdown, Explorer scope, hidden-number gutter, automatic line scan.
- [x] Reproduce normal versus threshold-zero scrolling with small wrapped fixtures. The lookahead regression failed with 7,796 bytes parsed instead of 4,500; the decoration-snapshot regression failed with 20 resolutions instead of 1. Both pass after their fixes.
- [x] Implement the smallest measured fixes; verify line numbers on/off, wrap on/off, edits and resize, Unicode, and EOF reachability. The eight-case screen matrix passes. A further screen regression reproduced pending wheel frames overriding Home/End and now passes after finishing the preceding wheel gesture before the key.
- [x] Run broader tests, formatting, type checks, optimized build and smoke checks. Review final diff and record limitations. The full suite is not green; see the verification record below.

## Execution record

- Initial baseline: `ae223fddb`, clean master in the mistakenly used `~/Sites/fresh-extra-corrupt` checkout, 14 fork commits; upstream had 279 additional commits. Nine files conflicted in the merge preview. The work was subsequently transferred to the intended `~/Sites/fresh-extra` repository; see below.
- Rust 1.95 exists under `~/.rustup/toolchains/1.95-aarch64-apple-darwin/bin`; it is not on the shell PATH.
- Ruling: follow the user's explicit execution steps inline; no new feature design is needed before the requested merge. Performance implementation is contingent on reproduction, not a speculative rewrite.
- Ruling: use upstream's existing `file_explorer::window_rows` to publish rendered Explorer scope. Upstream removed the model-owned viewport; restoring it would create two geometry owners and break scoped Git refreshes. Publish after `paint_shell`, not just layout, because the helper reads the painted display list.
- Merge commit: `d9d0e29ba`. Both upstream master (`b4fde85c6`) and the explicitly refreshed origin master (`b688023a8`) are ancestors. Local master is unchanged.
- Before the performance changes: 201 Markdown integration tests, 65 highlighter tests, 20 large-file tests, five Explorer scope tests, the scoped-repository integration test, and 21 Bun Git-scope tests passed.
- Profiling: a 150-line Unicode/inline-markup fixture with the full grammar registry, optimized profiling build. A macOS `sample` of scrolling attributes work to conceal/activation scans and repeated `EditorState::index_decorations`. The first diagnostic mistakenly used the harness's default empty grammar registry; its wall times are not used as evidence for syntax performance.
- The retained manual benchmark now disables smooth scrolling to avoid measuring queued animation frames as completed scroll distance. It explicitly enables line numbers after entering Compose when requested. Initial diagnostic timings with smooth scrolling are not presented as a fixed-distance speedup.
- Ruling: finish queued wheel distance before an ordinary key, then release the viewport hold. Bare modifier keys leave the animation alone. This preserves requested wheel distance but applies the outstanding distance immediately when a key takes over.
- Ruling: retain the fork's existing highest-priority directory status letters. Three inherited Explorer tests were waiting forever for the old dot glyph; update those exact expectations to M/U rather than change the fork's behavior.
- Verification limitation found: the generic Node plugin runner cannot execute the Bun-only Git-scope test; run that file with Bun. The upstream picker test additionally uses a panel mock without `setSelectedIndex` and fails even with TypeScript transformation enabled.
- Final review: self-review (no subagent tool). Checked cache invalidation, partial highlight tails, geometry/LRU ownership, cursor-aware decorations, hidden-number gutters, scope publication timing, and wheel/key ordering. Remaining conceal-manager scans are a future performance target, not claimed fixed here.
- Review fix: reusing lookahead must not cause a large speculative parse when the viewport reaches the cache edge. Added a failing byte-budget regression, then limited forward-extension lookahead to 1 KiB beyond the viewport. A second failing regression caught a UTF-8 cut at that new limit; the limit now snaps to a character boundary. All 68 highlighter tests pass.

## Original verification record

These checks ran before the checkout correction. Commands use the installed Rust 1.95 toolchain on PATH. Verification repeated in the intended repository is recorded below.

- `cargo build --release -p fresh-editor --bin fresh`: passed. Runnable CLI: `target/release/fresh`. Existing macOS `gpm_client` unused-variable warning remains.
- Real release-binary smoke test in an isolated tmux server and temporary HOME/project: opened 150 Unicode Markdown lines with wrapping and line numbers, navigated to EOF, resized, returned home, and inserted text. All five screen checks passed; the temporary server was stopped.
- `cargo test -p fresh-editor-core --lib`: 1,336 passed, 1 ignored.
- `cargo test -p fresh-editor --lib wrap_index`: 29 passed. The new decoration-snapshot work regression also passed after failing 20 versus 1 before the fix.
- Integration filters: `markdown` 203 passed / 9 ignored; `bare_modifier` 2 passed; `wheel` 69 passed; `gutter` 50 passed / 1 ignored. Filters overlap; these counts must not be summed as unique tests.
- Merge-focused tests: five scope API tests, one scoped-Git integration test, 20 large-file tests, and all three corrected directory-badge tests passed.
- `bun test crates/fresh-editor/plugins/tests/git_explorer_scope.test.ts`: 21 passed.
- `crates/fresh-editor/plugins/check-types.sh git_explorer.ts markdown_compose.ts lib/git_explorer_scope.ts`: passed.
- `cargo fmt --all -- --check`, `git diff --check`: passed.
- `cargo clippy --all-targets`: exit 0, with existing workspace/fork warnings. Not a warning-free result.
- Optimized manual benchmark: passed for 150-line Markdown, source/Compose, numbers on/off, normal/zero threshold. Warm passes parsed zero additional highlight bytes. Timings are not advertised as a percentage speedup: initial and final diagnostic workloads differed in animation settling, and compilation/test processes ran concurrently.

### Full-suite limitation

`cargo test --no-fail-fast -- --test-threads=8` was attempted, not skipped. Completed targets included 286 core API tests, 2,643 editor unit tests (2 ignored), and 73 main-binary tests. The integration target reported `e2e::live_grep::test_live_grep_uses_working_dir` as FAILED. The external runner stopped the still-running suite after 600 seconds, so there is no whole-suite pass claim.

That Live Grep test also exceeded a separate 20-second external limit when run alone. Other tests that had waited over 60 seconds in the parallel run passed when run separately: `test_live_grep_toolbar_is_on_the_prompts_ring`, `test_open_terminal_in_dock_has_only_terminal_tab`, `test_open_terminal_in_dock_spans_full_width_with_existing_vsplit`, `test_resume_live_grep_routes_through_plugin`, and the English-default, invalid-locale, and Chinese-buffer-name locale tests. `the_caret_is_never_lost_while_arrowing_through_a_composed_document` was slow in the broad run but passed in the focused Markdown run.

The generic Node plugin test runner also remains non-green: `git_explorer_scope.test.ts` requires Bun, and `pickers.test.ts` uses a panel mock missing `setSelectedIndex`. No claim is made that those runner issues are fixed.

## Transfer to the intended repository

The merge and all eleven changed/new files were transferred to `~/Sites/fresh-extra` on branch `perf/markdown-wrapped-scroll`. The merge commit was preserved, and every transferred file was compared byte-for-byte before committing. The ten code/test files still match the original verified changes exactly; this record was updated to explain the correction.

The changes are split into focused commits, keeping regression tests with their fixes:

- `d9d0e29ba`: preserved upstream merge and fork compatibility changes.
- `a20ae0e63`: cached Markdown highlighting and bounded, UTF-8-safe lookahead.
- `da9f08540`: reuse unchanged wrap decoration snapshots, with the manual benchmark.
- `731754078`: finish pending wheel motion before keyboard input, with screen regressions.
- `84326a301`: align directory-badge tests with the fork's inherited status letters.

Verification repeated from `~/Sites/fresh-extra`:

- 68 highlighter tests passed.
- The decoration-snapshot regression and 29 wrap-index tests passed.
- Both Markdown scroll integration tests passed, including the eight-case settings matrix.
- All three corrected directory-badge tests passed.
- `cargo fmt --all -- --check` and `git diff --check` passed.
- `cargo clippy --all-targets` exited 0 with the existing warnings.
- `cargo build --release -p fresh-editor --bin fresh` passed in the intended checkout.
- The rebuilt release binary passed all five real-terminal smoke checks: open, EOF navigation, resize, home navigation, and editing.
- `~/.local/bin/fresh` now resolves to `~/Sites/fresh-extra/target/release/fresh`; launching it with `--version` reports `fresh 0.5.3`.

The full suite was not rerun for this byte-identical transfer; its earlier failure and timeout remain documented above. No worktrees were created and nothing was pushed. The intended repository's existing branches were preserved: `master` remains at `b688023a8`, and `drdimitru/file-explorer-visible-scope` remains at `ae223fddb`. The original checkout was retained as a backup.
