# Git Explorer visible-scope implementation plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Keep Git Explorer badges enabled while limiting Git status work to workspace and visible or expanded nested repositories.

**Architecture:** Fresh core publishes a deduplicated, read-only File Explorer scope snapshot through the plugin state snapshot and a `file_explorer_scope_changed` hook. Git Explorer replaces recursive repository discovery with a single-flight, generation-aware cache and queue fed only by that scope.

**Tech Stack:** Rust, Fresh core/plugin runtime, QuickJS TypeScript plugins, Bun unit tests, Rust E2E harness.

**Spec:** `docs/superpowers/specs/2026-09-22-git-explorer-visible-scope-design.md`

## Global Constraints

- Keep `git_explorer_local` enabled; do not disable it during implementation or verification.
- Preserve file badges, parent-directory highest-priority badges, colors, and optional filename coloring.
- No recursive repository discovery, polling, synchronous Git calls, or Git work during rendering.
- `FileExplorerScope` paths are absolute, normalized, deduplicated, and derived without filesystem traversal.
- Explorer scope events occur only after meaningful Explorer state changes, never for an unchanged render or decoration update.
- Queue at most one Git status refresh at a time.
- Cap one scope candidate set and the repository cache at 128 entries.
- Preserve `/opt/homebrew/bin/fresh`; install builds only to `/Users/drdimitru/.local/bin/fresh`.

## Review Focus

- Compact-directory rows: test that ancestor candidates include an unrendered nested Git root.
- Sticky Explorer rows after scrolling: test that `visiblePaths` contains pinned ancestors and ordinary rendered rows exactly once.
- Hidden Explorer: test that scope is `null`, queue work stops, and cached badges are retained without a new Git call.
- A nested repository removed while cached: test that a later visible `.git` check evicts its cache and removes its decorations.
- Out-of-order Git responses: test that an older status response cannot replace a newer cached result or requeue a collapsed repository.

---

## Preconditions

The current `/tmp/fresh-source` directory is a partial source snapshot without Git metadata or a workspace `Cargo.toml`; it cannot build or commit this work. Before Task 1, recover a complete checkout of `https://github.com/sinelaw/fresh` at the local build revision `af8df8c90`, preserve the existing modified-source files and approved design/plan documents, and create an isolated worktree from that checkout. Do not overwrite either installed Fresh executable during checkout recovery.

### Task 1: Add bounded File Explorer scope projection

**Files:**
- Modify: `crates/fresh-editor/src/view/file_tree/view.rs`
- Test: `crates/fresh-editor/src/view/file_tree/view.rs`

**Interfaces:**
- Produces: `FileTreeView::plugin_scope_paths(&self) -> FileExplorerScopePaths`.
- Produces: `FileExplorerScopePaths { visible_paths: Vec<PathBuf>, expanded_directories: Vec<PathBuf> }`.
- Consumed by: Task 2 snapshot population.

- [ ] **Step 1: Write failing projection tests in `view.rs`**

```rust
async fn populated_scrolled_tree_view() -> (tempfile::TempDir, FileTreeView) {
    let (temp, mut tree) = create_test_tree().await;
    tree.expand_node(tree.root_id()).await.unwrap();
    let dir1 = tree.get_node(tree.root_id()).unwrap().children[0];
    tree.expand_node(dir1).await.unwrap();
    let mut view = FileTreeView::new(tree);
    view.set_viewport_height(3);
    view.set_scroll_offset_for_test(2);
    (temp, view)
}

#[tokio::test]
async fn plugin_scope_paths_uses_rendered_rows_and_expanded_directories() {
    let (temp, mut tree) = create_test_tree().await;
    tree.expand_node(tree.root_id()).await.unwrap();
    let mut view = FileTreeView::new(tree);
    view.set_viewport_height(2);

    let scope = view.plugin_scope_paths();

    assert_eq!(scope.visible_paths.len(), 2);
    assert_eq!(scope.visible_paths[0], temp.path());
    assert_eq!(scope.expanded_directories, vec![temp.path().to_path_buf()]);
}

#[tokio::test]
async fn plugin_scope_paths_includes_sticky_ancestor_once_after_scroll() {
    let (_temp, view) = populated_scrolled_tree_view().await;
    let scope = view.plugin_scope_paths();

    assert_eq!(scope.visible_paths.len(), 3);
    assert_eq!(scope.visible_paths.iter().collect::<std::collections::HashSet<_>>().len(), 3);
    assert!(scope.visible_paths.iter().any(|path| path.ends_with("dir1")));
}
```

