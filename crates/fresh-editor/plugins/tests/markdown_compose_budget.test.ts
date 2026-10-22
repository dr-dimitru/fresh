import { expect, test } from "bun:test";
import { readFileSync } from "node:fs";
import vm from "node:vm";
import { wrapToWidth } from "../lib/text.ts";
import * as reflow from "../lib/markdown_reflow.ts";

type Call = { name: string; args: any[] };
type Marker = { id: string; start: number; end: number; payload: unknown };

// This is a component port, not an E2E harness. It runs the real plugin's
// callbacks and records commands. Visible behavior is covered by Rust screens.
function plugin(limit: unknown = 0) {
  const calls: Call[] = [];
  const work: reflow.ReflowWork = { widthUnits: 0, cells: 0, wrapVisits: 0, lookaheadVisits: 0 };
  const hooks = new Map<string, Function>();
  const handlers = new Map<string, Function>();
  const markers = new Map<string, Marker>();
  let settings = { maxProseReflowCodeUnits: limit };
  let length = 100000;
  let markerEntriesVisited = 0;
  const port: Record<string, Function> = {
    on: (name: string, callback: Function | string) => hooks.set(name,
      typeof callback === "string" ? handlers.get(callback)! : callback),
    getPluginConfig: () => settings,
    getConfig: () => ({ editor: { page_width: 80 } }),
    getViewport: () => ({ width: 100, height: 24 }),
    getBufferLength: () => length,
    getBufferInfo: (id: number) => ({ id, view_mode: "compose", is_composing_in_any_split: true }),
    listBuffers: () => [{ id: 1, view_mode: "compose", is_composing_in_any_split: true }],
    listSplits: () => [],
    utf8ByteLength: (text: string) => Buffer.byteLength(text),
    stringWidth: (text: string) => [...text].length,
    charWidth: () => 1,
    // Match the host's documented O(n), inclusive-boundary snapshot query.
    // Counting this port work exposes projection-maintenance cost separately
    // from the pure per-character prose counters.
    queryMarkers: (_id: number, start: number, end: number) => [...markers.values()]
      .filter(m => { markerEntriesVisited++; return m.start <= end && m.end >= start; }),
    createMarker: (_buffer: number, id: string, start: number, end: number, payload: unknown) =>
      markers.set(id, { id, start, end, payload }),
    deleteMarker: (_buffer: number, id: string) => markers.delete(id),
  };
  const editor = new Proxy({}, { get: (_, name: string) => (...args: any[]) => {
    calls.push({ name, args });
    return port[name]?.(...args);
  } });
  const context = vm.createContext({
    getEditor: () => editor,
    registerHandler: (name: string, fn: Function) => handlers.set(name, fn),
    wrapToWidth,
    ...reflow,
    flowBreaks: (...args: Parameters<typeof reflow.flowBreaks>) =>
      reflow.flowBreaks(args[0], args[1], args[2], args[3], work),
  });
  const source = readFileSync(process.env.COMPOSE_BUDGET_PLUGIN ?? new URL("../markdown_compose.ts", import.meta.url), "utf8")
    .replace(/^import .*from .*;$/gm, "");
  vm.runInContext(new Bun.Transpiler({ loader: "ts" }).transformSync(source), context);
  calls.length = 0;
  const lines = (contents: string[], first = 0, start = 0, complete: boolean | undefined = true,
    regions: Array<string | undefined> = []) => {
    let offset = start;
    return contents.map((content, i) => {
      const line = { content, line_number: first + i, byte_start: offset,
        byte_end: offset + Buffer.byteLength(content), flow_run_complete: complete, region: regions[i] };
      offset = line.byte_end;
      return line;
    });
  };
  const batch = (offered: ReturnType<typeof lines>, id = 1) => {
    (hooks.get("pane_lines_changed") ?? hooks.get("lines_changed"))!({
      window_id: 1, split_id: 1, buffer_id: id, lines: offered, epoch: 0,
      layout_generation: 1, view_mode: "compose", layout: { textWidth: 80, gutterWidth: 0 },
      is_composing_in_any_split: true,
    });
  };
  return { calls, markers, lines, batch, hooks, context, work,
    markerWork: () => markerEntriesVisited,
    config: (value: unknown) => { settings = { maxProseReflowCodeUnits: value };
      hooks.get("config_changed")?.({}); },
    bufferLength: (value: number) => { length = value; },
  };
}

