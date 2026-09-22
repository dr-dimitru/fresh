export const MAX_SCOPE_CANDIDATES = 128;
export const MAX_CACHED_REPOSITORIES = 128;

export type FileExplorerScope = {
  root: string;
  revision: number;
  visiblePaths: string[];
  expandedDirectories: string[];
};

export type Decoration = {
  path: string;
  symbol: string;
  color: string;
  priority: number;
};

export type GitExplorerPort = {
  pathDirname(path: string): string;
  pathJoin(...parts: string[]): string;
  fileExists(path: string): boolean;
  resolveRepository(path: string): Promise<string | null>;
  readStatus(repoRoot: string): Promise<string | null>;
};

const COLORS = {
  added: "ui.file_status_added_fg",
  modified: "ui.file_status_modified_fg",
  deleted: "ui.file_status_deleted_fg",
  renamed: "ui.file_status_renamed_fg",
  untracked: "ui.file_status_untracked_fg",
  conflicted: "ui.file_status_conflicted_fg",
};

const PRIORITY = {
  conflicted: 90,
  deleted: 80,
  added: 60,
  modified: 50,
  renamed: 40,
  untracked: 30,
};

type StatusDecoration = Omit<Decoration, "path">;

function statusToDecoration(status: string, staged: boolean): StatusDecoration | null {
  switch (status) {
    case "A":
      return { symbol: "A", color: COLORS.added, priority: PRIORITY.added };
    case "M":
      return {
        symbol: "M",
        color: staged ? COLORS.added : COLORS.modified,
        priority: PRIORITY.modified + (staged ? 2 : 0),
      };
    case "D":
      return { symbol: "D", color: COLORS.deleted, priority: PRIORITY.deleted };
    case "R":
      return { symbol: "R", color: COLORS.renamed, priority: PRIORITY.renamed };
    case "C":
      return { symbol: "C", color: COLORS.renamed, priority: PRIORITY.renamed };
    case "U":
      return { symbol: "!", color: COLORS.conflicted, priority: PRIORITY.conflicted };
    default:
      return null;
  }
}

/** Parse `git status --porcelain -z` output into absolute file decorations. */
export function parseStatusOutput(
  output: string,
  repoRoot: string,
  port: Pick<GitExplorerPort, "pathJoin">,
): Decoration[] {
  const separator = output.includes("\0") ? "\0" : "\n";
  const entries = output
    .split(separator)
    .map((entry) => entry.replace(/\r$/, ""))
    .filter((entry) => entry.length > 0);
  const byPath = new Map<string, Decoration>();

  for (let index = 0; index < entries.length; index++) {
    const entry = entries[index];
    if (entry.length < 3) continue;

    const x = entry[0];
    const y = entry[1];
    let path = entry.slice(3);
    if ((x === "R" || x === "C") && separator === "\0" && index + 1 < entries.length) {
      index++;
      path = entries[index];
    } else if (
      entry.includes(" -> ") &&
      (x === "R" || x === "C" || y === "R" || y === "C")
    ) {
      path = entry.split(" -> ").pop() ?? path;
    }

    let decoration: StatusDecoration | null = null;
    if (x === "?" && y === "?") {
      decoration = { symbol: "U", color: COLORS.untracked, priority: PRIORITY.untracked };
    } else if (x !== " " && x !== "?") {
      decoration = statusToDecoration(x, true);
    } else if (y !== " ") {
      decoration = statusToDecoration(y, false);
    }
    if (!decoration) continue;

    const absolutePath = port.pathJoin(repoRoot, path);
    const existing = byPath.get(absolutePath);
    if (!existing || decoration.priority >= existing.priority) {
      byPath.set(absolutePath, { path: absolutePath, ...decoration });
    }
  }

  return [...byPath.values()].sort((left, right) => left.path.localeCompare(right.path));
}

function isWithin(path: string, root: string): boolean {
  if (path === root) return true;
  const prefix = root.endsWith("/") ? root : `${root}/`;
  return path.startsWith(prefix);
}

/** Build highest-priority parent-directory markers up through `root`. */
export function buildAncestorDecorations(
  statuses: Decoration[],
  root: string,
  port: Pick<GitExplorerPort, "pathDirname">,
): Decoration[] {
  const byPath = new Map<string, Decoration>();

  for (const status of statuses) {
    if (!isWithin(status.path, root)) continue;
    let descendant = status.path;
    while (descendant !== root) {
      const parent = port.pathDirname(descendant);
      if (!parent || parent === descendant || !isWithin(parent, root)) break;
      const existing = byPath.get(parent);
      if (!existing || status.priority >= existing.priority) {
        byPath.set(parent, {
          path: parent,
          symbol: status.symbol,
          color: status.color,
          priority: status.priority,
        });
      }
      descendant = parent;
    }
  }

  return [...byPath.values()].sort((left, right) => left.path.localeCompare(right.path));
}

