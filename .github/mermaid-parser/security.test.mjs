import assert from "node:assert/strict";
import { createRequire } from "node:module";
import test from "node:test";

const require = createRequire(import.meta.url);
// Exercise the package actually selected by Mermaid, including nested installs.
const katex = createRequire(require.resolve("mermaid"))("katex");
const expression = String.raw`\href{https://example.invalid}{safe}`;

test("inherited options cannot enable trusted math rendering", () => {
  const inheritedOptions = Object.create({ trust: true });
  const rendered = katex.renderToString(expression, inheritedOptions);
  assert.doesNotMatch(rendered, /href="https:\/\/example\.invalid"/);
});

test("explicit trusted rendering and ordinary math remain available", () => {
  const rendered = katex.renderToString(expression, { trust: true });
  assert.match(rendered, /href="https:\/\/example\.invalid"/);
  assert.match(katex.renderToString(String.raw`x^2 + \frac{1}{2}`), /katex/);
});