- [ ] **Step 2: Run projection tests and verify failure**

Run:

```bash
cargo test -p fresh --lib file_tree::view::tests::plugin_scope_paths -- --nocapture
```

Expected: compile failure because `FileTreeView::plugin_scope_paths` and the test helper do not exist.

- [ ] **Step 3: Add the minimal projection type and method**

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FileExplorerScopePaths {
    pub visible_paths: Vec<PathBuf>,
    pub expanded_directories: Vec<PathBuf>,
}

impl FileTreeView {
    pub(crate) fn plugin_scope_paths(&self) -> FileExplorerScopePaths {
        let display = self.get_display_nodes();
        let visible_paths = self
            .viewport_display_indices()
            .into_iter()
            .filter_map(|index| display.get(index).map(|(id, _)| *id))
            .filter_map(|id| self.tree.get_node(id).map(|node| node.entry.path.clone()))
            .collect();
        let expanded_directories = self
            .tree
            .all_nodes()
            .filter(|node| node.is_dir() && node.is_expanded())
            .map(|node| node.entry.path.clone())
            .collect();
        FileExplorerScopePaths { visible_paths, expanded_directories }
    }
}
```

Use the existing `viewport_display_indices()` and `get_display_nodes()` paths so sticky rows, filtering, and compact-directory behavior exactly match rendering. Sort and deduplicate both vectors before returning. Add a test-only scroll setter only behind `#[cfg(test)]`; production scroll mutation remains private.

- [ ] **Step 4: Run projection tests and the complete FileTreeView test module**

Run:

```bash
cargo test -p fresh --lib file_tree::view::tests -- --nocapture
```

Expected: PASS. Verify existing compact-chain and viewport tests remain green.

- [ ] **Step 5: Commit the projection**

```bash
git add crates/fresh-editor/src/view/file_tree/view.rs
git commit -m "feat: project visible file explorer scope"
```

### Task 2: Expose Explorer scope through the plugin state snapshot

**Files:**
- Modify: `crates/fresh-core/src/api.rs`
- Modify: `crates/fresh-core/src/hooks.rs`
- Modify: `crates/fresh-editor/src/app/window/mod.rs`
- Modify: `crates/fresh-editor/src/app/plugin_dispatch.rs`
- Modify: `crates/fresh-editor/src/services/plugins/event_hooks.rs`
- Modify: `crates/fresh-plugin-runtime/src/backend/quickjs_backend.rs`
- Modify: `crates/fresh-editor/plugins/lib/fresh.d.ts`
- Modify: `crates/fresh-editor/types/fresh.d.ts.template`
- Modify: `docs/plugins/development/index.md`
- Test: `crates/fresh-core/src/hooks.rs`
- Test: `crates/fresh-plugin-runtime/src/backend/quickjs_backend.rs`
- Test: `crates/fresh-editor/tests/e2e/plugins/file_explorer_slots.rs`

**Interfaces:**
- Consumes: `FileTreeView::plugin_scope_paths()` from Task 1.
- Produces: `fresh_core::api::FileExplorerScope`.
- Produces: `EditorStateSnapshot::file_explorer_scope: Option<FileExplorerScope>`.
- Produces: `editor.getFileExplorerScope(): FileExplorerScope | null`.
- Produces: hook name `file_explorer_scope_changed` with `{}` payload.
- Consumed by: Task 3 lifecycle notification and Task 4 Git Explorer plugin.

- [ ] **Step 1: Write failing core, runtime, and hook tests**

