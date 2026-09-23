import { expect, test } from "bun:test";
import { readFile, unlink, writeFile } from "node:fs/promises";
import { resolve } from "node:path";
import { pathToFileURL } from "node:url";
import {
  GitExplorerScopeCache,
  type FileExplorerScope,
  type GitExplorerPort,
} from "../lib/git_explorer_scope.ts";

class FakeGit implements GitExplorerPort {
  readonly statusCwds: string[] = [];

  constructor(protected readonly statuses: Record<string, string>) {}

  removeRepository(root: string) {
    delete this.statuses[root];
  }

  setStatus(root: string, status: string) {
    this.statuses[root] = status;
  }

  pathDirname(path: string) {
    return path.slice(0, path.lastIndexOf("/")) || "/";
  }

  pathJoin(...parts: string[]) {
    return parts.join("/").replace(/\/+/g, "/");
  }

  fileExists(path: string) {
    return path.endsWith("/.git") && Object.hasOwn(this.statuses, this.pathDirname(path));
  }

  async resolveRepository(path: string) {
    return (
      Object.keys(this.statuses)
        .filter((root) => path === root || path.startsWith(root.endsWith("/") ? root : `${root}/`))
        .sort((left, right) => right.length - left.length)[0] ?? null
    );
  }

  async readStatus(repoRoot: string) {
    this.statusCwds.push(repoRoot);
    return this.statuses[repoRoot] ?? null;
  }
}

type ImmediateStatus = { value: string | null } | { error: Error };
type PendingStatus = {
  repoRoot: string;
  resolve: (value: string | null) => void;
  reject: (error: Error) => void;
};

class ControlledGit extends FakeGit {
  readonly pending: PendingStatus[] = [];

  constructor(statuses: Record<string, string>, private readonly immediate: ImmediateStatus[] = []) {
    super(statuses);
  }

  async readStatus(repoRoot: string): Promise<string | null> {
    this.statusCwds.push(repoRoot);
    const response = this.immediate.shift();
    if (response) {
      if ("error" in response) throw response.error;
      return response.value;
    }
    return new Promise((resolve, reject) => {
      this.pending.push({ repoRoot, resolve, reject });
    });
  }
}

function scope(
  visiblePaths: string[],
  expandedDirectories: string[],
  revision = 1,
  root = "/workspace",
): FileExplorerScope {
  return { root, revision, visiblePaths, expandedDirectories };
}

async function flushUntil(predicate: () => boolean) {
  for (let attempt = 0; attempt < 100 && !predicate(); attempt++) {
    await Promise.resolve();
  }
  expect(predicate()).toBe(true);
}

function cacheInternals(cache: GitExplorerScopeCache) {
  return cache as unknown as {
    repositories: Map<string, unknown>;
    repositoryGenerations: Map<string, number>;
    pendingInvalidations: Set<string>;
  };
}

test("queues workspace and visible nested repo but not hidden sibling", async () => {
  const git = new FakeGit({
    "/workspace": "",
    "/workspace/packages/visible": " M src/edited.ts\0",
  });
  const cache = new GitExplorerScopeCache(git);

  await cache.refresh(
    scope(
      ["/workspace", "/workspace/packages/visible"],
      ["/workspace", "/workspace/packages"],
    ),
  );

  expect(git.statusCwds).toEqual(["/workspace", "/workspace/packages/visible"]);
  expect(git.statusCwds).not.toContain("/workspace/packages/hidden");
});

test("workspace root resolves a parent repository without a local .git entry", async () => {
  const git = new FakeGit({ "/repo": " M workspace/file.ts\0" });
  const cache = new GitExplorerScopeCache(git);

  await cache.refresh(
    scope(["/repo/workspace", "/repo/workspace/file.ts"], ["/repo/workspace"], 1, "/repo/workspace"),
  );

  expect(git.statusCwds).toEqual(["/repo"]);
});