function joins(calls: Call[]) {
  // Cross-line replacement conceals have an end beyond the next line start.
  return calls.filter(c => c.name === "addConceal" && c.args[4] === " ");
}

test("finite policy emits no joins for oversized prose", () => {
  const p = plugin(8);
  p.batch(p.lines(["Alpha\n", "Bravo\n"]));
  expect(joins(p.calls)).toHaveLength(0);
});

test("unlimited policy keeps upstream joins without allocating layout ownership", () => {
  const p = plugin(0);
  p.batch(p.lines(["Alpha\n", "Bravo\n"]));
  expect(joins(p.calls)).toHaveLength(1);
  expect(p.calls.filter(c => c.name === "replaceTextLayoutRanges")).toHaveLength(0);
  expect(p.markers.size).toBe(0);
});

test("fallback continuation quotes retain their own bar conceals", () => {
  const p = plugin(1);
  p.batch(p.lines(["> Alpha\n", "> Bravo\n"]));
  expect(joins(p.calls)).toHaveLength(0);
  expect(p.calls.filter(c => c.name === "addConceal" && c.args[4] === "▌")).toHaveLength(2);
});

test("finite policy does not join an incomplete fragment below budget", () => {
  const p = plugin(100);
  p.batch(p.lines(["Alpha\n", "Bravo\n"], 20, 1000, false));
  expect(joins(p.calls)).toHaveLength(0);
});

test("real plugin skips all counted prose work above budget and for partial batches", () => {
  for (const size of [513, 10000, 65536]) {
    const p = plugin(512);
    p.batch(p.lines(["word ".repeat(size) + "\n"]));
    expect(p.calls.filter(c => c.name === "addSoftBreak")).toHaveLength(0);
    expect(Object.values(p.work)).toEqual([0, 0, 0, 0]);
  }
  const p = plugin(512);
  p.batch(p.lines(["Alpha\n", "Bravo\n"]));
  expect(p.work.cells).toBe(11);
  expect(p.work.widthUnits).toBe(10);
});

test("partial edits delegate exact coverage and old-span retirement to the pane owner", () => {
  const p = plugin(20);
  p.batch(p.lines(["Alpha\n", "Bravo\n"]));
  expect(joins(p.calls)).toHaveLength(1);
  p.calls.length = 0;
  p.batch(p.lines(["Bravo".repeat(10) + "\n"], 1, 6, false));
  const replacement = p.calls.find(c => c.name === "replaceTextLayoutRanges")!;
  expect(replacement.args.slice(1)).toEqual(["md-flow", 20, [{ start: 6, end: 57 }], []]);
  expect(joins(p.calls)).toHaveLength(0);
  // The actual indexed retirement/re-offer is tested natively, not simulated
  // by this recording port. No JS snapshot scan or refresh loop remains.
  expect(p.calls.filter(c => c.name === "queryMarkers")).toHaveLength(0);
  expect(p.calls.filter(c => c.name === "refreshLines")).toHaveLength(0);
  p.calls.length = 0;
  p.batch(p.lines(["Bravo".repeat(10) + "\n"], 1, 6, false));
  expect(p.calls.filter(c => c.name === "refreshLines")).toHaveLength(0);
});

