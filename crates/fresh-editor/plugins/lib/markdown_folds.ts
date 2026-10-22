// Fold ranges for a Markdown document, in the shape `setFoldingRanges` takes:
// 0-based `startLine`/`endLine`, the start line staying visible.
//
// One set covers heading sections, fenced code blocks and list items with
// nested content. The host keeps a single range set per buffer and skips its
// indent-based fallback once a set exists, so a publisher that sent headings
// alone would take list folds away.

export type MarkdownFoldRange = {
  startLine: number;
  endLine: number;
  kind: "region";
};

const FENCE = /^( {0,3})(`{3,}|~{3,})/;
const HEADING = /^ {0,3}(#{1,6})(?:[ \t]|$)/;
const LIST_ITEM = /^([ \t]*)(?:[-*+]|\d{1,9}[.)])(?:[ \t]|$)/;

function isBlank(line: string): boolean {
  return line.trim().length === 0;
}

function indentWidth(line: string): number {
  let width = 0;
  for (const ch of line) {
    if (ch === " ") width += 1;
    else if (ch === "\t") width += 4 - (width % 4);
    else break;
  }
  return width;
}

/** Last non-blank line in `start..=end`, or `start` when all are blank. */
function trimTrailingBlank(lines: string[], start: number, end: number): number {
  while (end > start && isBlank(lines[end])) end--;
  return end;
}

/** Fold ranges for `text`, ordered by start line. Nothing inside a fenced
 * block is a heading or a list item. */
export function markdownFoldRanges(text: string): MarkdownFoldRange[] {
  const lines = text.split("\n").map((line) => (line.endsWith("\r") ? line.slice(0, -1) : line));
  const last = lines.length - 1;
  const ranges: MarkdownFoldRange[] = [];
  const headings: { line: number; level: number }[] = [];
  const inFence: boolean[] = new Array(lines.length).fill(false);

  for (let i = 0; i < lines.length; i++) {
    const open = lines[i].match(FENCE);
    if (!open) continue;
    const marker = open[2];
    // A closer uses the opener's character, is at least as long, and has
    // nothing after it but whitespace. An unclosed fence runs to the end.
    let close = last;
    for (let j = i + 1; j < lines.length; j++) {
      const m = lines[j].match(FENCE);
      if (m && m[2][0] === marker[0] && m[2].length >= marker.length
          && lines[j].slice(m[0].length).trim() === "") {
        close = j;
        break;
      }
    }
    for (let j = i + 1; j <= close; j++) inFence[j] = true;
    if (close > i) ranges.push({ startLine: i, endLine: close, kind: "region" });
    i = close;
  }

  for (let i = 0; i < lines.length; i++) {
    if (inFence[i] || FENCE.test(lines[i])) continue;
    const heading = lines[i].match(HEADING);
    if (heading) {
      headings.push({ line: i, level: heading[1].length });
      continue;
    }
    const item = lines[i].match(LIST_ITEM);
    if (!item) continue;
    // An item owns the lines after it that are blank or indented deeper
    // than its marker, up to the first line that is neither.
    const indent = indentWidth(item[1]);
    let end = i;
    for (let j = i + 1; j < lines.length; j++) {
      if (isBlank(lines[j])) continue;
      if (indentWidth(lines[j]) <= indent) break;
      end = j;
    }
    if (end > i) ranges.push({ startLine: i, endLine: end, kind: "region" });
  }

  // A section runs to the line before the next heading of the same or a
  // higher level, without the blank lines that separate it from that one.
  for (let h = 0; h < headings.length; h++) {
    let end = last;
    for (let k = h + 1; k < headings.length; k++) {
      if (headings[k].level <= headings[h].level) {
        end = headings[k].line - 1;
        break;
      }
    }
    end = trimTrailingBlank(lines, headings[h].line, end);
    if (end > headings[h].line) ranges.push({ startLine: headings[h].line, endLine: end, kind: "region" });
  }

  return ranges.sort((a, b) => a.startLine - b.startLine || b.endLine - a.endLine);
}