test("unchanged scope makes no additional Git status call", async () => {
  const git = new FakeGit({ "/workspace": "" });
  const cache = new GitExplorerScopeCache(git);
  const current = scope(["/workspace"], ["/workspace"]);

  await cache.refresh(current);
  await cache.refresh(current);

  expect(git.statusCwds).toEqual(["/workspace"]);
});

test("cached conflict bubbles to every parent directory", async () => {
  const git = new FakeGit({ "/workspace/packages/visible": "UU src/conflicted.ts\0" });
  const cache = new GitExplorerScopeCache(git);

  await cache.refresh(
    scope(["/workspace/packages/visible"], ["/workspace/packages"]),
  );

  expect(cache.decorations()).toContainEqual(
    expect.objectContaining({ path: "/workspace/packages", symbol: "!" }),
  );
  expect(cache.decorations()).toContainEqual(
    expect.objectContaining({ path: "/workspace", symbol: "!" }),
  );
});

test("compact row probes its unrendered repository ancestor", async () => {
  const git = new FakeGit({ "/workspace/pkg": " M src/index.ts\0" });
  const cache = new GitExplorerScopeCache(git);

  await cache.refresh(
    scope(["/workspace", "/workspace/pkg/src/index.ts"], ["/workspace/pkg/src"]),
  );

  expect(git.statusCwds).toEqual(["/workspace/pkg"]);
});

test("collapse removes a queued repository before its status starts", async () => {
  const git = new ControlledGit({
    "/workspace": "",
    "/workspace/slow": " M src/slow.ts\0",
  });
  const cache = new GitExplorerScopeCache(git);
  const first = cache.refresh(
    scope(["/workspace", "/workspace/slow"], ["/workspace", "/workspace/slow"]),
  );
  await flushUntil(() => git.pending.length === 1);
  const collapsed = cache.refresh(scope(["/workspace"], ["/workspace"], 2));

  git.pending[0].resolve("");
  await Promise.all([first, collapsed]);

  expect(git.statusCwds).toEqual(["/workspace"]);
  expect(git.statusCwds).not.toContain("/workspace/slow");
});

test("collapsed repository retains cached parent badge", async () => {
  const git = new FakeGit({ "/workspace/pkg": "UU src/conflicted.ts\0" });
  const cache = new GitExplorerScopeCache(git);
  await cache.refresh(scope(["/workspace/pkg"], ["/workspace", "/workspace/pkg"]));

  await cache.refresh(scope(["/workspace"], ["/workspace"], 2));

  expect(git.statusCwds).toEqual(["/workspace/pkg"]);
  expect(cache.decorations()).toContainEqual(
    expect.objectContaining({ path: "/workspace/pkg", symbol: "!" }),
  );
  expect(cache.decorations()).toContainEqual(
    expect.objectContaining({ path: "/workspace", symbol: "!" }),
  );
});

test("visible workspace evicts cached nested repo after its git entry disappears", async () => {
  const git = new FakeGit({
    "/workspace/pkg": " M subdir/edited.ts\0",
  });
  const cache = new GitExplorerScopeCache(git);
  const current = scope(
    ["/workspace/pkg/subdir"],
    ["/workspace/pkg/subdir"],
    1,
    "/workspace/pkg/subdir",
  );
  await cache.refresh(current);
  expect(cache.decorations()).toContainEqual(
    expect.objectContaining({ path: "/workspace/pkg/subdir/edited.ts", symbol: "M" }),
  );

  git.removeRepository("/workspace/pkg");
  await cache.refresh({ ...current, revision: 2 });

  expect(git.statusCwds).toEqual(["/workspace/pkg"]);
  expect(cache.decorations()).toEqual([]);
});

