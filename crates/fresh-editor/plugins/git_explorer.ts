/// <reference path="./lib/fresh.d.ts" />
import { GitExplorerScopeCache, type Decoration } from "./lib/git_explorer_scope.ts";

const editor = getEditor();
const NAMESPACE = "git-explorer";

editor.defineConfigBoolean("colorNames", {
  default: false,
  description: "Color file explorer entry names by git status",
});

const scopeCache = new GitExplorerScopeCache({
  pathDirname: (path) => editor.pathDirname(path),
  pathJoin: (...parts) => editor.pathJoin(...parts),
  fileExists: (path) => editor.fileExists(editor.authorityPath(path)),
  resolveRepository: async (path) => {
    try {
      const result = await editor.spawnProcess(
        "git",
        ["rev-parse", "--show-toplevel"],
        path,
      );
      return result.exit_code === 0 ? result.stdout.trim() || null : null;
    } catch {
      return null;
    }
  },
  readStatus: async (repoRoot) => {
    try {
      const result = await editor.spawnProcess(
        "git",
        ["status", "--porcelain", "-z"],
        repoRoot,
      );
      return result.exit_code === 0 ? result.stdout : null;
    } catch {
      return null;
    }
  },
});

let scopeApiWarningIssued = false;

function supportsExplorerScope(): boolean {
  if (typeof editor.getFileExplorerScope === "function") return true;
  if (!scopeApiWarningIssued) {
    console.warn(
      "[git-explorer] editor.getFileExplorerScope() is unavailable; Git Explorer refresh is disabled for this Fresh build.",
    );
    scopeApiWarningIssued = true;
  }
  return false;
}

function buildNameColorSlots(decorations: Decoration[]) {
  const byPath = new Map<string, { path: string; nameColor: string; priority: number }>();
  for (const decoration of decorations) {
    const existing = byPath.get(decoration.path);
    if (!existing || decoration.priority >= existing.priority) {
      byPath.set(decoration.path, {
        path: decoration.path,
        nameColor: decoration.color,
        priority: decoration.priority,
      });
    }
  }
  return [...byPath.values()].sort((left, right) => left.path.localeCompare(right.path));
}

function publishCachedDecorations() {
  if (!supportsExplorerScope()) return;
  const decorations = scopeCache.decorations();
  if (decorations.length === 0) {
    editor.clearFileExplorerDecorations(NAMESPACE);
    editor.clearFileExplorerSlots(NAMESPACE);
    return;
  }

  editor.setFileExplorerDecorations(NAMESPACE, decorations);
  const config = (editor.getPluginConfig() ?? {}) as { colorNames?: boolean };
  if (config.colorNames) {
    editor.setFileExplorerSlots(NAMESPACE, buildNameColorSlots(decorations));
  } else {
    editor.clearFileExplorerSlots(NAMESPACE);
  }
}

async function refreshGitExplorerDecorations() {
  if (!supportsExplorerScope()) return;
  try {
    await scopeCache.refresh(editor.getFileExplorerScope());
    publishCachedDecorations();
  } catch (error) {
    // Keep the previous decorations if a repository refresh fails.
    console.warn("[git-explorer] failed to refresh scoped Git decorations", error);
  }
}

function invalidateRepositoryForPath(path: string) {
  if (!supportsExplorerScope()) return;
  scopeCache.invalidatePath(path);
  void refreshGitExplorerDecorations();
}

editor.on("editor_initialized", () => {
  void refreshGitExplorerDecorations();
});
editor.on("file_explorer_scope_changed", () => {
  void refreshGitExplorerDecorations();
});
editor.on("after_file_save", ({ path }) => {
  invalidateRepositoryForPath(path);
});
editor.on("after_file_revert", ({ path }) => {
  invalidateRepositoryForPath(path);
});
editor.on("after_file_explorer_change", ({ path }) => {
  invalidateRepositoryForPath(path);
});
editor.on("config_changed", () => {
  publishCachedDecorations();
});

void refreshGitExplorerDecorations();