```rust
#[test]
fn file_explorer_scope_hook_serializes_as_empty_object() {
    let json = hook_args_to_json(&HookArgs::FileExplorerScopeChanged {}).unwrap();
    assert_eq!(json, serde_json::json!({}));
}

#[test]
fn get_file_explorer_scope_returns_snapshot_or_null() {
    let (mut backend, _rx) = create_test_backend();
    backend.state_snapshot.write().unwrap().file_explorer_scope = Some(FileExplorerScope {
        root: "/workspace".into(),
        revision: 7,
        visible_paths: vec!["/workspace/src".into()],
        expanded_directories: vec!["/workspace".into()],
    });
    backend.execute_js(r#"
        const scope = getEditor().getFileExplorerScope();
        globalThis._scope = JSON.stringify(scope);
    "#, "scope-test.ts").unwrap();
    backend.plugin_contexts.borrow().get("scope-test").unwrap().clone().with(|ctx| {
        let scope: String = ctx.globals().get("_scope").unwrap();
        assert_eq!(scope, r#"{"root":"/workspace","revision":7,"visiblePaths":["/workspace/src"],"expandedDirectories":["/workspace"]}"#);
    });
}
```

Add an E2E plugin fixture that subscribes to `file_explorer_scope_changed`, reads `getFileExplorerScope()`, and writes the revision and paths into a test-observable status token. Assert scope reads as `null` while Explorer is hidden.

- [ ] **Step 2: Run tests and verify failure**

Run:

```bash
cargo test -p fresh-core file_explorer_scope_hook -- --nocapture
cargo test -p fresh-plugin-runtime get_file_explorer_scope_returns_snapshot_or_null -- --nocapture
cargo test -p fresh --test all_tests file_explorer_scope_api -- --nocapture
```

Expected: compile failures for `FileExplorerScope`, `FileExplorerScopeChanged`, and `getFileExplorerScope`.

- [ ] **Step 3: Define the wire type and snapshot field**

```rust
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, rename_all = "camelCase")]
pub struct FileExplorerScope {
    pub root: String,
    pub revision: u64,
    pub visible_paths: Vec<String>,
    pub expanded_directories: Vec<String>,
}
```

Add `file_explorer_scope: Option<FileExplorerScope>` to `EditorStateSnapshot`, initialize it to `None`, and add `HookArgs::FileExplorerScopeChanged {}`. Update the event-hook registration allowlist and TypeScript `HookEventMap`.

- [ ] **Step 4: Populate and read the snapshot synchronously**

Add `file_explorer_scope_revision: u64` to `Window`, initialized to `0`, and `Window::current_file_explorer_scope(&self) -> Option<FileExplorerScope>`. It converts Task 1 paths to normalized strings, includes the current revision, and returns `None` if the Explorer is hidden or absent. During the existing `Window::populate_plugin_state_snapshot` call, write this value into `EditorStateSnapshot::file_explorer_scope`; do not send a plugin command or run a hook from the snapshot population path. Task 3 adds the fingerprint, increments this revision, and emits the hook after scope mutation.

Add the QuickJS API method using the existing snapshot-reader pattern:

```rust
#[plugin_api(ts_return = "FileExplorerScope | null")]
pub fn get_file_explorer_scope<'js>(
    &self,
    ctx: rquickjs::Ctx<'js>,
) -> rquickjs::Result<rquickjs::Value<'js>> {
    let scope = self.state_snapshot.read().ok().and_then(|state| {
        state.file_explorer_scope.clone()
    });
    rquickjs_serde::to_value(ctx, scope)
        .map_err(|error| rquickjs::Error::new_from_js_message(
            "serialize", "", &error.to_string(),
        ))
}
```

Add generated declaration coverage so the public API is exactly:

```ts
type FileExplorerScope = {
  root: string;
  revision: number;
  visiblePaths: string[];
  expandedDirectories: string[];
};

interface EditorAPI {
  getFileExplorerScope(): FileExplorerScope | null;
}
```

Document that the getter returns a snapshot and does not cause filesystem or Git work.

- [ ] **Step 5: Run focused API tests and declaration checks**

Run:

```bash
cargo test -p fresh-core file_explorer_scope_hook -- --nocapture
cargo test -p fresh-plugin-runtime get_file_explorer_scope_returns_snapshot_or_null -- --nocapture
cargo test -p fresh --test all_tests file_explorer_scope_api -- --nocapture
/Users/drdimitru/.local/bin/fresh --cmd=script --cmd=check
```