test("invalidate path refreshes only its owning visible repository", async () => {
  const git = new FakeGit({
    "/workspace": "",
    "/workspace/a": " M src/a.ts\0",
    "/workspace/b": " M src/b.ts\0",
  });
  const cache = new GitExplorerScopeCache(git);
  const current = scope(
    ["/workspace", "/workspace/a", "/workspace/b"],
    ["/workspace", "/workspace/a", "/workspace/b"],
  );

  await cache.refresh(current);
  cache.invalidatePath("/workspace/a/src/a.ts");
  await cache.refresh(current);

  expect(git.statusCwds).toEqual([
    "/workspace",
    "/workspace/a",
    "/workspace/b",
    "/workspace/a",
  ]);
});

test("hidden scope keeps invalidations and refreshes repository when reopened", async () => {
  const git = new FakeGit({ "/workspace/pkg": " M src/old.ts\0" });
  const cache = new GitExplorerScopeCache(git);
  const current = scope(["/workspace/pkg"], ["/workspace", "/workspace/pkg"]);
  await cache.refresh(current);
  await cache.refresh(null);

  git.setStatus("/workspace/pkg", " M src/new.ts\0");
  cache.invalidatePath("/workspace/pkg/src/new.ts");

  expect(git.statusCwds).toEqual(["/workspace/pkg"]);
  expect(cache.decorations()).toContainEqual(
    expect.objectContaining({ path: "/workspace/pkg/src/old.ts", symbol: "M" }),
  );

  await cache.refresh({ ...current, revision: 2 });

  expect(git.statusCwds).toEqual(["/workspace/pkg", "/workspace/pkg"]);
  expect(cache.decorations()).toContainEqual(
    expect.objectContaining({ path: "/workspace/pkg/src/new.ts", symbol: "M" }),
  );
  expect(cache.decorations()).not.toContainEqual(
    expect.objectContaining({ path: "/workspace/pkg/src/old.ts" }),
  );
});

test("collapsed scope keeps invalidations until repository becomes visible", async () => {
  const git = new FakeGit({ "/workspace/pkg": " M src/old.ts\0" });
  const cache = new GitExplorerScopeCache(git);
  const visible = scope(["/workspace/pkg"], ["/workspace", "/workspace/pkg"]);
  await cache.refresh(visible);
  await cache.refresh(scope(["/workspace"], ["/workspace"], 2));

  git.setStatus("/workspace/pkg", " M src/new.ts\0");
  cache.invalidatePath("/workspace/pkg/src/new.ts");

  expect(git.statusCwds).toEqual(["/workspace/pkg"]);
  await cache.refresh({ ...visible, revision: 3 });

  expect(git.statusCwds).toEqual(["/workspace/pkg", "/workspace/pkg"]);
  expect(cache.decorations()).toContainEqual(
    expect.objectContaining({ path: "/workspace/pkg/src/new.ts", symbol: "M" }),
  );
  expect(cache.decorations()).not.toContainEqual(
    expect.objectContaining({ path: "/workspace/pkg/src/old.ts" }),
  );
});

test("save during first status queues a fresh generation", async () => {
  const git = new ControlledGit({ "/workspace/pkg": "" });
  const cache = new GitExplorerScopeCache(git);
  const current = scope(["/workspace/pkg"], ["/workspace/pkg"]);
  const initialRefresh = cache.refresh(current);
  await flushUntil(() => git.pending.length === 1);

  cache.invalidatePath("/workspace/pkg/src/edited.ts");
  git.pending[0].resolve(" M src/old.ts\0");
  await flushUntil(() => git.pending.length === 2);

  expect(cache.decorations()).toEqual([]);
  git.pending[1].resolve(" M src/new.ts\0");
  await initialRefresh;

  expect(cache.decorations()).toContainEqual(
    expect.objectContaining({ path: "/workspace/pkg/src/new.ts", symbol: "M" }),
  );
  expect(cache.decorations()).not.toContainEqual(
    expect.objectContaining({ path: "/workspace/pkg/src/old.ts" }),
  );
});

