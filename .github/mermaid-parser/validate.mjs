import { JSDOM } from "jsdom";

const dom = new JSDOM("<!doctype html><html><body></body></html>");
globalThis.window = dom.window;
globalThis.document = dom.window.document;
globalThis.Option = dom.window.Option;
const { default: mermaid } = await import("mermaid");

let source = "";
for await (const chunk of process.stdin) {
  source += chunk;
}

mermaid.initialize({ startOnLoad: false });
try {
  await mermaid.parse(source);
} catch (error) {
  console.error(error instanceof Error ? error.message : String(error));
  process.exitCode = 1;
}