Expected: Rust tests PASS. Record existing unrelated script-check API diagnostics separately; the new getter and hook must not add diagnostics.

- [ ] **Step 6: Commit the plugin API**

```bash
git add crates/fresh-core/src/api.rs crates/fresh-core/src/hooks.rs \
  crates/fresh-editor/src/app/window/mod.rs \
  crates/fresh-editor/src/app/plugin_dispatch.rs \
  crates/fresh-editor/src/services/plugins/event_hooks.rs \
  crates/fresh-plugin-runtime/src/backend/quickjs_backend.rs \
  crates/fresh-editor/plugins/lib/fresh.d.ts \
  crates/fresh-editor/types/fresh.d.ts.template \
  crates/fresh-editor/tests/e2e/plugins/file_explorer_slots.rs \
  docs/plugins/development/index.md
git commit -m "feat: expose file explorer scope to plugins"
```

### Task 3: Emit scope changes from Explorer lifecycle actions

**Files:**
- Modify: `crates/fresh-editor/src/app/file_explorer.rs`
- Modify: `crates/fresh-editor/src/app/window/mod.rs`
- Modify: `crates/fresh-editor/src/view/ui/file_explorer.rs`
- Modify: `crates/fresh-editor/src/view/shell/file_explorer.rs`
- Test: `crates/fresh-editor/tests/e2e/file_explorer.rs`
- Test: `crates/fresh-editor/tests/e2e/plugins/file_explorer_slots.rs`

**Interfaces:**
- Consumes: snapshot scope helper and `file_explorer_scope_changed` from Task 2.
- Produces: one hook invocation per changed scope revision.
- Consumed by: Task 4 Git Explorer scheduler.

- [ ] **Step 1: Write failing lifecycle tests**

```rust
#[test]
fn explorer_scope_hook_fires_once_for_expand_and_once_for_collapse() {
    let mut harness = explorer_harness_with_scope_probe();
    harness.expand_explorer_path("packages");
    harness.wait_for_scope_revision(1);
    harness.collapse_explorer_path("packages");
    harness.wait_for_scope_revision(2);
    assert_eq!(harness.scope_hook_count(), 2);
}

#[test]
fn explorer_scope_hook_does_not_fire_for_repeated_render_or_decorations() {
    let mut harness = explorer_harness_with_scope_probe();
    harness.render().unwrap();
    harness.render().unwrap();
    harness.set_test_file_explorer_decoration("/workspace/src/lib.rs");
    assert_eq!(harness.scope_hook_count(), 0);
}
```

Add cases for Explorer hide/show, filtered rows, and scrolling into a sticky ancestor. Verify scope revision only advances when the returned snapshot differs.

- [ ] **Step 2: Run lifecycle tests and verify failure**

Run:

```bash
cargo test -p fresh --test all_tests file_explorer_scope_hook -- --nocapture
```

Expected: tests fail because no scope hook is emitted after expansion, collapse, visibility, filter, or scroll changes.

- [ ] **Step 3: Add a deduplicated notifier outside rendering**

Add an optional scope fingerprint to `Window`. Implement `Window::refresh_file_explorer_scope_state(&mut self) -> bool`: derive the current scope with Task 2's getter, compare the root, visible paths, expanded directories, and visibility against the previous fingerprint, increment `file_explorer_scope_revision` only on change, and return whether it changed.

Implement the notifier on `Editor`, which owns both the active Window and PluginManager:

```rust
fn refresh_file_explorer_scope_and_notify_plugins(&mut self) {
    let changed = self.active_window_mut().refresh_file_explorer_scope_state();
    if !changed {
        return;
    }
    self.update_plugin_state_snapshot();
    self.plugin_manager.read().unwrap().run_hook(
        "file_explorer_scope_changed",
        HookArgs::FileExplorerScopeChanged {},
    );
}
```

Call the Editor notifier after the Explorer has completed expand/collapse, tree reload, filtering, root replacement, visibility changes, and scroll-offset changes. Update Explorer viewport height from the layout path before invoking the notifier; do not invoke the notifier from render functions. Keep `setFileExplorerDecorations` and `setFileExplorerSlots` outside this path.

- [ ] **Step 4: Run lifecycle and regression suites**

Run:

```bash
cargo test -p fresh --test all_tests file_explorer -- --nocapture
cargo test -p fresh --test all_tests file_explorer_scope_hook -- --nocapture
cargo test -p fresh --test all_tests file_explorer_slots -- --nocapture
```

Expected: PASS. Verify rendering twice with no input does not advance scope revision.

- [ ] **Step 5: Commit Explorer lifecycle delivery**

```bash
git add crates/fresh-editor/src/app/file_explorer.rs \
  crates/fresh-editor/src/app/window/mod.rs \
  crates/fresh-editor/src/view/ui/file_explorer.rs \
  crates/fresh-editor/src/view/shell/file_explorer.rs \
  crates/fresh-editor/tests/e2e/file_explorer.rs \
  crates/fresh-editor/tests/e2e/plugins/file_explorer_slots.rs
git commit -m "feat: notify plugins of explorer scope changes"
```

### Task 4: Replace recursive Git Explorer discovery with a bounded cache

**Files:**
- Create: `crates/fresh-editor/plugins/lib/git_explorer_scope.ts`
- Create: `crates/fresh-editor/plugins/git_explorer_scope.test.ts`
- Modify: `crates/fresh-editor/plugins/git_explorer.ts`
- Modify: `/Users/drdimitru/.config/fresh/plugins/git_explorer_local.ts`
- Create: `/Users/drdimitru/.config/fresh/plugins/git_explorer_local_scope.ts`
- Delete: `/Users/drdimitru/.config/fresh/plugins/git_explorer_local_lib.ts`
- Test: `crates/fresh-editor/plugins/git_explorer_scope.test.ts`
- Test: `/tmp/test_git_explorer_parent_badge.ts`

**Interfaces:**
- Consumes: `editor.getFileExplorerScope()` and `file_explorer_scope_changed` from Tasks 2–3.
- Produces: `GitExplorerScopeCache.refresh(scope: FileExplorerScope | null): Promise<void>`.
- Produces: `GitExplorerScopeCache.invalidatePath(path: string): void`.
- Produces: `GitExplorerScopeCache.decorations(): Decoration[]`.
- Moves and preserves: `parseStatusOutput`, `buildAncestorDecorations`, marker priority, and color-slot inputs.

- [ ] **Step 1: Write failing Bun tests for scope candidates, queueing, caching, and parent badges**

```ts
import { expect, test } from "bun:test";
import { GitExplorerScopeCache, type GitExplorerPort } from "./lib/git_explorer_scope.ts";

class FakeGit implements GitExplorerPort {
  readonly statusCwds: string[] = [];
  constructor(private readonly statuses: Record<string, string>) {}
  pathDirname(path: string) { return path.slice(0, path.lastIndexOf("/")) || "/"; }
  pathJoin(...parts: string[]) { return parts.join("/").replace(/\/+/g, "/"); }
  fileExists(path: string) {
    return path.endsWith("/.git") && Object.hasOwn(this.statuses, this.pathDirname(path));
  }
  async resolveRepository(path: string) { return Object.hasOwn(this.statuses, path) ? path : null; }
  async readStatus(repoRoot: string) {
    this.statusCwds.push(repoRoot);
    return this.statuses[repoRoot] ?? null;
  }
}

test("queues workspace and visible nested repo but not hidden sibling", async () => {
  const git = new FakeGit({
    "/workspace": "",
    "/workspace/packages/visible": " M src/edited.ts\0",
  });
  const cache = new GitExplorerScopeCache(git);
  await cache.refresh({
    root: "/workspace", revision: 1,
    visiblePaths: ["/workspace", "/workspace/packages/visible"],
    expandedDirectories: ["/workspace", "/workspace/packages"],
  });
  expect(git.statusCwds).toEqual(["/workspace", "/workspace/packages/visible"]);
  expect(git.statusCwds).not.toContain("/workspace/packages/hidden");
});

test("unchanged scope makes no additional Git call", async () => {
  const git = new FakeGit({ "/workspace": "" });
  const cache = new GitExplorerScopeCache(git);
  const scope = {
    root: "/workspace", revision: 1,
    visiblePaths: ["/workspace"], expandedDirectories: ["/workspace"],
  };
  await cache.refresh(scope);
  await cache.refresh(scope);
  expect(git.statusCwds).toEqual(["/workspace"]);
});

test("cached conflict bubbles to every parent directory", async () => {
  const git = new FakeGit({ "/workspace/packages/visible": "UU src/conflicted.ts\0" });
  const cache = new GitExplorerScopeCache(git);
  await cache.refresh({
    root: "/workspace", revision: 1,
    visiblePaths: ["/workspace/packages/visible"],
    expandedDirectories: ["/workspace/packages"],
  });
  expect(cache.decorations()).toContainEqual(
    expect.objectContaining({ path: "/workspace/packages", symbol: "!" }),
  );
});
```