test("Windows native, Git, and extended paths match one scoped repository", async () => {
  const pathKey = (path: string) =>
    path
      .replaceAll("\\", "/")
      .replace(/^\/\/\?\/UNC\//i, "//")
      .replace(/^\/\/\?\//, "")
      .toLowerCase();
  class WindowsGit extends FakeGit {
    pathDirname(path: string) {
      const normalized = path.replaceAll("\\", "/");
      const parent = normalized.slice(0, normalized.lastIndexOf("/"));
      return parent.replaceAll("/", "\\");
    }

    pathJoin(...parts: string[]) {
      return parts.join("\\").replace(/[\\/]+/g, "\\");
    }

    fileExists(path: string) {
      if (!path.endsWith(".git")) return false;
      const root = pathKey(path).replace(/\/.git$/, "");
      return Object.keys(this.statuses).some((candidate) => pathKey(candidate) === root);
    }

    async resolveRepository(path: string) {
      const normalized = pathKey(path);
      return (
        Object.keys(this.statuses)
          .filter((root) => {
            const candidate = pathKey(root);
            return normalized === candidate || normalized.startsWith(`${candidate}/`);
          })
          .sort((left, right) => right.length - left.length)[0] ?? null
      );
    }

    async readStatus(repoRoot: string) {
      this.statusCwds.push(repoRoot);
      const root = Object.keys(this.statuses).find(
        (candidate) => pathKey(candidate) === pathKey(repoRoot),
      );
      return root ? this.statuses[root] : null;
    }
  }

  const git = new WindowsGit({
    "C:/workspace": "",
    "C:/workspace/pkg": " M src/edited.ts\0",
  });
  const root = String.raw`\\?\C:\workspace`;
  const cache = new GitExplorerScopeCache(git);

  await cache.refresh(
    scope(
      [root, String.raw`C:\workspace\pkg\src\edited.ts`],
      [root, String.raw`C:\workspace\pkg`],
      1,
      root,
    ),
  );

  expect(git.statusCwds).toEqual(["C:/workspace", "C:/workspace/pkg"]);
  expect(cache.decorations()).toContainEqual(
    expect.objectContaining({ path: expect.stringContaining("src\\edited.ts"), symbol: "M" }),
  );
});

test("scope candidate list stops at 128 paths", async () => {
  const statuses: Record<string, string> = { "/workspace": "" };
  const visiblePaths = ["/workspace"];
  for (let index = 0; index < 128; index++) {
    const path = `/workspace/nested-${index}`;
    statuses[path] = "";
    visiblePaths.push(path);
  }
  const git = new FakeGit(statuses);
  const cache = new GitExplorerScopeCache(git);

  await cache.refresh(scope(visiblePaths, ["/workspace"]));

  expect(git.statusCwds).toHaveLength(128);
  expect(git.statusCwds).toContain("/workspace/nested-126");
  expect(git.statusCwds).not.toContain("/workspace/nested-127");
});

test("evicted repositories release dirty and generation metadata", async () => {
  const statuses: Record<string, string> = { "/workspace": "" };
  for (let index = 0; index < 300; index++) {
    statuses[`/workspace/repo-${index}`] = "";
  }
  const git = new FakeGit(statuses);
  const cache = new GitExplorerScopeCache(git);
  const state = cacheInternals(cache);

  for (let index = 0; index < 300; index++) {
    const repo = `/workspace/repo-${index}`;
    await cache.refresh(scope(["/workspace", repo], ["/workspace"], index + 1));
    await cache.refresh(null);
    cache.invalidatePath(`${repo}/src/edited.ts`);
  }

  expect(state.repositories.size).toBeLessThanOrEqual(128);
  expect(state.pendingInvalidations.size).toBeLessThanOrEqual(128);
  expect(state.repositoryGenerations.size).toBeLessThanOrEqual(128);
});

test("stale first status after hiding releases uncached invalidation metadata", async () => {
  const git = new ControlledGit({ "/workspace/pkg": "" });
  const cache = new GitExplorerScopeCache(git);
  const state = cacheInternals(cache);
  const current = scope(["/workspace/pkg"], ["/workspace/pkg"]);
  const initialRefresh = cache.refresh(current);
  await flushUntil(() => git.pending.length === 1);

  cache.invalidatePath("/workspace/pkg/src/edited.ts");
  const hiddenRefresh = cache.refresh(null);
  git.pending[0].resolve(" M src/stale.ts\0");
  await Promise.all([initialRefresh, hiddenRefresh]);

  expect(state.repositories.size).toBe(0);
  expect(state.pendingInvalidations.size).toBe(0);
  expect(state.repositoryGenerations.size).toBe(0);

  const reopenedRefresh = cache.refresh(current);
  await flushUntil(() => git.pending.length === 2);
  git.pending[1].resolve(" M src/fresh.ts\0");
  await reopenedRefresh;

  expect(git.statusCwds).toEqual(["/workspace/pkg", "/workspace/pkg"]);
  expect(cache.decorations()).toContainEqual(
    expect.objectContaining({ path: "/workspace/pkg/src/fresh.ts", symbol: "M" }),
  );
  expect(cache.decorations()).not.toContainEqual(
    expect.objectContaining({ path: "/workspace/pkg/src/stale.ts" }),
  );
});

test("cache evicts least recently visible root after 128 entries", async () => {
  const statuses: Record<string, string> = {};
  for (let index = 0; index < 129; index++) {
    statuses[`/workspace/root${index}`] = "";
  }
  const git = new FakeGit(statuses);
  const cache = new GitExplorerScopeCache(git);

  for (let index = 0; index < 129; index++) {
    const path = `/workspace/root${index}`;
    await cache.refresh(scope(["/workspace", path], ["/workspace"], index + 1));
  }
  await cache.refresh(scope(["/workspace", "/workspace/root0"], ["/workspace"], 130));

  expect(git.statusCwds.filter((path) => path === "/workspace/root0")).toHaveLength(2);
  for (let index = 1; index < 129; index++) {
    expect(git.statusCwds.filter((path) => path === `/workspace/root${index}`)).toHaveLength(1);
  }
});

test("stale response cannot replace a newer repository decoration generation", async () => {
  const git = new ControlledGit(
    { "/workspace/pkg": "" },
    [{ value: " M src/kept.ts\0" }],
  );
  const cache = new GitExplorerScopeCache(git);
  const current = scope(["/workspace/pkg"], ["/workspace/pkg"]);
  await cache.refresh(current);

  cache.invalidatePath("/workspace/pkg/src/kept.ts");
  await flushUntil(() => git.pending.length === 1);
  cache.invalidatePath("/workspace/pkg/src/kept.ts");
  git.pending[0].resolve(" M src/edited.ts\0");
  await flushUntil(() => git.pending.length === 2);

  expect(cache.decorations()).toContainEqual(
    expect.objectContaining({ path: "/workspace/pkg/src/kept.ts", symbol: "M" }),
  );
  expect(cache.decorations()).not.toContainEqual(
    expect.objectContaining({ path: "/workspace/pkg/src/edited.ts", symbol: "M" }),
  );

  git.pending[1].resolve("UU src/conflicted.ts\0");
  await cache.refresh(current);

  expect(cache.decorations()).toContainEqual(
    expect.objectContaining({ path: "/workspace", symbol: "!" }),
  );
  expect(cache.decorations()).not.toContainEqual(
    expect.objectContaining({ path: "/workspace/pkg/src/edited.ts", symbol: "M" }),
  );
});

test("failed status retains previous badges and a first failure emits none", async () => {
  const git = new ControlledGit(
    { "/workspace": "" },
    [{ value: " M src/edited.ts\0" }, { error: new Error("temporary failure") }],
  );
  const cache = new GitExplorerScopeCache(git);
  const current = scope(["/workspace"], ["/workspace"]);
  await cache.refresh(current);

  cache.invalidatePath("/workspace/src/edited.ts");
  await cache.refresh(current);
  expect(cache.decorations()).toContainEqual(
    expect.objectContaining({ path: "/workspace/src/edited.ts", symbol: "M" }),
  );

  const failedFirst = new ControlledGit(
    { "/workspace": "" },
    [{ error: new Error("temporary failure") }],
  );
  const emptyCache = new GitExplorerScopeCache(failedFirst);
  await emptyCache.refresh(current);
  expect(emptyCache.decorations()).toEqual([]);
});

test("first failed status can recover after a visible file is invalidated", async () => {
  const git = new ControlledGit(
    { "/workspace/pkg": "" },
    [
      { error: new Error("temporary failure") },
      { value: " M src/recovered.ts\0" },
    ],
  );
  const cache = new GitExplorerScopeCache(git);
  const current = scope(["/workspace/pkg"], ["/workspace/pkg"]);

  await cache.refresh(current);
  expect(cache.decorations()).toEqual([]);
  cache.invalidatePath("/workspace/pkg/src/recovered.ts");
  await cache.refresh(current);

  expect(git.statusCwds).toEqual(["/workspace/pkg", "/workspace/pkg"]);
  expect(cache.decorations()).toContainEqual(
    expect.objectContaining({ path: "/workspace/pkg/src/recovered.ts", symbol: "M" }),
  );
});

test("missing scope API warns once and performs no recursive Git fallback", async () => {
  const handlers = new Map<string, () => void>();
  let gitStatusCalls = 0;
  const fakeEditor = {
    defineConfigBoolean() {},
    getCwd: () => "/workspace",
    getPluginConfig: () => ({ colorNames: true }),
    getFileExplorerScope: undefined,
    pathJoin: (...parts: string[]) => parts.join("/").replace(/\/+/g, "/"),
    pathDirname: (path: string) => path.slice(0, path.lastIndexOf("/")) || "/",
    fileExists: () => false,
    readDir: () => [],
    spawnProcess: async (_command: string, args: string[]) => {
      if (args[0] === "status") gitStatusCalls++;
      throw new Error(`unexpected Git command: ${args.join(" ")}`);
    },
    setFileExplorerDecorations: () => true,
    clearFileExplorerDecorations: () => true,
    setFileExplorerSlots: () => true,
    clearFileExplorerSlots: () => true,
    on: (event: string, handler: () => void) => handlers.set(event, handler),
  };
  const previousEditor = (globalThis as Record<string, unknown>).__gitExplorerEditor;
  const previousWarn = console.warn;
  const warnings: string[] = [];
  (globalThis as Record<string, unknown>).__gitExplorerEditor = fakeEditor;
  console.warn = (...args: unknown[]) => warnings.push(args.map(String).join(" "));

  const modulePath = resolve(import.meta.dir, "..", `git_explorer_scope_missing_api_${Date.now()}.ts`);
  try {
    let source = await readFile(resolve(import.meta.dir, "../git_explorer.ts"), "utf8");
    source = source.replace(/^const editor = getEditor\(\);/m, "const editor = globalThis.__gitExplorerEditor as EditorAPI;");
    await writeFile(modulePath, source);
    await import(pathToFileURL(modulePath).href);
    handlers.get("editor_initialized")?.();
    handlers.get("editor_initialized")?.();

    expect(gitStatusCalls).toBe(0);
    expect(warnings).toHaveLength(1);
    expect(warnings[0]).toContain("getFileExplorerScope");
  } finally {
    console.warn = previousWarn;
    if (previousEditor === undefined) delete (globalThis as Record<string, unknown>).__gitExplorerEditor;
    else (globalThis as Record<string, unknown>).__gitExplorerEditor = previousEditor;
    await unlink(modulePath).catch(() => {});
  }
});
