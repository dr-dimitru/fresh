# Markdown Compose

Use **Markdown: Toggle Compose/Preview** from the command palette to switch
between source and composed Markdown. Compose normally joins a paragraph's
source lines and computes wrapping with list hanging indents and repeated quote
rails. Code frames and tables have their own layout.

## Optional prose reflow budget

By default prose reflow is unlimited, preserving the existing Compose behavior.
For documents with very long paragraphs, set a finite source-text budget:

```json
{
  "plugins": {
    "markdown_compose": {
      "settings": {
        "maxProseReflowCodeUnits": 512
      }
    }
  }
}
```

The setting is also available under **Plugin Settings → markdown_compose**.
Saving or reloading configuration updates existing Compose views, including
previously visited offscreen content. It does not change the editor's line-number
preference, existing per-pane number pins, page width, or source text.

- **Default and unlimited value: `0`.** Omission also means unlimited.
- Positive integers allow reflow for a complete prose block with at most that
  many **source UTF-16 code units**. Exactly the limit is allowed.
- Count all source text, including whitespace, indentation, list/quote markers,
  and inline markup. Exclude LF/CRLF terminators and virtual spaces between
  joined lines. LF and CRLF therefore have identical thresholds.
- `🦀` counts as two units, `日` as one, and `e` followed by a combining accent
  as two. These are not UTF-8 bytes, code points, graphemes, or display columns.
- Invalid values (negative, fractional, non-numeric, or outside JavaScript's safe
  integer range) fall back to `0`. The Settings UI accepts nonnegative integers.

## Native-wrap fallback and trade-offs

When a prose block exceeds a finite budget, Compose skips its per-character
reflow widths/cells, explicit prose soft breaks, and cross-line joins. Native
editor wrapping stays enabled, and source text and logical line boundaries stay
unchanged. A hard-wrapped source paragraph therefore remains several separate,
possibly ragged lines instead of becoming one flowed paragraph.

Ordinary per-source-line inline formatting, list markers/indentation, and quote
markers still apply. Native continuation rows do **not** promise Compose's custom
list hanging indentation or repeated quote rails. A quote marker on the next
source line remains its own decoration rather than disappearing for a join that
will not be emitted. Code-frame rails and table-cell layout are not bypassed.

The editor offers plugins bounded portions of a document. With a finite budget,
Compose also uses fallback when it cannot prove that the complete
blank-line-delimited source run is in the offered batch. This can happen after a
read/viewport/line cap or when a run begins outside the bounded lookbehind. Even
a short visible fragment then uses native wrapping rather than being mistaken
for a short whole block. A short prose block within an incomplete run may also
fall back conservatively. Unlimited (`0`) retains the existing partial-offer
behavior.

This setting limits **prose reflow work**, not all editor work. Inline syntax
formatting, highlighting, native wrapping, code frames, and tables still cost
work proportional to their own input. It is not a general latency guarantee.