test("growing then shrinking a complete block removes and restores reflow", () => {
  const p = plugin(10);
  p.batch(p.lines(["Alpha\n", "Bravo\n"]));
  expect(joins(p.calls)).toHaveLength(1);
  p.calls.length = 0;
  p.batch(p.lines(["Alpha!\n", "Bravo\n"]));
  expect(joins(p.calls)).toHaveLength(0);
  expect(p.calls.some(c => c.name === "clearSoftBreaksInRange")).toBe(true);
  p.calls.length = 0;
  p.batch(p.lines(["Alpha\n", "Bravo\n"]));
  expect(joins(p.calls)).toHaveLength(1);
});

test("config invalidation includes offscreen buffers and only clears prose namespaces", () => {
  const p = plugin(0);
  p.batch(p.lines(["Alpha\n", "Bravo\n"]), 1);
  p.batch(p.lines(["Charlie\n", "Delta\n"]), 2);
  p.calls.length = 0;
  p.config(8);
  expect(p.calls.filter(c => c.name === "invalidateTextLayout").map(c => c.args)).toEqual([["md-flow"]]);
  expect(p.calls.filter(c => c.name === "clearConcealNamespace" || c.name === "clearSoftBreakNamespace")).toHaveLength(0);
  p.calls.length = 0;
  p.config(8);
  expect(p.calls).not.toContainEqual(expect.objectContaining({ name: "invalidateTextLayout" }));
  p.calls.length = 0;
  p.config(0);
  expect(p.calls.filter(c => c.name === "invalidateTextLayout").map(c => c.args)).toEqual([["md-flow"]]);
});

test("tiny prose budgets keep fenced code and table soft-break paths", () => {
  const code = plugin(1);
  code.batch(code.lines(["```js\n", "word ".repeat(50) + "\n", "```\n"], 0, 0, true,
    ["open", "body", "close"]));
  expect(code.calls.some(c => c.name === "addSoftBreak" && c.args[1] === "md-wrap")).toBe(true);
  expect(code.calls.some(c => c.name === "addConceal" && String(c.args[4]).includes("┌"))).toBe(true);
  const table = plugin(1);
  table.batch(table.lines(["| Header | Other |\n", "| --- | --- |\n", `| ${"word ".repeat(40)} | cell |\n`]));
  expect(table.calls.some(c => c.name === "addSoftBreak" && c.args[1] === "md-wrap")).toBe(true);
  expect(table.calls.some(c => c.name === "addVirtualLine")).toBe(true);
});

test("native ownership receives full byte coordinates beyond 2 GiB", () => {
  const p = plugin(10);
  const start = 2 ** 31 + 100;
  p.bufferLength(start + 100);
  p.batch(p.lines(["Alpha\n", "Bravo\n"], 100000, start));
  expect(p.markers.size).toBe(0);
  const replacement = p.calls.find(c => c.name === "replaceTextLayoutRanges")!;
  expect(replacement.args[4]).toEqual([{ start, end: start + 12 }]);
  p.calls.length = 0;
  p.config(1);
  expect(p.calls.find(c => c.name === "invalidateTextLayout")?.args).toEqual(["md-flow"]);
});

test("finite projection maintenance stays bounded as visited eligible paragraphs grow", () => {
  const probe = (visited: number) => {
    const p = plugin(20);
    for (let i = 0; i < visited; i++) {
      p.batch(p.lines(["Alpha\n", "Bravo\n"], i * 3, i * 100));
    }
    const before = p.markerWork();
    p.calls.length = 0;
    p.batch(p.lines(["Alpha\n", "Bravo\n"], visited * 3, visited * 100));
    expect(joins(p.calls)).toHaveLength(1);
    expect(p.calls.filter(c => c.name === "replaceTextLayoutRanges")).toHaveLength(1);
    expect(p.markers.size).toBe(0);
    return p.markerWork() - before;
  };
  const small = probe(10);
  const large = probe(500);
  // Same offered source, viewport and finite budget. A larger history must
  // not turn one scrolling batch into a scan of every visited paragraph.
  expect(large).toBeLessThanOrEqual(small + 4);
});
