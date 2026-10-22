import { expect, test } from "bun:test";
import { markdownFoldRanges } from "../lib/markdown_folds.ts";

const spans = (text: string) => markdownFoldRanges(text).map((r) => [r.startLine, r.endLine]);

test("a heading folds to the line before the next heading of its level or higher", () => {
  const doc = ["# A", "a", "## B", "b", "", "## C", "c", "# D", "d"].join("\n");
  expect(spans(doc)).toEqual([[0, 6], [2, 3], [5, 6], [7, 8]]);
});

test("a section drops the blank lines before the next heading", () => {
  expect(spans(["## A", "", "text", "", "", "## B", "x"].join("\n"))).toEqual([[0, 2], [5, 6]]);
});

test("a heading with nothing under it does not fold", () => {
  expect(spans(["# A", "", "# B"].join("\n"))).toEqual([]);
});

test("a fenced block folds from its opener to its closer", () => {
  expect(spans(["```bash", "echo", "```", "after"].join("\n"))).toEqual([[0, 2]]);
});

test("nothing inside a fence is a heading or a list item", () => {
  const doc = ["```", "# comment", "- not a list", "  nested", "```"].join("\n");
  expect(spans(doc)).toEqual([[0, 4]]);
});

test("a closer must match the opener's character and length", () => {
  const doc = ["~~~~", "```", "~~~", "~~~~", "## After", "x"].join("\n");
  expect(spans(doc)).toEqual([[0, 3], [4, 5]]);
});

test("an unclosed fence runs to the end", () => {
  expect(spans(["```", "a", "# b"].join("\n"))).toEqual([[0, 2]]);
});

test("a list item folds its nested items and continuation lines", () => {
  const doc = ["- a", "  - b", "    - c", "  - d", "- e", "", "1. f", "   more", "", "after"].join("\n");
  expect(spans(doc)).toEqual([[0, 3], [1, 2], [6, 7]]);
});

test("a flat list does not fold", () => {
  expect(spans(["- a", "- b", "- c"].join("\n"))).toEqual([]);
});

test("a list item keeps a nested fence and the fence folds too", () => {
  const doc = ["- item", "  ```", "  code", "  ```", "next"].join("\n");
  expect(spans(doc)).toEqual([[0, 3], [1, 3]]);
});

test("headings, lists and fences nest in one set", () => {
  const doc = ["## S", "- p", "  - c", "", "```", "x", "```", "", "## T"].join("\n");
  expect(spans(doc)).toEqual([[0, 6], [1, 2], [4, 6]]);
});

test("CRLF line endings do not hide a closer or a blank line", () => {
  expect(spans(["## A", "x", "", "```", "y", "```", "## B"].join("\r\n"))).toEqual([[0, 5], [3, 5]]);
});

test("a hash without a space is not a heading", () => {
  expect(spans(["#tag", "text", "#hashtag"].join("\n"))).toEqual([]);
});
