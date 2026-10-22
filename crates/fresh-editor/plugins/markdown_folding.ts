/// <reference path="./lib/fresh.d.ts" />
// Markdown Folding Plugin
// Publishes fold ranges for Markdown buffers so the standard fold toggle and
// the gutter's fold indicators work without a language server: a heading
// folds its section, a fenced code block its body, and a list item its
// nested items. Ranges are recomputed after edits, debounced.
import { markdownFoldRanges } from "./lib/markdown_folds.ts";

const editor = getEditor();

const REFRESH_DEBOUNCE_MS = 300;

function isMarkdownBuffer(bufferId: number): boolean {
  const info = editor.getBufferInfo(bufferId);
  if (!info) return false;
  const path = info.path;
  return path.endsWith(".md") || path.endsWith(".markdown") || path.endsWith(".mdx")
    || info.language === "markdown";
}

/** Above the editor's large-file threshold the buffer is not read whole. */
function scanByteLimit(): number {
  const cfg = editor.getConfig() as { editor?: { large_file_threshold_bytes?: number } } | null;
  const v = cfg?.editor?.large_file_threshold_bytes;
  return typeof v === "number" && v > 0 ? v : 1048576;
}

/** Latest refresh per buffer: an edit during the read makes it stale. */
const generations = new Map<number, number>();
const pending = new Set<number>();
let timer: number | null = null;

async function refresh(bufferId: number): Promise<void> {
  if (!isMarkdownBuffer(bufferId)) return;
  const generation = (generations.get(bufferId) ?? 0) + 1;
  generations.set(bufferId, generation);
  const limit = scanByteLimit();
  if (editor.getBufferLength(bufferId) > limit) return;
  let text: string;
  try {
    text = await editor.getBufferText(bufferId, 0, limit);
  } catch (e) {
    editor.debug(`Markdown folding skipped for buffer ${bufferId}: ${e}`);
    return;
  }
  if (generations.get(bufferId) !== generation) return;
  editor.setFoldingRanges(bufferId, markdownFoldRanges(text));
}

function markdownFoldingRefresh(): void {
  timer = null;
  const buffers = [...pending];
  pending.clear();
  for (const bufferId of buffers) void refresh(bufferId);
}
registerHandler("markdownFoldingRefresh", markdownFoldingRefresh);

function scheduleRefresh(bufferId: number): void {
  pending.add(bufferId);
  if (timer !== null) editor.clearInterval(timer);
  timer = editor.setTimeout(REFRESH_DEBOUNCE_MS, "markdownFoldingRefresh");
}

editor.on("after_file_open", (data) => {
  void refresh(data.buffer_id);
});

editor.on("buffer_activated", (data) => {
  void refresh(data.buffer_id);
});

editor.on("language_changed", (data) => {
  void refresh(data.buffer_id);
});

editor.on("after_insert", (data) => {
  if (isMarkdownBuffer(data.buffer_id)) scheduleRefresh(data.buffer_id);
});

editor.on("after_delete", (data) => {
  if (isMarkdownBuffer(data.buffer_id)) scheduleRefresh(data.buffer_id);
});

editor.on("buffer_closed", (data) => {
  generations.delete(data.buffer_id);
  pending.delete(data.buffer_id);
});
