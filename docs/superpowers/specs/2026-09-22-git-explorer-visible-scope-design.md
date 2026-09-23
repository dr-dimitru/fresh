# Git Explorer visible-scope status design

## Intent

Keep `git_explorer_local` enabled while preserving file and parent-directory Git badges. Remove attach-time repository storms in large multi-repository workspaces.

Success means a newly attached session does not recursively discover or run `git status` for every nested repository. With the Explorer visible, it initially checks only the workspace repository and repositories represented by visible or expanded Explorer directories. As a user reveals a nested repository, its badge appears asynchronously without blocking rendering.

## Scope

- Add a read-only File Explorer scope API and scope-change hook for TypeScript plugins.
- Update the durable local Git Explorer plugin to consume this API.
- Preserve the current marker alphabet, colors, parent-directory aggregation, and optional filename coloring.
- Keep all Git calls asynchronous and bounded.

Out of scope:

- Disabling `git_explorer_local`.
- Changing Git Gutter behavior.
- Whole-workspace repository indexing, polling, or synchronous Git work.

## Explorer scope API

Add a plugin API method:

```ts
editor.getFileExplorerScope(): FileExplorerScope | null
```

```ts
type FileExplorerScope = {
  root: string;
  revision: number;
  visiblePaths: string[];
  expandedDirectories: string[];
};
```

Paths are absolute, normalized Explorer paths. `visiblePaths` contains only rows currently rendered after Explorer filtering, compact-directory handling, and scroll offset. `expandedDirectories` contains expanded directory nodes known to the current Explorer tree. Both arrays are deduplicated.

When the Explorer is hidden or uninitialized, the method returns `null`. No filesystem traversal or Git work occurs while building this snapshot.

Add a payload-free plugin hook:

```ts
editor.on("file_explorer_scope_changed", () => {
  const scope = editor.getFileExplorerScope();
});
```

The core increments `revision` and emits the hook after a meaningful scope change:

- directory expand or collapse;
- Explorer scroll that changes rendered rows;
- filter or root change;
- Explorer becomes visible, hidden, initialized, or rebuilt.

It does not emit for a render pass that leaves the scope unchanged. The core must finish Explorer mutations before invoking the hook so the getter sees a consistent snapshot.

## Core implementation

File Explorer state already owns the tree, expanded-node state, filtering, and row scroll position. Add a snapshot builder beside that state. It derives rendered rows from the same flattened, filtered tree used by the renderer and collects expanded directory paths from loaded nodes.

Expose the snapshot through the plugin command/API layer and add the hook type to the Rust hook enum, JSON serialization, generated API bindings, and `fresh.d.ts`.

Route all scope-affecting Explorer actions through one deduplicated notifier. The notifier compares the new snapshot fingerprint with the previous fingerprint before incrementing `revision` or running hooks. Decoration and slot updates must not cause a scope event.

## Git Explorer implementation

Replace recursive `discoverSubRepos()` discovery with scope-driven repository candidates.

1. Read the current Explorer scope after initialization and each scope-change hook.
2. Always consider the workspace Git repository once.
3. Derive candidate directories from `visiblePaths`, their ancestors through `scope.root`, and `expandedDirectories`. This handles compact-directory rows whose nested repository root is an unrendered ancestor. Deduplicate candidates and cap the per-scope candidate set at 128, prioritizing the workspace root, visible rows, then expanded-only directories.
4. Test candidates for a `.git` entry using the existing filesystem API. Resolve a positive candidate with `git rev-parse --show-toplevel` and cache by resolved repository root.
5. Queue uncached or invalidated repositories. Visible candidates have priority over expanded-only candidates. A collapsed candidate is removed from queued work, but an already running process may finish.
6. Run at most one repository refresh at a time. Each refresh is `git status --porcelain -z`; no synchronous Git process or recursive directory scan is allowed.
7. Keep cached statuses and derive file, directory, and ancestor decorations from all known cached repositories. A parent directory retains a cached child status after collapse; an undiscovered nested repository has no badge until it enters scope. Cache at most 128 repository roots, evicting least-recently-visible entries first.
8. File save, revert, and Explorer filesystem mutation invalidate only a repository that contains the affected path. If it remains in scope, queue it again. `focus_gained` must not trigger a full refresh. A hidden or uninitialized Explorer queues no repositories.

A scope refresh is generation-tagged, and each repository cache entry has a monotonically increasing refresh generation. Results from superseded work must not re-add a no-longer-scoped repository to the pending queue or overwrite a newer result for that repository.

## Failure handling

- A non-repository candidate is skipped without a Git process.
- A failed `rev-parse` or `status` preserves the prior cached badge until a later successful refresh; a first-time failure contributes no decoration.
- A deleted repository or `.git` removal removes its cache entry when the corresponding path is invalidated or a visible/expanded candidate no longer contains `.git`. Collapsing a directory alone retains its cached badge subject to LRU eviction.
- Plugin API unavailability is treated as an unsupported Fresh build: log once and retain existing decorations rather than falling back to a recursive scan.

## Tests

### Core

- Expand, collapse, scroll, filter, root, and visibility changes update `FileExplorerScope` correctly.
- Unchanged render passes and decoration updates do not emit `file_explorer_scope_changed`.
- Snapshot paths remain normalized and deduplicated with compact directories and symlink aliases.
- Type declarations expose the getter, scope type, and hook.

### Plugin

- Unchanged scope creates no Git process.
- Initial scope checks only workspace root plus visible or expanded nested repository roots.
- Expanding a nested repository queues that repository without rescanning unrelated nested repositories.
- Parent status uses highest-priority cached descendant status.
- Saving a file invalidates and refreshes only its containing cached repository.
- Stale async results cannot resurrect collapsed queued work or replace newer cache data.

### Integration

Create a workspace containing multiple nested repositories. Assert Explorer startup does not invoke Git for hidden repositories, expansion triggers only the expanded repository, and file plus parent badges remain correct.

## Verification

Run focused core and plugin tests, then the relevant Explorer and plugin integration suites. Capture plugin process logs from a named attached session and verify no recursive Git sweep occurs: initial Git commands are bounded by workspace root plus visible/expanded repository candidates.