type Candidate = {
  path: string;
  workspaceRoot: boolean;
  revision: number;
  scopeGeneration: number;
};

type RepositoryJob = {
  root: string;
  revision: number;
  scopeGeneration: number;
  repositoryGeneration: number;
};

type QueueItem = { kind: "candidate"; candidate: Candidate } | { kind: "repository"; job: RepositoryJob };

type RepositoryCacheEntry = {
  generation: number;
  lastVisibleRevision: number;
  lastVisibleAt: number;
  decorations: Decoration[];
};

function sameScope(left: FileExplorerScope | null, right: FileExplorerScope | null): boolean {
  if (left === null || right === null) return left === right;
  return (
    left.root === right.root &&
    left.revision === right.revision &&
    left.visiblePaths.length === right.visiblePaths.length &&
    left.visiblePaths.every((path, index) => path === right.visiblePaths[index]) &&
    left.expandedDirectories.length === right.expandedDirectories.length &&
    left.expandedDirectories.every((path, index) => path === right.expandedDirectories[index])
  );
}

function scopeCandidatePaths(scope: FileExplorerScope, port: GitExplorerPort): string[] {
  const paths: string[] = [];
  const seen = new Set<string>();
  const add = (path: string) => {
    if (!isWithin(path, scope.root) || seen.has(path) || paths.length >= MAX_SCOPE_CANDIDATES) {
      return;
    }
    seen.add(path);
    paths.push(path);
  };
  const addAncestors = (path: string) => {
    let current = path;
    while (current !== scope.root) {
      const parent = port.pathDirname(current);
      if (!parent || parent === current || !isWithin(parent, scope.root)) break;
      if (parent === scope.root) break;
      add(parent);
      current = parent;
    }
  };

  add(scope.root);
  // Rendered rows lead. Add their unrendered ancestors next so compact-folder
  // rows still probe nested repository roots.
  for (const path of scope.visiblePaths) add(path);
  for (const path of scope.visiblePaths) addAncestors(path);
  // Expanded-only directories have lower priority than rows on screen.
  for (const path of scope.expandedDirectories) add(path);
  for (const path of scope.expandedDirectories) addAncestors(path);
  return paths;
}

function mergeDecorations(decorations: Decoration[]): Decoration[] {
  const byPath = new Map<string, Decoration>();
  for (const decoration of decorations) {
    const current = byPath.get(decoration.path);
    if (!current || decoration.priority >= current.priority) {
      byPath.set(decoration.path, decoration);
    }
  }
  return [...byPath.values()].sort((left, right) => left.path.localeCompare(right.path));
}

/** Scope-bounded, single-flight cache for Git Explorer status snapshots. */
export class GitExplorerScopeCache {
  private currentScope: FileExplorerScope | null = null;
  private currentCandidates: string[] = [];
  private workspaceRoot: string | null = null;
  private scopeGeneration = 0;
  private queue: QueueItem[] = [];
  private running: Promise<void> | null = null;
  private readonly repositories = new Map<string, RepositoryCacheEntry>();
  private readonly repositoryGenerations = new Map<string, number>();
  private readonly pendingInvalidations = new Set<string>();
  private clock = 0;

  constructor(private readonly git: GitExplorerPort) {}

