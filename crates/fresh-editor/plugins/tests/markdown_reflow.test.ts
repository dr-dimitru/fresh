import { expect, test } from "bun:test";
import { canReflowProse, flowBreaks, normalizeReflowLimit, type ReflowWork } from "../lib/markdown_reflow.ts";

const line = (content: string, complete: boolean | undefined = true) =>
  ({ content, flow_run_complete: complete });

test("default and invalid budgets resolve to unlimited", () => {
  for (const value of [undefined, null, -1, 1.5, "512", NaN, Infinity, Number.MAX_SAFE_INTEGER + 1, {}, true]) {
    expect(normalizeReflowLimit(value)).toBe(0);
  }
  expect(normalizeReflowLimit(0)).toBe(0);
  expect(normalizeReflowLimit(512)).toBe(512);
});

test("counts source UTF-16 units, not bytes, code points or columns", () => {
  for (const text of ["ASCII", "café", "e\u0301", "日本語", "🦀", " > **🦀** "]) {
    expect(canReflowProse([line(text)], text.length)).toBe(true);
    if (text.length > 1) expect(canReflowProse([line(text)], text.length - 1)).toBe(false);
  }
  expect(canReflowProse([line("🦀")], 1)).toBe(false);
  expect(canReflowProse([line("日本")], 2)).toBe(true);
});

test("excludes LF/CRLF but counts whitespace, markers and markup", () => {
  for (const ending of ["", "\n", "\r\n"]) {
    const lines = [line(" > **🦀** " + ending), line("Bravo" + ending)];
    const units = " > **🦀** ".length + 5;
    expect(canReflowProse(lines, units)).toBe(true);
    expect(canReflowProse(lines, units - 1)).toBe(false);
  }
});

test("finite policy rejects incomplete or unknown runs; unlimited keeps old behavior", () => {
  for (const complete of [false, undefined]) {
    const lines = [{ content: "Alpha\n", flow_run_complete: complete }, line("Bravo\n")];
    expect(canReflowProse(lines, 100)).toBe(false);
    expect(canReflowProse(lines, 0)).toBe(true);
  }
});

function counted(texts: string[], limit: number, complete = true) {
  const work: ReflowWork = { widthUnits: 0, cells: 0, wrapVisits: 0, lookaheadVisits: 0 };
  let widthCalls = 0;
  const lines = texts.map(text => line(text, complete));
  const breaks = canReflowProse(lines, limit) ? flowBreaks(
    texts.map(text => ({ text, contentStart: 0 })), 30, 2,
    text => { widthCalls++; return Array(text.length).fill(1); }, work,
  ) : [];
  return { work, widthCalls, breaks };
}

test("oversized prose performs zero per-character reflow work at every size", () => {
  for (const size of [513, 4096, 65536, 1_000_000]) {
    for (const texts of [["x".repeat(size)], Array(100).fill("word ".repeat(size / 100 + 1))]) {
      const result = counted(texts, 512);
      expect(result.widthCalls).toBe(0);
      expect(Object.values(result.work)).toEqual([0, 0, 0, 0]);
      expect(result.breaks).toEqual([]);
    }
  }
});

test("unknown fragment work stays zero even below finite budget", () => {
  expect(Object.values(counted(["small", "fragment"], 512, false).work)).toEqual([0, 0, 0, 0]);
});

test("eligible blocks have linear counted work bounded by source units and joins", () => {
  for (const texts of [["word ".repeat(40)], Array(20).fill("word ".repeat(4)), ["x".repeat(500)], [" ".repeat(500)]]) {
    const units = texts.reduce((n, text) => n + text.length, 0);
    const { work, widthCalls } = counted(texts, units);
    expect(widthCalls).toBe(texts.length);
    expect(work.widthUnits).toBe(units);
    expect(work.cells).toBeLessThanOrEqual(units + texts.length - 1);
    expect(work.wrapVisits).toBe(work.cells);
    expect(work.lookaheadVisits).toBeLessThanOrEqual(work.cells);
  }
});

test("extracted algorithm retains join and source-space break positions", () => {
  const { breaks } = counted(["Alpha", "Bravo Charlie"], 0);
  expect(breaks).toEqual([]);
  const narrow = flowBreaks([{ text: "Alpha", contentStart: 0 }, { text: "Bravo", contentStart: 0 }],
    10, 0, text => Array(text.length).fill(1));
  expect(narrow.map(cell => [cell.li, cell.ci])).toEqual([[1, -1]]);
});