Add `compact_row_probes_its_unrendered_repository_ancestor` with visible path `/workspace/pkg/src/index.ts`, expanded path `/workspace/pkg/src`, and only `/workspace/pkg/.git`; assert `/workspace/pkg` is the only nested status cwd.

Add `collapse_removes_queued_repository_before_status_starts` with a `ControlledGit` test port whose first `readStatus` promise stays unresolved; refresh scope containing `/workspace/slow`, refresh the collapsed scope, resolve the first promise, and assert `statusCwds` never gains `/workspace/slow`.

Add `collapsed_repository_retains_cached_parent_badge` by caching `UU src/conflicted.ts\0` for `/workspace/pkg`, refreshing a scope that no longer lists `/workspace/pkg`, and asserting `/workspace` and `/workspace/pkg` retain `!` badges without another status call.

Add `invalidate_path_refreshes_only_owning_visible_repository` with visible `/workspace/a` and `/workspace/b`; invalidate `/workspace/a/src/a.ts`; assert status cwd order is `[/workspace, /workspace/a, /workspace/b, /workspace/a]`.

Add `cache_evicts_least_recently_visible_root_after_128_entries` with 129 visible nested roots and empty statuses; re-show root 0 after the 129th scope; assert root 0 receives a second status call and every root except 0 receives one.

Add `stale_response_cannot_replace_newer_repository_decorations` with `ControlledGit` exposing two response resolvers for `/workspace/pkg`; resolve newer `UU src/conflicted.ts\0` before older ` M src/edited.ts\0`; assert cache decorations contain `!` for the parent and do not contain a newer-overwritten `M` entry for `edited.ts`.

Add `failed_status_retains_prior_decorations_and_first_failure_emits_none`: first cache ` M src/edited.ts\0`, then reject a refresh and assert the `M` decoration remains; in a separate empty cache reject the first refresh and assert `decorations()` is empty.

Add `missing_scope_api_performs_no_recursive_fallback` in the production-plugin mock: omit `getFileExplorerScope`, fire `editor_initialized` twice, assert no `git status` command was requested, and assert the mocked `console.warn` recorded the unsupported-API message once. `ControlledGit` is a local `FakeGit` subclass whose `readStatus` pushes `{ repoRoot, resolve, reject }` into a pending array and returns its promise; use those resolvers directly, never wall-clock sleeps.

- [ ] **Step 2: Run Bun tests and verify failure**

Run:

```bash
/Users/drdimitru/.bun/bin/bun test crates/fresh-editor/plugins/git_explorer_scope.test.ts
/Users/drdimitru/.bun/bin/bun /tmp/test_git_explorer_parent_badge.ts crates/fresh-editor/plugins/git_explorer.ts
```

Expected: first command fails because `GitExplorerScopeCache` does not exist; second preserves the established red/green parent-badge regression coverage once the test loader points at source dependencies.

- [ ] **Step 3: Implement pure scope candidate and cache logic**

Create `git_explorer_scope.ts` with explicit limits and no editor-global access:

```ts
export const MAX_SCOPE_CANDIDATES = 128;
export const MAX_CACHED_REPOSITORIES = 128;

export type FileExplorerScope = {
  root: string;
  revision: number;
  visiblePaths: string[];
  expandedDirectories: string[];
};

export type GitExplorerPort = {
  pathDirname(path: string): string;
  pathJoin(...parts: string[]): string;
  fileExists(path: string): boolean;
  resolveRepository(path: string): Promise<string | null>;
  readStatus(repoRoot: string): Promise<string | null>;
};
```

