// Pure prose-reflow policy and the existing Compose wrapping algorithm.
// All source offsets and budgets in this module are UTF-16 code units.

export function normalizeReflowLimit(value: unknown): number {
  return typeof value === "number" && Number.isSafeInteger(value) && value >= 0 ? value : 0;
}

export interface ReflowSource {
  content: string;
  flow_run_complete?: boolean;
}

/** 0 is unlimited. A finite policy requires a complete offered source run.
 * Terminators do not count; indentation, markup and other source text do.
 * Length reads and terminator checks do not walk a rejected line's characters. */
export function canReflowProse(lines: ReflowSource[], limit: number): boolean {
  if (limit === 0) return true;
  let units = 0;
  for (const line of lines) {
    if (line.flow_run_complete !== true) return false;
    const content = line.content;
    units += content.length - (content.endsWith("\r\n") ? 2 : content.endsWith("\n") ? 1 : 0);
    if (units > limit) return false;
  }
  return true;
}

export const JOIN_CELL = -1;
export interface FlowCell {
  w: number;
  brk: boolean;
  li: number;
  ci: number;
}

/** Optional deterministic work accounting, independent of wall-clock time. */
export interface ReflowWork {
  widthUnits: number;
  cells: number;
  wrapVisits: number;
  lookaheadVisits: number;
}

/** The upstream Compose cell algorithm, factored out without changing how it
 * measures Unicode or concealed markup. Call only after the policy decision.
 * `widths` constructs the composed width array for a full source line. */
export function flowBreaks(
  lines: Array<{ text: string; contentStart: number }>,
  width: number,
  hangingIndent: number,
  widths: (text: string, line: number) => number[],
  work?: ReflowWork,
): FlowCell[] {
  const cells: FlowCell[] = [];
  for (let li = 0; li < lines.length; li++) {
    const { text, contentStart } = lines[li];
    if (li > 0) cells.push({ w: 1, brk: true, li, ci: JOIN_CELL });
    const charW = widths(text, li);
    if (work) work.widthUnits += text.length;
    for (let ci = contentStart; ci < text.length; ci++) {
      cells.push({ w: charW[ci], brk: text[ci] === " " && charW[ci] > 0, li, ci });
    }
  }
  if (work) work.cells += cells.length;

  // Match the renderer's EOL cursor reservation, with the existing extra
  // safety column. This deliberately retains Compose's current wrap policy.
  const wrapBudget = Math.max(1, width - 2);
  const breaks: FlowCell[] = [];
  let column = 0;
  for (let i = 0; i < cells.length; i++) {
    const cell = cells[i];
    if (cell.brk && column > 0) {
      let nextWordLen = 0;
      let j = i + 1;
      for (; j < cells.length; j++) {
        if (cells[j].brk) break;
        nextWordLen += cells[j].w;
      }
      if (work) work.lookaheadVisits += j - i - 1 + (j < cells.length ? 1 : 0);
      if (column + cell.w + nextWordLen > wrapBudget && nextWordLen > 0) {
        breaks.push(cell);
        column = hangingIndent;
        continue;
      }
    }
    column += cell.w;
  }
  if (work) work.wrapVisits += cells.length;
  return breaks;
}