  refresh(scope: FileExplorerScope | null): Promise<void> {
    const nextScope = scope
      ? {
          root: scope.root,
          revision: scope.revision,
          visiblePaths: [...scope.visiblePaths],
          expandedDirectories: [...scope.expandedDirectories],
        }
      : null;

    if (sameScope(this.currentScope, nextScope)) {
      if (this.running) return this.running;
      if (this.queue.length === 0 && this.pendingInvalidations.size === 0) {
        return Promise.resolve();
      }
      return this.startDrain();
    }

    this.scopeGeneration++;
    this.currentScope = nextScope;
    if (!nextScope) {
      this.currentCandidates = [];
      this.queue = [];
      this.pendingInvalidations.clear();
      return this.running ?? Promise.resolve();
    }

    this.workspaceRoot = nextScope.root;
    const candidates = scopeCandidatePaths(nextScope, this.git);
    this.currentCandidates = candidates;
    for (const root of this.pendingInvalidations) {
      if (!this.repositoryIsVisible(root)) this.pendingInvalidations.delete(root);
    }

    const invalidated: QueueItem[] = [...this.pendingInvalidations].map((root) => ({
      kind: "repository",
      job: {
        root,
        revision: nextScope.revision,
        scopeGeneration: this.scopeGeneration,
        repositoryGeneration: this.repositoryGenerations.get(root) ?? 0,
      },
    }));
    const candidateItems: QueueItem[] = candidates.map((path) => ({
      kind: "candidate",
      candidate: {
        path,
        workspaceRoot: path === nextScope.root,
        revision: nextScope.revision,
        scopeGeneration: this.scopeGeneration,
      },
    }));
    this.queue = [...invalidated, ...candidateItems];
    return this.startDrain();
  }

  /** Invalidate and queue only the visible repository that owns `path`. */
  invalidatePath(path: string): void {
    if (!this.currentScope) return;
    const root = [...this.repositories.keys()]
      .filter((candidate) => isWithin(path, candidate) && this.repositoryIsVisible(candidate))
      .sort((left, right) => right.length - left.length)[0];
    if (!root) return;

    if (!this.git.fileExists(this.git.pathJoin(root, ".git"))) {
      this.removeRepository(root);
      return;
    }

    const generation = this.nextRepositoryGeneration(root);
    this.pendingInvalidations.add(root);
    this.queue = this.queue.filter(
      (item) => item.kind !== "repository" || item.job.root !== root,
    );
    this.queue.unshift({
      kind: "repository",
      job: {
        root,
        revision: this.currentScope.revision,
        scopeGeneration: this.scopeGeneration,
        repositoryGeneration: generation,
      },
    });
    void this.startDrain();
  }

  decorations(): Decoration[] {
    const fileDecorations = [...this.repositories.values()]
      .flatMap((entry) => entry.decorations)
      .filter((decoration) => !this.workspaceRoot || isWithin(decoration.path, this.workspaceRoot));
    const ancestors = this.workspaceRoot
      ? buildAncestorDecorations(fileDecorations, this.workspaceRoot, this.git)
      : [];
    return mergeDecorations([...fileDecorations, ...ancestors]);
  }

  private repositoryIsVisible(root: string): boolean {
    return this.currentScope !== null && this.currentCandidates.some((path) => isWithin(path, root));
  }

  private startDrain(): Promise<void> {
    if (this.running) return this.running;
    if (this.queue.length === 0) return Promise.resolve();

    let drain: Promise<void>;
    drain = this.drainQueue().finally(() => {
      if (this.running === drain) this.running = null;
      this.pruneGenerations();
      if (this.queue.length > 0) void this.startDrain();
    });
    this.running = drain;
    return drain;
  }

  private async drainQueue(): Promise<void> {
    while (this.queue.length > 0) {
      const item = this.queue.shift()!;
      if (!this.currentScope) continue;
      if (item.kind === "repository") {
        await this.refreshRepository(item.job);
      } else {
        await this.inspectCandidate(item.candidate);
      }
    }
  }

  private async inspectCandidate(candidate: Candidate): Promise<void> {
    if (candidate.scopeGeneration !== this.scopeGeneration || !this.currentScope) return;
    if (!candidate.workspaceRoot) {
      let hasGitEntry = false;
      try {
        hasGitEntry = this.git.fileExists(this.git.pathJoin(candidate.path, ".git"));
      } catch {
        return;
      }
      if (!hasGitEntry) {
        if (this.repositories.has(candidate.path)) this.removeRepository(candidate.path);
        return;
      }
    } else if (
      !this.git.fileExists(this.git.pathJoin(candidate.path, ".git")) &&
      this.repositories.has(candidate.path)
    ) {
      this.removeRepository(candidate.path);
    }

    let repoRoot: string | null = null;
    try {
      // The workspace root is resolved even without a local `.git` entry:
      // the workspace can open below its repository root.
      repoRoot = await this.git.resolveRepository(candidate.path);
    } catch {
      return;
    }
    if (!repoRoot) {
      if (candidate.workspaceRoot) this.evictMissingCachedAncestors(candidate.path);
      return;
    }
    if (candidate.scopeGeneration !== this.scopeGeneration) return;
    if (!this.repositoryIsVisible(repoRoot)) return;

    const cached = this.repositories.get(repoRoot);
    if (cached) {
      cached.lastVisibleRevision = candidate.revision;
      cached.lastVisibleAt = ++this.clock;
      if (!this.pendingInvalidations.has(repoRoot)) return;
    }

    const generation = this.pendingInvalidations.has(repoRoot)
      ? this.repositoryGenerations.get(repoRoot) ?? this.nextRepositoryGeneration(repoRoot)
      : this.nextRepositoryGeneration(repoRoot);
    await this.readAndCacheStatus(repoRoot, generation, candidate.revision);
  }