Move `parseStatusOutput` and `buildAncestorDecorations` into this module and export them, so the cache can store parsed decorations and unit tests use production porcelain parsing. Resolve `scope.root` once even if it has no `.git` entry, because a workspace may open below a Git root. Derive nested candidates from visible paths, their ancestors through `root`, and expanded directories. Deduplicate before the 128-entry cap. For nested candidates, use `fileExists(pathJoin(candidate, ".git"))` before `resolveRepository`; never call `discoverSubRepos`.

Use one pending queue and one running refresh. Store `{ generation, lastVisibleRevision, decorations }` by resolved Git root. Give visible candidates priority, drop queued candidates absent from the newest scope, and reject a response whose per-root generation is stale. Evict least-recently-visible cache entries above 128.

- [ ] **Step 4: Wire the production and durable local plugins**

In `plugins/git_explorer.ts`:

- import `parseStatusOutput` and `buildAncestorDecorations` from `lib/git_explorer_scope.ts`, retain name-color slot construction, and preserve all marker priorities and symbols;
- replace `refreshGitExplorerDecorations()` with scope-cache publishing;
- subscribe to `editor_initialized` and `file_explorer_scope_changed`;
- invalidate the matching cached repository on `after_file_save`, `after_file_revert`, and `after_file_explorer_change`;
- remove the `focus_gained` full refresh and all `discoverSubRepos` imports.

Port the same final source into `/Users/drdimitru/.config/fresh/plugins/git_explorer_local.ts`, retaining local namespace `git-explorer-local`. Copy the new pure helper to `git_explorer_local_scope.ts`, update its relative import, and remove the now-unused `git_explorer_local_lib.ts`. Do not change the enabled `git_explorer_local` config entry.

- [ ] **Step 5: Run unit tests and the existing parent-badge regression**

Run:

```bash
/Users/drdimitru/.bun/bin/bun test crates/fresh-editor/plugins/git_explorer_scope.test.ts
/Users/drdimitru/.bun/bin/bun /tmp/test_git_explorer_parent_badge.ts /Users/drdimitru/.config/fresh/plugins/git_explorer_local.ts
python3 -m json.tool /Users/drdimitru/.config/fresh/config.json >/dev/null
```

Expected: PASS. Confirm the local plugin remains enabled and the bundled `git_explorer` remains disabled.

- [ ] **Step 6: Commit source plugin changes**

```bash
git add crates/fresh-editor/plugins/git_explorer.ts \
  crates/fresh-editor/plugins/lib/git_explorer_scope.ts \
  crates/fresh-editor/plugins/git_explorer_scope.test.ts
git commit -m "perf: scope git explorer status refreshes"
```

### Task 5: Add multi-repository integration coverage and deliver locally

**Files:**
- Create: `crates/fresh-editor/tests/e2e/plugins/git_explorer_scope.rs`
- Modify: `crates/fresh-editor/tests/e2e/plugins/mod.rs`
- Modify: `crates/fresh-editor/tests/all_tests.rs`
- Modify: `/Users/drdimitru/.config/fresh/plugins/git_explorer_local.ts`
- Modify: `/Users/drdimitru/.config/fresh/plugins/git_explorer_local_scope.ts`
- Test: `crates/fresh-editor/tests/e2e/plugins/git_explorer_scope.rs`

**Interfaces:**
- Consumes: core scope API/hook and Git Explorer cache from Tasks 2–4.
- Produces: integration evidence that hidden repositories receive no Git status process and an expanded repository receives correct file and parent badge.

- [ ] **Step 1: Write failing nested-repository integration tests**