  private async refreshRepository(job: RepositoryJob): Promise<void> {
    if (
      job.scopeGeneration !== this.scopeGeneration ||
      job.repositoryGeneration !== this.repositoryGenerations.get(job.root) ||
      !this.repositoryIsVisible(job.root)
    ) {
      return;
    }
    let hasGitEntry = false;
    try {
      hasGitEntry = this.git.fileExists(this.git.pathJoin(job.root, ".git"));
    } catch {
      return;
    }
    if (!hasGitEntry) {
      this.removeRepository(job.root);
      return;
    }
    await this.readAndCacheStatus(job.root, job.repositoryGeneration, job.revision);
  }

  private async readAndCacheStatus(
    repoRoot: string,
    generation: number,
    revision: number,
  ): Promise<void> {
    try {
      const output = await this.git.readStatus(repoRoot);
      if (this.repositoryGenerations.get(repoRoot) !== generation) return;
      if (output === null) {
        this.retainPreviousStatus(repoRoot, generation, revision);
        return;
      }
      this.repositories.set(repoRoot, {
        generation,
        lastVisibleRevision: revision,
        lastVisibleAt: ++this.clock,
        decorations: parseStatusOutput(output, repoRoot, this.git),
      });
      this.pendingInvalidations.delete(repoRoot);
      this.evictLeastRecentlyVisible();
    } catch {
      if (this.repositoryGenerations.get(repoRoot) === generation) {
        this.retainPreviousStatus(repoRoot, generation, revision);
      }
    }
  }

  private retainPreviousStatus(repoRoot: string, generation: number, revision: number): void {
    const cached = this.repositories.get(repoRoot);
    if (cached) {
      cached.generation = generation;
      cached.lastVisibleRevision = revision;
      cached.lastVisibleAt = ++this.clock;
    } else {
      this.repositories.set(repoRoot, {
        generation,
        lastVisibleRevision: revision,
        lastVisibleAt: ++this.clock,
        decorations: [],
      });
    }
    this.pendingInvalidations.delete(repoRoot);
    this.evictLeastRecentlyVisible();
  }

  private nextRepositoryGeneration(repoRoot: string): number {
    const generation = (this.repositoryGenerations.get(repoRoot) ?? 0) + 1;
    this.repositoryGenerations.set(repoRoot, generation);
    return generation;
  }

  private removeRepository(repoRoot: string): void {
    this.nextRepositoryGeneration(repoRoot);
    this.pendingInvalidations.delete(repoRoot);
    this.repositories.delete(repoRoot);
  }

  private evictMissingCachedAncestors(path: string): void {
    for (const root of [...this.repositories.keys()]) {
      if (!isWithin(path, root)) continue;
      try {
        if (!this.git.fileExists(this.git.pathJoin(root, ".git"))) {
          this.removeRepository(root);
        }
      } catch {
        // Keep cached badges if the filesystem cannot answer this check.
      }
    }
  }

  private evictLeastRecentlyVisible(): void {
    while (this.repositories.size > MAX_CACHED_REPOSITORIES) {
      const oldest = [...this.repositories.entries()].sort(
        (left, right) => left[1].lastVisibleAt - right[1].lastVisibleAt,
      )[0];
      if (!oldest) return;
      this.repositories.delete(oldest[0]);
      if (!this.pendingInvalidations.has(oldest[0])) {
        this.repositoryGenerations.delete(oldest[0]);
      }
    }
  }

  private pruneGenerations(): void {
    const queuedRoots = new Set(
      this.queue
        .filter((item): item is Extract<QueueItem, { kind: "repository" }> => item.kind === "repository")
        .map((item) => item.job.root),
    );
    for (const root of this.repositoryGenerations.keys()) {
      if (
        !this.repositories.has(root) &&
        !this.pendingInvalidations.has(root) &&
        !queuedRoots.has(root)
      ) {
        this.repositoryGenerations.delete(root);
      }
    }
  }
}