```rust
fn write_git_wrapper(bin_dir: &Path, log: &Path) {
    std::fs::write(
        bin_dir.join("git"),
        format!("#!/bin/sh\nprintf '%s\\t%s\\n' \"$PWD\" \"$*\" >> {}\nexec /usr/bin/git \"$@\"\n", log.display()),
    ).unwrap();
    std::fs::set_permissions(bin_dir.join("git"), std::fs::Permissions::from_mode(0o755)).unwrap();
}

#[test]
fn git_explorer_checks_only_visible_or_expanded_nested_repositories() {
    let fixture = MultiRepoFixture::new();
    fixture.init_workspace_repo();
    fixture.init_nested_repo("visible", &["src/edited.ts"], Some("src/edited.ts"));
    fixture.init_nested_repo("hidden", &["src/new.ts"], Some("src/new.ts"));
    fixture.install_git_wrapper();
    let mut harness = fixture.harness_with_git_explorer();

    harness.open_file_explorer();
    harness.wait_until(|_| fixture.status_cwds().contains(&fixture.root()));
    assert!(!fixture.status_cwds().contains(&fixture.repo_root("hidden")));

    harness.expand_explorer_path("visible");
    harness.wait_until(|_| fixture.status_cwds().contains(&fixture.repo_root("visible")));
    assert!(harness.screen_to_string().contains("M"));
    assert!(harness.explorer_decoration(fixture.root()).is_some());
}
```

Define `MultiRepoFixture` in this test file with `TempDir`, `root: PathBuf`, `git_log: PathBuf`, and `bin_dir: PathBuf`. `init_workspace_repo` and `init_nested_repo` run `/usr/bin/git init`, set test-local identity, commit an initial tracked file, then modify the supplied file so real `git status --porcelain -z` returns a deterministic modification. `install_git_wrapper` calls `write_git_wrapper` and prepends `bin_dir` to the harness process `PATH`; `status_cwds` parses only wrapper-log lines whose argv is `status --porcelain -z`. Reuse `EditorTestHarness::wait_until` and the existing Git plugin setup helper to load the source Git Explorer plugin. Register the module in the plugin E2E module tree and `all_tests.rs`.

- [ ] **Step 2: Run integration tests and verify failure**

Run:

```bash
cargo test -p fresh --test all_tests git_explorer_scope -- --nocapture
```

Expected: failure on hidden-repository process count because the old implementation recursively discovers the nested repository tree.

- [ ] **Step 3: Assert bounded work in the fixture**

After the initial Explorer scope settles, parse `git-status.log` and assert its status cwd set equals `{ fixture.root() }`. After expanding `visible`, assert the set equals `{ fixture.root(), fixture.repo_root("visible") }`. Assert the wrapper never records `fixture.repo_root("hidden")`. Keep every asynchronous assertion inside `EditorTestHarness::wait_until`; do not add sleeps.

- [ ] **Step 4: Run focused and broad verification**

Run:

```bash
cargo test -p fresh --test all_tests git_explorer_scope -- --nocapture
cargo test -p fresh --test all_tests file_explorer -- --nocapture
cargo test -p fresh --test all_tests plugins::git -- --nocapture
cargo test -p fresh --test all_tests -- --nocapture
/Users/drdimitru/.bun/bin/bun test crates/fresh-editor/plugins/git_explorer_scope.test.ts
```

Expected: all commands PASS. If the full suite cannot run, report the exact unavailable prerequisite and the results of every focused command.

- [ ] **Step 5: Build and install only the local executable**

Run:

```bash
cargo build --release -p fresh
install -m 755 target/release/fresh /Users/drdimitru/.local/bin/fresh
shasum -a 256 /Users/drdimitru/.local/bin/fresh
cmp -s /opt/homebrew/bin/fresh /Users/drdimitru/.local/bin/fresh || true
```

Expected: local binary hash changes; Homebrew binary remains unmodified.

- [ ] **Step 6: Verify a named attached session without disabling the plugin**

Create a disposable multi-repository workspace and start a named local session with the enabled `git_explorer_local` configuration. Expand exactly one nested repository, inspect the server log, and assert:

- no `git_explorer_local` `git status --porcelain -z` call runs for hidden repositories;
- workspace root and expanded repository each run at most one status check for an unchanged scope revision;
- parent and file markers render after the expanded repository status returns.

Terminate only the disposable session after verification; leave user sessions untouched.

- [ ] **Step 7: Commit integration coverage**

```bash
git add crates/fresh-editor/tests/e2e/plugins/git_explorer_scope.rs \
  crates/fresh-editor/tests/e2e/plugins/mod.rs \
  crates/fresh-editor/tests/all_tests.rs
git commit -m "test: cover scoped git explorer refreshes"
```
